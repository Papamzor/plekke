use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use humansize::{DECIMAL, format_size};

use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::big_file::{BigFile, BigFileParameters, Info, SearchMode};

impl BigFile {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.big_files.clone()))
    }
}

impl PrintResults for BigFile {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        if self.information.number_of_real_files != 0 {
            if self.get_params().search_mode == SearchMode::BiggestFiles {
                writeln!(writer, "{} the biggest files.\n\n", self.information.number_of_real_files)?;
            } else {
                writeln!(writer, "{} the smallest files.\n\n", self.information.number_of_real_files)?;
            }
            for file_entry in &self.big_files {
                writeln!(
                    writer,
                    "{} ({}) - \"{}\"",
                    format_size(file_entry.size, DECIMAL),
                    file_entry.size,
                    file_entry.path.to_string_lossy()
                )?;
            }
        } else {
            writeln!(writer, "Not found any files.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.big_files, pretty_print)
    }
}

impl Search for BigFile {
    #[fun_time(message = "find_big_files", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }
            self.look_for_big_files(stop_flag, progress_sender)?;
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl_common_data! {
    BigFile, Info, BigFileParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_real_files > 0
    }
}
