use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;

use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::bad_extensions::{BadExtensions, BadExtensionsParameters, Info};

impl Search for BadExtensions {
    #[fun_time(message = "find_bad_extensions_files", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }
            self.check_files(stop_flag, progress_sender)?;
            self.look_for_bad_extensions_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl BadExtensions {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.bad_extensions_files.clone()))
    }
}

impl PrintResults for BadExtensions {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;
        writeln!(writer, "Found {} files with invalid extension.\n", self.information.number_of_files_with_bad_extension)?;

        for file_entry in &self.bad_extensions_files {
            writeln!(writer, "\"{}\" ----- {}", file_entry.path.to_string_lossy(), file_entry.proper_extensions_group)?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.bad_extensions_files, pretty_print)
    }
}

impl_common_data! {
    BadExtensions, Info, BadExtensionsParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_files_with_bad_extension > 0
    }
}
