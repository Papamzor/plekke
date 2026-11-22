use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use clap::Parser;
use commands::Commands;
use crossbeam_channel::{Receiver, Sender, unbounded};
use czkawka_core::common::config_cache_path::{print_infos_and_warnings, set_config_cache_path};
use czkawka_core::common::consts::DEFAULT_THREAD_SIZE;
use czkawka_core::common::image::register_image_decoding_hooks;
use czkawka_core::common::logger::{filtering_messages, print_version_mode, setup_logger};
use czkawka_core::common::progress_data::ProgressData;
use czkawka_core::common::set_number_of_threads;
use czkawka_core::common::tool_data::CommonData;
use czkawka_core::common::traits::{PrintResults, Search};
use czkawka_core::tools::bad_extensions::{BadExtensions, BadExtensionsParameters};
use czkawka_core::tools::bad_names::{BadNames, BadNamesParameters, NameIssues};
use czkawka_core::tools::big_file::{BigFile, BigFileParameters, SearchMode};
use czkawka_core::tools::broken_files::{BrokenFiles, BrokenFilesParameters, CheckedTypes};
use czkawka_core::tools::disk_usage::{DiskUsage, DiskUsageParameters};
use czkawka_core::tools::duplicate::{DuplicateFinder, DuplicateFinderParameters};
use czkawka_core::tools::empty_files::{EmptyFiles, EmptyFilesParameters};
use czkawka_core::tools::empty_folder::EmptyFolder;
use czkawka_core::tools::exif_remover::{ExifRemover, ExifRemoverParameters, ExifTagsFixerParams};
use czkawka_core::tools::invalid_symlinks::InvalidSymlinks;
use czkawka_core::tools::same_music::{SameMusic, SameMusicParameters};
use czkawka_core::tools::similar_images::{SimilarImages, SimilarImagesParameters};
use czkawka_core::tools::similar_videos::{SimilarVideos, SimilarVideosParameters};
use czkawka_core::tools::temporary::{Temporary, TemporaryParameters};
use czkawka_core::tools::video_optimizer::{
    HardwareEncoder, VideoCropFixParams, VideoCropParams, VideoOptimizer, VideoOptimizerFixParams, VideoOptimizerParameters, VideoTranscodeFixParams, VideoTranscodeParams,
};
use log::{debug, error, info};

use crate::commands::{
    Args, BadExtensionsArgs, BadNamesArgs, BiggestFilesArgs, BrokenFilesArgs, CommonCliItems, CompletionsArgs, DMethod, DiskUsageArgs, DuplicatesArgs, EmptyFilesArgs,
    EmptyFoldersArgs, ExifRemoverArgs, InvalidSymlinksArgs, SameMusicArgs, SimilarImagesArgs, SimilarVideosArgs, TemporaryArgs, VideoOptimizerArgs, VideoOptimizerMode,
};
use crate::progress::connect_progress;

mod commands;
mod parsers;
mod progress;

#[derive(Debug)]
pub struct CliOutput {
    pub found_any_files: bool,
    pub ignored_error_code_on_found: bool,
    pub output: String,
}

