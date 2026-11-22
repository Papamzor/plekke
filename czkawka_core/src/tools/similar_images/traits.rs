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
use crate::common::reference_groups::{GroupHeaders, delete_reference_groups, save_reference_groups_as_json, write_reference_groups};
use crate::common::tool_data::{CommonData, impl_common_data};
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
        let headers = GroupHeaders {
            plain_summary: " images which have similar friends\n\n",
            referenced_summary: " images which have similar friends\n\n\n",
            plain_group: " images which have similar friends",
            referenced_group: " images which have similar friends",
            not_found: "Not found any similar images.",
        };
        let hash_size = self.get_params().hash_size;
        write_reference_groups(writer, &self.similar_vectors, &self.similar_referenced_vectors, &headers, |writer, file_entry| {
            writeln!(
                writer,
                "\"{}\" - {}x{} - {} - {}",
                file_entry.path.to_string_lossy(),
                file_entry.width,
                file_entry.height,
                format_size(file_entry.size, DECIMAL),
                get_string_from_similarity(file_entry.difference, hash_size)
            )
        })
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        save_reference_groups_as_json(self, file_name, &self.similar_vectors, &self.similar_referenced_vectors, pretty_print)
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
        delete_reference_groups(&mut self.common_data, stop_flag, progress_sender, &self.similar_vectors, &self.similar_referenced_vectors)
    }
}
