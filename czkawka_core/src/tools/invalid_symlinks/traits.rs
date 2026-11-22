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
use crate::tools::invalid_symlinks::{ErrorType, Info, InvalidSymlinks};

impl Search for InvalidSymlinks {
    #[fun_time(message = "find_invalid_links", level = "info")]
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

impl PrintResults for InvalidSymlinks {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;
        if !self.invalid_symlinks.is_empty() {
            writeln!(writer, "Found {} invalid symlinks.", self.information.number_of_invalid_symlinks)?;
            for file_entry in &self.invalid_symlinks {
                writeln!(
                    writer,
                    "\"{}\"\t\t\"{}\"\t\t{}",
                    file_entry.path.to_string_lossy(),
                    file_entry.symlink_info.destination_path.to_string_lossy(),
                    match file_entry.symlink_info.type_of_error {
                        ErrorType::InfiniteRecursion => "Infinite Recursion",
                        ErrorType::NonExistentFile => "Non Existent File",
                    }
                )?;
            }
        } else {
            write!(writer, "Not found any invalid symlinks.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.invalid_symlinks, pretty_print)
    }
}

impl_common_data! {
    InvalidSymlinks, Info, (),
    fn found_any_items(&self) -> bool {
        self.information.number_of_invalid_symlinks > 0
    }
}
impl InvalidSymlinks {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.invalid_symlinks.clone()))
    }
}
