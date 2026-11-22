use std::collections::BTreeMap;
use std::fs;
use std::fs::{DirEntry, FileType, Metadata};
use std::ops::ControlFlow;
#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::UNIX_EPOCH;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use log::debug;
use rayon::prelude::*;

use crate::common::directories::Directories;
use crate::common::extensions::Extensions;
use crate::common::items::ExcludedItems;
use crate::common::model::{CheckingMethod, FileEntry, ToolType};
use crate::common::progress_data::{ProgressData, ToolStage};
use crate::common::progress_stop_handler::{check_if_stop_received, prepare_thread_handler_common};
use crate::common::tool_data::CommonToolData;
use crate::flc;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Collect {
    InvalidSymlinks,
    Files,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum EntryType {
    File,
    Dir,
    Symlink,
    Other,
}

/// Parallel walk over a tool's included paths; tools adjust the `pub(crate)` fields in the `configure` closure of [`CommonToolData::collect_files`].
pub(crate) struct DirTraversal<'b, F> {
    group_by: F,
    pub(crate) root_files: Vec<PathBuf>,
    stop_flag: &'b Arc<AtomicBool>,
    progress_sender: Option<&'b Sender<ProgressData>>,
    recursive_search: bool,
    directories: Directories,
    excluded_items: ExcludedItems,
    pub(crate) extensions: Extensions,
    pub(crate) minimal_file_size: u64,
    pub(crate) maximal_file_size: u64,
    tool_type: ToolType,
    /// Tags the collecting stage, so the progress label can read e.g. "Scanning size of N file".
    pub(crate) checking_method: CheckingMethod,
    pub(crate) collect: Collect,
}

type GroupedFiles<T> = BTreeMap<T, Vec<FileEntry>>;

impl CommonToolData {
    /// Walks the included paths and groups the found files by `group_by`; traversal warnings go to `text_messages`.
    pub(crate) fn collect_grouped_files<'b, F, T>(
        &mut self,
        stop_flag: &'b Arc<AtomicBool>,
        progress_sender: Option<&'b Sender<ProgressData>>,
        group_by: F,
        configure: impl FnOnce(&mut DirTraversal<'b, F>),
    ) -> ControlFlow<(), GroupedFiles<T>>
    where
        F: Fn(&FileEntry) -> T,
        T: Ord,
    {
        let mut traversal = DirTraversal::new(self, stop_flag, progress_sender, group_by);
        configure(&mut traversal);
        let (grouped_file_entries, warnings) = traversal.run()?;
        self.text_messages.warnings.extend(warnings);
        ControlFlow::Continue(grouped_file_entries)
    }

