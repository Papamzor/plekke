use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;

use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::disk_usage::report::{build_report, write_report};
use crate::tools::disk_usage::{DiskUsage, DiskUsageParameters, Info};

impl PrintResults for DiskUsage {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        if self.found_any_items() {
            writeln!(
                writer,
                "{} files in {} directories.\n",
                self.information.number_of_files, self.information.number_of_directories
            )?;
            write_report(writer, &build_report(&self.tree, self.params.report_depth, self.params.report_entries_per_directory))
        } else {
            writeln!(writer, "Not found any files.")
        }
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        let report = build_report(&self.tree, self.params.report_depth, self.params.report_entries_per_directory);
        self.save_results_to_file_as_json_internal(file_name, &report, pretty_print)
    }
}

impl Search for DiskUsage {
    #[fun_time(message = "find_disk_usage", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(None).is_err() {
                return ControlFlow::Continue(());
            }
            self.scan_disk_usage(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl_common_data! {
    DiskUsage, Info, DiskUsageParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_files > 0
    }
}
