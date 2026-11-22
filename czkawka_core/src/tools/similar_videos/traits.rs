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
use crate::common::model::CheckingMethod;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, DeleteMethod, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::flc;
use crate::tools::similar_videos::core::{format_bitrate_opt, format_duration_opt};
use crate::tools::similar_videos::{Info, SimilarVideos, SimilarVideosParameters};

impl Search for SimilarVideos {
    #[fun_time(message = "find_similar_videos", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.params.check_audio_content {
                // Audio fingerprint mode - no need for ffmpeg
                if self.prepare_items(Some(VIDEO_FILES_EXTENSIONS)).is_err() {
                    return ControlFlow::Continue(());
                }
                self.common_data.use_reference_folders = !self.common_data.directories.reference_directories.is_empty() || !self.common_data.directories.reference_files.is_empty();
                self.check_for_similar_videos(stop_flag, progress_sender)?;
                self.calculate_audio_fingerprints(stop_flag, progress_sender)?;
                self.compare_audio_fingerprints(stop_flag, progress_sender)?;
                self.create_thumbnails(progress_sender, stop_flag)?;
            } else {
                // Visual similarity mode (existing behaviour, requires ffmpeg)
                if !check_if_ffprobe_ffmpeg_exists() {
                    self.common_data.text_messages.critical = Some(flc!("core_ffmpeg_not_found"));
                    #[cfg(target_os = "windows")]
                    self.common_data.text_messages.errors.push(flc!("core_ffmpeg_not_found_windows"));
                    return ControlFlow::Continue(());
                }

                if self.prepare_items(Some(VIDEO_FILES_EXTENSIONS)).is_err() {
                    return ControlFlow::Continue(());
                }
                self.common_data.use_reference_folders = !self.common_data.directories.reference_directories.is_empty() || !self.common_data.directories.reference_files.is_empty();
                self.check_for_similar_videos(stop_flag, progress_sender)?;
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
        if self.get_cd().delete_method == DeleteMethod::None {
            return ControlFlow::Continue(());
        }
        if self.get_use_reference_folders() {
            let files_to_delete: Vec<_> = self.similar_referenced_vectors.iter().flat_map(|(_, files)| files.iter().cloned()).collect();
            return self.delete_simple_elements_and_add_to_messages(stop_flag, progress_sender, DeleteItemType::DeletingFiles(files_to_delete));
        }
        let files_to_delete = self.similar_vectors.clone();
        self.delete_advanced_elements_and_add_to_messages(stop_flag, progress_sender, files_to_delete)
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

        if !self.similar_vectors.is_empty() {
            write!(writer, "{} videos which have similar friends\n\n", self.similar_vectors.len())?;

            for struct_similar in &self.similar_vectors {
                writeln!(
                    writer,
                    "Found {} videos which have similar friends (path, size, bitrate, fps, codec, dimensions, duration)",
                    struct_similar.len()
                )?;
                for file_entry in struct_similar {
                    write_video_entry(writer, file_entry)?;
                }
                writeln!(writer)?;
            }
        } else if !self.similar_referenced_vectors.is_empty() {
            write!(
                writer,
                "{} videos which have similar friends (path, size, bitrate, fps, codec, dimensions, duration)\n\n",
                self.similar_referenced_vectors.len()
            )?;

            for (fe, struct_similar) in &self.similar_referenced_vectors {
                writeln!(writer, "Found {} videos which have similar friends", struct_similar.len())?;
                writeln!(writer)?;
                write_video_entry(writer, fe)?;
                for file_entry in struct_similar {
                    write_video_entry(writer, file_entry)?;
                }
                writeln!(writer)?;
            }
        } else {
            write!(writer, "Not found any similar videos.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        if self.get_use_reference_folders() {
            self.save_results_to_file_as_json_internal(file_name, &self.similar_referenced_vectors, pretty_print)
        } else {
            self.save_results_to_file_as_json_internal(file_name, &self.similar_vectors, pretty_print)
        }
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
