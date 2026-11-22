use std::num::ParseFloatError;
use std::ops::{BitOr, RangeInclusive};
use std::str::FromStr;

use clap::builder::RangedI64ValueParser;
use czkawka_core::common::model::{CheckingMethod, HashType};
use czkawka_core::common::tool_data::DeleteMethod;
use czkawka_core::re_exported::{FilterType, HashAlg};
use czkawka_core::tools::broken_files::CheckedTypes;
use czkawka_core::tools::same_music::MusicSimilarity;
use czkawka_core::tools::similar_images::GeometricInvariance;
use czkawka_core::tools::video_optimizer::{VideoCodec, VideoCroppingMechanism};

/// Values above this threshold are practically meaningless for audio segment matching
const MAX_SAME_MUSIC_DIFFERENCE: f64 = 10.0;

const MUSIC_SIMILARITY_NAMES: [(&str, MusicSimilarity); 6] = [
    ("tracktitle", MusicSimilarity::TRACK_TITLE),
    ("trackartist", MusicSimilarity::TRACK_ARTIST),
    ("year", MusicSimilarity::YEAR),
    ("bitrate", MusicSimilarity::BITRATE),
    ("genre", MusicSimilarity::GENRE),
    ("length", MusicSimilarity::LENGTH),
];

fn parse_finite<T: FromStr<Err = ParseFloatError> + Into<f64> + Copy>(src: &str) -> Result<T, String> {
    let value: T = src.parse().map_err(|e: ParseFloatError| e.to_string())?;
    if value.into().is_finite() {
        Ok(value)
    } else {
        Err(format!("Value '{src}' is not finite"))
    }
}

pub(crate) fn in_range(range: RangeInclusive<f64>, name: &'static str) -> impl Fn(&str) -> Result<f64, String> + Clone + Send + Sync + 'static {
    move |src| {
        let value = parse_finite(src)?;
        if range.contains(&value) {
            Ok(value)
        } else {
            Err(format!("{name} must be in range {range:?}"))
        }
    }
}

pub(crate) fn u32_in(range: RangeInclusive<u32>) -> RangedI64ValueParser<u32> {
    clap::value_parser!(u32).range(i64::from(*range.start())..=i64::from(*range.end()))
}

pub(crate) fn parse_maximum_difference(src: &str) -> Result<f64, String> {
    let maximum_difference = parse_finite(src)?;
    if maximum_difference <= 0.0 {
        Err("Maximum difference must be bigger than 0".to_string())
    } else if maximum_difference > MAX_SAME_MUSIC_DIFFERENCE {
        Err(format!("Maximum difference must be at most {MAX_SAME_MUSIC_DIFFERENCE}"))
    } else {
        Ok(maximum_difference)
    }
}

pub(crate) fn parse_minimum_segment_duration(src: &str) -> Result<f32, String> {
    let minimum_segment_duration = parse_finite(src)?;
    if minimum_segment_duration <= 0.0 {
        Err("Minimum segment duration must be bigger than 0".to_string())
    } else if minimum_segment_duration >= 3600.0 {
        Err("Minimum segment duration must be smaller than 3600(greater values not have much sense)".to_string())
    } else {
        Ok(minimum_segment_duration)
    }
}

pub(crate) fn parse_audio_maximum_difference(src: &str) -> Result<f64, String> {
    let v = parse_finite(src)?;
    if v >= 0.0 { Ok(v) } else { Err("Audio maximum difference must be >= 0.0".to_string()) }
}

pub(crate) fn parse_hash_type(src: &str) -> Result<HashType, &'static str> {
    match src.to_ascii_lowercase().as_str() {
        "blake3" => Ok(HashType::Blake3),
        "crc32" => Ok(HashType::Crc32),
        "xxh3" => Ok(HashType::Xxh3),
        _ => Err("Couldn't parse the hash type (allowed: BLAKE3, CRC32, XXH3)"),
    }
}

pub(crate) fn parse_checking_method_duplicate(src: &str) -> Result<CheckingMethod, &'static str> {
    match src.to_ascii_lowercase().as_str() {
        "name" => Ok(CheckingMethod::Name),
        "size" => Ok(CheckingMethod::Size),
        "size_name" => Ok(CheckingMethod::SizeName),
        "hash" => Ok(CheckingMethod::Hash),
        _ => Err("Couldn't parse the search method (allowed: NAME, SIZE, SIZE_NAME, HASH)"),
    }
}

