use std::collections::BTreeMap;
use std::mem;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use log::info;
use rayon::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::common::cache::{load_and_split_cache_generalized_by_path, save_and_connect_cache_generalized_by_path};
use crate::common::model::ToolType;
use crate::common::progress_data::{ProgressData, ToolStage, VideoOptimizerStage};
use crate::common::progress_stop_handler::{check_if_stop_received, prepare_thread_handler_common};
use crate::common::tool_data::CommonToolData;
use crate::common::video_utils::{ThumbnailEntry, generate_thumbnails};
use crate::tools::video_optimizer::{
    Info, VideoCropEntry, VideoCropParams, VideoCropSingleFixParams, VideoOptimizer, VideoOptimizerFixParams, VideoOptimizerParameters, VideoTranscodeEntry,
};

mod video_converter;
mod video_cropper;

pub use video_converter::process_video;
pub use video_cropper::fix_video_crop;

use crate::common::cache::CACHE_VIDEO_OPTIMIZE_VERSION;
use crate::common::traits::ResultEntry;
use crate::flc;

/// Entry checked by one of the optimizer modes.
trait CheckedVideo: ThumbnailEntry + Serialize + DeserializeOwned + Clone + Sync {
    fn error(&self) -> Option<&String>;
}

macro_rules! impl_checked_video {
    ($($entry:ty),+) => {$(
        impl ThumbnailEntry for $entry {
            fn duration(&self) -> Option<f64> {
                Some(self.duration)
            }
            fn set_thumbnail_path(&mut self, thumbnail_path: PathBuf) {
                self.thumbnail_path = Some(thumbnail_path);
            }
        }
        impl CheckedVideo for $entry {
            fn error(&self) -> Option<&String> {
                self.error.as_ref()
            }
        }
    )+};
}
impl_checked_video!(VideoTranscodeEntry, VideoCropEntry);

impl VideoOptimizer {
    pub fn new(params: VideoOptimizerParameters) -> Self {
        Self {
            common_data: CommonToolData::new(ToolType::VideoOptimizer),
            information: Info::default(),
            video_transcode_test_entries: Default::default(),
            video_crop_test_entries: Default::default(),
            video_transcode_result_entries: Vec::new(),
            video_crop_result_entries: Vec::new(),
            params,
        }
    }