    /// [`Self::collect_grouped_files`] without grouping.
    pub(crate) fn collect_files(
        &mut self,
        stop_flag: &Arc<AtomicBool>,
        progress_sender: Option<&Sender<ProgressData>>,
        configure: impl FnOnce(&mut DirTraversal<'_, fn(&FileEntry)>),
    ) -> ControlFlow<(), Vec<FileEntry>> {
        let ungrouped: fn(&FileEntry) = |_| ();
        let grouped_file_entries = self.collect_grouped_files(stop_flag, progress_sender, ungrouped, configure)?;
        ControlFlow::Continue(grouped_file_entries.into_values().flatten().collect())
    }
}

fn entry_type(file_type: FileType) -> EntryType {
    if file_type.is_dir() {
        EntryType::Dir
    } else if file_type.is_symlink() {
        EntryType::Symlink
    } else if file_type.is_file() {
        EntryType::File
    } else {
        EntryType::Other
    }
}

fn has_valid_utf8_path(path: &Path, warnings: &mut Vec<String>) -> bool {
    if path.to_str().is_some() {
        return true;
    }
    warnings.push(flc!("core_skipping_non_utf8_path", path = path.to_string_lossy().to_string()));
    false
}

impl<'b, F, T> DirTraversal<'b, F>
where
    F: Fn(&FileEntry) -> T,
    T: Ord,
{
    fn new(common_data: &CommonToolData, stop_flag: &'b Arc<AtomicBool>, progress_sender: Option<&'b Sender<ProgressData>>, group_by: F) -> Self {
        Self {
            group_by,
            root_files: common_data.directories.included_files.clone(),
            stop_flag,
            progress_sender,
            recursive_search: common_data.recursive_search,
            directories: common_data.directories.clone(),
            excluded_items: common_data.excluded_items.clone(),
            extensions: common_data.extensions.clone(),
            minimal_file_size: common_data.minimal_file_size,
            maximal_file_size: common_data.maximal_file_size,
            tool_type: common_data.tool_type,
            checking_method: CheckingMethod::None,
            collect: Collect::Files,
        }
    }

    /// Returns the grouped files plus the warnings met on the way, or `Break` when stopped.
    #[fun_time(message = "run(collecting files/dirs)", level = "debug")]
    fn run(self) -> ControlFlow<(), (GroupedFiles<T>, Vec<String>)> {
        assert_ne!(self.tool_type, ToolType::None, "Tool type cannot be None");

        let DirTraversal {
            group_by,
            root_files,
            stop_flag,
            progress_sender,
            recursive_search,
            directories,
            excluded_items,
            extensions,
            minimal_file_size,
            maximal_file_size,
            tool_type,
            checking_method,
            collect,
        } = self;

        let mut all_warnings = Vec::new();
        let mut grouped_file_entries: GroupedFiles<T> = BTreeMap::new();

        // Add root folders and files for finding
        let mut folders_to_check: Vec<PathBuf> = directories.included_directories.clone();
        let mut files_to_check: Vec<PathBuf> = root_files;

        let collect_stage = if tool_type == ToolType::EmptyFolders {
            ToolStage::CollectingFolders
        } else {
            ToolStage::CollectingFiles(checking_method)
        };
        let progress_handler = prepare_thread_handler_common(progress_sender, collect_stage, 0, 0);

        let mut file_results = Vec::new();
        // File traversal
        while let Some(current_file) = files_to_check.pop() {
            let Some(metadata) = common_get_metadata_from_path(&current_file, &mut all_warnings) else {
                continue;
            };
            let file_type = metadata.file_type();
            match (entry_type(file_type), collect) {
                (EntryType::File, Collect::Files) => {
                    progress_handler.increase_items(1);
                    process_file_in_file_mode_path_check(
                        &current_file,
                        &metadata,
                        &mut all_warnings,
                        &mut file_results,
                        &extensions,
                        &excluded_items,
                        &directories,
                        minimal_file_size,
                        maximal_file_size,
                    );
                }
                (EntryType::File, Collect::InvalidSymlinks) => {
                    progress_handler.increase_items(1);
                }
                (EntryType::Symlink, Collect::InvalidSymlinks) => {
                    progress_handler.increase_items(1);
                    process_symlink_in_symlink_mode_path_check(&current_file, &metadata, &mut all_warnings, &mut file_results, &extensions, &excluded_items);
                }
                (EntryType::Symlink | EntryType::Dir | EntryType::Other, _) => {
                    // nothing to do
                }
            }
        }
        file_results.sort_by_cached_key(|fe| fe.path.to_str().expect("path validated as UTF-8 while collecting file entries").to_owned());
        for fe in file_results {
            let key = group_by(&fe);
            grouped_file_entries.entry(key).or_default().push(fe);
        }

        // Folder traversal
        while !folders_to_check.is_empty() {
            if check_if_stop_received(stop_flag) {
                progress_handler.join_thread();
                return ControlFlow::Break(());
            }

            let segments: Vec<_> = folders_to_check
                .into_par_iter()
                .with_max_len(2) // Avoiding checking too many folders in batch
                .map(|current_folder| {
                    let mut dir_result = Vec::new();
                    let mut warnings = Vec::new();
                    let mut fe_result = Vec::new();

                    let Some(read_dir) = common_read_dir(&current_folder, &mut warnings) else {
                        return Some((dir_result, warnings, fe_result));
                    };

                    let mut counter = 0;
                    // Check every sub folder/file/link etc.
                    for entry in read_dir {
                        if check_if_stop_received(stop_flag) {
                            return None;
                        }

                        let Some(entry_data) = common_get_entry_data(&entry, &mut warnings, &current_folder) else {
                            continue;
                        };
                        let Ok(file_type) = entry_data.file_type() else { continue };

                        match (entry_type(file_type), collect) {
                            (EntryType::Dir, Collect::Files | Collect::InvalidSymlinks) => {
                                process_dir_in_file_symlink_mode(recursive_search, entry_data, &directories, &mut dir_result, &mut warnings, &excluded_items);
                            }
                            (EntryType::File, Collect::Files) => {
                                counter += 1;
                                process_file_in_file_mode(
                                    entry_data,
                                    &mut warnings,
                                    &mut fe_result,
                                    &extensions,
                                    &directories,
                                    &excluded_items,
                                    minimal_file_size,
                                    maximal_file_size,
                                );
                            }
                            (EntryType::File, Collect::InvalidSymlinks) => {
                                counter += 1;
                            }
                            (EntryType::Symlink, Collect::InvalidSymlinks) => {
                                counter += 1;
                                process_symlink_in_symlink_mode(entry_data, &mut warnings, &mut fe_result, &extensions, &directories, &excluded_items);
                            }
                            (EntryType::Symlink, Collect::Files) | (EntryType::Other, _) => {
                                // nothing to do
                            }
                        }
                    }
                    if counter > 0 {
                        // Increase counter in batch, because usually it may be slow to add multiple times atomic value
                        progress_handler.increase_items(counter);
                    }
                    Some((dir_result, warnings, fe_result))
                })
                .while_some()
                .collect();

            let required_size = segments.iter().map(|(segment, _, _)| segment.len()).sum::<usize>();
            folders_to_check = Vec::with_capacity(required_size);

            // Process collected data
            for (segment, warnings, mut fe_result) in segments {
                folders_to_check.extend(segment);
                all_warnings.extend(warnings);
                fe_result.sort_by_cached_key(|fe| fe.path.to_str().expect("path validated as UTF-8 while collecting file entries").to_owned());
                for fe in fe_result {
                    let key = group_by(&fe);
                    grouped_file_entries.entry(key).or_default().push(fe);
                }
            }
        }

        progress_handler.join_thread();

        debug!("Collected {} files", grouped_file_entries.values().map(Vec::len).sum::<usize>());

        ControlFlow::Continue((grouped_file_entries, all_warnings))
    }
}

fn process_file_in_file_mode(
    entry_data: &DirEntry,
    warnings: &mut Vec<String>,
    fe_result: &mut Vec<FileEntry>,
    extensions: &Extensions,
    directories: &Directories,
    excluded_items: &ExcludedItems,
    minimal_file_size: u64,
    maximal_file_size: u64,
) {
    if !extensions.check_if_entry_have_valid_extension(&entry_data.file_name()) {
        return;
    }

    let current_file_name = entry_data.path();
    if excluded_items.is_excluded(&current_file_name) {
        return;
    }

    if directories.is_excluded_file(&current_file_name) {
        return;
    }

    #[cfg(target_family = "unix")]
    if directories.exclude_other_filesystems() {
        match directories.is_on_other_filesystems(&current_file_name) {
            Ok(true) => return,
            Err(e) => warnings.push(e),
            _ => (),
        }
    }

    #[cfg(windows)]
    let _ = directories; // Silence unused variable warning on Windows

    let Some(metadata) = common_get_metadata_dir(entry_data, warnings, &current_file_name) else {
        return;
    };

    if !has_valid_utf8_path(&current_file_name, warnings) {
        return;
    }

    if (minimal_file_size..=maximal_file_size).contains(&metadata.len()) {
        // Creating new file entry
        let fe: FileEntry = FileEntry {
            size: metadata.len(),
            modified_date: get_modified_time(&metadata, warnings, &current_file_name, false),
            path: current_file_name,
        };

        fe_result.push(fe);
    }
}
// Same as above, but working with Path instead of DirEntry
// Sadly this cannot be merged, due to a little crazy optimizations done in this functions
fn process_file_in_file_mode_path_check(
    path: &Path,
    metadata: &Metadata,
    warnings: &mut Vec<String>,
    fe_result: &mut Vec<FileEntry>,
    extensions: &Extensions,
    excluded_items: &ExcludedItems,
    directories: &Directories,
    minimal_file_size: u64,
    maximal_file_size: u64,
) {
    let Some(file_name) = path.file_name() else {
        return;
    };
    if !extensions.check_if_entry_have_valid_extension(file_name) {
        return;
    }

    if directories.is_excluded_file(path) {
        return;
    }
    if directories.is_excluded_item_in_dir(path) {
        return;
    }

    if excluded_items.is_excluded(path) {
        return;
    }

    if !has_valid_utf8_path(path, warnings) {
        return;
    }

    if (minimal_file_size..=maximal_file_size).contains(&metadata.len()) {
        // Creating new file entry
        let fe: FileEntry = FileEntry {
            size: metadata.len(),
            modified_date: get_modified_time(metadata, warnings, path, false),
            path: path.to_path_buf(),
        };

        fe_result.push(fe);
    }
}

#[cfg_attr(target_family = "windows", expect(clippy::needless_pass_by_ref_mut))]
fn process_dir_in_file_symlink_mode(
    recursive_search: bool,
    entry_data: &DirEntry,
    directories: &Directories,
    dir_result: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
    excluded_items: &ExcludedItems,
) {
    if !recursive_search {
        return;
    }

    let dir_path = entry_data.path();
    if directories.is_excluded_dir(&dir_path) {
        return;
    }

    if excluded_items.is_excluded(&dir_path) {
        return;
    }

    #[cfg(target_family = "unix")]
    if directories.exclude_other_filesystems() {
        match directories.is_on_other_filesystems(&dir_path) {
            Ok(true) => return,
            Err(e) => warnings.push(e),
            _ => (),
        }
    }

    #[cfg(windows)]
    let _ = warnings; // Silence unused variable warning on Windows

    dir_result.push(dir_path);
}

fn process_symlink_in_symlink_mode(
    entry_data: &DirEntry,
    warnings: &mut Vec<String>,
    fe_result: &mut Vec<FileEntry>,
    extensions: &Extensions,
    directories: &Directories,
    excluded_items: &ExcludedItems,
) {
    if !extensions.check_if_entry_have_valid_extension(&entry_data.file_name()) {
        return;
    }

    let current_file_name = entry_data.path();
    if excluded_items.is_excluded(&current_file_name) {
        return;
    }

    #[cfg(target_family = "unix")]
    if directories.exclude_other_filesystems() {
        match directories.is_on_other_filesystems(&current_file_name) {
            Ok(true) => return,
            Err(e) => warnings.push(e),
            _ => (),
        }
    }

    #[cfg(windows)]
    let _ = directories; // Silence unused variable warning on Windows

    let Some(metadata) = common_get_metadata_dir(entry_data, warnings, &current_file_name) else {
        return;
    };

    if !has_valid_utf8_path(&current_file_name, warnings) {
        return;
    }

    // Creating new file entry
    let fe: FileEntry = FileEntry {
        size: metadata.len(),
        modified_date: get_modified_time(&metadata, warnings, &current_file_name, false),
        path: current_file_name,
    };

    fe_result.push(fe);
}
fn process_symlink_in_symlink_mode_path_check(
    path: &Path,
    metadata: &Metadata,
    warnings: &mut Vec<String>,
    fe_result: &mut Vec<FileEntry>,
    extensions: &Extensions,
    excluded_items: &ExcludedItems,
) {
    let Some(file_name) = path.file_name() else {
        return;
    };
    if !extensions.check_if_entry_have_valid_extension(file_name) {
        return;
    }

    if excluded_items.is_excluded(path) {
        return;
    }

    if !has_valid_utf8_path(path, warnings) {
        return;
    }

    // Creating new file entry
    let fe: FileEntry = FileEntry {
        size: metadata.len(),
        modified_date: get_modified_time(metadata, warnings, path, false),
        path: path.to_path_buf(),
    };

    fe_result.push(fe);
}

pub(crate) fn common_read_dir(current_folder: &Path, warnings: &mut Vec<String>) -> Option<Vec<Result<DirEntry, std::io::Error>>> {
    fs::read_dir(current_folder)
        .map(Iterator::collect)
        .map_err(|e| warnings.push(flc!("core_cannot_open_dir", dir = current_folder.to_string_lossy().to_string(), reason = e.to_string())))
        .ok()
}
pub(crate) fn common_get_entry_data<'a>(entry: &'a Result<DirEntry, std::io::Error>, warnings: &mut Vec<String>, current_folder: &Path) -> Option<&'a DirEntry> {
    entry
        .as_ref()
        .map_err(|e| {
            warnings.push(flc!(
                "core_cannot_read_entry_dir",
                dir = current_folder.to_string_lossy().to_string(),
                reason = e.to_string()
            ));
        })
        .ok()
}
pub(crate) fn common_get_metadata_dir(entry_data: &DirEntry, warnings: &mut Vec<String>, current_folder: &Path) -> Option<Metadata> {
    entry_data
        .metadata()
        .map_err(|e| {
            warnings.push(flc!(
                "core_cannot_read_metadata_dir",
                dir = current_folder.to_string_lossy().to_string(),
                reason = e.to_string()
            ));
        })
        .ok()
}

