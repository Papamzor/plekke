use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use serde::Serialize;

use crate::common::progress_data::ProgressData;
use crate::common::tool_data::CommonData;

pub trait PrintResults: CommonData {
    fn write_base_search_paths<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        let dirs = &self.get_cd().directories;
        let included_paths = dirs.included_files.iter().chain(dirs.included_directories.iter()).collect::<Vec<_>>();
        let excluded_paths = dirs.excluded_files.iter().chain(dirs.excluded_directories.iter()).collect::<Vec<_>>();
        let reference_paths = dirs.reference_files.iter().chain(dirs.reference_directories.iter()).collect::<Vec<_>>();
        let excluded_items = self.get_cd().excluded_items.get_excluded_items();
        if self.get_cd().tool_type.may_use_reference_paths() {
            writeln!(
                writer,
                "Results of searching {included_paths:?} with reference paths {reference_paths:?}, excluded paths {excluded_paths:?} and excluded items {excluded_items:?}"
            )?;
            writeln!(
                writer,
                "(Before optimizations - included paths: {:?}, excluded paths: {:?}, reference paths: {:?})",
                dirs.original_included_paths, dirs.original_excluded_paths, dirs.original_reference_paths
            )?;
        } else {
            writeln!(
                writer,
                "Results of searching {included_paths:?} with excluded paths {excluded_paths:?} and excluded items {excluded_items:?}"
            )?;
            writeln!(
                writer,
                "(Before optimizations - included paths: {:?}, excluded paths: {:?})",
                dirs.original_included_paths, dirs.original_excluded_paths
            )?;
        }

        Ok(())
    }

    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()>;

    #[fun_time(message = "print_results_to_file", level = "debug")]
    fn print_results_to_file(&self, file_name: &str) -> std::io::Result<()> {
        let file_name: String = match file_name {
            "" => "results.txt".to_string(),
            k => k.to_string(),
        };

        let file_handler = File::create(file_name)?;
        let mut writer = BufWriter::new(file_handler);
        self.write_results(&mut writer)?;
        writer.flush()?;
        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()>;

    #[fun_time(message = "save_results_to_file_as_json", level = "debug")]
    fn save_results_to_file_as_json_internal<T: Serialize + std::fmt::Debug>(&self, file_name: &str, item_to_serialize: &T, pretty_print: bool) -> std::io::Result<()> {
        let mut writer = BufWriter::new(File::create(file_name)?);
        if pretty_print {
            serde_json::to_writer_pretty(&mut writer, item_to_serialize)?;
        } else {
            serde_json::to_writer(&mut writer, item_to_serialize)?;
        }
        Ok(())
    }
}

pub trait ResultEntry {
    fn get_path(&self) -> &Path;
    fn get_modified_date(&self) -> u64;
    fn get_size(&self) -> u64;
}

/// Implements [`ResultEntry`] for entries with `path`, `modified_date` and `size` fields.
macro_rules! impl_result_entry {
    ($($entry:ty),+ $(,)?) => {
        $(
            impl $crate::common::traits::ResultEntry for $entry {
                fn get_path(&self) -> &std::path::Path {
                    &self.path
                }
                fn get_modified_date(&self) -> u64 {
                    self.modified_date
                }
                fn get_size(&self) -> u64 {
                    self.size
                }
            }
        )+
    };
}
pub(crate) use impl_result_entry;

pub trait Search {
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>);
}
