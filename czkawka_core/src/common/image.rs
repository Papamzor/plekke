use std::fmt::Debug;
use std::panic;
use std::path::Path;

use fast_image_resize::{FilterType as FirFilterType, ResizeAlg, ResizeOptions as FirResizeOptions, Resizer};
use image::metadata::Orientation;
use image::{DynamicImage, ImageReader};
use little_exif::exif_tag::ExifTag;
use little_exif::ifd::ExifTagGroup;
use little_exif::metadata::Metadata;
use log::{error, trace};
use rawler::decoders::{Decoder, RawDecodeParams};
use rawler::imgop::develop::RawDevelop;
use rawler::rawsource::RawSource;

use crate::common::consts::{HEIC_EXTENSIONS, IMAGE_RS_EXTENSIONS, RAW_IMAGE_EXTENSIONS};
use crate::common::create_crash_message;
use crate::flc;

const MAXIMUM_IMAGE_PIXELS: u32 = 2_000_000_000;
const MAXIMUM_IMAGE_FILE_SIZE: u64 = 500 * 1024 * 1024;

pub fn register_image_decoding_hooks() {
    imgalifier_heif::register_decoding_hooks();
    imgalifier_avif::register_decoding_hooks();
    jxl_oxide::integration::register_image_decoding_hook();
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ImageType {
    Normal,
    Raw,
}

/// Decodes a non-raw image from bytes; no EXIF rotation is applied.
pub(crate) fn get_dynamic_image_from_bytes(bytes: &[u8]) -> Result<DynamicImage, String> {
    ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())
}

pub struct LoadedImage {
    pub image: DynamicImage,
    pub original_width: u32,
    pub original_height: u32,
}
impl Debug for LoadedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedImage")
            .field("original_width", &self.original_width)
            .field("original_height", &self.original_height)
            .field(
                "image",
                &format!(
                    "DynamicImage of type {:?} with dimensions {}x{}",
                    self.image.color(),
                    self.image.width(),
                    self.image.height()
                ),
            )
            .finish()
    }
}

pub fn get_dynamic_image_from_path(path: &str, opts: Option<ImgResizeOptions>) -> Result<LoadedImage, String> {
    let path_lower = Path::new(path).extension().unwrap_or_default().to_string_lossy().to_lowercase();
    let image_type = if RAW_IMAGE_EXTENSIONS.iter().any(|ext| path_lower.ends_with(ext)) {
        ImageType::Raw
    } else {
        ImageType::Normal
    };

    trace!("decoding file \"{path}\"");

    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > MAXIMUM_IMAGE_FILE_SIZE
    {
        let limit_mb = MAXIMUM_IMAGE_FILE_SIZE / 1024 / 1024;
        return Err(flc!("core_image_file_too_large", size = meta.len(), limit = limit_mb));
    }

    let res = panic::catch_unwind(|| {
        let img = match image_type {
            ImageType::Normal => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                get_dynamic_image_from_bytes(&bytes).map_err(|e| flc!("core_image_open_failed", path = path, reason = e))?
            }
            ImageType::Raw => get_raw_image(path).map_err(|e| flc!("core_image_open_failed", path = path, reason = e))?,
        };

        if img.width() == 0 || img.height() == 0 {
            return Err(flc!("core_image_zero_dimensions", path = path));
        }
        if img.width() as u64 * img.height() as u64 > MAXIMUM_IMAGE_PIXELS as u64 {
            return Err(flc!("core_image_too_large", width = img.width(), height = img.height(), max = MAXIMUM_IMAGE_PIXELS));
        }

        let original_width = img.width();
        let original_height = img.height();

        if let Some(opts) = opts {
            Ok((resize_image(img, opts), original_width, original_height))
        } else {
            Ok((img, original_width, original_height))
        }
    });

    if let Ok(res) = res {
        match res {
            Ok((mut img, w, h)) => {
                let rotation = match image_type {
                    // rawler applies the orientation from the metadata of the raw file itself
                    ImageType::Raw => None,
                    ImageType::Normal => get_rotation_from_exif(path).unwrap_or({
                        // TODO - library doesn't properly read some files
                        // Also, even light problems, causes completely failing to read tags from file(e.g. invalid tag type)
                        //warn!("Failed to read EXIF rotation from {path}: {e}");
                        None
                    }),
                };
                if let Some(orientation) = rotation {
                    img.apply_orientation(orientation);
                }

                Ok(LoadedImage {
                    image: img,
                    original_width: w,
                    original_height: h,
                })
            }
            Err(e) => Err(flc!("core_image_open_failed", path = path, reason = e)),
        }
    } else {
        let message = create_crash_message("Image-rs or rawler or jxl-oxide", path, "https://github.com/image-rs/image/issues");
        error!("{message}");
        Err(message)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImgResizeOptions {
    pub max_width: u32,
    pub max_height: u32,
    pub filter: FirFilterType,
}

fn resize_image(img: DynamicImage, opts: ImgResizeOptions) -> DynamicImage {
    let orig_w = img.width();
    let orig_h = img.height();

    if orig_w <= opts.max_width && orig_h <= opts.max_height {
        return img;
    }

    let scale = f32::min(opts.max_width as f32 / orig_w as f32, opts.max_height as f32 / orig_h as f32);
    let new_w = ((orig_w as f32 * scale) as u32).max(1).min(img.width());
    let new_h = ((orig_h as f32 * scale) as u32).max(1).min(img.height());

    let mut dst = DynamicImage::new(new_w, new_h, img.color());
    let fir_opts = FirResizeOptions::new().resize_alg(ResizeAlg::Interpolation(opts.filter));

    match Resizer::new().resize(&img, &mut dst, Some(&fir_opts)) {
        Ok(()) => dst,
        Err(_) => {
            // Fall back to the image-rs built-in resizer if fast_image_resize fails, quite unlikely
            img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3)
        }
    }
}

