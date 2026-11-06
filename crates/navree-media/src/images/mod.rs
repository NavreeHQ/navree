//! Image conversions on pure-Rust codecs: decoding with a pixel limit and
//! the EXIF orientation applied, resizing with `fast_image_resize`,
//! filters, encoding to JPEG, PNG or WebP (never copying metadata),
//! responsive variants, SVG rasterizing with resvg, and metadata
//! stripping for originals.
//!
//! These functions are what [`Media`](crate::Media) runs for each
//! conversion; they also work on their own:
//!
//! ```
//! use navree_media::images::{self, ImageOptions};
//! use navree_media::{Conversion, Fit, OutputFormat};
//!
//! # fn main() -> Result<(), navree_media::MediaError> {
//! # let logo = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50"/></svg>"#;
//! let png = images::rasterize_svg(logo, Some(400), None, &ImageOptions::default())?;
//! let thumb = Conversion::new("thumb").fit(Fit::Cover, 64, 64).format(OutputFormat::WebP);
//! let out = images::convert(&png, "image/png", &thumb, &ImageOptions::default())?;
//! assert_eq!((out.width, out.height), (64, 64));
//! assert_eq!(out.mime_type, "image/webp");
//! # Ok(())
//! # }
//! ```

mod metadata;
mod svg;

use std::io::Cursor;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::metadata::Orientation;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits,
    RgbImage,
};
use navree_config::ConfigTable;
use serde::{Deserialize, Deserializer, Serialize};

pub use metadata::strip_metadata;
pub(crate) use svg::looks_like_svg;
pub use svg::rasterize_svg;

use crate::MediaError;
use crate::conversion::{Conversion, Fit, OutputFormat};

/// Image settings (configuration section `media.images`).
///
/// ```toml
/// [media.images]
/// max_pixels = 50000000
/// responsive_widths = [320, 640, 960, 1280, 1920, 2560]
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct ImageOptions {
    /// Images (and SVG renderings) with more pixels than this are refused
    /// before they are decoded (default 50 000 000, a 50-megapixel photo).
    pub max_pixels: u64,
    /// The widths `responsive()` conversions generate, each only when it
    /// is narrower than the conversion itself (default 320, 640, 960,
    /// 1280, 1920 and 2560). From the environment, a comma-separated list.
    #[serde(deserialize_with = "widths")]
    pub responsive_widths: Vec<u32>,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            max_pixels: 50_000_000,
            responsive_widths: vec![320, 640, 960, 1280, 1920, 2560],
        }
    }
}

/// A list of widths, or one comma-separated string (an environment
/// variable).
fn widths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u32>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Widths {
        List(Vec<u32>),
        Joined(String),
    }
    match Widths::deserialize(deserializer)? {
        Widths::List(list) => Ok(list),
        Widths::Joined(joined) => joined
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<u32>().map_err(serde::de::Error::custom))
            .collect(),
    }
}

/// The type and displayed size of an image (`width` and `height` with
/// the EXIF orientation applied).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImageInfo {
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The type read from the content (`image/png`, `image/svg+xml`).
    pub mime_type: &'static str,
}

/// An encoded conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvertedImage {
    /// The encoded file.
    pub bytes: Vec<u8>,
    /// Its type (`image/webp`).
    pub mime_type: &'static str,
    /// Its extension, without the dot.
    pub extension: &'static str,
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The narrower copies of a `responsive()` conversion, narrowest
    /// first (empty otherwise).
    pub variants: Vec<ConvertedImage>,
}

/// Reads an image's type and size from its header (an SVG is parsed).
pub fn probe(bytes: &[u8]) -> Result<ImageInfo, MediaError> {
    if svg::looks_like_svg(bytes) {
        let (width, height) = svg::size(bytes)?;
        return Ok(ImageInfo {
            width,
            height,
            mime_type: "image/svg+xml",
        });
    }
    let format = guess(bytes)?;
    let (width, height) = reader(bytes)?
        .into_dimensions()
        .map_err(MediaError::image)?;
    let (width, height) = match orientation(bytes) {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        _ => (width, height),
    };
    Ok(ImageInfo {
        width,
        height,
        mime_type: format.to_mime_type(),
    })
}

