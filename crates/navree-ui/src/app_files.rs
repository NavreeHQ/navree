//! [`AppSheet`], [`AppFont`] and [`AppImage`]: the app's own stylesheet, typefaces and
//! pictures (a logo, a mark), served by the toolkit the way it serves its own files.
//!
//! The page's content security policy lets a page load styles and fonts from its own
//! origin only, so a brand's art direction (a hero's backdrop, a different typeface,
//! section rhythm the components don't have) can't be written inline. It is a file the
//! app compiles in and the toolkit serves, named by its content hash and cached for good,
//! and a page links it with [`View::stylesheet`](crate::View::stylesheet) and
//! [`View::font`](crate::View::font) (or the same methods of a [`Shell`](crate::Shell) or
//! an [`AppPage`](crate::AppPage)).
//!
//! The files come after everything the toolkit links, the page's theme sheet included, so
//! a rule of the app's wins over a rule of equal weight. A theme of your own sets the
//! brand's colours, corners, density and text face from the toolkit's list through
//! [`Theme::custom`](crate::theme::Theme::custom), which refuses a palette whose text
//! falls short of WCAG AA; the app's sheet adds what a theme can't say, and keeps to the
//! toolkit's tokens (`var(--accent)`, `var(--ink-1)`) so it follows light and dark.
//!
//! ```
//! use navree_ui::theme::{Appearance, Mode, Palette, Rgb, Theme, ThemeId};
//! use navree_ui::{AppFont, AppSheet, View, el};
//!
//! static BRAND: AppSheet = AppSheet::new(
//!     "brand",
//!     b".hero{background:linear-gradient(135deg,var(--accent),var(--ink-2))}",
//!     16 * 1024,
//! );
//! // A woff2 file starts with its signature; a real app includes one with `include_bytes!`.
//! static DISPLAY: AppFont = AppFont::new("display", "Fraunces", b"wOF2....", 200 * 1024)
//!     .weights(400, 700);
//!
//! // The brand's accent, in both palettes, with the text on it and the focus ring derived.
//! let mut spec = *Theme::NAVREE.spec();
//! spec = spec.with_accent(Rgb::hex(0x2dd4bf), Rgb::hex(0x0f766e));
//! let theme = Theme::custom(ThemeId::parse("brand").unwrap(), "Brand", spec).unwrap();
//!
//! let page = View::new("Today")
//!     .appearance(Appearance::new(theme, Mode::System))
//!     .font(&DISPLAY)
//!     .stylesheet(&BRAND)
//!     .body(el("main").attr("class", "hero").child("Hello"));
//! let html = page.to_html();
//! assert!(html.contains(&BRAND.url()));
//! assert!(html.contains(r#"rel="preload" as="font""#));
//! ```
//!
//! The app hands both to the toolkit from one of its modules, so the toolkit serves them:
//! `m.contribute(BRAND)` and `m.contribute(DISPLAY)`.
//!
//! A picture every page draws, such as the app's mark, is an [`AppImage`] the same way:
//! compiled in, named by its hash, cached for good, and contributed with `m.contribute(MARK)`.
//!
//! ```
//! use navree_ui::{AppImage, Shell};
//!
//! static MARK: AppImage = AppImage::new(
//!     "mark",
//!     br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14"/></svg>"#,
//!     8 * 1024,
//! );
//! assert!(MARK.url().ends_with(".svg"));
//! let html = Shell::new("Kesar").logo(&MARK).page("Today", "/").to_view().to_html();
//! assert!(html.contains(&format!(r#"<img src="{}""#, MARK.url())));
//! ```

use bytes::Bytes;
use http::HeaderMap;
use navree_http::response::{IntoResponse, Response};

use crate::assets;

/// Where the app's files are served, under the toolkit's mount.
pub const APP_PATH: &str = "/_navree/ui/app";

