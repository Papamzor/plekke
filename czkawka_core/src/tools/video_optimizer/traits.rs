use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use humansize::{DECIMAL, format_size};

use crate::common::consts::VIDEO_FILES_EXTENSIONS;
use crate::common::ffmpeg_utils::check_if_ffprobe_ffmpeg_exists;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::flc;
use crate::tools::video_optimizer::{Info, VideoOptimizer, VideoOptimizerParameters};

impl PrintResults for VideoOptimizer {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        match self.params.clone() {
            VideoOptimizerParameters::VideoTranscode(_) => {
                writeln!(writer)?;

                let total_entries = self.video_transcode_result_entries.len();
                let entries_needing_optimization = self.video_transcode_result_entries.iter().filter(|e| !e.codec.is_empty() && e.error.is_none()).count();
                let failed_entries = self.video_transcode_result_entries.iter().filter(|e| e.error.is_some()).count();

                writeln!(writer, "Total files found: {total_entries}")?;
                writeln!(writer, "Files needing optimization: {entries_needing_optimization}")?;
                writeln!(writer, "Failed to analyze: {failed_entries}")?;
                writeln!(writer)?;

                for entry in &self.video_transcode_result_entries {
                    if !entry.codec.is_empty() {
                        writeln!(
                            writer,
                            "\"{}\" - Codec: {} - Dimensions: {}x{} - Size: {}",
                            entry.path.to_string_lossy(),
                            entry.codec,
                            entry.width,
                            entry.height,
                            format_size(entry.size, DECIMAL)
                        )?;
                    }
                }
            }
            VideoOptimizerParameters::VideoCrop(_) => {
                writeln!(writer)?;

                let total_entries = self.video_crop_result_entries.len();
                let entries_with_crop_info = self.video_crop_result_entries.iter().filter(|e| !e.codec.is_empty() && e.error.is_none()).count();
                let failed_entries = self.video_crop_result_entries.iter().filter(|e| e.error.is_some()).count();

                writeln!(writer, "Total files found: {total_entries}")?;
                writeln!(writer, "Files with crop information: {entries_with_crop_info}")?;
                writeln!(writer, "Failed to analyze: {failed_entries}")?;
                writeln!(writer)?;

                for entry in &self.video_crop_result_entries {
                    if !entry.codec.is_empty() {
                        let (left, top, right, bottom) = entry.new_image_dimensions;
                        let new_image_dimensions = format!("  Crop: left={left}, top={top}, right={right}, bottom={bottom}");
                        writeln!(
                            writer,
                            "\"{}\" - Codec: {} - Dimensions: {}x{} - Size: {}{new_image_dimensions}",
                            entry.path.to_string_lossy(),
                            entry.codec,
                            entry.width,
                            entry.height,
                            format_size(entry.size, DECIMAL)
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        match &self.params {
            VideoOptimizerParameters::VideoTranscode(_) => self.save_results_to_file_as_json_internal(file_name, &self.video_transcode_result_entries, pretty_print),
            VideoOptimizerParameters::VideoCrop(_) => self.save_results_to_file_as_json_internal(file_name, &self.video_crop_result_entries, pretty_print),
        }
    }
}

impl Search for VideoOptimizer {
    #[fun_time(message = "scan_media_files", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if !check_if_ffprobe_ffmpeg_exists() {
                self.common_data.text_messages.critical = Some(flc!("core_ffmpeg_not_found"));
                #[cfg(target_os = "windows")]
                self.common_data.text_messages.errors.push(flc!("core_ffmpeg_not_found_windows"));
                return ControlFlow::Continue(());
            }

            if self.prepare_items(Some(VIDEO_FILES_EXTENSIONS)).is_err() {
                return ControlFlow::Continue(());
            }
            self.scan_files(stop_flag, progress_sender)?;
            self.check_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl_common_data! {
    VideoOptimizer, Info, VideoOptimizerParameters,
    fn found_any_items(&self) -> bool {
        match &self.params {
            VideoOptimizerParameters::VideoTranscode(_) => self.information.number_of_videos_to_transcode > 0,
            VideoOptimizerParameters::VideoCrop(_) => self.information.number_of_videos_to_crop > 0,
        }
    }
}
