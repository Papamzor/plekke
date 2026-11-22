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
use crate::flc;
use crate::tools::bad_names::{BadNames, BadNamesParameters, Info};

impl Search for BadNames {
    #[fun_time(message = "find_bad_names", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.params.checked_issues.is_empty() {
                self.common_data.text_messages.critical = Some(flc!("core_needs_to_set_at_least_one_bad_name_option"));
                return ControlFlow::Continue(());
            }

            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }

            self.check_files(stop_flag, progress_sender)?;
            self.look_for_bad_names_files(stop_flag, progress_sender)?;
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for BadNames {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        if !self.bad_names_files.is_empty() {
            writeln!(writer, "Found {} files with bad names.", self.information.number_of_files_with_bad_names)?;
            for file_entry in &self.bad_names_files {
                writeln!(writer, "\"{}\" -> \"{}\"", file_entry.path.to_string_lossy(), file_entry.new_name)?;
            }
        } else {
            write!(writer, "Not found any files with bad names.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.bad_names_files, pretty_print)
    }
}

impl BadNames {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.bad_names_files.clone()))
    }
}

impl_common_data! {
    BadNames, Info, BadNamesParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_files_with_bad_names > 0
    }
}