/// The FNV-1a hash the toolkit's files carry, over `bytes`, continuing `hash`.
const fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    let mut at = 0;
    while at < bytes.len() {
        hash ^= bytes[at] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        at += 1;
    }
    hash
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// A hash as the first 12 hex digits, as the toolkit's files carry.
fn short(hash: u64) -> String {
    let mut out = format!("{hash:016x}");
    out.truncate(12);
    out
}

/// Whether `name` is 1 to 64 lowercase letters, digits and `-`: what a file's name may be.
const fn is_file_name(name: &str) -> bool {
    let letters = name.as_bytes();
    if letters.is_empty() || letters.len() > 64 {
        return false;
    }
    let mut at = 0;
    while at < letters.len() {
        if !matches!(letters[at], b'a'..=b'z' | b'0'..=b'9' | b'-') {
            return false;
        }
        at += 1;
    }
    true
}

/// A stylesheet of the app's own, compiled in with `include_bytes!` and served by the
/// toolkit at `/_navree/ui/app/{name}-{hash}.css` as `immutable`. See [the module](self).
///
/// A page that links it ([`View::stylesheet`](crate::View::stylesheet)) loads it after the
/// toolkit's sheets and the page's theme. Its size is checked when the app compiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppSheet {
    name: &'static str,
    bytes: &'static [u8],
    hash: u64,
}

impl AppSheet {
    /// The stylesheet `bytes`, named `name` (1 to 64 lowercase letters, digits and `-`), at
    /// most `most` bytes long. A sheet past its ceiling, or a name of anything else, fails
    /// the app's build.
    pub const fn new(name: &'static str, bytes: &'static [u8], most: usize) -> Self {
        assert!(
            is_file_name(name),
            "a stylesheet's name has 1 to 64 lowercase letters, digits and `-`"
        );
        assert!(
            bytes.len() <= most,
            "a stylesheet is larger than its ceiling"
        );
        Self {
            name,
            bytes,
            hash: fnv(FNV_START, bytes),
        }
    }

    /// Its name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The stylesheet.
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The first 12 hex digits of its content hash, as the toolkit's own files carry.
    pub fn hash(&self) -> String {
        short(self.hash)
    }

    /// Where it is served: `/_navree/ui/app/{name}-{hash}.css`.
    pub fn url(&self) -> String {
        format!("{APP_PATH}/{}-{}.css", self.name, self.hash())
    }
}

/// How a browser treats text in a font that hasn't loaded: the `font-display` of its face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontDisplay {
    /// Text shows at once in the fallback and swaps when the font arrives: the default,
    /// right for a brand's headings and body text.
    #[default]
    Swap,
    /// A very short wait for the font, then the fallback for good if it hasn't come: no
    /// swap, so the page never jumps. The toolkit's own Geist is drawn this way.
    Optional,
    /// A short wait, then the fallback, and a swap if the font arrives soon enough.
    Fallback,
    /// Invisible text until the font arrives: only for an icon font.
    Block,
}

impl FontDisplay {
    /// The `font-display` value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Swap => "swap",
            Self::Optional => "optional",
            Self::Fallback => "fallback",
            Self::Block => "block",
        }
    }
}

/// A typeface of the app's own: one woff2 file, compiled in with `include_bytes!`, served
/// at `/_navree/ui/app/{name}-{hash}.woff2` as `immutable`, with the `@font-face` rule
/// that names it. See [the module](self).
///
/// A page that links it ([`View::font`](crate::View::font)) preloads the file, so the text
/// is drawn in it on first paint, and links the rule, which says `font-display` (swap by
/// default). Use the family in the app's stylesheet: `font-family: "Fraunces", serif`, or
/// as the page's text face with `:root { --font-sans: "Fraunces", system-ui, sans-serif }`.
/// A variable font gives a range of [`weights`](Self::weights).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppFont {
    name: &'static str,
    family: &'static str,
    bytes: &'static [u8],
    weight: (u16, u16),
    italic: bool,
    display: FontDisplay,
    preload: bool,
    file_hash: u64,
    face_hash: u64,
}