pub(crate) fn get_raw_image<P: AsRef<Path>>(path: P) -> Result<DynamicImage, String> {
    let raw_source = RawSource::new(path.as_ref()).map_err(|err| format!("Failed to create RawSource from path {}: {err}", path.as_ref().to_string_lossy()))?;
    decode_raw_source(&raw_source)
}

fn decode_raw_source(raw_source: &RawSource) -> Result<DynamicImage, String> {
    let mut timer = crate::helpers::debug_timer::Timer::new("Rawler");

    let decoder = rawler::get_decoder(raw_source).map_err(|e| e.to_string())?;
    timer.checkpoint("Got decoder");

    let params = RawDecodeParams::default();
    let mut image = match decoder.full_image(raw_source, &params).ok().flatten() {
        Some(embedded_image) => embedded_image,
        None => develop_raw_image(decoder.as_ref(), raw_source, &params)?,
    };
    timer.checkpoint("Decoded image");

    if let Some(orientation) = decoder
        .raw_metadata(raw_source, &params)
        .ok()
        .and_then(|metadata| metadata.exif.orientation)
        .and_then(orientation_from_exif)
    {
        image.apply_orientation(orientation);
    }

    trace!("{}", timer.report("Everything", false));

    Ok(image)
}

fn develop_raw_image(decoder: &dyn Decoder, raw_source: &RawSource, params: &RawDecodeParams) -> Result<DynamicImage, String> {
    let raw_image = decoder.raw_image(raw_source, params, false).map_err(|e| e.to_string())?;
    let developed_image = RawDevelop::default().develop_intermediate(&raw_image).map_err(|e| e.to_string())?;
    developed_image.to_dynamic_image().ok_or_else(|| "Failed to convert image to DynamicImage".to_string())
}

pub fn check_if_can_display_image(path: &str) -> bool {
    let Some(extension) = Path::new(path).extension() else {
        return false;
    };
    let extension_str = extension.to_string_lossy().to_lowercase();
    let allowed_extensions = &[IMAGE_RS_EXTENSIONS, RAW_IMAGE_EXTENSIONS, HEIC_EXTENSIONS].concat();

    allowed_extensions.iter().any(|ext| &extension_str == ext)
}

fn orientation_from_exif(value: u16) -> Option<Orientation> {
    u8::try_from(value).ok().and_then(Orientation::from_exif)
}

