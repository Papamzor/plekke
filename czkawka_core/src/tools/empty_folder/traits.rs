use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use rayon::prelude::*;

use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::empty_folder::{EmptyFolder, Info};

impl Search for EmptyFolder {
    #[fun_time(message = "find_empty_folders", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }
            self.check_for_empty_folders(stop_flag, progress_sender)?;
            self.optimize_folders();

            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for EmptyFolder {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;
        if !self.empty_folder_list.is_empty() {
            writeln!(writer, "--------------------------Empty folder list--------------------------")?;
            writeln!(writer, "Found {} empty folders", self.information.number_of_empty_folders)?;
            let mut empty_folder_list = self.empty_folder_list.keys().collect::<Vec<_>>();
            empty_folder_list.par_sort_unstable();
            for name in empty_folder_list {
                writeln!(writer, "{name}")?;
            }
        } else {
            write!(writer, "Not found any empty folders.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.empty_folder_list.keys().collect::<Vec<_>>(), pretty_print)
    }
}

impl_common_data! {
    EmptyFolder, Info, (),
    fn found_any_items(&self) -> bool {
        self.information.number_of_empty_folders > 0
    }
}

impl EmptyFolder {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| {
            DeleteItemType::DeletingFolders(tool.empty_folder_list.values().cloned().collect())
        })
    }
}