impl AppFont {
    /// The woff2 file `bytes`, named `name` (1 to 64 lowercase letters, digits and `-`),
    /// for the CSS family `family` (letters, digits, spaces and `-`, at most 64), at most
    /// `most` bytes long. A file that isn't woff2, past its ceiling, or a name or family of
    /// anything else, fails the app's build. Regular weight, upright, `font-display: swap`
    /// and preloaded until you say otherwise.
    pub const fn new(
        name: &'static str,
        family: &'static str,
        bytes: &'static [u8],
        most: usize,
    ) -> Self {
        assert!(
            is_file_name(name),
            "a font's name has 1 to 64 lowercase letters, digits and `-`"
        );
        let letters = family.as_bytes();
        assert!(
            !letters.is_empty() && letters.len() <= 64,
            "a font's family has 1 to 64 characters"
        );
        let mut at = 0;
        while at < letters.len() {
            assert!(
                matches!(letters[at], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'-'),
                "a font's family has letters, digits, spaces and `-` only"
            );
            at += 1;
        }
        assert!(bytes.len() <= most, "a font is larger than its ceiling");
        assert!(
            bytes.len() >= 4
                && bytes[0] == b'w'
                && bytes[1] == b'O'
                && bytes[2] == b'F'
                && bytes[3] == b'2',
            "a font is a woff2 file"
        );
        Self {
            name,
            family,
            bytes,
            weight: (400, 400),
            italic: false,
            display: FontDisplay::Swap,
            preload: true,
            file_hash: fnv(FNV_START, bytes),
            face_hash: 0,
        }
        .rehash()
    }

    /// Its weight: `600` for a semibold file.
    pub const fn weight(mut self, weight: u16) -> Self {
        assert!(weight >= 1 && weight <= 1000, "a weight is 1 to 1000");
        self.weight = (weight, weight);
        self.rehash()
    }

    /// The weights a variable file covers: `weights(100, 900)`.
    pub const fn weights(mut self, from: u16, to: u16) -> Self {
        assert!(
            from >= 1 && from <= to && to <= 1000,
            "weights run from 1 to 1000, lowest first"
        );
        self.weight = (from, to);
        self.rehash()
    }

    /// An italic file.
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self.rehash()
    }

    /// What the browser does while the font loads (`swap` by default).
    pub const fn display(mut self, display: FontDisplay) -> Self {
        self.display = display;
        self.rehash()
    }

    /// Leaves the file out of the page's head as a preload: a font only some pages use is
    /// fetched when the text that needs it is drawn.
    pub const fn lazy(mut self) -> Self {
        self.preload = false;
        self
    }

    /// The face's hash follows the file and everything its rule says, so the rule's
    /// address changes whenever the rule does.
    const fn rehash(mut self) -> Self {
        let mut hash = self.file_hash;
        hash = fnv(hash, self.family.as_bytes());
        hash = fnv(hash, &self.weight.0.to_le_bytes());
        hash = fnv(hash, &self.weight.1.to_le_bytes());
        hash = fnv(hash, &[self.italic as u8]);
        hash = fnv(hash, self.display.as_str().as_bytes());
        self.face_hash = hash;
        self
    }

    /// Its name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The CSS family it is called.
    pub const fn family(&self) -> &'static str {
        self.family
    }

    /// The font file.
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// Whether a page preloads it.
    pub const fn preloads(&self) -> bool {
        self.preload
    }

    /// Where the file is served: `/_navree/ui/app/{name}-{hash}.woff2`.
    pub fn url(&self) -> String {
        format!("{APP_PATH}/{}-{}.woff2", self.name, short(self.file_hash))
    }

    /// Where its `@font-face` rule is served: `/_navree/ui/app/{name}.face-{hash}.css`.
    pub fn face_url(&self) -> String {
        format!(
            "{APP_PATH}/{}.face-{}.css",
            self.name,
            short(self.face_hash)
        )
    }

    /// The `@font-face` rule: the family, the file's address, the weights, the style and
    /// the display.
    pub fn face(&self) -> String {
        let weight = if self.weight.0 == self.weight.1 {
            self.weight.0.to_string()
        } else {
            format!("{} {}", self.weight.0, self.weight.1)
        };
        format!(
            "@font-face{{font-family:\"{}\";src:url({}) format(\"woff2\");font-weight:{weight};font-style:{};font-display:{}}}",
            self.family,
            self.url(),
            if self.italic { "italic" } else { "normal" },
            self.display.as_str(),
        )
    }
}

