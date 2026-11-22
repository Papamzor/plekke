use std::io::prelude::*;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crossbeam_channel::Sender;
use fun_time::fun_time;

use crate::common::consts::{AUDIO_FILES_CONTENT_EXTENSIONS, AUDIO_FILES_TAGS_EXTENSIONS};
use crate::common::model::CheckingMethod;
use crate::common::progress_data::ProgressData;
use crate::common::reference_groups::{GroupHeaders, delete_reference_groups, save_reference_groups_as_json, write_reference_groups};
use crate::common::tool_data::{CommonData, impl_common_data};
use crate::common::traits::{PrintResults, Search};
use crate::flc;
use crate::tools::same_music::core::format_audio_duration;
use crate::tools::same_music::{Info, MusicEntry, MusicSimilarity, SameMusic, SameMusicParameters};

impl Search for SameMusic {
    #[fun_time(message = "find_same_music", level = "info")]
    fn search(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) {
        let start_time = Instant::now();

        self.common_data.stopped_search |= (|| {
            let audio_extensions = match self.params.check_type {
                CheckingMethod::AudioTags => AUDIO_FILES_TAGS_EXTENSIONS,
                CheckingMethod::AudioContent => AUDIO_FILES_CONTENT_EXTENSIONS,
                _ => unreachable!(),
            };
            if self.prepare_items(Some(audio_extensions)).is_err() {
                return ControlFlow::Continue(());
            }
            self.check_files(stop_flag, progress_sender)?;
            match self.params.check_type {
                CheckingMethod::AudioTags => {
                    if self.params.music_similarity == MusicSimilarity::NONE {
                        self.common_data.text_messages.critical = flc!("core_no_similarity_method_selected").into();
                        return ControlFlow::Continue(());
                    }

                    // Comparing tags is fast, so groups are built from already read tags even after stop
                    let read_tags = self.read_tags(stop_flag, progress_sender);
                    self.check_for_duplicate_tags(progress_sender);
                    read_tags?;
                }
                CheckingMethod::AudioContent => {
                    self.read_tags(stop_flag, progress_sender)?;
                    self.calculate_fingerprint(stop_flag, progress_sender)?;
                    self.check_for_duplicate_fingerprints(stop_flag, progress_sender)?;
                }
                _ => panic!(),
            }
            self.delete_files(stop_flag, progress_sender)
        })()
        .is_break();

        self.information.scanning_time = start_time.elapsed();
    }
}

impl PrintResults for SameMusic {
    fn write_results<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.write_base_search_paths(writer)?;
        let headers = GroupHeaders {
            plain_summary: " music files which have similar friends\n\n.\n",
            referenced_summary: " music files which have similar friends\n\n.\n",
            plain_group: " music files which have similar friends",
            referenced_group: " music files which have similar friends",
            not_found: "Not found any similar music files.",
        };
        write_reference_groups(
            writer,
            &self.duplicated_music_entries,
            &self.duplicated_music_entries_referenced,
            &headers,
            write_music_entry,
        )
    }

    fn save_results_to_file_as_json(&self, file_name: &str, pretty_print: bool) -> std::io::Result<()> {
        save_reference_groups_as_json(self, file_name, &self.duplicated_music_entries, &self.duplicated_music_entries_referenced, pretty_print)
    }
}

fn write_music_entry<T: Write>(writer: &mut T, file_entry: &MusicEntry) -> std::io::Result<()> {
    writeln!(
        writer,
        "TT: {}  -  TA: {}  -  Y: {}  -  L: {}  -  G: {}  -  B: {}  -  P: \"{}\"",
        file_entry.track_title,
        file_entry.track_artist,
        file_entry.year,
        format_audio_duration(file_entry.length),
        file_entry.genre,
        file_entry.bitrate,
        file_entry.path.to_string_lossy()
    )
}

impl_common_data! {
    SameMusic, Info, SameMusicParameters,
    fn get_check_method(&self) -> CheckingMethod {
        self.params.check_type
    }
    fn found_any_items(&self) -> bool {
        self.information.number_of_duplicates > 0
    }
}

impl SameMusic {
    #[fun_time(message = "delete_files", level = "debug")]
    pub fn delete_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        delete_reference_groups(
            &mut self.common_data,
            stop_flag,
            progress_sender,
            &self.duplicated_music_entries,
            &self.duplicated_music_entries_referenced,
        )
    }
}
