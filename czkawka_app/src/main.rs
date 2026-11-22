//! Plekke, the desktop app: czkawka_core scans behind an mwp-ui page (see `ui/`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{mem, thread};

use crossbeam_channel::unbounded;
use czkawka_core::common::config_cache_path::{print_infos_and_warnings, set_config_cache_path};
use czkawka_core::common::image::{ImgResizeOptions, get_dynamic_image_from_path, register_image_decoding_hooks};
use czkawka_core::common::logger::{filtering_messages, print_version_mode, setup_logger};
use czkawka_core::common::model::{CheckingMethod, HashType};
use czkawka_core::common::progress_data::ProgressData;
use czkawka_core::common::tool_data::CommonData;
use czkawka_core::common::traits::Search;
use czkawka_core::common::trash_delete;
use czkawka_core::re_exported::{FilterType, FirFilterType, HashAlg};
use czkawka_core::tools::big_file::{BigFile, BigFileParameters, SearchMode};
use czkawka_core::tools::disk_usage::tree::{DiskTree, NodeKind};
use czkawka_core::tools::disk_usage::{ContentDetection, DiskUsage, DiskUsageParameters};
use czkawka_core::tools::duplicate::{DuplicateEntry, DuplicateFinder, DuplicateFinderParameters};
use czkawka_core::tools::empty_files::{EmptyFiles, EmptyFilesParameters};
use czkawka_core::tools::empty_folder::EmptyFolder;
use czkawka_core::tools::similar_images::core::{get_string_from_similarity, return_similarity_from_similarity_preset};
use czkawka_core::tools::similar_images::{GeometricInvariance, ImagesEntry, SimilarImages, SimilarImagesParameters, SimilarityPreset};
use log::{error, warn};
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};

/// Smallest file kept in the duplicate hash cache, same default as the CLI.
const MINIMAL_CACHED_FILE_SIZE: u64 = 257_144;
/// Perceptual hash size for similar images; 16 is the CLI default and fits the similarity presets.
const IMAGE_HASH_SIZE: u8 = 16;
/// Longest side of a preview in pixels; the page shows it at half that for sharp HiDPI previews.
const THUMBNAIL_PX: u32 = 192;
/// Entries listed per folder in disk usage; the rest is summed up as one line.
const DISK_ENTRIES_SHOWN: usize = 500;
const NOT_IN_RESULTS: &str = "Not part of the last scan's results";

#[derive(Default)]
struct AppState {
    stop_flag: Arc<AtomicBool>,
    /// Paths of the last scan's results; trashing or previewing anything else is refused.
    last_results: Arc<Mutex<HashSet<PathBuf>>>,
    /// Tree of the last disk usage scan, browsed folder by folder.
    disk_tree: Arc<Mutex<Option<DiskTree>>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanRequest {
    included: Vec<PathBuf>,
    excluded: Vec<PathBuf>,
    tool: Tool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Tool {
    Duplicates { method: DuplicateMethod, minimal_file_size: u64 },
    BigFiles { count: usize, smallest: bool },
    EmptyFolders,
    EmptyFiles,
    SimilarImages { similarity: Similarity },
    DiskUsage,
}

/// The presets that differ at hash size 16; smaller ones all allow the maximum difference.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum Similarity {
    VeryHigh,
    High,
    Medium,
    Small,
}

impl From<Similarity> for SimilarityPreset {
    fn from(similarity: Similarity) -> Self {
        match similarity {
            Similarity::VeryHigh => Self::VeryHigh,
            Similarity::High => Self::High,
            Similarity::Medium => Self::Medium,
            Similarity::Small => Self::Small,
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum DuplicateMethod {
    Hash,
    Size,
    Name,
    SizeName,
}

impl From<DuplicateMethod> for CheckingMethod {
    fn from(method: DuplicateMethod) -> Self {
        match method {
            DuplicateMethod::Hash => Self::Hash,
            DuplicateMethod::Size => Self::Size,
            DuplicateMethod::Name => Self::Name,
            DuplicateMethod::SizeName => Self::SizeName,
        }
    }
}

/// One file or folder in a result, as the page shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    path: String,
    size: u64,
    modified_date: u64,
    /// Similar images only: resolution and how close the image is to the others in its group.
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<ImageInfo>,
}

#[derive(Serialize)]
struct ImageInfo {
    width: u32,
    height: u32,
    similarity: String,
}

impl Item {
    fn new(path: &Path, size: u64, modified_date: u64) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            size,
            modified_date,
            image: None,
        }
    }