fn main() {
    register_image_decoding_hooks();
    if cfg!(debug_assertions) {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }
    let command = Args::parse().command;

    if let Commands::Completions(CompletionsArgs { shell }) = command {
        use clap::CommandFactory;
        clap_complete::generate(shell, &mut Args::command(), "czkawka_cli", &mut std::io::stdout());
        return;
    }

    let config_cache_path_set_result = set_config_cache_path("Czkawka", "Czkawka");
    setup_logger(true, "czkawka_cli", filtering_messages);
    print_version_mode("Czkawka cli");
    print_infos_and_warnings(config_cache_path_set_result.infos, config_cache_path_set_result.warnings);

    if cfg!(debug_assertions) {
        debug!("Running command - {command:?}");
    }

    let (progress_sender, progress_receiver): (Sender<ProgressData>, Receiver<ProgressData>) = unbounded();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let store_flag_cloned = stop_flag.clone();

    let calculate_thread = thread::Builder::new()
        .stack_size(DEFAULT_THREAD_SIZE)
        .spawn(move || match &command {
            Commands::Duplicates(duplicates_args) => duplicates(duplicates_args, &stop_flag, &progress_sender),
            Commands::EmptyFolders(empty_folders_args) => empty_folders(empty_folders_args, &stop_flag, &progress_sender),
            Commands::BiggestFiles(biggest_files_args) => biggest_files(biggest_files_args, &stop_flag, &progress_sender),
            Commands::DiskUsage(disk_usage_args) => disk_usage(disk_usage_args, &stop_flag, &progress_sender),
            Commands::EmptyFiles(empty_files_args) => empty_files(empty_files_args, &stop_flag, &progress_sender),
            Commands::Temporary(temporary_args) => temporary(temporary_args, &stop_flag, &progress_sender),
            Commands::SimilarImages(similar_images_args) => similar_images(similar_images_args, &stop_flag, &progress_sender),
            Commands::SameMusic(same_music_args) => same_music(same_music_args, &stop_flag, &progress_sender),
            Commands::InvalidSymlinks(invalid_symlinks_args) => invalid_symlinks(invalid_symlinks_args, &stop_flag, &progress_sender),
            Commands::BrokenFiles(broken_files_args) => broken_files(broken_files_args, &stop_flag, &progress_sender),
            Commands::SimilarVideos(similar_videos_args) => similar_videos(similar_videos_args, &stop_flag, &progress_sender),
            Commands::BadExtensions(bad_extensions_args) => bad_extensions(bad_extensions_args, &stop_flag, &progress_sender),
            Commands::BadNames(bad_names_args) => bad_names(bad_names_args, &stop_flag, &progress_sender),
            Commands::VideoOptimizer(video_optimizer_args) => video_optimizer(video_optimizer_args, &stop_flag, &progress_sender),
            Commands::ExifRemover(exif_remover_args) => exif_remover(exif_remover_args, &stop_flag, &progress_sender),
            Commands::Completions(_) => unreachable!("Completions are handled before the scan starts"),
        })
        .expect("Failed to spawn calculation thread");

    ctrlc::set_handler(move || {
        if store_flag_cloned.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        info!("Got Ctrl+C signal, stopping...");
        store_flag_cloned.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    connect_progress(&progress_receiver);

    let cli_output = calculate_thread.join().expect("Failed to join calculation thread");

    #[expect(clippy::print_stdout)]
    if !cli_output.output.is_empty() {
        println!("{}", cli_output.output);
    }

    if cli_output.found_any_files && !cli_output.ignored_error_code_on_found {
        std::process::exit(11);
    }
    std::process::exit(0);
}

fn duplicates(args: &DuplicatesArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = DuplicateFinderParameters::new(
        args.search_method,
        args.hash_type,
        args.use_prehash_cache,
        args.minimal_cached_file_size,
        args.minimal_prehash_cache_file_size,
        args.case_sensitive_name_comparison,
    );
    let mut tool = DuplicateFinder::new(params);
    args.file_size.apply(&mut tool);
    tool.set_hide_hard_links(!args.allow_hard_links.allow_hard_links);
    set_delete(&mut tool, args.delete_method);
    run(
        tool,
        &args.common_cli_items,
        &args.reference_directories.reference_directories,
        stop_flag,
        progress_sender,
        |_| {},
    )
}

fn empty_folders(args: &EmptyFoldersArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let mut tool = EmptyFolder::new();
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn biggest_files(args: &BiggestFilesArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let search_mode = if args.smallest_mode { SearchMode::SmallestFiles } else { SearchMode::BiggestFiles };
    let mut tool = BigFile::new(BigFileParameters::new(args.number_of_files, search_mode));
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn disk_usage(args: &DiskUsageArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = DiskUsageParameters {
        report_depth: args.depth.max(1),
        report_entries_per_directory: args.top,
        ignore_excluded_items: args.ignore_excluded_items,
        ignore_excluded_directories: args.ignore_excluded_directories,
        ..DiskUsageParameters::new(args.detect_by_content.into())
    };
    run(DiskUsage::new(params), &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn empty_files(args: &EmptyFilesArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = EmptyFilesParameters {
        search_zero_byte_content_files: args.zero_byte_content || args.non_printable_content,
        search_non_printable_content_files: args.non_printable_content,
    };
    let mut tool = EmptyFiles::new(params);
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn temporary(args: &TemporaryArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = if args.extensions.is_empty() {
        TemporaryParameters::default()
    } else {
        TemporaryParameters {
            extensions: args.extensions.clone(),
        }
    };
    let mut tool = Temporary::new(params);
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn similar_images(args: &SimilarImagesArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = SimilarImagesParameters::new(
        args.max_difference,
        args.hash_size,
        args.hash_alg,
        args.image_filter,
        args.ignore_same_size.ignore_same_size,
        args.ignore_same_resolution.ignore_same_resolution,
        args.geometric_invariance,
    );
    let mut tool = SimilarImages::new(params);
    args.file_size.apply(&mut tool);
    tool.set_hide_hard_links(!args.allow_hard_links.allow_hard_links);
    set_delete(&mut tool, args.delete_method);
    run(
        tool,
        &args.common_cli_items,
        &args.reference_directories.reference_directories,
        stop_flag,
        progress_sender,
        |_| {},
    )
}

fn same_music(args: &SameMusicArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = SameMusicParameters::new(
        args.music_similarity,
        args.approximate_comparison,
        args.search_method,
        args.minimum_segment_duration,
        args.maximum_difference,
        args.compare_fingerprints_only_with_similar_titles,
    );
    let mut tool = SameMusic::new(params);
    args.file_size.apply(&mut tool);
    set_delete(&mut tool, args.delete_method);
    run(
        tool,
        &args.common_cli_items,
        &args.reference_directories.reference_directories,
        stop_flag,
        progress_sender,
        |_| {},
    )
}

fn invalid_symlinks(args: &InvalidSymlinksArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let mut tool = InvalidSymlinks::new();
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn broken_files(args: &BrokenFilesArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let checked_types = args.checked_types.iter().fold(CheckedTypes::NONE, |acc, checked_type| acc | *checked_type);
    let mut tool = BrokenFiles::new(BrokenFilesParameters::new(checked_types));
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |_| {})
}

fn similar_videos(args: &SimilarVideosArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let params = SimilarVideosParameters::new(
        args.tolerance,
        args.ignore_same_size.ignore_same_size,
        args.ignore_same_resolution.ignore_same_resolution,
        args.skip_forward_amount,
        args.scan_duration,
        args.crop_detect,
        args.window_count,
        args.duration_tolerance_pct,
        args.min_matching_windows,
        args.subclip_min_match,
        args.generate_thumbnails,
        args.thumbnail_video_percentage_from_start,
        args.generate_thumbnail_grid,
        args.thumbnail_grid_tiles_per_side,
        args.check_audio_content,
        args.audio_similarity_percent,
        args.audio_maximum_difference,
        args.audio_length_ratio,
        args.audio_min_duration_seconds,
    );
    let mut tool = SimilarVideos::new(params);
    args.file_size.apply(&mut tool);
    tool.set_hide_hard_links(!args.allow_hard_links.allow_hard_links);
    set_delete(&mut tool, args.delete_method);
    run(
        tool,
        &args.common_cli_items,
        &args.reference_directories.reference_directories,
        stop_flag,
        progress_sender,
        |_| {},
    )
}

fn bad_extensions(args: &BadExtensionsArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let tool = BadExtensions::new(BadExtensionsParameters::new());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |tool| {
        if args.fix_extensions {
            tool.fix_bad_extensions(stop_flag);
        }
    })
}

fn bad_names(args: &BadNamesArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let restricted_charset_allowed = args.restricted_charset.as_deref().and_then(|s| {
        let mut items: Vec<_> = s.chars().collect();
        items.sort_unstable();
        items.dedup();
        if items.is_empty() { None } else { Some(items) }
    });
    let name_issues = NameIssues {
        uppercase_extension: args.uppercase_extension,
        emoji_used: args.emoji_used,
        space_at_start_or_end: args.space_at_start_or_end,
        non_ascii_graphical: args.non_ascii_graphical,
        restricted_charset_allowed,
        remove_duplicated_non_alphanumeric: args.remove_duplicated_non_alphanumeric,
    };
    let mut tool = BadNames::new(BadNamesParameters::new(name_issues));
    set_delete(&mut tool, args.delete_method.into());
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |tool| {
        if args.fix_names {
            tool.fix_bad_names(stop_flag);
        }
    })
}

fn video_optimizer(args: &VideoOptimizerArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    match &args.mode {
        VideoOptimizerMode::Transcode(transcode) => {
            let excluded_codecs = transcode.excluded_codecs.as_deref().map_or_else(
                || ["hevc", "h265", "av1", "vp9"].map(String::from).to_vec(),
                |s| s.split(',').map(|c| c.trim().to_string()).collect(),
            );
            let thumbnails = transcode.thumbnails;
            let params = VideoOptimizerParameters::VideoTranscode(VideoTranscodeParams::new(
                excluded_codecs,
                thumbnails.generate_thumbnails,
                thumbnails.thumbnail_percentage,
                thumbnails.thumbnail_grid,
                thumbnails.thumbnail_grid_tiles_per_side,
            ));
            run(VideoOptimizer::new(params), &args.common_cli_items, &[], stop_flag, progress_sender, |tool| {
                if transcode.fix_videos {
                    let fix_params = VideoOptimizerFixParams::VideoTranscode(VideoTranscodeFixParams {
                        codec: transcode.target_codec,
                        quality: transcode.quality,
                        fail_if_not_smaller: transcode.fail_if_not_smaller,
                        overwrite_original: transcode.overwrite_original,
                        max_dimensions: transcode.max_width.zip(transcode.max_height),
                        max_fps: transcode.max_fps,
                        noise_reduction: transcode.noise_reduction.clone(),
                        noise_reduction_strength: transcode.noise_reduction_strength,
                        custom_ffmpeg_command: transcode.custom_ffmpeg_command.clone(),
                        hardware_encoder: HardwareEncoder::None, // TODO  - missing hardware encoder
                    });
                    tool.fix_files(stop_flag, fix_params);
                }
            })
        }
        VideoOptimizerMode::Crop(crop) => {
            let thumbnails = crop.thumbnails;
            let params = VideoOptimizerParameters::VideoCrop(VideoCropParams::with_custom_params(
                crop.crop_mechanism,
                crop.black_pixel_threshold,
                crop.black_bar_percentage,
                crop.max_samples,
                crop.min_crop_size,
                thumbnails.generate_thumbnails,
                thumbnails.thumbnail_percentage,
                thumbnails.thumbnail_grid,
                thumbnails.thumbnail_grid_tiles_per_side,
            ));
            run(VideoOptimizer::new(params), &args.common_cli_items, &[], stop_flag, progress_sender, |tool| {
                if crop.fix_videos {
                    let fix_params = VideoOptimizerFixParams::VideoCrop(VideoCropFixParams {
                        overwrite_original: crop.overwrite_original,
                        target_codec: crop.target_codec,
                        quality: crop.quality,
                        crop_mechanism: crop.crop_mechanism,
                    });
                    tool.fix_files(stop_flag, fix_params);
                }
            })
        }
    }
}

fn exif_remover(args: &ExifRemoverArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let ignored_tags = args
        .ignored_tags
        .as_deref()
        .map(|s| s.split(',').map(|tag| tag.trim().to_string()).collect())
        .unwrap_or_default();
    let tool = ExifRemover::new(ExifRemoverParameters::new(ignored_tags));
    run(tool, &args.common_cli_items, &[], stop_flag, progress_sender, |tool| {
        if args.fix_exif {
            tool.fix_files(
                stop_flag,
                ExifTagsFixerParams {
                    override_file: args.override_file,
                },
            );
        }
    })
}

/// Applies the settings shared by every tool, scans, runs the optional `fix` step and collects the output.
fn run<T: Search + PrintResults>(
    mut tool: T,
    common: &CommonCliItems,
    reference_directories: &[PathBuf],
    stop_flag: &Arc<AtomicBool>,
    progress_sender: &Sender<ProgressData>,
    fix: impl FnOnce(&mut T),
) -> CliOutput {
    set_number_of_threads(common.thread_number);
    tool.set_reference_paths(reference_directories.to_vec());
    tool.set_included_paths([common.directories.as_slice(), reference_directories].concat());
    tool.set_excluded_paths(common.excluded_directories.clone());
    tool.set_excluded_items(common.excluded_items.clone());
    tool.set_recursive_search(!common.not_recursive);
    #[cfg(target_family = "unix")]
    tool.set_exclude_other_filesystems(common.exclude_other_filesystems);
    tool.set_allowed_extensions(common.allowed_extensions.clone());
    tool.set_excluded_extensions(common.excluded_extensions.clone());
    tool.set_use_cache(!common.disable_cache);

    tool.search(stop_flag, Some(progress_sender));
    fix(&mut tool);
    save_and_write_results_to_writer(&tool, common)
}

fn save_and_write_results_to_writer<T: PrintResults>(tool: &T, common: &CommonCliItems) -> CliOutput {
    if let Some(file_name) = common.file_to_save.as_deref().and_then(Path::to_str)
        && let Err(e) = tool.print_results_to_file(file_name)
    {
        error!("Failed to save results to file {e}");
    }
    if let Some(file_name) = common.compact_file_to_save.as_deref().and_then(Path::to_str)
        && let Err(e) = tool.save_results_to_file_as_json(file_name, false)
    {
        error!("Failed to save compact json results to file {e}");
    }
    if let Some(file_name) = common.pretty_file_to_save.as_deref().and_then(Path::to_str)
        && let Err(e) = tool.save_results_to_file_as_json(file_name, true)
    {
        error!("Failed to save pretty json results to file {e}");
    }

    let mut output = Vec::new();
    if !common.do_not_print_results
        && let Err(e) = tool.write_results(&mut output)
    {
        error!("Failed to print results to output: {e}");
    }
    if !common.do_not_print_messages
        && let Err(e) = tool.get_text_messages().print_messages_to_writer(&mut output)
    {
        error!("Failed to print messages to output: {e}");
    }

    CliOutput {
        found_any_files: tool.found_any_items(),
        ignored_error_code_on_found: common.ignore_error_code_on_found,
        output: String::from_utf8_lossy(&output).into_owned(),
    }
}

fn set_delete<T: CommonData>(tool: &mut T, delete: DMethod) {
    tool.set_delete_method(delete.delete_method);
    tool.set_dry_run(delete.flags.dry_run);
    tool.set_move_to_trash(delete.flags.move_to_trash);
}