/// The kind of an [`AppImage`], read from its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageKind {
    /// `image/svg+xml`.
    Svg,
    /// `image/png`.
    Png,
    /// `image/webp`.
    Webp,
    /// `image/jpeg`.
    Jpeg,
    /// `image/avif`.
    Avif,
    /// `image/gif`.
    Gif,
    /// `image/x-icon`.
    Ico,
}

impl ImageKind {
    /// The kind `bytes` start as, if they are a picture a page may show.
    pub const fn of(bytes: &[u8]) -> Option<Self> {
        let n = bytes.len();
        if n >= 8 && bytes[0] == 0x89 && bytes[1] == b'P' && bytes[2] == b'N' && bytes[3] == b'G' {
            return Some(Self::Png);
        }
        if n >= 12 && spells(bytes, 0, b"riff") && spells(bytes, 8, b"webp") {
            return Some(Self::Webp);
        }
        if n >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff {
            return Some(Self::Jpeg);
        }
        if n >= 12 && spells(bytes, 4, b"ftypavif") {
            return Some(Self::Avif);
        }
        if n >= 6 && spells(bytes, 0, b"gif8") {
            return Some(Self::Gif);
        }
        if n >= 6 && bytes[0] == 0 && bytes[1] == 0 && bytes[2] == 1 && bytes[3] == 0 {
            return Some(Self::Ico);
        }
        if is_svg(bytes) {
            return Some(Self::Svg);
        }
        None
    }

    /// The file's extension.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Jpeg => "jpg",
            Self::Avif => "avif",
            Self::Gif => "gif",
            Self::Ico => "ico",
        }
    }

    /// The `Content-Type` it is served with.
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Jpeg => "image/jpeg",
            Self::Avif => "image/avif",
            Self::Gif => "image/gif",
            Self::Ico => "image/x-icon",
        }
    }
}

/// Whether `bytes` at `at` spell `word` (lowercase), ignoring ASCII case.
const fn spells(bytes: &[u8], at: usize, word: &[u8]) -> bool {
    if at + word.len() > bytes.len() {
        return false;
    }
    let mut i = 0;
    while i < word.len() {
        if bytes[at + i].to_ascii_lowercase() != word[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `bytes` are an SVG document with none of the plainest ways to run something:
/// an `<svg` element, and no `<script` or `<foreignObject` element (with a namespace prefix
/// or not), no `on…=` event attribute and no `javascript:`, in any case.
///
/// This is a check against mistakes, not a sanitizer: an SVG file can run script in more
/// ways than it reads (entities, `<set>`, an outside `<use>`, CSS). What keeps a picture
/// inert is how it is served and drawn: as an `<img>`, or on its own under [`IMAGE_CSP`].
const fn is_svg(bytes: &[u8]) -> bool {
    let mut svg = false;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'<' {
            // The element's name, after a namespace prefix if it has one.
            let mut name = at + 1;
            let mut end = name;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b'_')
            {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b':' {
                name = end + 1;
            }
            if spells(bytes, at + 1, b"svg") {
                svg = true;
            }
            if spells(bytes, name, b"script") || spells(bytes, name, b"foreignobject") {
                return false;
            }
        }
        if spells(bytes, at, b"javascript:") || is_event_attribute(bytes, at) {
            return false;
        }
        at += 1;
    }
    svg
}

/// Whether an event attribute (`onload=`, ` onclick =`) starts at `at`: a space or a `/`,
/// `on`, letters, and `=` after any spaces.
const fn is_event_attribute(bytes: &[u8], at: usize) -> bool {
    if !matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r' | b'/') || !spells(bytes, at + 1, b"on") {
        return false;
    }
    let mut end = at + 3;
    let start = end;
    while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
        end += 1;
    }
    if end == start {
        return false;
    }
    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t' | b'\n' | b'\r') {
        end += 1;
    }
    end < bytes.len() && bytes[end] == b'='
}

