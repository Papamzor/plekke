use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs::{self, File};
use std::ops::ControlFlow;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::{io, mem, panic};

use crossbeam_channel::Sender;
use fun_time::fun_time;
use log::{debug, error};
use lopdf::Document;
use rayon::prelude::*;

use crate::common::cache::{CACHE_BROKEN_FILES_VERSION, load_and_split_cache_generalized_by_path, save_and_connect_cache_generalized_by_path};
use crate::common::consts::{
    AUDIO_FILES_CONTENT_EXTENSIONS, BZ2_FILES_EXTENSIONS, FONT_FILES_EXTENSIONS, GZ_FILES_EXTENSIONS, IMAGE_RS_BROKEN_FILES_EXTENSIONS, JSON_FILES_EXTENSIONS,
    PDF_FILES_EXTENSIONS, SEVENZ_FILES_EXTENSIONS, SVG_FILES_EXTENSIONS, TAR_FILES_EXTENSIONS, TOML_FILES_EXTENSIONS, VIDEO_FILES_EXTENSIONS, XML_FILES_EXTENSIONS,
    XZ_FILES_EXTENSIONS, YAML_FILES_EXTENSIONS, ZIP_FILES_EXTENSIONS, ZST_FILES_EXTENSIONS,
};
use crate::common::model::ToolType;
use crate::common::process_utils::run_command_interruptible;
use crate::common::progress_data::{ProgressData, ToolStage};
use crate::common::progress_stop_handler::{check_if_stop_received, prepare_thread_handler_common};
use crate::common::tool_data::CommonToolData;
use crate::common::{create_crash_message_generic, normalize_error_string};
use crate::helpers::audio_checker;
use crate::tools::broken_files::{BrokenEntry, BrokenFiles, BrokenFilesParameters, CheckedTypes, CheckedTypesSingle, Info, TypeOfFile};

/// File types, the checked types that enable them and their extensions; the first matching extension wins.
pub(crate) const EXTENSIONS_BY_TYPE: &[(TypeOfFile, CheckedTypes, &[&str])] = &[
    (TypeOfFile::Pdf, CheckedTypes::PDF, PDF_FILES_EXTENSIONS),
    (TypeOfFile::Audio, CheckedTypes::AUDIO, AUDIO_FILES_CONTENT_EXTENSIONS),
    (TypeOfFile::Image, CheckedTypes::IMAGE, IMAGE_RS_BROKEN_FILES_EXTENSIONS),
    (TypeOfFile::ArchiveZip, CheckedTypes::ARCHIVE, ZIP_FILES_EXTENSIONS),
    (TypeOfFile::Archive7z, CheckedTypes::ARCHIVE, SEVENZ_FILES_EXTENSIONS),
    (TypeOfFile::ArchiveGz, CheckedTypes::ARCHIVE, GZ_FILES_EXTENSIONS),
    (TypeOfFile::ArchiveTar, CheckedTypes::ARCHIVE, TAR_FILES_EXTENSIONS),
    (TypeOfFile::ArchiveZst, CheckedTypes::ARCHIVE, ZST_FILES_EXTENSIONS),
    (TypeOfFile::ArchiveBz2, CheckedTypes::ARCHIVE, BZ2_FILES_EXTENSIONS),
    (TypeOfFile::ArchiveXz, CheckedTypes::ARCHIVE, XZ_FILES_EXTENSIONS),
    (TypeOfFile::Font, CheckedTypes::FONT, FONT_FILES_EXTENSIONS),
    (TypeOfFile::Json, CheckedTypes::MARKUP, JSON_FILES_EXTENSIONS),
    (TypeOfFile::Xml, CheckedTypes::MARKUP, XML_FILES_EXTENSIONS),
    (TypeOfFile::Toml, CheckedTypes::MARKUP, TOML_FILES_EXTENSIONS),
    (TypeOfFile::Yaml, CheckedTypes::MARKUP, YAML_FILES_EXTENSIONS),
    (TypeOfFile::Svg, CheckedTypes::MARKUP, SVG_FILES_EXTENSIONS),
    (TypeOfFile::Video, CheckedTypes::VIDEO_FFPROBE.union(CheckedTypes::VIDEO_FFMPEG), VIDEO_FILES_EXTENSIONS),
];