/// Generates `conversion` from an image of type `mime_type` (`image/*`;
/// SVGs are rendered at the conversion's size).
pub fn convert(
    bytes: &[u8],
    mime_type: &str,
    conversion: &Conversion,
    options: &ImageOptions,
) -> Result<ConvertedImage, MediaError> {
    if mime_type == "image/svg+xml" || svg::looks_like_svg(bytes) {
        let img = svg::render_for(bytes, conversion, options)?;
        let mut conversion = conversion.clone();
        conversion.upscale = true;
        return render(img, "image/svg+xml", &conversion, options);
    }
    let img = decode(bytes, options)?;
    render(img, mime_type, conversion, options)
}

/// Decodes `bytes` with the pixel limit, applying the EXIF orientation.
/// Animated images give their first frame.
pub(crate) fn decode(bytes: &[u8], options: &ImageOptions) -> Result<DynamicImage, MediaError> {
    guess(bytes)?;
    let (width, height) = reader(bytes)?
        .into_dimensions()
        .map_err(MediaError::image)?;
    check_pixels(u64::from(width) * u64::from(height), options)?;
    let mut decoder = reader(bytes)?.into_decoder().map_err(MediaError::image)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(MediaError::image)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Sizes, filters and encodes a decoded image (`source_type` decides the
/// format a conversion without one keeps).
pub(crate) fn render(
    img: DynamicImage,
    source_type: &str,
    conversion: &Conversion,
    options: &ImageOptions,
) -> Result<ConvertedImage, MediaError> {
    let mut img = normalize(img);
    let (width, height) = target_size(img.width(), img.height(), conversion);
    if (width, height) != (img.width(), img.height()) {
        let crop = conversion.fit == Fit::Cover
            && conversion.width.is_some()
            && conversion.height.is_some();
        img = resize(&img, width, height, crop)?;
    }
    if conversion.greyscale {
        img = DynamicImage::ImageLuma8(img.to_luma8());
        img = normalize(img);
    }
    if let Some(sigma) = conversion.blur {
        img = img.fast_blur(sigma);
    }
    let format = conversion
        .format
        .unwrap_or_else(|| OutputFormat::keep(source_type));
    let mut variants = Vec::new();
    if conversion.responsive {
        let mut widths: Vec<u32> = options
            .responsive_widths
            .iter()
            .copied()
            .filter(|w| *w > 0 && *w < img.width())
            .collect();
        widths.sort_unstable();
        widths.dedup();
        for variant_width in widths {
            let variant_height = scaled(img.height(), variant_width, img.width());
            let variant = resize(&img, variant_width, variant_height, false)?;
            variants.push(ConvertedImage {
                bytes: encode(&variant, format, conversion.quality)?,
                mime_type: format.mime_type(),
                extension: format.extension(),
                width: variant.width(),
                height: variant.height(),
                variants: Vec::new(),
            });
        }
    }
    Ok(ConvertedImage {
        bytes: encode(&img, format, conversion.quality)?,
        mime_type: format.mime_type(),
        extension: format.extension(),
        width: img.width(),
        height: img.height(),
        variants,
    })
}

/// A reader for `bytes` with the format guessed from the content and a
/// 512 MiB allocation limit.
fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, MediaError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(MediaError::image)?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

/// The format of `bytes`, or an error for anything the codecs cannot read.
fn guess(bytes: &[u8]) -> Result<ImageFormat, MediaError> {
    let format =
        image::guess_format(bytes).map_err(|_| MediaError::image("unrecognized image format"))?;
    match format {
        ImageFormat::Png
        | ImageFormat::Jpeg
        | ImageFormat::Gif
        | ImageFormat::WebP
        | ImageFormat::Bmp => Ok(format),
        other => Err(MediaError::image(format!(
            "{} images are not supported",
            other.to_mime_type()
        ))),
    }
}

/// The EXIF orientation of `bytes` (none when it cannot be read).
pub(crate) fn orientation(bytes: &[u8]) -> Orientation {
    reader(bytes)
        .ok()
        .and_then(|reader| reader.into_decoder().ok())
        .and_then(|mut decoder| decoder.orientation().ok())
        .unwrap_or(Orientation::NoTransforms)
}

/// Refuses images with more than `max_pixels` pixels.
pub(crate) fn check_pixels(pixels: u64, options: &ImageOptions) -> Result<(), MediaError> {
    if pixels > options.max_pixels {
        return Err(MediaError::image(format!(
            "the image has {pixels} pixels; the limit is {}",
            options.max_pixels
        )));
    }
    Ok(())
}

/// 8-bit RGB, or RGBA when the image has transparency: the pixel types
/// the resizer and the encoders share.
fn normalize(img: DynamicImage) -> DynamicImage {
    match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => img,
        other if other.color().has_alpha() => DynamicImage::ImageRgba8(other.to_rgba8()),
        other => DynamicImage::ImageRgb8(other.to_rgb8()),
    }
}

/// The output size of `conversion` for a `width` x `height` image.
fn target_size(width: u32, height: u32, conversion: &Conversion) -> (u32, u32) {
    let (w, h) = (conversion.width, conversion.height);
    match (conversion.fit, w, h) {
        (_, None, None) => (width, height),
        (Fit::Fill, Some(w), Some(h)) => {
            if conversion.upscale || (w <= width && h <= height) {
                (w, h)
            } else {
                let k = (f64::from(width) / f64::from(w))
                    .min(f64::from(height) / f64::from(h))
                    .min(1.0);
                (round(f64::from(w) * k), round(f64::from(h) * k))
            }
        }
        (Fit::Cover, Some(w), Some(h)) => {
            if conversion.upscale {
                (w, h)
            } else {
                let k = (f64::from(width) / f64::from(w))
                    .min(f64::from(height) / f64::from(h))
                    .min(1.0);
                (round(f64::from(w) * k), round(f64::from(h) * k))
            }
        }
        _ => {
            let sx = w.map_or(f64::INFINITY, |w| f64::from(w) / f64::from(width));
            let sy = h.map_or(f64::INFINITY, |h| f64::from(h) / f64::from(height));
            let mut scale = sx.min(sy);
            if !conversion.upscale {
                scale = scale.min(1.0);
            }
            (
                round(f64::from(width) * scale),
                round(f64::from(height) * scale),
            )
        }
    }
}

/// `value * numerator / denominator`, rounded, at least 1.
fn scaled(value: u32, numerator: u32, denominator: u32) -> u32 {
    round(f64::from(value) * f64::from(numerator) / f64::from(denominator))
}

fn round(value: f64) -> u32 {
    // Sizes are small positive numbers; the cast saturates otherwise.
    (value.round() as u32).max(1)
}

/// Resizes with a Lanczos3 convolution; `crop` first trims the source to
/// the target's aspect ratio, centered.
fn resize(
    img: &DynamicImage,
    width: u32,
    height: u32,
    crop: bool,
) -> Result<DynamicImage, MediaError> {
    let mut target = DynamicImage::new(width, height, img.color());
    let mut options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    if crop {
        options = options.fit_into_destination(Some((0.5, 0.5)));
    }
    Resizer::new()
        .resize(img, &mut target, &options)
        .map_err(MediaError::image)?;
    Ok(target)
}

/// Encodes `img`; nothing but pixels is written (no EXIF, XMP or text).
pub(crate) fn encode(
    img: &DynamicImage,
    format: OutputFormat,
    quality: u8,
) -> Result<Vec<u8>, MediaError> {
    let mut out = Vec::new();
    match format {
        OutputFormat::Jpeg => {
            let rgb = flatten(img);
            JpegEncoder::new_with_quality(&mut out, quality)
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .map_err(MediaError::image)?;
        }
        OutputFormat::Png => {
            let img = normalize(img.clone());
            PngEncoder::new_with_quality(
                &mut out,
                CompressionType::Default,
                image::codecs::png::FilterType::Adaptive,
            )
            .write_image(
                img.as_bytes(),
                img.width(),
                img.height(),
                img.color().into(),
            )
            .map_err(MediaError::image)?;
        }
        OutputFormat::WebP => {
            let img = normalize(img.clone());
            WebPEncoder::new_lossless(&mut out)
                .write_image(
                    img.as_bytes(),
                    img.width(),
                    img.height(),
                    img.color().into(),
                )
                .map_err(MediaError::image)?;
        }
    }
    Ok(out)
}

/// The image on a white background (JPEG has no transparency).
fn flatten(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let alpha = u32::from(a);
        let blend = |c: u8| ((u32::from(c) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
        image::Rgb([blend(r), blend(g), blend(b)])
    })
}
