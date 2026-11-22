//! Plekke, the desktop app: czkawka_core scans behind an mwp-ui page (see `ui/`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::unbounded;
use czkawka_core::common::config_cache_path::{print_infos_and_warnings, set_config_cache_path};
use czkawka_core::common::logger::{filtering_messages, print_version_mode, setup_logger};
use czkawka_core::common::model::{CheckingMethod, HashType};
use czkawka_core::common::progress_data::ProgressData;
use czkawka_core::common::tool_data::CommonData;
use czkawka_core::common::traits::Search;
use czkawka_core::common::trash_delete;
use czkawka_core::tools::big_file::{BigFile, BigFileParameters, SearchMode};
use czkawka_core::tools::duplicate::{DuplicateEntry, DuplicateFinder, DuplicateFinderParameters};
use czkawka_core::tools::empty_files::{EmptyFiles, EmptyFilesParameters};
use czkawka_core::tools::empty_folder::EmptyFolder;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

/// Smallest file kept in the duplicate hash cache, same default as the CLI.
const MINIMAL_CACHED_FILE_SIZE: u64 = 257_144;

#[derive(Default)]
struct AppState {
    stop_flag: Arc<AtomicBool>,
    /// Paths of the last scan's results; trashing anything else is refused.
    last_results: Arc<Mutex<HashSet<PathBuf>>>,
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
}

impl Item {
    fn new(path: &Path, size: u64, modified_date: u64) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            size,
            modified_date,
        }
    }
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
    stop_flag.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || run_scan(&app, request, &stop_flag))
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
    let error = if known.contains(path) {
        trash_delete(path).err()?
    } else {
        "Not part of the last scan's results".to_string()
    };
    Some(TrashFailure {
        path: path.to_string_lossy().into_owned(),
        error,
    })
}

fn run_scan(app: &AppHandle, request: ScanRequest, stop_flag: &Arc<AtomicBool>) -> ScanResult {
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![scan, stop_scan, move_to_trash])
        .run(tauri::generate_context!())
        .expect("Tauri runtime failed to start; nothing to recover");
}

#[cfg(test)]
mod tests {
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