    fn from_image(entry: &ImagesEntry) -> Self {
        Self {
            image: Some(ImageInfo {
                width: entry.width,
                height: entry.height,
                similarity: get_string_from_similarity(entry.difference, IMAGE_HASH_SIZE),
            }),
            ..Self::new(&entry.path, entry.size, entry.modified_date)
        }
    }
}

/// One folder of the disk usage tree, biggest entries first.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskFolder {
    /// From the first scanned folder down to this one; empty for the overview of all scanned folders.
    crumbs: Vec<DiskCrumb>,
    size: u64,
    entries: Vec<DiskEntry>,
    /// Entries beyond [`DISK_ENTRIES_SHOWN`], summed up.
    more_entries: usize,
    more_size: u64,
}

#[derive(Serialize)]
struct DiskCrumb {
    name: String,
    address: Vec<u32>,
}

#[derive(Serialize)]
struct DiskEntry {
    name: String,
    path: String,
    size: u64,
    kind: &'static str,
    category: &'static str,
    /// Entries directly inside a directory; 0 for anything else.
    items: usize,
    address: Vec<u32>,
}

#[derive(Serialize)]
struct ScanResult {
    /// Duplicates come in groups; every other tool returns a single group.
    groups: Vec<Vec<Item>>,
    warnings: Vec<String>,
    stopped: bool,
}

#[derive(Serialize, Clone)]
struct Progress {
    label: String,
    /// `None` while the stage has no meaningful percentage.
    percent: Option<i32>,
}

#[derive(Serialize)]
struct TrashFailure {
    path: String,
    error: String,
}

#[tauri::command]
async fn scan(app: AppHandle, state: State<'_, AppState>, request: ScanRequest) -> Result<ScanResult, String> {
    let stop_flag = Arc::clone(&state.stop_flag);
    let last_results = Arc::clone(&state.last_results);
    let disk_tree = Arc::clone(&state.disk_tree);
    stop_flag.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || run_scan(&app, request, &stop_flag, &disk_tree))
        .await
        .map_err(|e| e.to_string())?;
    let paths = result.groups.iter().flatten().map(|item| PathBuf::from(&item.path)).collect();
    *last_results.lock().map_err(|e| e.to_string())? = paths;
    Ok(result)
}

#[tauri::command]
#[expect(clippy::needless_pass_by_value, reason = "Tauri commands receive State by value")]
fn stop_scan(state: State<'_, AppState>) {
    state.stop_flag.store(true, Ordering::Relaxed);
}