/// A picture of the app's own (its mark, a logo, an illustration every page shows), compiled
/// in with `include_bytes!` and served by the toolkit at `/_navree/ui/app/{name}-{hash}.{ext}`
/// as `immutable`. See [the module](self).
///
/// SVG, PNG, WebP, JPEG, AVIF, GIF and ICO files are taken, told apart by their first bytes.
///
/// A picture is inert because of how it is used, not because of what it holds: an `<img>`
/// never runs an SVG's script, and opened on its own every picture is served under
/// [`IMAGE_CSP`], which runs nothing, loads nothing and sandboxes the document. As a guard
/// against mistakes, an SVG file with a `<script>` or `<foreignObject>` element, an `on…=`
/// event attribute or a `javascript:` address fails the app's build; that check can be
/// got round, and is no reason to trust the file. Never write an `AppImage`'s bytes into a
/// page (with [`SafeHtml::trusted`](crate::SafeHtml::trusted) or an inline `<svg>`): there,
/// the page's policy is all that stands between its script and the reader. Use it wherever
/// an address goes: `el("img").attr("src", &MARK)`, [`Shell::logo`](crate::Shell::logo),
/// [`Picture::new`](crate::Picture::new).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppImage {
    name: &'static str,
    bytes: &'static [u8],
    kind: ImageKind,
    hash: u64,
}

impl AppImage {
    /// The picture `bytes`, named `name` (1 to 64 lowercase letters, digits and `-`), at most
    /// `most` bytes long. A file of another kind, an SVG file that fails the build check
    /// (a `<script>` or `<foreignObject>` element, an event attribute, a `javascript:`
    /// address), a file past its ceiling, or a name of anything else, fails the app's build.
    /// Passing the check doesn't make an SVG file safe to put in a page: it is inert as an
    /// `<img>` and as the toolkit serves it, under [`IMAGE_CSP`].
    pub const fn new(name: &'static str, bytes: &'static [u8], most: usize) -> Self {
        assert!(
            is_file_name(name),
            "a picture's name has 1 to 64 lowercase letters, digits and `-`"
        );
        assert!(bytes.len() <= most, "a picture is larger than its ceiling");
        let Some(kind) = ImageKind::of(bytes) else {
            panic!(
                "a picture is a PNG, WebP, JPEG, AVIF, GIF or ICO file, or an SVG file with no script or foreignObject element, event attribute or javascript: address"
            )
        };
        Self {
            name,
            bytes,
            kind,
            hash: fnv(FNV_START, bytes),
        }
    }

    /// Its name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The file.
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// What kind of picture it is.
    pub const fn kind(&self) -> ImageKind {
        self.kind
    }

    /// The first 12 hex digits of its content hash.
    pub fn hash(&self) -> String {
        short(self.hash)
    }

    /// Where it is served: `/_navree/ui/app/{name}-{hash}.{ext}`.
    pub fn url(&self) -> String {
        format!(
            "{APP_PATH}/{}-{}.{}",
            self.name,
            self.hash(),
            self.kind.extension()
        )
    }
}

impl From<&AppImage> for crate::html::Href {
    fn from(image: &AppImage) -> Self {
        Self::from(image.url())
    }
}

/// Its address, for an API that takes the address as text ([`Picture::new`](crate::Picture::new)).
impl From<&AppImage> for String {
    fn from(image: &AppImage) -> Self {
        image.url()
    }
}

/// The content security policy a picture of the app's own is served with: it runs no script,
/// loads nothing and is drawn in a sandbox when opened on its own.
pub const IMAGE_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