pub(crate) fn common_get_metadata_from_path(path: &Path, warnings: &mut Vec<String>) -> Option<Metadata> {
    fs::metadata(path)
        .map_err(|e| warnings.push(flc!("core_cannot_read_metadata_file", file = path.to_string_lossy().to_string(), reason = e.to_string())))
        .ok()
}

pub(crate) fn get_modified_time(metadata: &Metadata, warnings: &mut Vec<String>, current_file_name: &Path, is_folder: bool) -> u64 {
    match metadata.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(_inspected) => {
                if is_folder {
                    warnings.push(flc!("core_folder_modified_before_epoch", name = current_file_name.to_string_lossy().to_string()));
                } else {
                    warnings.push(flc!("core_file_modified_before_epoch", name = current_file_name.to_string_lossy().to_string()));
                }
                0
            }
        },
        Err(e) => {
            if is_folder {
                warnings.push(flc!(
                    "core_folder_no_modification_date",
                    name = current_file_name.to_string_lossy().to_string(),
                    reason = e.to_string()
                ));
            } else {
                warnings.push(flc!(
                    "core_file_no_modification_date",
                    name = current_file_name.to_string_lossy().to_string(),
                    reason = e.to_string()
                ));
            }
            0
        }
    }
}

#[cfg(target_family = "windows")]
pub(crate) fn inode(_fe: &FileEntry) -> Option<u64> {
    None
}

