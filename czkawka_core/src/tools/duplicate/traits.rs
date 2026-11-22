use std::io::prelude::*;
use std::io::{self};
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use humansize::{DECIMAL, format_size};

use crate::common::model::CheckingMethod;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, DeleteMethod, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::duplicate::{DuplicateEntry, DuplicateFinder, DuplicateFinderParameters, Info};

impl DuplicateFinder {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        if self.common_data.delete_method == DeleteMethod::None {
            return ControlFlow::Continue(());
        }

        if self.common_data.use_reference_folders {
            if self.common_data.delete_method == DeleteMethod::HardLink {
                // For each group the referenced map already holds (original, destinations).
                // Feed this directly into HardlinkingFiles so the reference file is used as
                // the hardlink source and the non-reference duplicates are replaced.
                let hardlink_items = match self.get_params().check_method {
                    CheckingMethod::Name => self.files_with_identical_names_referenced.values().cloned().collect::<Vec<_>>(),
                    CheckingMethod::SizeName => self.files_with_identical_size_names_referenced.values().cloned().collect::<Vec<_>>(),
                    CheckingMethod::Size => self.files_with_identical_size_referenced.values().cloned().collect::<Vec<_>>(),
                    CheckingMethod::Hash => self.files_with_identical_hashes_referenced.values().flatten().cloned().collect::<Vec<_>>(),
                    _ => panic!(),
                };
                return self.delete_simple_elements_and_add_to_messages(stop_flag, progress_sender, DeleteItemType::HardlinkingFiles(hardlink_items));
            }
            // For non-HardLink methods with reference folders, delete every non-reference
            // duplicate.  The reference file is already retained as the "original", so we
            // flatten all destination lists and delete them unconditionally - bypassing the
            // "keep one" group-selection logic of delete_advanced_elements which would
            // incorrectly preserve files in single-element groups (AllExcept* modes).
            let files_to_delete: Vec<_> = match self.get_params().check_method {
                CheckingMethod::Name => self.files_with_identical_names_referenced.values().flat_map(|(_, files)| files.iter().cloned()).collect(),
                CheckingMethod::SizeName => self
                    .files_with_identical_size_names_referenced
                    .values()
                    .flat_map(|(_, files)| files.iter().cloned())
                    .collect(),
                CheckingMethod::Size => self.files_with_identical_size_referenced.values().flat_map(|(_, files)| files.iter().cloned()).collect(),
                CheckingMethod::Hash => self
                    .files_with_identical_hashes_referenced
                    .values()
                    .flatten()
                    .flat_map(|(_, files)| files.iter().cloned())
                    .collect(),
                _ => panic!(),
            };
            return self.delete_simple_elements_and_add_to_messages(stop_flag, progress_sender, DeleteItemType::DeletingFiles(files_to_delete));
        }

        let files_to_delete = match self.get_params().check_method {
            CheckingMethod::Name => self.files_with_identical_names.values().cloned().collect::<Vec<_>>(),
            CheckingMethod::SizeName => self.files_with_identical_size_names.values().cloned().collect::<Vec<_>>(),
            CheckingMethod::Hash => self.files_with_identical_hashes.values().flatten().cloned().collect::<Vec<_>>(),
            CheckingMethod::Size => self.files_with_identical_size.values().cloned().collect::<Vec<_>>(),
            _ => panic!(),
        };
        self.delete_advanced_elements_and_add_to_messages(stop_flag, progress_sender, files_to_delete)
    }
}

impl Search for DuplicateFinder {
    #[fun_time(message = "find_duplicates", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }

