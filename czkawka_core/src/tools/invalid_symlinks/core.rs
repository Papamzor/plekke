use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use fun_time::fun_time;
use log::debug;

use crate::common::dir_traversal::Collect;
use crate::common::model::ToolType;
use crate::common::progress_data::ProgressData;
use crate::common::tool_data::CommonToolData;
use crate::tools::invalid_symlinks::{ErrorType, Info, InvalidSymlinks, MAX_NUMBER_OF_SYMLINK_JUMPS, SymlinkInfo};

impl InvalidSymlinks {
    pub fn new() -> Self {
        Self {
            common_data: CommonToolData::new(ToolType::InvalidSymlinks),
            information: Info::default(),
            invalid_symlinks: Vec::new(),
        }
    }

    #[fun_time(message = "check_files", level = "debug")]
    pub(crate) fn check_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.invalid_symlinks = self
            .common_data
            .collect_files(stop_flag, progress_sender, |traversal| traversal.collect = Collect::InvalidSymlinks)?
            .into_iter()
            .filter_map(|e| {
                let (destination_path, type_of_error) = Self::check_invalid_symlinks(&e.path)?;
                Some(e.into_symlinks_entry(SymlinkInfo { destination_path, type_of_error }))
            })
            .collect();
        self.information.number_of_invalid_symlinks = self.invalid_symlinks.len();
        debug!("Found {} invalid symlinks.", self.information.number_of_invalid_symlinks);
        ControlFlow::Continue(())
    }

    fn check_invalid_symlinks(current_file_name: &Path) -> Option<(PathBuf, ErrorType)> {
        let mut destination_path = PathBuf::new();
        let type_of_error;

        match current_file_name.read_link() {
            Ok(t) => {
                destination_path.push(t);
                let mut loop_count = 0;
                let mut current_path = current_file_name.to_path_buf();
                loop {
                    if loop_count == 0 && !current_path.exists() {
                        type_of_error = ErrorType::NonExistentFile;
                        break;
                    }
                    if loop_count == MAX_NUMBER_OF_SYMLINK_JUMPS {
                        type_of_error = ErrorType::InfiniteRecursion;
                        break;
                    }

                    let parent_dir = current_path.parent().map(|p| p.to_path_buf());
                    current_path = match current_path.read_link() {
                        Ok(t) => {
                            if t.is_relative() {
                                if let Some(parent) = parent_dir { parent.join(t) } else { t }
                            } else {
                                t
                            }
                        }
                        Err(_inspected) => {
                            // Looks that some next symlinks are broken, but we do nothing with it - TODO why they are broken
                            return None;
                        }
                    };

                    loop_count += 1;
                }
            }
            Err(_inspected) => {
                // Failed to load info about it
                type_of_error = ErrorType::NonExistentFile;
            }
        }
        Some((destination_path, type_of_error))
    }
}