pub(crate) fn get_rotation_from_exif(path: &str) -> Result<Option<Orientation>, std::io::Error> {
    if let Some(extension) = Path::new(path).extension()
        && HEIC_EXTENSIONS.contains(&extension.to_string_lossy().to_lowercase().as_str())
    {
        return Ok(None); // the HEIF decoder already applies orientation
    }

    let res = panic::catch_unwind(|| {
        let metadata = match Metadata::new_from_path(Path::new(path)) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => return Ok(None),
            Err(e) if e.to_string().contains("No EXIF data") => return Ok(None),
            Err(e) if e.to_string().contains("No metadata found") => return Ok(None),
            Err(e) => return Err(e),
        };

        if let Some(ExifTag::Orientation(values)) = metadata.get_tag_by_hex(0x0112, Some(ExifTagGroup::GENERIC)).next()
            && let Some(&value) = values.first()
        {
            return Ok(orientation_from_exif(value));
        }
        Ok(None)
    });

    res.unwrap_or_else(|_| {
        let message = create_crash_message("little-exif", path, "https://github.com/TechnikTobi/little_exif");
        error!("{message}");
        Err(std::io::Error::other("Panic in get_rotation_from_exif"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NORMAL_IMAGE: &str = "test_resources/images/normal.jpg";
    const TEST_ROTATED_IMAGE: &str = "test_resources/images/rotated.jpg";

    #[test]
    fn test_image_loading_and_exif_rotation() {
        let normal_img = get_dynamic_image_from_path(TEST_NORMAL_IMAGE, None).unwrap().image;
        let rotated_img = get_dynamic_image_from_path(TEST_ROTATED_IMAGE, None).unwrap().image;

        assert!(normal_img.width() > 0 && normal_img.height() > 0);
        assert!(rotated_img.width() > 0 && rotated_img.height() > 0);

        let normal_exif = get_rotation_from_exif(TEST_NORMAL_IMAGE).ok();
        let rotated_exif = get_rotation_from_exif(TEST_ROTATED_IMAGE).ok();

        if let Some(normal_orientation) = normal_exif {
            assert!(normal_orientation == Some(Orientation::NoTransforms) || normal_orientation.is_none());
        }

        if let Some(rotated_orientation) = rotated_exif
            && rotated_orientation.is_some()
        {
            let raw_bytes = std::fs::read(TEST_ROTATED_IMAGE).unwrap();
            let raw_rotated = get_dynamic_image_from_bytes(&raw_bytes).unwrap();
            if rotated_orientation == Some(Orientation::Rotate90) || rotated_orientation == Some(Orientation::Rotate270) {
                assert_eq!(rotated_img.width(), raw_rotated.height());
                assert_eq!(rotated_img.height(), raw_rotated.width());
            }
        }
    }

    #[test]
    fn test_orientation_from_exif_values() {
        assert_eq!(orientation_from_exif(1), Some(Orientation::NoTransforms));
        assert_eq!(orientation_from_exif(6), Some(Orientation::Rotate90));
        assert_eq!(orientation_from_exif(8), Some(Orientation::Rotate270));
        assert_eq!(orientation_from_exif(0), None);
        assert_eq!(orientation_from_exif(9), None);
        assert_eq!(orientation_from_exif(257), None);
    }

    #[test]
    fn test_raw_decoding_of_invalid_data_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.cr2");
        std::fs::write(&path, b"not a raw image").unwrap();
        get_raw_image(&path).unwrap_err();
    }

    #[test]
    fn test_check_if_can_display_image() {
        assert!(check_if_can_display_image("test.jpg"));
        assert!(check_if_can_display_image("test.png"));
        assert!(check_if_can_display_image("test.webp"));
        assert!(check_if_can_display_image("test.jxl"));
        assert!(check_if_can_display_image("test.cr2"));
        assert!(check_if_can_display_image("test.dng"));
        assert!(check_if_can_display_image("test.JPG"));

        assert!(!check_if_can_display_image("test.txt"));
        assert!(!check_if_can_display_image("test.mp4"));
        assert!(!check_if_can_display_image("test"));
    }

    #[test]
    fn test_error_handling() {
        get_dynamic_image_from_path("nonexistent.jpg", None).unwrap_err();
        get_dynamic_image_from_bytes(b"not an image").unwrap_err();
        get_rotation_from_exif("nonexistent.jpg").unwrap_err();
    }
}