pub(crate) fn parse_broken_files(src: &str) -> Result<CheckedTypes, &'static str> {
    match src.to_ascii_lowercase().as_str() {
        "pdf" => Ok(CheckedTypes::PDF),
        "audio" => Ok(CheckedTypes::AUDIO),
        "image" => Ok(CheckedTypes::IMAGE),
        "archive" => Ok(CheckedTypes::ARCHIVE),
        "video_ffprobe" => Ok(CheckedTypes::VIDEO_FFPROBE),
        "video_ffmpeg" => Ok(CheckedTypes::VIDEO_FFMPEG),
        "font" => Ok(CheckedTypes::FONT),
        "markup" => Ok(CheckedTypes::MARKUP),
        _ => Err("Couldn't parse the broken files type (allowed: PDF, AUDIO, IMAGE, ARCHIVE, FONT, MARKUP, VIDEO_FFPROBE, VIDEO_FFMPEG)"),
    }
}

pub(crate) fn parse_checking_method_same_music(src: &str) -> Result<CheckingMethod, &'static str> {
    match src.to_ascii_lowercase().as_str() {
        "tags" => Ok(CheckingMethod::AudioTags),
        "content" => Ok(CheckingMethod::AudioContent),
        _ => Err("Couldn't parse the search method (allowed: TAGS, CONTENT)"),
    }
}

pub(crate) fn parse_video_codec(src: &str) -> Result<VideoCodec, &'static str> {
    match src.to_ascii_lowercase().as_str() {
        "h264" => Ok(VideoCodec::H264),
        "h265" | "hevc" => Ok(VideoCodec::H265),
        "av1" => Ok(VideoCodec::Av1),
        "vp9" => Ok(VideoCodec::Vp9),
        _ => Err("Couldn't parse the video codec (allowed: h264, h265, av1, vp9)"),
    }
}

pub(crate) fn parse_delete_method(src: &str) -> Result<DeleteMethod, &'static str> {
    match src.to_ascii_lowercase().as_str() {
        "none" => Ok(DeleteMethod::None),
        "aen" => Ok(DeleteMethod::AllExceptNewest),
        "aeo" => Ok(DeleteMethod::AllExceptOldest),
        "hard" => Ok(DeleteMethod::HardLink),
        "on" => Ok(DeleteMethod::OneNewest),
        "oo" => Ok(DeleteMethod::OneOldest),
        "aeb" => Ok(DeleteMethod::AllExceptBiggest),
        "aes" => Ok(DeleteMethod::AllExceptSmallest),
        "ob" => Ok(DeleteMethod::OneBiggest),
        "os" => Ok(DeleteMethod::OneSmallest),
        _ => Err("Couldn't parse the delete method (allowed: AEN, AEO, ON, OO, HARD, AEB, AES, OB, OS)"),
    }
}

pub(crate) fn parse_similar_image_filter(src: &str) -> Result<FilterType, String> {
    let filter_type = match src.to_lowercase().as_str() {
        "lanczos3" => FilterType::Lanczos3,
        "nearest" => FilterType::Nearest,
        "triangle" => FilterType::Triangle,
        "gaussian" => FilterType::Gaussian,
        "catmullrom" => FilterType::CatmullRom,
        _ => return Err("Couldn't parse the image resize filter (allowed: Lanczos3, Nearest, Triangle, Gaussian, Catmullrom)".to_string()),
    };
    Ok(filter_type)
}

pub(crate) fn parse_similar_hash_algorithm(src: &str) -> Result<HashAlg, String> {
    let algorithm = match src.to_lowercase().as_str() {
        "mean" => HashAlg::Mean,
        "gradient" => HashAlg::Gradient,
        "blockhash" => HashAlg::Blockhash,
        "vertgradient" => HashAlg::VertGradient,
        "doublegradient" => HashAlg::DoubleGradient,
        "median" => HashAlg::Median,
        _ => return Err("Couldn't parse the hash algorithm (allowed: Mean, Gradient, Blockhash, VertGradient, DoubleGradient, Median)".to_string()),
    };
    Ok(algorithm)
}