/// One file the toolkit serves for the app: a sheet, a font, a font's rule or a picture.
#[derive(Clone, Debug)]
pub(crate) struct Served {
    /// The name before the hash (`brand`, `display.face`).
    stem: String,
    ext: &'static str,
    content_type: &'static str,
    hash: String,
    bytes: Bytes,
    brotli: Option<Bytes>,
    /// Whether it is a picture, served under [`IMAGE_CSP`].
    image: bool,
}

impl Served {
    fn image(image: &AppImage) -> Self {
        Self {
            stem: image.name.to_owned(),
            ext: image.kind.extension(),
            content_type: image.kind.content_type(),
            hash: image.hash(),
            bytes: Bytes::from_static(image.bytes),
            // Only SVG is text: the others are compressed already.
            brotli: (image.kind == ImageKind::Svg)
                .then(|| Bytes::from(crate::compress::brotli(image.bytes))),
            image: true,
        }
    }

    fn sheet(sheet: &AppSheet) -> Self {
        Self {
            stem: sheet.name.to_owned(),
            ext: "css",
            content_type: "text/css; charset=utf-8",
            hash: sheet.hash(),
            bytes: Bytes::from_static(sheet.bytes),
            brotli: Some(Bytes::from(crate::compress::brotli(sheet.bytes))),
            image: false,
        }
    }

    fn font(font: &AppFont) -> Self {
        Self {
            stem: font.name.to_owned(),
            ext: "woff2",
            content_type: "font/woff2",
            hash: short(font.file_hash),
            bytes: Bytes::from_static(font.bytes),
            // A woff2 file is compressed already.
            brotli: None,
            image: false,
        }
    }

    fn face(font: &AppFont) -> Self {
        let rule = font.face();
        Self {
            stem: format!("{}.face", font.name),
            ext: "css",
            content_type: "text/css; charset=utf-8",
            hash: short(font.face_hash),
            brotli: Some(Bytes::from(crate::compress::brotli(rule.as_bytes()))),
            bytes: Bytes::from(rule.into_bytes()),
            image: false,
        }
    }
}

/// What the app contributed, as the toolkit serves it: each name once, the first to take
/// it keeping it.
pub(crate) fn served(
    sheets: impl IntoIterator<Item = AppSheet>,
    fonts: impl IntoIterator<Item = AppFont>,
    images: impl IntoIterator<Item = AppImage>,
) -> Vec<Served> {
    let mut out: Vec<Served> = Vec::new();
    let mut add = |file: Served| {
        if !out
            .iter()
            .any(|have| have.stem == file.stem && have.ext == file.ext)
        {
            out.push(file);
        }
    };
    for sheet in sheets {
        add(Served::sheet(&sheet));
    }
    for font in fonts {
        add(Served::font(&font));
        add(Served::face(&font));
    }
    for image in images {
        add(Served::image(&image));
    }
    out
}

