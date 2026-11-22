use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use humansize::DECIMAL;

use crate::common::consts::EXIF_FILES_EXTENSIONS;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::exif_remover::{ExifRemover, ExifRemoverParameters, Info};

impl ExifRemover {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.delete_if_requested(stop_flag, progress_sender, |tool| DeleteItemType::DeletingFiles(tool.exif_files.clone()))
    }
}

impl PrintResults for ExifRemover {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;

        if self.information.number_of_files_with_exif != 0 {
            writeln!(writer, "Found {} files with EXIF data.\n", self.information.number_of_files_with_exif)?;

            for exif_entry in &self.exif_files {
                writeln!(
                    writer,
                    "\nFile: \"{}\" - {} - {} - {:?}",
                    exif_entry.path.to_string_lossy(),
                    humansize::format_size(exif_entry.size, DECIMAL),
                    exif_entry.modified_date,
                    exif_entry.exif_tags.iter().map(|item_tag| item_tag.name.clone()).collect::<Vec<_>>()
                )?;
            }
        } else {
            writeln!(writer, "Not found any files with EXIF data.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        self.save_results_to_file_as_json_internal(file_name, &self.exif_files, pretty_print)
    }
}

impl Search for ExifRemover {
    #[fun_time(message = "find_exif_data", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            if self.prepare_items(Some(EXIF_FILES_EXTENSIONS)).is_err() {
                return ControlFlow::Continue(());
            }
            self.find_exif_files(stop_flag, progress_sender)?;

            self.check_exif_in_files(stop_flag, progress_sender)?;

            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl_common_data! {
    ExifRemover, Info, ExifRemoverParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_files_with_exif > 0
    }
}
