//! Charts as pictures, drawn on the server: a PNG for a place that shows only an image,
//! such as a home-screen widget, an email or an Open Graph card. The `image` feature.
//!
//! [`ChartImage`] takes a toolkit [`Node`] that draws with SVG (a ring, a sparkline, a
//! gauge, a [`Tile`] of several) and draws it at a size, in a theme and a mode. A page
//! colours a chart from its stylesheets; an image has none, so every colour is resolved
//! from the theme's tokens first, light or dark, with the accent of your choice, and
//! written into the drawing. The words are set in Geist, the toolkit's own typeface,
//! which is built in (with the rupee sign from Noto Sans), so no font on the server is
//! read and the same inputs always make the same bytes.
//!
//! ```
//! use navree_ui::charts::Ring;
//! use navree_ui::image::ChartImage;
//! use navree_ui::theme::{Mode, Theme};
//!
//! let ring = Ring::new("Recovery", 82).max(100).unit("%");
//! let png = ChartImage::new(240, 240)
//!     .theme(&Theme::NAVREE)
//!     .mode(Mode::Light)
//!     .png(&ring.into())
//!     .expect("a ring draws");
//! assert!(png.bytes().starts_with(b"\x89PNG"));
//! ```
//!
//! What a page draws around its drawing in HTML (a ring's number in the middle, a
//! legend, a chart's title) is not in the picture: a [`Tile`] sets its own words, in
//! SVG. A route answers with [`ChartImage::respond`], which tags the picture with an
//! `ETag` made from everything that draws it and answers a request that already holds
//! that picture with `304 Not Modified`, before drawing anything.
//!
//! Rendering is pure Rust (resvg on tiny-skia), and costs nothing in an app that leaves
//! the feature off.

mod svg;
mod tile;

use std::sync::{Arc, LazyLock};

use http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, HeaderValue, IF_NONE_MATCH};
use http::{HeaderMap, StatusCode};
use navree_http::response::{IntoResponse, Response};
use resvg::{tiny_skia, usvg};
use sha2::{Digest, Sha256};

use crate::html::SafeHtml;
use crate::node::Node;
use crate::theme::{Mode, Rgb, Side, Theme, Token};

pub use tile::{Block, Tile, TileSize};

use svg::{Inks, Writer};

/// The toolkit's typeface as TrueType: the glyphs and the weight axis of the page's own
/// `geist.woff2`, which a renderer can't read.
static GEIST: &[u8] = include_bytes!("../../assets/fonts/geist.ttf");
/// The rupee sign, which Geist lacks, from Noto Sans.
static RUPEE: &[u8] = include_bytes!("../../assets/fonts/noto-rupee.ttf");

/// What changes every picture's bytes besides its inputs: the renderer and the fonts.
const RENDERER: &str = concat!("navree-ui ", env!("CARGO_PKG_VERSION"), " resvg 0.49 geist");

/// The largest side of a picture, in pixels.
pub const MAX_SIDE: u32 = 4096;

/// The fonts a picture may use: these two, and never one of the server's.
static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_font_data(GEIST.to_vec());
    fonts.load_font_data(RUPEE.to_vec());
    // Every generic family is Geist: a drawing that names another font still draws.
    fonts.set_sans_serif_family("Geist");
    fonts.set_serif_family("Geist");
    fonts.set_monospace_family("Geist");
    fonts.set_cursive_family("Geist");
    fonts.set_fantasy_family("Geist");
    Arc::new(fonts)
});