impl BrokenFiles {
    pub fn new(params: BrokenFilesParameters) -> Self {
        Self {
            common_data: CommonToolData::new(ToolType::BrokenFiles),
            information: Info::default(),
            files_to_check: Default::default(),
            broken_files: Default::default(),
            params,
        }
    }

    #[fun_time(message = "check_files", level = "debug")]
    pub(crate) fn check_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        self.files_to_check = self
            .common_data
            .collect_files(stop_flag, progress_sender, |_| {})?
            .into_iter()
            .map(|fe| {
                let broken_entry = fe.into_broken_entry();
                (broken_entry.path.to_string_lossy().to_string(), broken_entry)
            })
            .collect();
        debug!("check_files - Found {} files to check.", self.files_to_check.len());
        ControlFlow::Continue(())
    }

    fn check_broken_audio(mut file_entry: BrokenEntry, stop_flag: &Arc<AtomicBool>) -> Option<Option<BrokenEntry>> {
        match File::open(&file_entry.path) {
            Ok(file) => {
                let error = match audio_checker::parse_audio_file(file, stop_flag) {
                    Err(audio_checker::AudioCheckError::Other(err_str)) => Some(normalize_error_string(&err_str)),
                    Ok(None) => return None, // stop flag was set
                    Err(audio_checker::AudioCheckError::UnsupportedCodec) | Ok(Some(())) => None,
                };
                file_entry.errors.insert(CheckedTypesSingle::Audio, error);
                Some(Some(file_entry))
            }
            Err(_inspected) => Some(None),
        }
    }
    fn check_broken_video_ffprobe(mut file_entry: BrokenEntry, stop_flag: &Arc<AtomicBool>) -> Option<BrokenEntry> {
        let ffprobe_errors = [
            ("moov atom not found", Some("broken file structure")),
            ("error reading header", Some("broken file structure")),
            ("EBML header parsing failed", None),
            ("exceeds containing master element", Some("broken file structure")),
            ("invalid frame index table", Some("broken file structure")),
            ("Invalid argument", Some("ffprobe seems to not recognize file format")),
        ];

        let mut command = Command::new("ffprobe");
        command.arg("-v").arg("error").arg(&file_entry.path);

        let error = match run_command_interruptible(command, stop_flag) {
            None => return None,
            Some(Err(e)) => {
                debug!("Failed to run ffprobe on {:?}: {}", file_entry.path, e);
                Some(normalize_error_string(&format!("Failed to run ffprobe: {e}")))
            }
            Some(Ok(output)) => {
                let combined = format!("{}{}", output.stdout.trim(), output.stderr.trim());

                if let Some((error_message, additional_message)) = ffprobe_errors.iter().find(|(err, _)| combined.contains(err)) {
                    Some(format!("{error_message}{}", additional_message.map(|e| format!(" ({e})")).unwrap_or_default()))
                } else if !output.status.success() {
                    Some(format!("ffprobe exited with non-zero status: {}", output.status))
                } else {
                    None
                }
            }
        };
        file_entry.errors.insert(CheckedTypesSingle::VideoFfprobe, error);
        Some(file_entry)
    }

    fn check_broken_video_ffmpeg(mut file_entry: BrokenEntry, stop_flag: &Arc<AtomicBool>) -> Option<BrokenEntry> {
        let ffmpeg_message = [
            ("Output file does not contain any stream", Some("cannot find video stream - possible not even video file")),
            ("missing mandatory atoms, broken header", Some("broken file structure")),
            ("Cannot determine format of input", None),
            ("decode_slice_header error", Some("corrupted video data, may be still fully/partially playable")),
            ("Truncating packet", Some("corrupted video data, may be still fully/partially playable")),
            ("Invalid NAL unit size", Some("corrupted video data, may be still fully/partially playable")),
            (
                "exceeds containing master element ending",
                Some("corrupted video data, may be still fully/partially playable"),
            ),
            ("corrupt input packet in stream", Some("Possible corruption in audio/video stream, may be still playable")),
            (
                "invalid as first byte of an EBML number",
                Some("corrupted video data, may be still fully/partially playable"),
            ),
            // Last resort for all other errors
            ("Invalid data found when processing input", Some("generic error")), // Must be last to not override more precise errors
            // Warnings
            ("corrupt decoded frame", Some("may be still playable")),
        ];
        let ffmpeg_allowed_messages = [
            "Input buffer exhausted before END element found", // Looks like quite popular message, so ignoring it
            "Invalid color space",                             // https://fftrac-bg.ffmpeg.org/ticket/11020 - seems to be non-fatal
        ];

        let mut command = Command::new("ffmpeg");
        command
            .arg("-v")
            .arg("error")
            .arg("-xerror")
            .arg("-threads")
            .arg("1")
            .arg("-i")
            .arg(&file_entry.path)
            .arg("-f")
            .arg("null")
            .arg("-");

        let error = match run_command_interruptible(command, stop_flag) {
            None => return None,
            Some(Err(e)) => {
                debug!("Failed to run ffmpeg on {:?}: {}", file_entry.path, e);
                Some(normalize_error_string(&format!("Failed to run ffmpeg: {e}")))
            }
            Some(Ok(output)) => {
                let combined = format!("{}{}", output.stdout.trim(), output.stderr.trim());

                if ffmpeg_allowed_messages.iter().any(|msg| combined.contains(msg)) {
                    None
                } else if let Some((error_message, additional_message)) = ffmpeg_message.iter().find(|(err, _)| combined.contains(err)) {
                    Some(format!("{error_message}{}", additional_message.map(|e| format!(" ({e})")).unwrap_or_default()))
                } else if !output.status.success() {
                    Some(format!("ffmpeg exited with non-zero status: {}", output.status))
                } else {
                    None
                }
            }
        };
        file_entry.errors.insert(CheckedTypesSingle::VideoFfmpeg, error);
        Some(file_entry)
    }

    fn check_broken_video(mut file_entry: BrokenEntry, stop_flag: &Arc<AtomicBool>, checked_types: CheckedTypes) -> Option<BrokenEntry> {
        if checked_types.contains(CheckedTypes::VIDEO_FFPROBE) && !file_entry.errors.contains_key(&CheckedTypesSingle::VideoFfprobe) {
            file_entry = Self::check_broken_video_ffprobe(file_entry, stop_flag)?;
        }
        if checked_types.contains(CheckedTypes::VIDEO_FFMPEG) && !file_entry.errors.contains_key(&CheckedTypesSingle::VideoFfmpeg) {
            file_entry = Self::check_broken_video_ffmpeg(file_entry, stop_flag)?;
        }
        Some(file_entry)
    }

    pub(crate) fn video_entry_missing_required_checks(file_entry: &BrokenEntry, checked_types: CheckedTypes) -> bool {
        if check_extension_availability(&file_entry.path) != Some(TypeOfFile::Video) {
            return false;
        }
        (checked_types.contains(CheckedTypes::VIDEO_FFPROBE) && !file_entry.errors.contains_key(&CheckedTypesSingle::VideoFfprobe))
            || (checked_types.contains(CheckedTypes::VIDEO_FFMPEG) && !file_entry.errors.contains_key(&CheckedTypesSingle::VideoFfmpeg))
    }

    pub(crate) fn move_cached_entries_missing_required_checks(
        records_already_cached: &mut BTreeMap<String, BrokenEntry>,
        non_cached_files_to_check: &mut BTreeMap<String, BrokenEntry>,
        checked_types: CheckedTypes,
    ) {
        let keys_to_recheck: Vec<String> = records_already_cached
            .iter()
            .filter(|(_, file_entry)| Self::video_entry_missing_required_checks(file_entry, checked_types))
            .map(|(path, _)| path.clone())
            .collect();

        for path in keys_to_recheck {
            if let Some(file_entry) = records_already_cached.remove(&path) {
                non_cached_files_to_check.insert(path, file_entry);
            }
        }
    }

    fn file_type_to_checked_type_single(file_type: TypeOfFile) -> CheckedTypesSingle {
        match file_type {
            TypeOfFile::Image => CheckedTypesSingle::Image,
            TypeOfFile::ArchiveZip
            | TypeOfFile::Archive7z
            | TypeOfFile::ArchiveGz
            | TypeOfFile::ArchiveTar
            | TypeOfFile::ArchiveZst
            | TypeOfFile::ArchiveBz2
            | TypeOfFile::ArchiveXz => CheckedTypesSingle::Archive,
            TypeOfFile::Audio => CheckedTypesSingle::Audio,
            TypeOfFile::Pdf => CheckedTypesSingle::Pdf,
            TypeOfFile::Video => CheckedTypesSingle::VideoFfprobe,
            TypeOfFile::Font => CheckedTypesSingle::Font,
            TypeOfFile::Json | TypeOfFile::Xml | TypeOfFile::Toml | TypeOfFile::Yaml | TypeOfFile::Svg => CheckedTypesSingle::Markup,
        }
    }

    fn check_file(file_entry: BrokenEntry, stop_flag: &Arc<AtomicBool>, checked_types: CheckedTypes) -> Option<Option<BrokenEntry>> {
        let Some(file_type) = check_extension_availability(&file_entry.path) else {
            error!("Unknown file type of: {file_entry:?}");
            debug_assert!(false, "Unknown file type: {:?}", file_entry.path);
            return Some(None);
        };

        let mut file_entry_fallback = file_entry.clone();

        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| match file_type {
            TypeOfFile::Audio => Self::check_broken_audio(file_entry, stop_flag),
            TypeOfFile::Video => Self::check_broken_video(file_entry, stop_flag, checked_types).map(Some),
            // A file that cannot be read is skipped
            _ => Some(find_content_error(file_type, &file_entry.path).ok().map(|error| {
                let mut file_entry = file_entry;
                file_entry.errors.insert(Self::file_type_to_checked_type_single(file_type), error);
                file_entry
            })),
        }));

        match result {
            Ok(v) => v,
            Err(_) => {
                let checked_type_single = Self::file_type_to_checked_type_single(file_type);
                let message = create_crash_message_generic(&file_entry_fallback.path.to_string_lossy());
                error!("{message}");
                file_entry_fallback.errors.insert(checked_type_single, Some(message));
                Some(Some(file_entry_fallback))
            }
        }
    }

    #[fun_time(message = "look_for_broken_files", level = "debug")]
    pub(crate) fn look_for_broken_files(&mut self, stop_flag: &Arc<AtomicBool>, progress_sender: Option<&Sender<ProgressData>>) -> ControlFlow<()> {
        if self.files_to_check.is_empty() {
            return ControlFlow::Continue(());
        }

        let (loaded_hash_map, mut records_already_cached, mut non_cached_files_to_check) =
            load_and_split_cache_generalized_by_path(&get_broken_files_cache_file(), mem::take(&mut self.files_to_check), &mut self.common_data);

        let checked_types = self.params.checked_types;
        Self::move_cached_entries_missing_required_checks(&mut records_already_cached, &mut non_cached_files_to_check, checked_types);

        let progress_handler = prepare_thread_handler_common(
            progress_sender,
            ToolStage::BrokenFilesChecking,
            non_cached_files_to_check.len(),
            non_cached_files_to_check.values().map(|item| item.size).sum::<u64>(),
        );

        let non_cached_files_to_check = non_cached_files_to_check.into_iter().collect::<Vec<_>>();

        debug!("look_for_broken_files - started finding for broken files");
        let mut vec_file_entry: Vec<BrokenEntry> = non_cached_files_to_check
            .into_par_iter()
            .with_max_len(3)
            .map(|(_, file_entry)| {
                if check_if_stop_received(stop_flag) {
                    return None;
                }

                let size = file_entry.size;
                let res = Self::check_file(file_entry, stop_flag, checked_types);

                progress_handler.increase_items(1);
                progress_handler.increase_size(size);

                res
            })
            .while_some()
            .flatten()
            .collect::<Vec<BrokenEntry>>();
        debug!("look_for_broken_files - ended finding for broken files");

        progress_handler.join_thread();

        // Just connect loaded results with already calculated
        vec_file_entry.extend(records_already_cached.into_values());

        save_and_connect_cache_generalized_by_path(&get_broken_files_cache_file(), &vec_file_entry, loaded_hash_map, &mut self.common_data);

        self.broken_files = vec_file_entry.into_iter().filter(|f| f.has_errors()).collect();

        self.information.number_of_broken_files = self.broken_files.len();
        debug!("Found {} broken files.", self.information.number_of_broken_files);
        // Clean unused data
        self.files_to_check = Default::default();

        if check_if_stop_received(stop_flag) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

#[expect(clippy::string_slice)] // Valid, because we address up to the dot, which is known ascii character
fn check_extension_availability(full_name: &Path) -> Option<TypeOfFile> {
    let Some(file_name) = full_name.file_name() else {
        error!("Missing file name in file - \"{}\"", full_name.to_string_lossy());
        debug_assert!(false, "Missing file name in file - \"{}\"", full_name.to_string_lossy());
        return None;
    };

    // Faster manual conversion than using Path::extension()
    let file_name_str = file_name.to_str()?;
    let extension_idx = file_name_str.rfind('.')?;
    let extension_str = &file_name_str[extension_idx + 1..];

    let extension_lowercase = extension_str.to_ascii_lowercase();

    let file_type = EXTENSIONS_BY_TYPE
        .iter()
        .find(|(_, _, extensions)| extensions.contains(&extension_lowercase.as_str()))
        .map(|(file_type, _, _)| *file_type);
    if file_type.is_none() {
        error!("File with unknown extension: \"{}\" - {extension_lowercase}", full_name.to_string_lossy());
        debug_assert!(false, "File with unknown extension - \"{}\" - {extension_lowercase}", full_name.to_string_lossy());
    }
    file_type
}

/// Error found in a file of a type without a dedicated checker, or `Err` when the file cannot be read.
fn find_content_error(file_type: TypeOfFile, path: &Path) -> io::Result<Option<String>> {
    Ok(match file_type {
        TypeOfFile::Image => match image::open(path) {
            Ok(img) if img.width() == 0 || img.height() == 0 => Some("Image has zero width or height".to_string()),
            Ok(_) => None,
            Err(e) => Some(normalize_error_string(&e.to_string())),
        },
        TypeOfFile::ArchiveZip => error_of(zip::ZipArchive::new(File::open(path)?)),
        TypeOfFile::Archive7z => match sevenz_rust2::Archive::open(path) {
            Err(sevenz_rust2::Error::PasswordRequired) | Ok(_) => None,
            Err(e) => Some(normalize_error_string(&e.to_string())),
        },
        TypeOfFile::ArchiveGz => error_of(io::copy(&mut flate2::read::GzDecoder::new(File::open(path)?), &mut io::sink())),
        TypeOfFile::ArchiveTar => match tar::Archive::new(File::open(path)?).entries() {
            Err(e) => Some(normalize_error_string(&e.to_string())),
            Ok(mut entries) => entries.find_map(Result::err).map(|e| normalize_error_string(&e.to_string())),
        },
        TypeOfFile::ArchiveZst => match ruzstd::decoding::StreamingDecoder::new(File::open(path)?) {
            Err(e) => Some(normalize_error_string(&e.to_string())),
            Ok(mut decoder) => error_of(io::copy(&mut decoder, &mut io::sink())),
        },
        TypeOfFile::ArchiveBz2 => error_of(io::copy(&mut bzip2::read::MultiBzDecoder::new(File::open(path)?), &mut io::sink())),
        TypeOfFile::ArchiveXz => error_of(lzma_rs::xz_decompress(&mut io::BufReader::new(File::open(path)?), &mut io::sink())),
        TypeOfFile::Font => ttf_parser::Face::parse(&fs::read(path)?, 0).err().map(|e| normalize_error_string(&format!("{e:?}"))),
        TypeOfFile::Json => error_of(serde_json::from_slice::<serde_json::Value>(&fs::read(path)?)),
        TypeOfFile::Xml => {
            let data = fs::read(path)?;
            let mut reader = quick_xml::Reader::from_reader(data.as_slice());
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Err(e) => break Some(normalize_error_string(&e.to_string())),
                    Ok(quick_xml::events::Event::Eof) => break None,
                    Ok(_) => {}
                }
            }
        }
        TypeOfFile::Toml => error_of(toml::from_str::<toml::Table>(&fs::read_to_string(path)?)),
        TypeOfFile::Yaml => error_of(yaml_rust2::YamlLoader::load_from_str(&fs::read_to_string(path)?)),
        TypeOfFile::Svg => usvg::Tree::from_data(&fs::read(path)?, &usvg::Options::default())
            .err()
            .map(|e| normalize_error_string(&format!("{e:?}"))),
        // An unreadable PDF is reported as broken instead of being skipped
        TypeOfFile::Pdf => match File::open(path) {
            Ok(file) => error_of(Document::load_from(file)),
            Err(e) => Some(normalize_error_string(&e.to_string())),
        },
        TypeOfFile::Audio | TypeOfFile::Video => unreachable!("audio and video have dedicated checkers"),
    })
}

fn error_of<T, E: Display>(result: Result<T, E>) -> Option<String> {
    result.err().map(|e| normalize_error_string(&e.to_string()))
}

pub fn get_broken_files_cache_file() -> String {
    format!("cache_broken_files_{CACHE_BROKEN_FILES_VERSION}.bin")
}