#[cfg(target_family = "unix")]
pub(crate) fn inode(fe: &FileEntry) -> Option<u64> {
    if let Ok(meta) = fs::metadata(&fe.path) { Some(meta.ino()) } else { None }
}

pub(crate) fn take_1_per_inode((k, mut v): (Option<u64>, Vec<FileEntry>)) -> Vec<FileEntry> {
    if k.is_some() {
        v.drain(1..);
    }
    v
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::prelude::*;
    use std::time::{Duration, SystemTime};
    use std::{fs, io};

    use indexmap::IndexSet;

    use super::*;
    use crate::common::tool_data::*;

    impl CommonData for CommonToolData {
        type Info = ();
        type Parameters = ();
        fn get_information(&self) -> Self::Info {}
        fn get_params(&self) -> &Self::Parameters {
            &()
        }
        fn get_cd(&self) -> &CommonToolData {
            self
        }
        fn get_cd_mut(&mut self) -> &mut CommonToolData {
            self
        }
        fn found_any_items(&self) -> bool {
            false
        }
    }

    static NOW: std::sync::LazyLock<SystemTime> = std::sync::LazyLock::new(|| SystemTime::UNIX_EPOCH + Duration::new(100, 0));
    const CONTENT: &[u8; 1] = b"a";

    fn normalize_path(item: &Path) -> PathBuf {
        let canonicalized = if cfg!(windows) {
            // Only canonicalize if it's not a network path
            // This can be done by checking if path starts with \\?\UNC\
            if let Ok(dir_can) = item.canonicalize()
                && let Some(dir_can_str) = dir_can.to_string_lossy().strip_prefix(r"\\?\")
                && dir_can_str.chars().nth(1) == Some(':')
            {
                PathBuf::from(dir_can_str)
            } else {
                item.to_path_buf()
            }
        } else {
            if let Ok(dir) = item.canonicalize() { dir } else { item.to_path_buf() }
        };

        #[cfg(target_family = "windows")]
        return crate::common::normalize_windows_path(&canonicalized);
        #[cfg(not(target_family = "windows"))]
        return canonicalized;
    }

    fn create_files(dir: &Path) -> io::Result<(PathBuf, PathBuf, PathBuf)> {
        let (src, hard, other_file) = (dir.join("a"), dir.join("b"), dir.join("c"));

        let mut file = File::create(&src)?;
        file.write_all(CONTENT)?;
        fs::hard_link(&src, &hard)?;
        file.set_modified(*NOW)?;

        let mut file = File::create(&other_file)?;
        file.write_all(CONTENT)?;
        file.set_modified(*NOW)?;

        Ok((normalize_path(&src), normalize_path(&hard), normalize_path(&other_file)))
    }

    #[test]
    fn test_traversal() -> io::Result<()> {
        let dir = tempfile::Builder::new().tempdir()?;
        let dir_path = normalize_path(dir.path());
        let (src, hard, other_file) = create_files(&dir_path)?;
        let secs = NOW.duration_since(SystemTime::UNIX_EPOCH).expect("Cannot fail calculating duration since epoch").as_secs();

        let mut common_data = CommonToolData::new(ToolType::SimilarImages);
        common_data.directories.set_included_paths([dir.path().to_owned()].to_vec());
        common_data.set_minimal_file_size(0);

        let grouped_file_entries = run_grouped(&common_data, |_fe| ());
        let actual: IndexSet<_> = grouped_file_entries.into_values().flatten().collect();
        assert_eq!(
            IndexSet::from([
                FileEntry {
                    path: normalize_path(&src),
                    size: 1,
                    modified_date: secs,
                },
                FileEntry {
                    path: normalize_path(&hard),
                    size: 1,
                    modified_date: secs,
                },
                FileEntry {
                    path: normalize_path(&other_file),
                    size: 1,
                    modified_date: secs,
                },
            ]),
            actual
        );
        Ok(())
    }

    fn create_temp_structure(dir: &Path) -> io::Result<(PathBuf, PathBuf, PathBuf)> {
        let global_file = dir.join("global_file.txt");
        let other_dir = dir.join("other_file");
        fs::create_dir_all(&other_dir)?;
        let other_file = other_dir.join("other_file.txt");

        let mut f = File::create(&global_file)?;
        f.write_all(b"global_file")?;
        f.set_modified(*NOW)?;

        let mut f2 = File::create(&other_file)?;
        f2.write_all(b"other_file")?;
        f2.set_modified(*NOW)?;

        let global_file = normalize_path(&global_file);
        let other_file = normalize_path(&other_file);
        let other_dir = normalize_path(&other_dir);

        Ok((global_file, other_file, other_dir))
    }

    fn run_grouped<F: Fn(&FileEntry) -> T, T: Ord>(common_data: &CommonToolData, group_by: F) -> GroupedFiles<T> {
        let ControlFlow::Continue((grouped_file_entries, _warnings)) = DirTraversal::new(common_data, &Arc::default(), None, group_by).run() else {
            panic!("Expect SuccessFiles.");
        };
        grouped_file_entries
    }

    fn run_traversal(common_data: &CommonToolData) -> Vec<FileEntry> {
        run_grouped(common_data, |_fe| ()).into_values().flatten().collect()
    }

    #[test]
    #[expect(clippy::needless_for_each)]
    fn test_traversal_with_and_without_excluded_dir() -> io::Result<()> {
        let dir = tempfile::Builder::new().tempdir()?;
        let dir_path = dir.path().to_path_buf();
        let dir_path = normalize_path(&dir_path);
        let (global_file, other_file, other_dir) = create_temp_structure(&dir_path)?;
        let secs = NOW.duration_since(SystemTime::UNIX_EPOCH).expect("Cannot fail calculating duration since epoch").as_secs();

        let mut common_data = CommonToolData::new(ToolType::SimilarImages);
        common_data.directories.set_included_paths([dir.path().to_owned()].to_vec());
        common_data.set_minimal_file_size(0);

        let mut actual: Vec<_> = run_traversal(&common_data);
        actual.iter_mut().for_each(|e| e.path = normalize_path(&e.path));
        assert_eq!(2, actual.len());
        assert!(actual.contains(&FileEntry {
            path: global_file.clone(),
            size: 11,
            modified_date: secs
        }));
        assert!(actual.contains(&FileEntry {
            path: other_file.clone(),
            size: 10,
            modified_date: secs
        }));

        let mut common_data2 = CommonToolData::new(ToolType::SimilarImages);
        common_data2.directories.set_included_paths([dir.path().to_owned()].to_vec());
        common_data2.directories.set_excluded_paths([other_dir].to_vec());
        common_data2.set_minimal_file_size(0);

        let mut actual: Vec<_> = run_traversal(&common_data2);
        actual.iter_mut().for_each(|e| e.path = normalize_path(&e.path));
        assert_eq!(1, actual.len());
        assert!(actual.contains(&FileEntry {
            path: global_file.clone(),
            size: 11,
            modified_date: secs
        }));

        let mut common_data3 = CommonToolData::new(ToolType::SimilarImages);
        common_data3.directories.set_included_paths([dir.path().to_owned()].to_vec());
        common_data3.directories.set_excluded_paths([other_file.clone()].to_vec());
        common_data3.set_minimal_file_size(0);

        let mut actual: Vec<_> = run_traversal(&common_data3);
        actual.iter_mut().for_each(|e| e.path = normalize_path(&e.path));
        assert_eq!(1, actual.len());
        assert!(actual.contains(&FileEntry {
            path: global_file.clone(),
            size: 11,
            modified_date: secs
        }));

        let mut common_data4 = CommonToolData::new(ToolType::SimilarImages);
        common_data4.directories.set_included_paths([global_file.clone()].to_vec());
        common_data4.set_minimal_file_size(0);

        let mut actual: Vec<_> = run_traversal(&common_data4);
        actual.iter_mut().for_each(|e| e.path = normalize_path(&e.path));
        assert_eq!(1, actual.len());
        assert!(actual.contains(&FileEntry {
            path: global_file.clone(),
            size: 11,
            modified_date: secs
        }));

        let mut common_data5 = CommonToolData::new(ToolType::SimilarImages);
        common_data5.directories.set_included_paths([global_file.clone(), other_file.clone()].to_vec());
        common_data5.set_minimal_file_size(0);

        let mut actual: Vec<_> = run_traversal(&common_data5);
        actual.iter_mut().for_each(|e| e.path = normalize_path(&e.path));
        assert_eq!(2, actual.len());
        assert!(actual.contains(&FileEntry {
            path: global_file.clone(),
            size: 11,
            modified_date: secs
        }));
        assert!(actual.contains(&FileEntry {
            path: other_file.clone(),
            size: 10,
            modified_date: secs
        }));

        // Other file should be excluded by optimizer, but it works even without it, so we can keep this test, but can be removed if it will start to fail
        let mut common_data6 = CommonToolData::new(ToolType::SimilarImages);
        common_data6.directories.set_included_paths([global_file.clone(), other_file.clone()].to_vec());
        common_data6.directories.set_excluded_paths([other_file].to_vec());
        common_data6.set_minimal_file_size(0);

        let mut actual: Vec<_> = run_traversal(&common_data6);
        actual.iter_mut().for_each(|e| e.path = normalize_path(&e.path));
        assert_eq!(1, actual.len());
        assert!(actual.contains(&FileEntry {
            path: global_file,
            size: 11,
            modified_date: secs
        }));

        // This test is invalid - other dir should be removed by optimizer
        // let mut common_data7 = CommonToolData::new(ToolType::SimilarImages);
        // common_data7.directories.set_included_paths([other_file.clone()].to_vec());
        // common_data7.directories.set_excluded_paths([other_dir.clone()].to_vec());
        // common_data7.set_minimal_file_size(0);
        //
        // let actual: IndexSet<_> = run_traversal(&common_data7).into_iter().collect();
        // assert_eq!(0, actual.len());

        Ok(())
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn test_traversal_group_by_inode() -> io::Result<()> {
        let dir = tempfile::Builder::new().tempdir()?;
        let dir_path = normalize_path(dir.path());
        let (src, _, other) = create_files(&dir_path)?;
        let secs = NOW.duration_since(SystemTime::UNIX_EPOCH).expect("Cannot fail calculating duration since epoch").as_secs();

        let mut common_data = CommonToolData::new(ToolType::SimilarImages);
        common_data.directories.set_included_paths([dir.path().to_owned()].to_vec());
        common_data.set_minimal_file_size(0);

        let grouped_file_entries = run_grouped(&common_data, inode);
        let actual: IndexSet<_> = grouped_file_entries.into_iter().flat_map(take_1_per_inode).collect();
        assert_eq!(
            IndexSet::from([
                FileEntry {
                    path: normalize_path(&src),
                    size: 1,
                    modified_date: secs,
                },
                FileEntry {
                    path: normalize_path(&other),
                    size: 1,
                    modified_date: secs,
                },
            ]),
            actual
        );
        Ok(())
    }

    #[cfg(target_family = "windows")]
    #[test]
    fn test_traversal_group_by_inode() -> io::Result<()> {
        let dir = tempfile::Builder::new().tempdir()?;
        let dir_path = normalize_path(dir.path());
        let (src, hard, other) = create_files(&dir_path)?;
        let secs = NOW.duration_since(SystemTime::UNIX_EPOCH).expect("Cannot fail duration from epoch").as_secs();

        let mut common_data = CommonToolData::new(ToolType::SimilarImages);
        common_data.directories.set_included_paths(vec![dir_path]);
        common_data.set_minimal_file_size(0);

        let grouped_file_entries = run_grouped(&common_data, inode);
        let actual: IndexSet<_> = grouped_file_entries.into_iter().flat_map(take_1_per_inode).collect();
        assert_eq!(
            IndexSet::from([
                FileEntry {
                    path: src,
                    size: 1,
                    modified_date: secs,
                },
                FileEntry {
                    path: hard,
                    size: 1,
                    modified_date: secs,
                },
                FileEntry {
                    path: other,
                    size: 1,
                    modified_date: secs,
                },
            ]),
            actual
        );
        Ok(())
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn test_traversal_skips_non_utf8_path() -> io::Result<()> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::Builder::new().tempdir()?;
        let dir_path = normalize_path(dir.path());

        let valid_file = dir_path.join("valid.txt");
        fs::write(&valid_file, CONTENT)?;

        let invalid_name = OsStr::from_bytes(b"invalid_\xFF_name.txt");
        let invalid_file = dir_path.join(invalid_name);
        fs::write(&invalid_file, CONTENT)?;

        let mut common_data = CommonToolData::new(ToolType::SimilarImages);
        common_data.directories.set_included_paths([dir.path().to_owned()].to_vec());
        common_data.set_minimal_file_size(0);

        let actual = run_traversal(&common_data);

        assert_eq!(actual.len(), 1, "Only the valid UTF-8 named file should be collected");
        assert_eq!(actual[0].path.file_name(), Some(OsStr::new("valid.txt")));

        Ok(())
    }
}