    #[fun_time(message = "scan_files", level = "debug")]
    pub(crate) fn scan_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        let files = self.common_data.collect_files(stop_flag, progress_sender, |_| {})?;
        match &self.params {
            VideoOptimizerParameters::VideoTranscode(_) => {
                self.video_transcode_test_entries = files
                    .into_iter()
                    .map(|fe| (fe.get_path().to_string_lossy().to_string(), fe.into_video_transcode_entry()))
                    .collect();
                info!("Found {} files to check", self.video_transcode_test_entries.len());
            }
            VideoOptimizerParameters::VideoCrop(_) => {
                self.video_crop_test_entries = files
                    .into_iter()
                    .map(|fe| (fe.get_path().to_string_lossy().to_string(), fe.into_video_crop_entry()))
                    .collect();
                info!("Found {} files to check", self.video_crop_test_entries.len());
            }
        }
        ControlFlow::Continue(())
    }

    #[fun_time(message = "check_files", level = "debug")]
    pub(crate) fn check_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        let thumbnail_stage = ToolStage::VideoOptimizer(VideoOptimizerStage::CreatingThumbnails);
        let thumbnail_settings = self.params.thumbnail_settings();
        let cd = &mut self.common_data;
        match self.params.clone() {
            VideoOptimizerParameters::VideoTranscode(params) => {
                let mut entries = check_videos(
                    cd,
                    mem::take(&mut self.video_transcode_test_entries),
                    &get_video_transcode_cache_file(),
                    stop_flag,
                    progress_sender,
                    |entry| Some(video_converter::check_video(entry)),
                );
                entries.retain(|e| e.error.is_none() && !params.excluded_codecs.contains(&e.codec));
                self.information.number_of_videos_to_transcode = entries.len();
                self.video_transcode_result_entries = entries;
                let entries = self.video_transcode_result_entries.iter_mut().collect();
                generate_thumbnails(entries, thumbnail_settings, thumbnail_stage, progress_sender, stop_flag, &mut cd.text_messages.warnings)
            }
            VideoOptimizerParameters::VideoCrop(params) => {
                let mut entries = check_videos(
                    cd,
                    mem::take(&mut self.video_crop_test_entries),
                    &get_video_crop_cache_file(&params),
                    stop_flag,
                    progress_sender,
                    |entry| video_cropper::check_video_crop(entry, &params, stop_flag),
                );
                entries.retain(|e| e.error.is_none() && e.new_image_dimensions != (0, 0, 0, 0));
                self.information.number_of_videos_to_crop = entries.len();
                self.video_crop_result_entries = entries;
                let entries = self.video_crop_result_entries.iter_mut().collect();
                generate_thumbnails(entries, thumbnail_settings, thumbnail_stage, progress_sender, stop_flag, &mut cd.text_messages.warnings)
            }
        }
    }

    #[fun_time(message = "fix_files", level = "debug")]
    pub fn fix_files(&mut self, stop_flag: &Arc<AtomicBool>, fix_params: VideoOptimizerFixParams) {
        match self.params.clone() {
            VideoOptimizerParameters::VideoTranscode(_) => {
                let VideoOptimizerFixParams::VideoTranscode(video_transcode_params) = fix_params else {
                    unreachable!("VideoTranscode mode should have VideoTranscode fix_params(caller is responsible for that)");
                };

                let transcode_warnings: Vec<_> = mem::take(&mut self.video_transcode_result_entries)
                    .into_par_iter()
                    .map(|entry| {
                        if check_if_stop_received(stop_flag) {
                            return None;
                        }

                        match process_video(stop_flag, &entry.path.to_string_lossy(), entry.size, &video_transcode_params) {
                            Ok(_new_size) => Some(None),
                            Err(e) => Some(Some(flc!("core_failed_to_optimize_video", file = entry.path.to_string_lossy(), reason = e))),
                        }
                    })
                    .while_some()
                    .flatten()
                    .collect();

                self.common_data.text_messages.warnings.extend(transcode_warnings);
            }
            VideoOptimizerParameters::VideoCrop(_) => {
                let VideoOptimizerFixParams::VideoCrop(video_crop_params) = fix_params else {
                    unreachable!("VideoCrop mode should have VideoCrop fix_params(caller is responsible for that)");
                };

                let crop_warnings: Vec<_> = mem::take(&mut self.video_crop_result_entries)
                    .into_par_iter()
                    .map(|entry| {
                        if check_if_stop_received(stop_flag) {
                            return None;
                        }

                        let (left, top, right, bottom) = entry.new_image_dimensions;
                        let entry_crop_params = VideoCropSingleFixParams {
                            overwrite_original: video_crop_params.overwrite_original,
                            target_codec: video_crop_params.target_codec,
                            quality: video_crop_params.quality,
                            crop_rectangle: (left, top, right, bottom),
                            crop_mechanism: video_crop_params.crop_mechanism,
                        };

                        match fix_video_crop(&entry.path, &entry_crop_params, stop_flag, &entry.codec) {
                            Ok(()) => Some(None),
                            Err(e) => Some(Some(flc!("core_failed_to_crop_video", file = entry.path.to_string_lossy(), reason = e))),
                        }
                    })
                    .while_some()
                    .flatten()
                    .collect();

                self.common_data.text_messages.warnings.extend(crop_warnings);
            }
        }
    }
}

/// Runs `check` (cache-aware, in parallel) over `entries`; check errors become warnings.
#[fun_time(message = "check_videos", level = "debug")]
fn check_videos<T: CheckedVideo>(
    common_data: &mut CommonToolData,
    entries: BTreeMap<String, T>,
    cache_file: &str,
    stop_flag: &Arc<AtomicBool>,
    progress_sender: Option<&Sender<ProgressData>>,
    check: impl Fn(T) -> Option<T> + Sync,
) -> Vec<T> {
    if entries.is_empty() {
        return Vec::new();
    }
    let (loaded_hash_map, records_already_cached, non_cached_files_to_check) = load_and_split_cache_generalized_by_path(cache_file, entries, common_data);

    let progress_handler = prepare_thread_handler_common(
        progress_sender,
        ToolStage::VideoOptimizer(VideoOptimizerStage::ProcessingVideos),
        non_cached_files_to_check.len(),
        non_cached_files_to_check.values().map(ResultEntry::get_size).sum(),
    );

    let mut checked: Vec<T> = non_cached_files_to_check
        .into_par_iter()
        .map(|(_path, entry)| {
            if check_if_stop_received(stop_flag) {
                return None;
            }
            let size = entry.get_size();
            let res = check(entry);
            progress_handler.increase_items(1);
            progress_handler.increase_size(size);
            res
        })
        .while_some()
        .collect();

    common_data.text_messages.warnings.extend(checked.iter().filter_map(CheckedVideo::error).cloned());
    checked.extend(records_already_cached.into_values());

    progress_handler.join_thread();

    save_and_connect_cache_generalized_by_path(cache_file, &checked, loaded_hash_map, common_data);
    checked
}

pub fn get_video_transcode_cache_file() -> String {
    format!("cache_video_transcode_{CACHE_VIDEO_OPTIMIZE_VERSION}.bin")
}

pub fn get_video_crop_cache_file(params: &VideoCropParams) -> String {
    format!(
        "cache_video_crop_{CACHE_VIDEO_OPTIMIZE_VERSION}_{:?}_t{}_p{}_s{}_c{}.bin",
        params.crop_detect, params.black_pixel_threshold, params.black_bar_min_percentage, params.max_samples, params.min_crop_size
    )
}