/// Moves the given result paths to the recycle bin and returns the ones that failed.
#[tauri::command]
async fn move_to_trash(state: State<'_, AppState>, paths: Vec<PathBuf>) -> Result<Vec<TrashFailure>, String> {
    let last_results = Arc::clone(&state.last_results);
    tauri::async_runtime::spawn_blocking(move || {
        let known = last_results.lock().map_err(|e| e.to_string())?;
        Ok(paths.iter().filter_map(|path| trash_one(&known, path)).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn trash_one(known: &HashSet<PathBuf>, path: &Path) -> Option<TrashFailure> {
    let error = if known.contains(path) { trash_delete(path).err()? } else { NOT_IN_RESULTS.to_string() };
    Some(TrashFailure {
        path: path.to_string_lossy().into_owned(),
        error,
    })
}

/// PNG preview of an image from the last scan's results, sent as raw bytes.
#[tauri::command]
async fn thumbnail(state: State<'_, AppState>, path: PathBuf) -> Result<Response, String> {
    let last_results = Arc::clone(&state.last_results);
    tauri::async_runtime::spawn_blocking(move || {
        let known = last_results.lock().map_err(|e| e.to_string())?.contains(&path);
        thumbnail_png(known, &path).map(Response::new)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn thumbnail_png(known: bool, path: &Path) -> Result<Vec<u8>, String> {
    if !known {
        return Err(NOT_IN_RESULTS.to_string());
    }
    let resize = ImgResizeOptions {
        max_width: THUMBNAIL_PX,
        max_height: THUMBNAIL_PX,
        filter: FirFilterType::Bilinear,
    };
    let loaded = get_dynamic_image_from_path(&path.to_string_lossy(), Some(resize))?;
    // PNG cannot hold every pixel format the decoders produce (e.g. HDR floats), 8-bit RGBA it always can.
    let image = image::DynamicImage::from(loaded.image.to_rgba8());
    let mut png = Vec::new();
    image.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(png)
}

/// One folder of the last disk usage scan; an empty address lists the scanned folders themselves.
#[tauri::command]
#[expect(clippy::needless_pass_by_value, reason = "Tauri commands receive State by value")]
fn disk_folder(state: State<'_, AppState>, address: Vec<u32>) -> Result<DiskFolder, String> {
    let tree = state.disk_tree.lock().map_err(|e| e.to_string())?;
    let tree = tree.as_ref().ok_or("No disk usage scan yet")?;
    disk_folder_of(tree, &address).ok_or_else(|| "This folder is not in the last scan".to_string())
}

fn disk_folder_of(tree: &DiskTree, address: &[u32]) -> Option<DiskFolder> {
    let (children, size, path) = if address.is_empty() {
        (tree.roots.as_slice(), tree.total_size(), None)
    } else {
        let node = tree.node(address)?;
        (node.children.as_slice(), node.size, Some(tree.path(address)?))
    };
    let crumbs = (1..=address.len())
        .map(|len| {
            let prefix = address.get(..len)?;
            Some(DiskCrumb {
                name: tree.node(prefix)?.name.to_string(),
                address: prefix.to_vec(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let entries = children
        .iter()
        .zip(0_u32..)
        .take(DISK_ENTRIES_SHOWN)
        .map(|(child, index)| DiskEntry {
            name: child.name.to_string(),
            // Scanned folders are named by their full path, everything below by its own name.
            path: path
                .as_ref()
                .map_or_else(|| child.name.to_string(), |parent| parent.join(&*child.name).to_string_lossy().into_owned()),
            size: child.size,
            kind: match child.kind {
                NodeKind::File => "file",
                NodeKind::Directory => "directory",
                NodeKind::Symlink => "symlink",
                NodeKind::Inaccessible => "inaccessible",
            },
            category: child.category.name(),
            items: child.children.len(),
            address: address.iter().copied().chain([index]).collect(),
        })
        .collect();
    Some(DiskFolder {
        crumbs,
        size,
        entries,
        more_entries: children.len().saturating_sub(DISK_ENTRIES_SHOWN),
        more_size: children.iter().skip(DISK_ENTRIES_SHOWN).map(|child| child.size).sum(),
    })
}

fn run_scan(app: &AppHandle, request: ScanRequest, stop_flag: &Arc<AtomicBool>, disk_tree: &Mutex<Option<DiskTree>>) -> ScanResult {
    let ScanRequest { included, excluded, tool } = request;
    match tool {
        Tool::Duplicates { method, minimal_file_size } => {
            let params = DuplicateFinderParameters::new(method.into(), HashType::Blake3, false, MINIMAL_CACHED_FILE_SIZE, MINIMAL_CACHED_FILE_SIZE, false);
            let mut finder = DuplicateFinder::new(params);
            finder.set_minimal_file_size(minimal_file_size);
            search(&mut finder, included, excluded, app, stop_flag);
            let groups = duplicate_groups(&finder, method);
            finish(&finder, groups)
        }
        Tool::BigFiles { count, smallest } => {
            let mode = if smallest { SearchMode::SmallestFiles } else { SearchMode::BiggestFiles };
            let mut finder = BigFile::new(BigFileParameters::new(count, mode));
            search(&mut finder, included, excluded, app, stop_flag);
            let group = finder.get_big_files().iter().map(|e| Item::new(&e.path, e.size, e.modified_date)).collect();
            finish(&finder, vec![group])
        }
        Tool::EmptyFolders => {
            let mut finder = EmptyFolder::new();
            search(&mut finder, included, excluded, app, stop_flag);
            let group = finder.get_empty_folder_list().values().map(|e| Item::new(&e.path, 0, e.modified_date)).collect();
            finish(&finder, vec![group])
        }
        Tool::EmptyFiles => {
            let mut finder = EmptyFiles::new(EmptyFilesParameters::default());
            search(&mut finder, included, excluded, app, stop_flag);
            let group = finder.get_empty_files().iter().map(|e| Item::new(&e.path, e.size, e.modified_date)).collect();
            finish(&finder, vec![group])
        }
        Tool::SimilarImages { similarity } => {
            let max_difference = return_similarity_from_similarity_preset(similarity.into(), IMAGE_HASH_SIZE);
            let params = SimilarImagesParameters::new(
                max_difference,
                IMAGE_HASH_SIZE,
                HashAlg::Gradient,
                FilterType::Lanczos3,
                false,
                false,
                GeometricInvariance::Off,
            );
            let mut finder = SimilarImages::new(params);
            search(&mut finder, included, excluded, app, stop_flag);
            let groups = finder.get_similar_images().iter().map(|group| group.iter().map(Item::from_image).collect()).collect();
            finish(&finder, groups)
        }
        Tool::DiskUsage => {
            let mut finder = DiskUsage::new(DiskUsageParameters::new(ContentDetection::LargeFiles));
            search(&mut finder, included, excluded, app, stop_flag);
            match disk_tree.lock() {
                Ok(mut tree) => *tree = Some(mem::take(finder.get_tree_mut())),
                Err(e) => error!("Cannot keep the disk usage tree - {e}"),
            }
            finish(&finder, Vec::new())
        }
    }
}

/// Runs the scan, forwarding core progress to the page as `scan-progress` events.
fn search<T: CommonData + Search>(tool: &mut T, included: Vec<PathBuf>, excluded: Vec<PathBuf>, app: &AppHandle, stop_flag: &Arc<AtomicBool>) {
    tool.set_included_paths(included);
    tool.set_excluded_paths(excluded);

    let (sender, receiver) = unbounded::<ProgressData>();
    let app = app.clone();
    let forwarder = thread::spawn(move || {
        for progress in receiver {
            let display = progress.to_display();
            let percent = (display.all_progress >= 0).then_some(display.all_progress).or(display.current_progress);
            if let Err(e) = app.emit("scan-progress", Progress { label: display.label, percent }) {
                warn!("Cannot send scan progress to the page - {e}");
            }
        }
    });
    tool.search(stop_flag, Some(&sender));
    drop(sender);
    if forwarder.join().is_err() {
        error!("Progress forwarding thread panicked");
    }
}

/// Largest groups first, so the most space to win is at the top.
fn duplicate_groups(finder: &DuplicateFinder, method: DuplicateMethod) -> Vec<Vec<Item>> {
    let items = |group: &Vec<DuplicateEntry>| group.iter().map(|e| Item::new(&e.path, e.size, e.modified_date)).collect::<Vec<_>>();
    match method {
        DuplicateMethod::Hash => finder.get_files_sorted_by_hash().values().rev().flatten().map(items).collect(),
        DuplicateMethod::Size => finder.get_files_sorted_by_size().values().rev().map(items).collect(),
        DuplicateMethod::Name => finder.get_files_sorted_by_names().values().map(items).collect(),
        DuplicateMethod::SizeName => finder.get_files_sorted_by_size_name().values().rev().map(items).collect(),
    }
}

fn finish<T: CommonData>(tool: &T, groups: Vec<Vec<Item>>) -> ScanResult {
    let messages = tool.get_text_messages();
    let warnings = messages.critical.iter().chain(&messages.errors).chain(&messages.warnings).cloned().collect();
    ScanResult {
        groups,
        warnings,
        stopped: tool.get_stopped_search(),
    }
}

fn main() {
    let config_cache = set_config_cache_path("Plekke", "Plekke");
    setup_logger(false, "plekke", filtering_messages);
    print_version_mode("Plekke");
    print_infos_and_warnings(config_cache.infos, config_cache.warnings);
    register_image_decoding_hooks();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![scan, stop_scan, move_to_trash, thumbnail, disk_folder])
        .run(tauri::generate_context!())
        .expect("Tauri runtime failed to start; nothing to recover");
}

#[cfg(test)]
mod tests {
    use czkawka_core::tools::disk_usage::category::FileCategory;
    use czkawka_core::tools::disk_usage::tree::DiskNode;

    use super::*;

    #[test]
    fn parses_requests_as_the_page_sends_them() {
        let duplicates: ScanRequest =
            serde_json::from_str(r#"{"included":["C:\\a"],"excluded":[],"tool":{"kind":"duplicates","method":"sizeName","minimalFileSize":8000}}"#).unwrap();
        assert!(matches!(
            duplicates.tool,
            Tool::Duplicates {
                method: DuplicateMethod::SizeName,
                minimal_file_size: 8000
            }
        ));
        let big: ScanRequest = serde_json::from_str(r#"{"included":[],"excluded":[],"tool":{"kind":"bigFiles","count":50,"smallest":true}}"#).unwrap();
        assert!(matches!(big.tool, Tool::BigFiles { count: 50, smallest: true }));
        let empty: ScanRequest = serde_json::from_str(r#"{"included":[],"excluded":[],"tool":{"kind":"emptyFolders"}}"#).unwrap();
        assert!(matches!(empty.tool, Tool::EmptyFolders));
        let images: ScanRequest = serde_json::from_str(r#"{"included":[],"excluded":[],"tool":{"kind":"similarImages","similarity":"veryHigh"}}"#).unwrap();
        assert!(matches!(images.tool, Tool::SimilarImages { similarity: Similarity::VeryHigh }));
        let disk: ScanRequest = serde_json::from_str(r#"{"included":[],"excluded":[],"tool":{"kind":"diskUsage"}}"#).unwrap();
        assert!(matches!(disk.tool, Tool::DiskUsage));
    }

    #[test]
    fn previews_only_known_images_and_keeps_them_small() {
        let dir = std::env::temp_dir().join(format!("plekke_thumb_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("wide.png");
        image::RgbImage::new(800, 400).save(&file).unwrap();

        assert_eq!(thumbnail_png(false, &file).unwrap_err(), NOT_IN_RESULTS);
        let preview = image::load_from_memory(&thumbnail_png(true, &file).unwrap()).unwrap();
        assert_eq!((preview.width(), preview.height()), (THUMBNAIL_PX, THUMBNAIL_PX / 2));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lists_disk_folders_biggest_first_with_crumbs_and_paths() {
        let tree = DiskTree::new(vec![DiskNode::directory(
            "C:\\data",
            vec![
                DiskNode::file("small.txt", 10, FileCategory::Document),
                DiskNode::directory("photos", vec![DiskNode::file("a.jpg", 300, FileCategory::Image)]),
            ],
        )]);

        let overview = disk_folder_of(&tree, &[]).unwrap();
        assert!(overview.crumbs.is_empty());
        assert_eq!(overview.size, 310);
        assert_eq!(overview.entries.len(), 1);
        assert_eq!(overview.entries[0].path, "C:\\data");

        let root = disk_folder_of(&tree, &[0]).unwrap();
        let names: Vec<_> = root.entries.iter().map(|e| (e.name.as_str(), e.kind, e.items, e.address.clone())).collect();
        assert_eq!(names, [("photos", "directory", 1, vec![0, 0]), ("small.txt", "file", 0, vec![0, 1])]);
        assert_eq!(root.entries[0].path, Path::new("C:\\data").join("photos").to_string_lossy());

        let photos = disk_folder_of(&tree, &[0, 0]).unwrap();
        let crumbs: Vec<_> = photos.crumbs.iter().map(|c| (c.name.as_str(), c.address.clone())).collect();
        assert_eq!(crumbs, [("C:\\data", vec![0]), ("photos", vec![0, 0])]);
        assert_eq!(photos.entries[0].category, "Image");
        assert!(disk_folder_of(&tree, &[0, 5]).is_none());
    }

    #[test]
    fn refuses_to_trash_paths_outside_the_last_results() {
        let dir = std::env::temp_dir().join(format!("plekke_guard_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("keep.txt");
        std::fs::write(&file, "x").unwrap();

        let failure = trash_one(&HashSet::new(), &file).unwrap();
        assert_eq!(failure.error, "Not part of the last scan's results");
        assert!(file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
