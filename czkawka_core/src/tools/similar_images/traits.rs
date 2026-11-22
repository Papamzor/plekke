use std::io::Write;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use humansize::{DECIMAL, format_size};

use crate::common::consts::{HEIC_EXTENSIONS, IMAGE_RS_SIMILAR_IMAGES_EXTENSIONS, RAW_IMAGE_EXTENSIONS};
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::{CommonData, DeleteItemType, DeleteMethod, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::tools::similar_images::core::get_string_from_similarity;
use crate::tools::similar_images::{Info, SimilarImages, SimilarImagesParameters};

impl Search for SimilarImages {
    #[fun_time(message = "find_similar_images", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            let extensions = [IMAGE_RS_SIMILAR_IMAGES_EXTENSIONS, RAW_IMAGE_EXTENSIONS, HEIC_EXTENSIONS].concat();

            if self.prepare_items(Some(&extensions)).is_err() {
                return ControlFlow::Continue(());
            }
            self.common_data.use_reference_folders = !self.common_data.directories.reference_directories.is_empty() || !self.common_data.directories.reference_files.is_empty();
            self.check_for_similar_images(stop_flag, progress_sender)?;
            self.hash_images(stop_flag, progress_sender)?;
            self.find_similar_hashes(stop_flag, progress_sender)?;
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for SimilarImages {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;
        if !self.similar_vectors.is_empty() {
            write!(writer, "{} images which have similar friends\n\n", self.similar_vectors.len())?;

            for struct_similar in &self.similar_vectors {
                writeln!(writer, "Found {} images which have similar friends", struct_similar.len())?;
                for file_entry in struct_similar {
                    writeln!(
                        writer,
                        "\"{}\" - {}x{} - {} - {}",
                        file_entry.path.to_string_lossy(),
                        file_entry.width,
                        file_entry.height,
                        format_size(file_entry.size, DECIMAL),
                        get_string_from_similarity(file_entry.difference, self.get_params().hash_size)
                    )?;
                }
                writeln!(writer)?;
            }
        } else if !self.similar_referenced_vectors.is_empty() {
            writeln!(writer, "{} images which have similar friends\n\n", self.similar_referenced_vectors.len())?;

            for (file_entry, vec_file_entry) in &self.similar_referenced_vectors {
                writeln!(writer, "Found {} images which have similar friends", vec_file_entry.len())?;
                writeln!(writer)?;
                writeln!(
                    writer,
                    "\"{}\" - {}x{} - {} - {}",
                    file_entry.path.to_string_lossy(),
                    file_entry.width,
                    file_entry.height,
                    format_size(file_entry.size, DECIMAL),
                    get_string_from_similarity(file_entry.difference, self.get_params().hash_size)
                )?;
                for file_entry in vec_file_entry {
                    writeln!(
                        writer,
                        "\"{}\" - {}x{} - {} - {}",
                        file_entry.path.to_string_lossy(),
                        file_entry.width,
                        file_entry.height,
                        format_size(file_entry.size, DECIMAL),
                        get_string_from_similarity(file_entry.difference, self.get_params().hash_size)
                    )?;
                }
                writeln!(writer)?;
            }
        } else {
            write!(writer, "Not found any similar images.")?;
        }

        Ok(())
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        if self.get_use_reference_folders() {
            self.save_results_to_file_as_json_internal(file_name, &self.similar_referenced_vectors, pretty_print)
        } else {
            self.save_results_to_file_as_json_internal(file_name, &self.similar_vectors, pretty_print)
        }
    }
}
impl_common_data! {
    SimilarImages, Info, SimilarImagesParameters,
    fn found_any_items(&self) -> bool {
        self.information.number_of_duplicates > 0
    }
}

impl SimilarImages {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        if self.get_cd().delete_method == DeleteMethod::None {
            return ControlFlow::Continue(());
        }
        if self.get_use_reference_folders() {
            let files_to_delete: Vec<_> = self.similar_referenced_vectors.iter().flat_map(|(_, files)| files.iter().cloned()).collect();
            return self.delete_simple_elements_and_add_to_messages(stop_flag, progress_sender, DeleteItemType::DeletingFiles(files_to_delete));
        }
        let files_to_delete = self.similar_vectors.clone();
        self.delete_advanced_elements_and_add_to_messages(stop_flag, progress_sender, files_to_delete)
    }
}