/// Why a picture wasn't drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageError {
    /// The node holds no `<svg>` with anything in it.
    NoDrawing,
    /// The size is zero, or larger than [`MAX_SIDE`] once scaled.
    Size,
    /// The renderer couldn't read the drawing.
    Svg(String),
    /// The picture couldn't be written as a PNG.
    Encode(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDrawing => f.write_str("the node draws no SVG"),
            Self::Size => write!(f, "a picture is 1 to {MAX_SIDE} pixels a side"),
            Self::Svg(why) => write!(f, "the drawing couldn't be read: {why}"),
            Self::Encode(why) => write!(f, "the picture couldn't be written: {why}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// What a picture sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Backdrop {
    /// The page's colour (`ink-0`). The default.
    #[default]
    Page,
    /// A card's colour (`ink-1`), as a tile on a home screen.
    Card,
    /// Nothing: a clear picture, for a place that brings its own colour.
    Clear,
}

/// A picture of a drawing, at a size, in a theme and a mode.
///
/// ```
/// use navree_ui::charts::Sparkline;
/// use navree_ui::image::{Backdrop, ChartImage};
/// use navree_ui::theme::{Mode, Rgb, Theme};
///
/// let image = ChartImage::new(320, 64)
///     .theme(&Theme::OCEAN)
///     .mode(Mode::Dark)
///     .accent(Rgb::hex(0x22d3ee))
///     .backdrop(Backdrop::Card)
///     .scale(2);
/// let line = Sparkline::new("Resting heart rate", [62, 61, 60, 59, 61, 58]).height(64);
/// let png = image.png(&line.into()).unwrap();
/// assert_eq!((png.width(), png.height()), (640, 128));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ChartImage {
    width: u32,
    height: u32,
    scale: u32,
    theme: Theme,
    side: Side,
    accent: Option<Rgb>,
    backdrop: Backdrop,
    cache: &'static str,
}

