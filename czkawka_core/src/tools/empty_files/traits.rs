use std::io::prelude::*;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;

use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::empty_files::{EmptyFiles, EmptyFilesParameters, Info};

impl Search for EmptyFiles {
    #[fun_time(message = "find_empty_files", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }
            self.check_files(stop_flag, progress_sender)?;
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for EmptyFiles {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        if !self.empty_files.is_empty() {
            writeln!(writer, "Found {} empty files.", self.information.number_of_empty_files)?;
            for file_entry in &self.empty_files {
                writeln!(writer, "\"{}\"", file_entry.path.to_string_lossy())?;
            }
        } else {
            write!(writer, "Not found any empty files.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.empty_files, pretty_print)
    }
}
impl_common_data! {
    EmptyFiles, Info, EmptyFilesParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_empty_files > 0
    }
}
impl EmptyFiles {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.empty_files.clone()))
    }
}
