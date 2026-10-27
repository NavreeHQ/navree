//! SVG rendering with resvg: never loads files or URLs, sized to a
//! conversion, under the pixel limit.

use std::sync::{Arc, OnceLock};

use image::{DynamicImage, RgbaImage};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, ImageHrefResolver, Options, Tree};

use super::{ImageOptions, check_pixels, encode};
use crate::MediaError;
use crate::conversion::{Conversion, Fit, OutputFormat};

/// The system fonts, loaded once, for SVGs with text.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// Parsing options: system fonts, embedded `data:` images only (an
/// `href` naming a file or a URL draws nothing).
fn options() -> Options<'static> {
    Options {
        resources_dir: None,
        image_href_resolver: ImageHrefResolver {
            resolve_data: ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: fonts(),
        ..Options::default()
    }
}

/// Whether `bytes` look like an SVG document: an `<svg` element in the
/// first kilobytes of a text file.
pub(crate) fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let Ok(text) = std::str::from_utf8(head).or_else(|error| {
        // A multi-byte character cut at the boundary.
        std::str::from_utf8(&head[..error.valid_up_to()])
    }) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    (text.starts_with('<')) && text.contains("<svg")
}

fn parse(bytes: &[u8]) -> Result<Tree, MediaError> {
    Tree::from_data(bytes, &options()).map_err(|error| MediaError::image(format!("SVG: {error}")))
}

/// The SVG's intrinsic size, rounded up.
pub(crate) fn size(bytes: &[u8]) -> Result<(u32, u32), MediaError> {
    let tree = parse(bytes)?;
    let size = tree.size();
    Ok((ceil(size.width()), ceil(size.height())))
}

fn ceil(value: f32) -> u32 {
    (value.ceil() as u32).max(1)
}

/// Renders an SVG as a PNG: at `width` and/or `height` (keeping the
/// aspect ratio when only one is given; stretching when both are), or at
/// its intrinsic size.
pub fn rasterize_svg(
    bytes: &[u8],
    width: Option<u32>,
    height: Option<u32>,
    options: &ImageOptions,
) -> Result<Vec<u8>, MediaError> {
    let mut conversion = Conversion::new("svg");
    conversion.width = width;
    conversion.height = height;
    conversion.fit = if width.is_some() && height.is_some() {
        Fit::Fill
    } else {
        Fit::Contain
    };
    let img = render_for(bytes, &conversion, options)?;
    encode(&img, OutputFormat::Png, 100)
}

/// Renders an SVG at the size `conversion` asks for (for `Cover`, large
/// enough to cover the box; the caller crops).
pub(crate) fn render_for(
    bytes: &[u8],
    conversion: &Conversion,
    options: &ImageOptions,
) -> Result<DynamicImage, MediaError> {
    let tree = parse(bytes)?;
    let size = tree.size();
    let (sw, sh) = (f64::from(size.width()), f64::from(size.height()));
    let (sx, sy) = match (conversion.fit, conversion.width, conversion.height) {
        (_, None, None) => (1.0, 1.0),
        (Fit::Fill, Some(w), Some(h)) => (f64::from(w) / sw, f64::from(h) / sh),
        (Fit::Cover, Some(w), Some(h)) => {
            let scale = (f64::from(w) / sw).max(f64::from(h) / sh);
            (scale, scale)
        }
        (_, w, h) => {
            let scale = w
                .map_or(f64::INFINITY, |w| f64::from(w) / sw)
                .min(h.map_or(f64::INFINITY, |h| f64::from(h) / sh));
            (scale, scale)
        }
    };
    let width = ((sw * sx).round() as u32).max(1);
    let height = ((sh * sy).round() as u32).max(1);
    check_pixels(u64::from(width) * u64::from(height), options)?;
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| MediaError::image("SVG: the rendering has no pixels"))?;
    resvg::render(
        &tree,
        Transform::from_scale(sx as f32, sy as f32),
        &mut pixmap.as_mut(),
    );
    let rgba = RgbaImage::from_raw(width, height, pixmap.take_demultiplied())
        .ok_or_else(|| MediaError::image("SVG: the rendering has the wrong size"))?;
    Ok(DynamicImage::ImageRgba8(rgba))
}