impl ChartImage {
    /// A picture `width` by `height` pixels (before [`scale`](Self::scale)), in the
    /// default theme, dark, on the page's colour.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            scale: 1,
            theme: Theme::NAVREE,
            side: Side::Dark,
            accent: None,
            backdrop: Backdrop::Page,
            cache: "no-cache",
        }
    }

    /// The picture of `tile`, at its size.
    pub fn for_tile(tile: &Tile) -> Self {
        let (width, height) = tile.size().pixels();
        Self::new(width, height).backdrop(Backdrop::Card)
    }

    /// Draws in `theme`'s colours.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.theme = theme.clone();
        self
    }

    /// Draws light or dark. A picture can't follow a device, so [`Mode::System`] is
    /// dark, the toolkit's default.
    pub fn mode(mut self, mode: Mode) -> Self {
        self.side = match mode {
            Mode::Light => Side::Light,
            _ => Side::Dark,
        };
        self
    }

    /// Makes `colour` the accent, with the text on it and the accent as text derived
    /// from it as the theme builder derives them. What is drawn in the accent
    /// (`Ink::accent()`, every block of a [`Tile`] that names no colour) takes it.
    pub fn accent(mut self, colour: Rgb) -> Self {
        self.accent = Some(colour);
        self
    }

    /// What the picture sits on.
    pub fn backdrop(mut self, backdrop: Backdrop) -> Self {
        self.backdrop = backdrop;
        self
    }

    /// Draws `factor` pixels for each one of the size, for a screen that packs 2 or 3
    /// into a point. 1 to 4.
    pub fn scale(mut self, factor: u32) -> Self {
        self.scale = factor.clamp(1, 4);
        self
    }

    /// The `Cache-Control` of [`respond`](Self::respond)'s answers: `no-cache` by
    /// default, so a client asks each time and gets `304 Not Modified` while the
    /// picture is the same. A picture of private data should say `private, no-cache`.
    pub fn cache_control(mut self, value: &'static str) -> Self {
        self.cache = value;
        self
    }

    /// The size in pixels, scale included.
    pub fn pixels(&self) -> (u32, u32) {
        (
            self.width.saturating_mul(self.scale),
            self.height.saturating_mul(self.scale),
        )
    }

    fn inks(&self) -> Inks {
        let mut palette = *self.theme.spec().palette(self.side);
        if let Some(accent) = self.accent {
            palette.derive_accent(accent);
        }
        let surface = match self.backdrop {
            Backdrop::Card => palette.get(Token::Ink1),
            _ => palette.get(Token::Ink0),
        };
        Inks {
            palette,
            side: self.side,
            surface,
        }
    }

    /// The drawing as a standalone SVG document, every colour resolved: what the PNG is
    /// drawn from, and a picture in its own right for a place that shows SVG.
    ///
    /// # Errors
    ///
    /// [`ImageError::NoDrawing`] when `node` holds no `<svg>` with anything in it, and
    /// [`ImageError::Size`] for a side of 0 or past [`MAX_SIDE`].
    pub fn svg(&self, node: &Node) -> Result<String, ImageError> {
        let (pw, ph) = self.pixels();
        if self.width == 0 || self.height == 0 || pw > MAX_SIDE || ph > MAX_SIDE {
            return Err(ImageError::Size);
        }
        let inks = self.inks();
        let background = match self.backdrop {
            Backdrop::Clear => None,
            _ => Some(inks.surface),
        };
        Writer::new(&inks)
            .document(node, self.width, self.height, background)
            .ok_or(ImageError::NoDrawing)
    }

    /// The same document as a node, to put in a page as it is: it fills the width it is
    /// given and keeps its shape.
    ///
    /// # Errors
    ///
    /// As [`svg`](Self::svg).
    pub fn svg_node(&self, node: &Node) -> Result<Node, ImageError> {
        let prefix = self.ids_of(&self.svg(node)?);
        let inks = self.inks();
        let background = match self.backdrop {
            Backdrop::Clear => None,
            _ => Some(inks.surface),
        };
        let svg = Writer::new(&inks)
            .ids(prefix)
            .document(node, self.width, self.height, background)
            .ok_or(ImageError::NoDrawing)?;
        let sized = format!(r#"width="{}" height="{}" "#, self.width, self.height);
        // The document is written by this module alone, every text and value escaped.
        Ok(Node::Raw(SafeHtml::trusted(svg.replacen(
            &sized,
            r#"width="100%" "#,
            1,
        ))))
    }

    /// What every id of a drawing put in a page starts with: `i` and ten hex digits of a
    /// hash of the drawing and its size. Two drawings that differ share no id. The
    /// renderer is no part of it, as it is of the picture's tag: that changes with every
    /// release of the toolkit, and the HTML of a page that draws the same thing must not.
    fn ids_of(&self, svg: &str) -> String {
        let (pw, ph) = self.pixels();
        let mut hash = Sha256::new();
        hash.update(format!("{pw}x{ph}|").as_bytes());
        hash.update(svg.as_bytes());
        let digest = hash.finalize();
        let mut out = String::with_capacity(11);
        out.push('i');
        for byte in &digest[..5] {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }

    /// The tag of the picture of `node`: a hash of the resolved drawing, the size and
    /// the renderer, quoted as an `ETag` is. The same inputs always give the same tag,
    /// and it is made without drawing anything.
    ///
    /// # Errors
    ///
    /// As [`svg`](Self::svg).
    pub fn etag(&self, node: &Node) -> Result<String, ImageError> {
        self.svg(node).map(|svg| self.tag_of(&svg))
    }

    fn tag_of(&self, svg: &str) -> String {
        let (pw, ph) = self.pixels();
        let mut hash = Sha256::new();
        hash.update(RENDERER.as_bytes());
        hash.update(format!("|{pw}x{ph}|").as_bytes());
        hash.update(svg.as_bytes());
        let digest = hash.finalize();
        let mut out = String::with_capacity(34);
        out.push('"');
        for byte in &digest[..16] {
            out.push_str(&format!("{byte:02x}"));
        }
        out.push('"');
        out
    }

    /// The picture of `node`, as a PNG.
    ///
    /// # Errors
    ///
    /// As [`svg`](Self::svg), and [`ImageError::Svg`] or [`ImageError::Encode`] when the
    /// renderer refuses the drawing.
    pub fn png(&self, node: &Node) -> Result<Png, ImageError> {
        let svg = self.svg(node)?;
        let etag = self.tag_of(&svg);
        let (width, height) = self.pixels();
        let bytes = rasterise(&svg, width, height, self.scale)?;
        Ok(Png {
            bytes,
            width,
            height,
            etag,
            cache: self.cache,
        })
    }

    /// The answer to a request for the picture of `node`: `304 Not Modified` when the
    /// request's `If-None-Match` holds its tag (nothing is drawn), otherwise the PNG
    /// with its `ETag` and `Cache-Control`. A drawing that can't be made answers 500.
    pub fn respond(&self, node: &Node, request: &HeaderMap) -> Response {
        let svg = match self.svg(node) {
            Ok(svg) => svg,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let etag = self.tag_of(&svg);
        if matches_tag(request, &etag) {
            return not_modified(&etag, self.cache);
        }
        let (width, height) = self.pixels();
        match rasterise(&svg, width, height, self.scale) {
            Ok(bytes) => Png {
                bytes,
                width,
                height,
                etag,
                cache: self.cache,
            }
            .into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

/// The picture of `chart` at `width` by `height` pixels, in `theme` and `mode`, on the
/// page's colour: [`ChartImage`] with nothing else set.
///
/// # Errors
///
/// As [`ChartImage::png`].
pub fn png(
    chart: impl Into<Node>,
    width: u32,
    height: u32,
    theme: &Theme,
    mode: Mode,
) -> Result<Png, ImageError> {
    ChartImage::new(width, height)
        .theme(theme)
        .mode(mode)
        .png(&chart.into())
}

/// A picture, as PNG bytes, with its tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Png {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    etag: String,
    cache: &'static str,
}

impl Png {
    /// The file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The file, taken.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Its width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Its height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Its `ETag`, quoted: the same for the same inputs.
    pub fn etag(&self) -> &str {
        &self.etag
    }

    /// The answer to `request`: `304 Not Modified` when its `If-None-Match` holds this
    /// picture's tag, otherwise the picture.
    pub fn respond(self, request: &HeaderMap) -> Response {
        if matches_tag(request, &self.etag) {
            return not_modified(&self.etag, self.cache);
        }
        self.into_response()
    }
}

impl IntoResponse for Png {
    fn into_response(self) -> Response {
        let etag = HeaderValue::from_str(&self.etag)
            .unwrap_or_else(|_| HeaderValue::from_static("\"navree-ui-image\""));
        (
            [
                (CONTENT_TYPE, HeaderValue::from_static("image/png")),
                (CACHE_CONTROL, HeaderValue::from_static(self.cache)),
                (ETAG, etag),
            ],
            self.bytes,
        )
            .into_response()
    }
}

/// Whether `request`'s `If-None-Match` names `etag`, or is `*`.
fn matches_tag(request: &HeaderMap, etag: &str) -> bool {
    request
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|tag| tag.trim().trim_start_matches("W/"))
        .any(|tag| tag == etag || tag == "*")
}

fn not_modified(etag: &str, cache: &'static str) -> Response {
    let etag = HeaderValue::from_str(etag)
        .unwrap_or_else(|_| HeaderValue::from_static("\"navree-ui-image\""));
    (
        StatusCode::NOT_MODIFIED,
        [
            (ETAG, etag),
            (CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
    )
        .into_response()
}

/// Draws `svg` into a `width` by `height` PNG, `scale` pixels to a unit.
fn rasterise(svg: &str, width: u32, height: u32, scale: u32) -> Result<Vec<u8>, ImageError> {
    let options = usvg::Options {
        font_family: "Geist".to_owned(),
        fontdb: Arc::clone(&FONTS),
        languages: vec!["en".to_owned()],
        // Nothing is read from a file or from data: a drawing of the toolkit's draws no
        // picture, and one from anywhere else must not make the server read one.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        resources_dir: None,
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| ImageError::Svg(e.to_string()))?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(ImageError::Size)?;
    // At most 4: an exact conversion.
    #[allow(clippy::cast_precision_loss)]
    let factor = scale as f32;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(factor, factor),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| ImageError::Encode(e.to_string()))
}

#[cfg(test)]
mod tests;
