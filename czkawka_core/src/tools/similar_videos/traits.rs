use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use humansize::{DECIMAL, format_size};

use crate::common::consts::VIDEO_FILES_EXTENSIONS;
use crate::common::ffmpeg_utils::ffmpeg_available;
use crate::common::model::CheckingMethod;
use crate::common::progress_data::ProgressData;
use crate::common::reference_groups::{GroupHeaders, delete_reference_groups, save_reference_groups_as_json, write_reference_groups};
use crate::common::tool_data::{CommonData, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::similar_videos::core::{format_bitrate_opt, format_duration_opt};
use crate::tools::similar_videos::{Info, SimilarVideos, SimilarVideosParameters};

impl Search for SimilarVideos {
    #[fun_time(message = "find_similar_videos", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            let check_audio = self.params.check_audio_content;
            // Only the visual mode needs ffmpeg for hashing
            if !check_audio && !ffmpeg_available(&mut self.common_data.text_messages) {
                return ControlFlow::Continue(());
            }
            if self.prepare_items(Some(VIDEO_FILES_EXTENSIONS)).is_err() {
                return ControlFlow::Continue(());
            }
            self.check_for_similar_videos(stop_flag, progress_sender)?;
            if check_audio {
                self.calculate_audio_fingerprints(stop_flag, progress_sender)?;
                self.compare_audio_fingerprints(stop_flag, progress_sender)?;
                self.create_thumbnails(progress_sender, stop_flag)?;
            } else {
                self.sort_videos(stop_flag, progress_sender)?;
            }
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl SimilarVideos {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        delete_reference_groups(&mut self.common_data, stop_flag, progress_sender, &self.similar_vectors, &self.similar_referenced_vectors)
    }
}

impl PrintResults for SimilarVideos {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        fn write_video_entry<T: Write>(writer: &mut T, file_entry: &crate::tools::similar_videos::VideosEntry) -> std::io::Result<()> {
            let bitrate = format_bitrate_opt(file_entry.bitrate);
            let fps = file_entry.fps.map(|e| format!("{e:.2}")).unwrap_or_default();
            let codec = file_entry.codec.clone().unwrap_or_default();
            let dimensions = if let (Some(w), Some(h)) = (file_entry.width, file_entry.height) {
                format!("{w}x{h}")
            } else {
                "".to_string()
            };
            let duration = format_duration_opt(file_entry.duration);

            writeln!(
                writer,
                "\"{}\" - {} - {} - {} - {} - {} - {}",
                file_entry.path.to_string_lossy(),
                format_size(file_entry.size, DECIMAL),
                bitrate,
                fps,
                codec,
                dimensions,
                duration
            )
        }

        let headers = GroupHeaders {
            plain_summary: " videos which have similar friends\n\n",
            referenced_summary: " videos which have similar friends (path, size, bitrate, fps, codec, dimensions, duration)\n\n",
            plain_group: " videos which have similar friends (path, size, bitrate, fps, codec, dimensions, duration)",
            referenced_group: " videos which have similar friends",
            not_found: "Not found any similar videos.",
        };
        write_reference_groups(writer, &self.similar_vectors, &self.similar_referenced_vectors, &headers, write_video_entry)
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        save_reference_groups_as_json(self, file_name, &self.similar_vectors, &self.similar_referenced_vectors, pretty_print)
    }
}

impl_common_data! {
    SimilarVideos, Info, SimilarVideosParameters,
    fn get_check_method(&self) -> CheckingMethod {
        if self.params.check_audio_content {
            CheckingMethod::VideoAudioContent
        } else {
            CheckingMethod::None
        }
    }
    fn found_any_items(&self) -> bool {
        self.information.number_of_duplicates > 0
    }
}