/// Answers `GET /_navree/ui/app/{file}`: the file named in `file`, `immutable` when the hash
/// is its own, uncached under an older hash (a page open across a deploy keeps working),
/// and not found for a name the app has no file for.
pub(crate) fn respond(file: &str, request: &HeaderMap, files: &[Served]) -> Response {
    let found = file
        .rsplit_once('.')
        .and_then(|(rest, ext)| rest.rsplit_once('-').map(|(stem, hash)| (stem, hash, ext)))
        .and_then(|(stem, hash, ext)| {
            files
                .iter()
                .find(|served| served.stem == stem && served.ext == ext)
                .map(|served| (served, served.hash == hash))
        });
    let Some((served, current)) = found else {
        return http::StatusCode::NOT_FOUND.into_response();
    };
    let mut response = assets::answer(
        &served.hash,
        served.content_type,
        current,
        request,
        served.bytes.clone(),
        None,
        served.brotli.clone(),
    );
    if served.image {
        response.headers_mut().insert(
            http::header::CONTENT_SECURITY_POLICY,
            http::HeaderValue::from_static(IMAGE_CSP),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::{
        ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, HeaderValue,
    };

    static BRAND: AppSheet = AppSheet::new("brand", b":root{--hero:1}", 1024);
    static DISPLAY: AppFont =
        AppFont::new("display", "Fraunces", b"wOF2\0\0\0\0", 1024).weights(400, 700);

    #[test]
    fn a_sheet_is_named_by_its_content_hash() {
        assert_eq!(BRAND.hash(), assets::content_hash(BRAND.bytes()));
        assert_eq!(
            BRAND.url(),
            format!("/_navree/ui/app/brand-{}.css", BRAND.hash())
        );
        assert!(BRAND.url().starts_with(assets::MOUNT));
    }

    #[test]
    fn a_font_has_a_file_and_a_rule_each_named_by_what_it_says() {
        assert_eq!(
            DISPLAY.url(),
            format!(
                "/_navree/ui/app/display-{}.woff2",
                assets::content_hash(DISPLAY.bytes())
            )
        );
        let rule = DISPLAY.face();
        assert_eq!(
            rule,
            format!(
                "@font-face{{font-family:\"Fraunces\";src:url({}) format(\"woff2\");font-weight:400 700;font-style:normal;font-display:swap}}",
                DISPLAY.url()
            )
        );
        // The rule's address follows everything the rule says.
        let swapped = DISPLAY.display(FontDisplay::Optional);
        let italic = DISPLAY.italic();
        let single = DISPLAY.weight(600);
        let others = [swapped, italic, single];
        for other in others {
            assert_eq!(other.url(), DISPLAY.url(), "the file is the same file");
            assert_ne!(other.face_url(), DISPLAY.face_url());
        }
        assert!(italic.face().contains("font-style:italic"));
        assert!(swapped.face().contains("font-display:optional"));
        assert!(single.face().contains("font-weight:600;"));
        assert!(!DISPLAY.lazy().preloads() && DISPLAY.preloads());
    }

    #[test]
    fn the_files_are_served_by_their_hash_and_an_old_hash_still_gets_them_uncached() {
        let files = served([BRAND, BRAND], [DISPLAY, DISPLAY], []);
        assert_eq!(files.len(), 3, "a name once: a sheet, a font, a rule");
        let mut request = HeaderMap::new();
        request.insert(ACCEPT_ENCODING, HeaderValue::from_static("br"));

        let sheet = respond(&format!("brand-{}.css", BRAND.hash()), &request, &files);
        assert_eq!(sheet.status(), http::StatusCode::OK);
        assert_eq!(sheet.headers()[CONTENT_TYPE], "text/css; charset=utf-8");
        assert_eq!(
            sheet.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(sheet.headers()[CONTENT_ENCODING], "br");

        let font = respond(DISPLAY.url().rsplit('/').next().unwrap(), &request, &files);
        assert_eq!(font.status(), http::StatusCode::OK);
        assert_eq!(font.headers()[CONTENT_TYPE], "font/woff2");
        assert!(font.headers().get(CONTENT_ENCODING).is_none());

        let rule = respond(
            DISPLAY.face_url().rsplit('/').next().unwrap(),
            &request,
            &files,
        );
        assert_eq!(rule.status(), http::StatusCode::OK);
        assert_eq!(rule.headers()[CONTENT_TYPE], "text/css; charset=utf-8");

        let old = respond("brand-000000000000.css", &HeaderMap::new(), &files);
        assert_eq!(old.status(), http::StatusCode::OK);
        assert_eq!(old.headers()[CACHE_CONTROL], "no-cache");

        for missing in [
            "other-000000000000.css",
            "brand.css",
            "brand-000000000000.js",
            "brand-000000000000.woff2",
            "../brand.css",
            "display-000000000000.css",
        ] {
            assert_eq!(
                respond(missing, &request, &files).status(),
                http::StatusCode::NOT_FOUND,
                "{missing}"
            );
        }
    }

    static MARK: AppImage = AppImage::new(
        "mark",
        br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8"/></svg>"#,
        1024,
    );
    static PHOTO: AppImage = AppImage::new("photo", b"\x89PNG\r\n\x1a\n....", 1024);

    #[test]
    fn a_picture_is_named_by_its_hash_and_kind_and_served_inert() {
        assert_eq!(MARK.kind(), ImageKind::Svg);
        assert_eq!(
            MARK.url(),
            format!(
                "/_navree/ui/app/mark-{}.svg",
                assets::content_hash(MARK.bytes())
            )
        );
        assert_eq!(PHOTO.kind(), ImageKind::Png);
        assert!(PHOTO.url().ends_with(".png"));
        assert_eq!(crate::html::Href::from(&MARK).as_str(), MARK.url());

        let files = served([], [], [MARK, PHOTO, MARK]);
        assert_eq!(files.len(), 2, "a name once");
        let mut request = HeaderMap::new();
        request.insert(ACCEPT_ENCODING, HeaderValue::from_static("br"));
        let svg = respond(MARK.url().rsplit('/').next().unwrap(), &request, &files);
        assert_eq!(svg.status(), http::StatusCode::OK);
        assert_eq!(svg.headers()[CONTENT_TYPE], "image/svg+xml");
        assert_eq!(
            svg.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            svg.headers()[http::header::CONTENT_SECURITY_POLICY],
            IMAGE_CSP
        );
        assert_eq!(svg.headers()[CONTENT_ENCODING], "br");
        let png = respond(PHOTO.url().rsplit('/').next().unwrap(), &request, &files);
        assert_eq!(png.headers()[CONTENT_TYPE], "image/png");
        assert!(png.headers().get(CONTENT_ENCODING).is_none());
        // A picture's name with another extension is nothing.
        for missing in [
            format!("mark-{}.png", MARK.hash()),
            format!("mark-{}.css", MARK.hash()),
            "mark.svg".to_owned(),
        ] {
            assert_eq!(
                respond(&missing, &request, &files).status(),
                http::StatusCode::NOT_FOUND,
                "{missing}"
            );
        }
    }

    #[test]
    fn a_file_that_could_run_anything_is_no_picture() {
        for bad in [
            &b"<svg><script>alert(1)</script></svg>"[..],
            b"<svg><SCRIPT>alert(1)</SCRIPT></svg>",
            b"<svg><a href=\"JavaScript:alert(1)\"/></svg>",
            b"<svg><foreignObject><p>x</p></foreignObject></svg>",
            b"<svg onload=\"alert(1)\"></svg>",
            b"<svg><rect\n  OnClick = \"alert(1)\"/></svg>",
            b"<svg/onload=alert(1)>",
            b"<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\"><svg:script>alert(1)</svg:script></svg:svg>",
            b"<svg><x:foreignObject/></svg>",
            b"<html><body>no picture</body></html>",
            b"GIF",
            b"",
        ] {
            assert_eq!(ImageKind::of(bad), None, "{}", String::from_utf8_lossy(bad));
        }
        assert_eq!(
            ImageKind::of(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageKind::Webp)
        );
        assert_eq!(ImageKind::of(b"\xff\xd8\xff\xe0"), Some(ImageKind::Jpeg));
        assert_eq!(ImageKind::of(b"\0\0\0\x1cftypavif"), Some(ImageKind::Avif));
        assert_eq!(ImageKind::of(b"GIF89a"), Some(ImageKind::Gif));
        // Ordinary attributes that start with the letters o and n are not events.
        assert_eq!(
            ImageKind::of(b"<svg viewBox=\"0 0 8 8\"><path opacity=\"1\" d=\"M0 0\"/><text>on time</text></svg>"),
            Some(ImageKind::Svg)
        );
    }

    #[test]
    #[should_panic(expected = "woff2")]
    fn a_font_that_is_not_woff2_is_refused() {
        let _ = AppFont::new("x", "X", b"OTTO....", 1024);
    }

    #[test]
    fn a_family_cannot_carry_css() {
        for bad in ["A\"}body{display:none", "A;B", "A\\B", "", "A,B"] {
            let result = std::panic::catch_unwind(|| AppFont::new("x", bad, b"wOF2....", 1024));
            assert!(result.is_err(), "{bad:?} is not a family");
        }
    }
}
