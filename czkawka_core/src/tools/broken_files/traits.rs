use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;

use crate::common::ffmpeg_utils::ffmpeg_available;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::flc;
use crate::tools::broken_files::core::EXTENSIONS_BY_TYPE;
use crate::tools::broken_files::{BrokenFiles, BrokenFilesParameters, CheckedTypes, Info};

impl Search for BrokenFiles {
    #[fun_time(message = "find_broken_files", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            let video_types = CheckedTypes::VIDEO_FFPROBE | CheckedTypes::VIDEO_FFMPEG;
            if self.params.checked_types.intersects(video_types) && !ffmpeg_available(&mut self.common_data.text_messages) {
                return ControlFlow::Continue(());
            }

            let checked_types = self.params.checked_types;
            let extensions: Vec<&str> = EXTENSIONS_BY_TYPE
                .iter()
                .filter(|(_, required, _)| checked_types.intersects(*required))
                .flat_map(|(_, _, extensions)| extensions.iter().copied())
                .collect();

            if extensions.is_empty() {
                self.common_data.text_messages.critical = Some(flc!("core_needs_to_set_at_least_one_broken_option"));
                return ControlFlow::Continue(());
            }

            if self.prepare_items(Some(&extensions)).is_err() {
                return ControlFlow::Continue(());
            }
            self.check_files(stop_flag, progress_sender)?;
            self.look_for_broken_files(stop_flag, progress_sender)?;
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for BrokenFiles {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        if !self.broken_files.is_empty() {
            writeln!(writer, "Found {} broken files.", self.information.number_of_broken_files)?;
            for file_entry in &self.broken_files {
                writeln!(writer, "\"{}\" - {}", file_entry.path.to_string_lossy(), file_entry.get_error_string())?;
            }
        } else {
            write!(writer, "Not found any broken files.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.broken_files, pretty_print)
    }
}
impl BrokenFiles {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.broken_files.clone()))
    }
}

impl_common_data! {
    BrokenFiles, Info, BrokenFilesParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_broken_files > 0
    }
}