            match self.get_params().check_method {
                CheckingMethod::Name => self.check_files_name(stop_flag, progress_sender)?,
                CheckingMethod::SizeName => self.check_files_size_name(stop_flag, progress_sender)?,
                CheckingMethod::Size => self.check_files_size(stop_flag, progress_sender)?,
                CheckingMethod::Hash => {
                    self.check_files_size(stop_flag, progress_sender)?;
                    self.check_files_hash(stop_flag, progress_sender)?;
                }
                _ => panic!(),
            }
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for DuplicateFinder {
    fn write_results<T: Write>(&self, writer: &mut T) -> io::Result<()> {
        self.write_base_search_paths(writer)?;

        let info = &self.information;
        match self.params.check_method {
            CheckingMethod::Name => {
                let groups: Vec<_> = if self.files_with_identical_names.is_empty() {
                    self.files_with_identical_names_referenced
                        .iter()
                        .rev()
                        .map(|(name, (reference, files))| (name, Some(reference), files))
                        .collect()
                } else {
                    self.files_with_identical_names.iter().rev().map(|(name, files)| (name, None, files)).collect()
                };
                let summary = format!(
                    "Found {} files in {} groups with same name(may have different content)",
                    info.number_of_duplicated_files_by_name, info.number_of_groups_by_name
                );
                let headers = ("Files with same names", summary.as_str(), "Not found any files with same names.");
                write_groups(writer, headers, groups, true, |writer, name, count, _| writeln!(writer, "Name - {name} - {count} files "))
            }
            CheckingMethod::SizeName => {
                let groups: Vec<_> = if self.files_with_identical_size_names.is_empty() {
                    let referenced = self.files_with_identical_size_names_referenced.iter().rev();
                    referenced.map(|(size_name, (reference, files))| (size_name, Some(reference), files)).collect()
                } else {
                    self.files_with_identical_size_names
                        .iter()
                        .rev()
                        .map(|(size_name, files)| (size_name, None, files))
                        .collect()
                };
                let summary = format!(
                    "Found {} files in {} groups with same size and name(may have different content)",
                    info.number_of_duplicated_files_by_size_name, info.number_of_groups_by_size_name
                );
                let headers = ("Files with same size and names", summary.as_str(), "Not found any files with same size and names.");
                write_groups(writer, headers, groups, true, |writer, (size, name), count, _| {
                    writeln!(writer, "Name - {name}, {} - {count} files ", format_size(*size, DECIMAL))
                })
            }
            CheckingMethod::Size => {
                let groups: Vec<_> = if self.files_with_identical_size.is_empty() {
                    self.files_with_identical_size_referenced
                        .iter()
                        .rev()
                        .map(|(size, (reference, files))| (size, Some(reference), files))
                        .collect()
                } else {
                    self.files_with_identical_size.iter().rev().map(|(size, files)| (size, None, files)).collect()
                };
                let summary = lost_space_summary(info.number_of_duplicated_files_by_size, info.number_of_groups_by_size, info.lost_space_by_size);
                let headers = ("Files with same size", summary.as_str(), "Not found any duplicates.");
                // The plain variant keeps its historical trailing space
                write_groups(writer, headers, groups, false, |writer, size, count, referenced| {
                    let space = if referenced { "" } else { " " };
                    writeln!(writer, "\n---- Size {} ({size}) - {count} files{space}", format_size(*size, DECIMAL))
                })
            }
            CheckingMethod::Hash => {
                let groups: Vec<_> = if self.files_with_identical_hashes.is_empty() {
                    let referenced = self.files_with_identical_hashes_referenced.iter().rev();
                    referenced
                        .flat_map(|(size, groups)| groups.iter().map(move |(reference, files)| (size, Some(reference), files)))
                        .collect()
                } else {
                    let groups = self.files_with_identical_hashes.iter().rev();
                    groups.flat_map(|(size, groups)| groups.iter().map(move |files| (size, None, files))).collect()
                };
                let summary = lost_space_summary(info.number_of_duplicated_files_by_hash, info.number_of_groups_by_hash, info.lost_space_by_hash);
                let headers = ("Files with same hashes", summary.as_str(), "Not found any duplicates.");
                write_groups(writer, headers, groups, false, |writer, size, count, _| {
                    writeln!(writer, "\n---- Size {} ({size}) - {count} files", format_size(*size, DECIMAL))
                })
            }
            _ => panic!(),
        }
    }

    // TODO - check if is possible to save also data in header about size and name in SizeName mode - https://github.com/qarmin/czkawka/issues/1137
    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> io::Result<()> {
        if self.get_use_reference_folders() {
            match self.get_params().check_method {
                CheckingMethod::Name => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_names_referenced, pretty_print),
                CheckingMethod::SizeName => {
                    self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_size_names_referenced.values().collect::<Vec<_>>(), pretty_print)
                }
                CheckingMethod::Size => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_size_referenced, pretty_print),
                CheckingMethod::Hash => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_hashes_referenced, pretty_print),
                _ => panic!(),
            }
        } else {
            match self.get_params().check_method {
                CheckingMethod::Name => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_names, pretty_print),
                CheckingMethod::SizeName => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_size_names.values().collect::<Vec<_>>(), pretty_print),
                CheckingMethod::Size => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_size, pretty_print),
                CheckingMethod::Hash => self.save_results_to_file_as_json_internal(file_name, &self.files_with_identical_hashes, pretty_print),
                _ => panic!(),
            }
        }
    }
}

impl_common_data! {
    DuplicateFinder, Info, DuplicateFinderParameters,
    fn get_check_method(&self) -> CheckingMethod {
        self.params.check_method
    }
    fn found_any_items(&self) -> bool {
        // Hash mode also fills the size counters during its pre-filter, so only the active method counts
        let info = &self.information;
        let found = match self.params.check_method {
            CheckingMethod::Hash => info.number_of_duplicated_files_by_hash,
            CheckingMethod::Name => info.number_of_duplicated_files_by_name,
            CheckingMethod::Size => info.number_of_duplicated_files_by_size,
            CheckingMethod::SizeName => info.number_of_duplicated_files_by_size_name,
            _ => 0,
        };
        found > 0
    }
}

fn lost_space_summary(files: usize, groups: usize, lost_space: u64) -> String {
    format!("Found {files} duplicated files which in {groups} groups which takes {}.", format_size(lost_space, DECIMAL))
}

/// Writes one check method's groups (reference file first in reference-folder mode).
/// `headers` is (title, summary, not found message); `group_header` gets (key, number of files, referenced).
fn write_groups<W: Write, K>(
    writer: &mut W,
    (title, summary, not_found): (&str, &str, &str),
    groups: Vec<(K, Option<&DuplicateEntry>, &Vec<DuplicateEntry>)>,
    blank_line_after_group: bool,
    mut group_header: impl FnMut(&mut W, K, usize, bool) -> io::Result<()>,
) -> io::Result<()> {
    const DASHES: &str = "-------------------------------------------------";
    let Some((_, first_reference, _)) = groups.first() else {
        return write!(writer, "{not_found}");
    };
    let referenced = first_reference.is_some();
    let title_suffix = if referenced { " in referenced folders" } else { "" };
    writeln!(writer, "{DASHES}{title}{title_suffix}{DASHES}")?;
    writeln!(writer, "{summary}")?;
    for (key, reference, files) in groups {
        group_header(writer, key, files.len(), referenced)?;
        if let Some(reference) = reference {
            writeln!(writer, "Reference file - \"{}\"", reference.path.to_string_lossy())?;
        }
        for file in files {
            writeln!(writer, "\"{}\"", file.path.to_string_lossy())?;
        }
        if blank_line_after_group {
            writeln!(writer)?;
        }
    }
    Ok(())
}