pub(crate) fn parse_image_hash_size(src: &str) -> Result<u8, String> {
    let hash_size = match src.to_lowercase().as_str() {
        "8" => 8,
        "16" => 16,
        "32" => 32,
        "64" => 64,
        _ => return Err("Couldn't parse the image hash size (allowed: 8, 16, 32, 64)".to_string()),
    };
    Ok(hash_size)
}

pub(crate) fn parse_geometric_invariance(src: &str) -> Result<GeometricInvariance, String> {
    let geometric_invariance = match src.to_lowercase().replace('_', "-").as_str() {
        "off" => GeometricInvariance::Off,
        "mirror-flip" => GeometricInvariance::MirrorFlip,
        "mirror-flip-rotate90" => GeometricInvariance::MirrorFlipRotate90,
        _ => return Err("Couldn't parse geometric invariance (allowed: off, mirror-flip, mirror-flip-rotate90)".to_string()),
    };
    Ok(geometric_invariance)
}

pub(crate) fn parse_music_duplicate_type(src: &str) -> Result<MusicSimilarity, String> {
    if src.trim().is_empty() {
        return Ok(MusicSimilarity::NONE);
    }

    // Unknown names are ignored as long as at least one known name is present
    let similarity = src
        .split(',')
        .map(|part| part.to_lowercase().replace('_', ""))
        .filter_map(|part| MUSIC_SIMILARITY_NAMES.iter().find(|(name, _)| *name == part).map(|(_, flag)| *flag))
        .fold(MusicSimilarity::NONE, BitOr::bitor);

    if similarity == MusicSimilarity::NONE {
        return Err("Couldn't parse the music search method (allowed: track_title,track_artist,year,bitrate,genre,length)".to_string());
    }
    Ok(similarity)
}

pub(crate) fn parse_crop_mechanism(src: &str) -> Result<VideoCroppingMechanism, String> {
    match src.to_lowercase().as_str() {
        "blackbars" => Ok(VideoCroppingMechanism::BlackBars),
        "staticcontent" => Ok(VideoCroppingMechanism::StaticContent),
        _ => Err("Invalid crop mechanism. Allowed values: blackbars, staticcontent".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use czkawka_core::tools::similar_videos::{ALLOWED_DURATION_TOLERANCE_PCT, ALLOWED_MATCH_FRACTION};

    use super::*;

    #[test]
    fn test_parse_geometric_invariance() {
        assert_eq!(parse_geometric_invariance("off"), Ok(GeometricInvariance::Off));
        assert_eq!(parse_geometric_invariance("mirror-flip"), Ok(GeometricInvariance::MirrorFlip));
        assert_eq!(parse_geometric_invariance("mirror-flip-rotate90"), Ok(GeometricInvariance::MirrorFlipRotate90));
    }

    #[test]
    fn test_parse_finite_float_rejects_nan_and_inf() {
        let duration_tolerance = in_range(ALLOWED_DURATION_TOLERANCE_PCT, "Duration tolerance");
        let match_fraction = in_range(ALLOWED_MATCH_FRACTION, "Match fraction");
        for value in ["NaN", "nan", "inf", "+inf", "-inf", "INF", "-INF"] {
            assert!(parse_maximum_difference(value).is_err(), "value {value:?} should be rejected");
            assert!(parse_audio_maximum_difference(value).is_err(), "value {value:?} should be rejected");
            assert!(parse_minimum_segment_duration(value).is_err(), "value {value:?} should be rejected");
            assert!(duration_tolerance(value).is_err(), "value {value:?} should be rejected");
            assert!(match_fraction(value).is_err(), "value {value:?} should be rejected");
        }
    }

    #[test]
    fn test_in_range() {
        let match_fraction = in_range(ALLOWED_MATCH_FRACTION, "Match fraction");
        assert_eq!(match_fraction("0.5"), Ok(0.5));
        assert_eq!(match_fraction("1.5"), Err("Match fraction must be in range 0.0..=1.0".to_string()));
    }

    #[test]
    fn test_parse_music_duplicate_type() {
        assert_eq!(parse_music_duplicate_type(" "), Ok(MusicSimilarity::NONE));
        assert_eq!(
            parse_music_duplicate_type("Track_Title,unknown,YEAR,year"),
            Ok(MusicSimilarity::TRACK_TITLE | MusicSimilarity::YEAR)
        );
        // Parts are not trimmed
        parse_music_duplicate_type(" genre").unwrap_err();
    }
}
