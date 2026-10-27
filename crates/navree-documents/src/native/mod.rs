//! The native PDF engine: HTML templates and [`PdfDocument`]s laid out in
//! process (harfrust shaping, unicode-bidi, icu_segmenter line breaking)
//! and written with krilla, with the bundled Noto fonts.
//!
//! [`NativeEngine`] renders `.html` templates (the documented subset of
//! HTML, below) and inline HTML; [`PdfDocument`] builds a document in
//! code with the types of this module.
//!
//! **The HTML subset.** `h1`–`h6`, `p`, `strong`/`b`, `em`/`i`, `u`,
//! `s`/`del`, `a href`, `span`, `code`, `small`, `br`, `ul`/`ol`/`li`
//! (nested; `start` and `type` on `ol`), `table` with `thead`, `tbody`,
//! `tfoot`, `tr`, `th`, `td` (`colspan`, `align`, `valign`, `width`;
//! `border` and `cellpadding` on the table), `img` (`src` in the template
//! tree or a `data:` URI, `width`, `height`, `alt`), `hr`, `pre`, `div`
//! and the sectioning elements, `header` and `footer` as children of
//! `body` (printed on every page; `<span class="pageNumber">`,
//! `totalPages`, `title` and `date` fill in), `<title>`,
//! `<meta name="author">`, `lang` and `dir`, and the `page-break` class.
//! Inline `style` attributes may set `font-weight`, `font-style`, `color`,
//! `font-size`, `font-family`, `text-align`, `text-decoration`,
//! `line-height`, `direction`, `background-color` (cells and rows),
//! `width` and `height` (images, cells), `vertical-align` (cells) and
//! `break-before`/`break-after`. From `<style>`, only `@page { size;
//! margin }` is read. Everything else is ignored, so the same template
//! prints with full CSS in Chrome.

mod convert;
mod css;
mod fonts;
mod html;
mod layout;
mod model;
mod tags;
mod text;
mod write;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use navree_contracts::{Clock, SystemClock};

pub use self::model::{
    Align, Block, Borders, Cell, Color, Direction, Image, List, ListKind, Paragraph, PdfDocument,
    Run, Table, VAlign,
};
use self::fonts::FontBook;
use self::layout::LayoutError;
use self::model::PageSetup;
use crate::engine::{EngineKind, PdfEngine};
use crate::options::NativeOptions;
use crate::print::{Margins, PrintOptions};
use crate::render::{Render, is_rtl};
use crate::templates::{Templates, normalize};
use crate::views::HtmlTemplates;
use crate::{DocumentsError, Pdf};

const ENGINE: &str = "native";

/// PDF from HTML templates, laid out in process: no browser, no external
/// program, no network.
///
/// - **Templates** are `.html` files in the template directories (or
///   embedded by modules), rendered with navree-view, the same files the
///   Chrome engine prints. The render's data are the template's variables,
///   plus `locale` and `dir`. The engine reads the HTML subset listed in
///   the [module documentation](self); what it does not know, it skips.
/// - **Pages**: paper, orientation and margins come from the render's
///   [`PrintOptions`] (else `documents.native.print`), and an
///   `@page { size; margin }` rule in the template wins while
///   `prefer_css_page_size` is on. `header` and `footer` elements (or the
///   print options' header and footer HTML) repeat on every page.
/// - **Images** load from the template tree (relative to the document,
///   `/`-paths from the root) or from `data:` URIs; nothing is fetched
///   from another origin.
/// - **Fonts**: the bundled Noto Sans, Serif and Mono, Noto Sans Arabic,
///   Hebrew and Devanagari, plus every font under
///   `documents.native.font_dirs`. Each character falls back to a family
///   that has it, so text in those scripts prints without naming a font.
/// - **Locale**: the template's `<html lang dir>`, else the render's
///   locale, sets the language (metadata, shaping) and the direction.
///
/// Renders run on Tokio's blocking pool; one that takes longer than the
/// engine's [`timeout`](NativeEngine::timeout) (60 seconds by default) is
/// [`DocumentsError::Timeout`].
///
/// ```
/// use navree_documents::{NativeEngine, Render, Templates};
/// use serde_json::json;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_documents::DocumentsError> {
/// let engine = NativeEngine::new(Templates::from_files([(
///     "certificates/classic.html",
///     "<h1>Certificate of completion</h1>\
///      <p>{{ name }} completed <strong>{{ course }}</strong>.</p>",
/// )]));
/// let pdf = engine
///     .render(&Render::template("certificates/classic")
///         .data(json!({ "name": "Ada Lovelace", "course": "Analytical Engines" })))
///     .await?;
/// assert!(pdf.text()?.contains("Ada Lovelace completed Analytical Engines."));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct NativeEngine {
    templates: Templates,
    options: NativeOptions,
    clock: Arc<dyn Clock>,
    timeout: Duration,
    state: Arc<State>,
}

/// What is built once and shared by every render.
#[derive(Default)]
struct State {
    fonts: OnceLock<Arc<FontBook>>,
    html: HtmlTemplates,
}

impl std::fmt::Debug for NativeEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeEngine")
            .field("templates", &self.templates)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl NativeEngine {
    /// The engine over `templates`, with the bundled fonts, the default
    /// page settings and the system clock.
    pub fn new(templates: Templates) -> Self {
        Self::with_options(templates, NativeOptions::default())
    }

    /// The engine over `templates` configured by `options`
    /// (`documents.native`).
    pub fn with_options(templates: Templates, options: NativeOptions) -> Self {
        let timeout = Duration::from_secs(options.timeout_secs.max(1));
        Self {
            templates,
            options,
            clock: Arc::new(SystemClock),
            timeout,
            state: Arc::default(),
        }
    }

    /// Whether to load the bundled Noto fonts (default true).
    pub fn bundled_fonts(mut self, on: bool) -> Self {
        self.options.bundled_fonts = on;
        self.state = Arc::default();
        self
    }

    /// Directories to load more fonts from (`.ttf`, `.otf`, `.ttc`,
    /// `.otc`, searched recursively).
    pub fn font_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.options.font_dirs = dirs;
        self.state = Arc::default();
        self
    }

    /// The page settings renders use unless they bring their own.
    pub fn print(mut self, print: PrintOptions) -> Self {
        self.options.print = print;
        self
    }

    /// The clock for the PDF's creation date and `Run::date`.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The time limit of one render (default 60 seconds).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The templates.
    pub fn templates(&self) -> &Templates {
        &self.templates
    }

    /// The engine's settings.
    pub fn options(&self) -> &NativeOptions {
        &self.options
    }

    /// The font families documents can use, sorted (loads the fonts:
    /// blocking).
    pub fn font_families(&self) -> Vec<String> {
        self.fonts().families()
    }

    /// Renders `render` on Tokio's blocking pool.
    pub async fn render(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        let engine = self.clone();
        let render = render.clone();
        self.blocking(move || engine.render_blocking(&render)).await
    }

    /// Renders `render` on the current thread (it blocks: laying out takes
    /// from milliseconds to seconds).
    pub fn render_blocking(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        let (path, html) = self.state.html.document(&self.templates, render)?;
        let document = self.document_of(&path, &html, render);
        self.write(&document)
    }

    /// The HTML that `render` lays out: the template rendered with its
    /// data, or the inline HTML.
    pub async fn render_html(&self, render: &Render) -> Result<String, DocumentsError> {
        let engine = self.clone();
        let render = render.clone();
        self.blocking(move || {
            Ok(engine
                .state
                .html
                .document(&engine.templates, &render)?
                .1)
        })
        .await
    }

    /// Lays out and writes `document` with this engine's fonts and clock,
    /// on Tokio's blocking pool.
    pub async fn render_document(&self, document: &PdfDocument) -> Result<Pdf, DocumentsError> {
        let engine = self.clone();
        let document = document.clone();
        self.blocking(move || engine.render_document_blocking(&document))
            .await
    }

    /// [`render_document`](NativeEngine::render_document) on the current
    /// thread.
    pub fn render_document_blocking(&self, document: &PdfDocument) -> Result<Pdf, DocumentsError> {
        self.write(document)
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, DocumentsError> + Send + 'static,
    ) -> Result<T, DocumentsError> {
        let task = tokio::task::spawn_blocking(work);
        tokio::time::timeout(self.timeout, task)
            .await
            .map_err(|_| DocumentsError::Timeout {
                engine: ENGINE,
                secs: self.timeout.as_secs().max(1),
            })?
            .map_err(|error| DocumentsError::Engine {
                engine: ENGINE,
                message: format!("the layout stopped: {error}"),
            })?
    }

    fn fonts(&self) -> Arc<FontBook> {
        self.state
            .fonts
            .get_or_init(|| {
                if self.options.bundled_fonts && self.options.font_dirs.is_empty() {
                    FontBook::bundled()
                } else {
                    Arc::new(FontBook::load(
                        self.options.bundled_fonts,
                        &self.options.font_dirs,
                    ))
                }
            })
            .clone()
    }

    /// The document an HTML page at `path` describes, with the render's
    /// page settings, locale and title.
    fn document_of(&self, path: &str, html: &str, render: &Render) -> PdfDocument {
        let templates = &self.templates;
        let mut images = |src: &str| image_source(templates, path, src);
        let converted = convert::convert(html, &mut images);
        let mut document = converted.document;
        let print = render.print.as_ref().unwrap_or(&self.options.print);
        document.page = PageSetup {
            paper: print.paper,
            landscape: print.landscape,
            margins: print.margin,
        };
        if print.prefer_css_page_size {
            let rule = &converted.page;
            if let Some(paper) = rule.paper {
                document.page.paper = paper;
                document.page.landscape = false;
            }
            if let Some(landscape) = rule.landscape {
                document.page.landscape = landscape;
            }
            let margins = &mut document.page.margins;
            let [top, right, bottom, left] = rule.margins;
            *margins = Margins {
                top: top.unwrap_or(margins.top),
                right: right.unwrap_or(margins.right),
                bottom: bottom.unwrap_or(margins.bottom),
                left: left.unwrap_or(margins.left),
            };
        }
        // The print options' header and footer, as Chrome prints them.
        for (html, header) in [(&print.header, true), (&print.footer, false)] {
            let Some(html) = html else {
                continue;
            };
            let tag = if header { "header" } else { "footer" };
            let wrapped = format!("<body><{tag}>{html}</{tag}></body>");
            let mut no_images = |src: &str| image_source(templates, path, src);
            let blocks = convert::convert(&wrapped, &mut no_images).document;
            let blocks = if header { blocks.header } else { blocks.footer };
            if header && document.header.is_empty() {
                document.header = blocks;
            } else if !header && document.footer.is_empty() {
                document.footer = blocks;
            }
        }
        let lang = converted.lang.or_else(|| render.locale.clone());
        document.direction = converted
            .direction
            .or_else(|| lang.as_deref().map(|l| if is_rtl(l) { Direction::Rtl } else { Direction::Ltr }));
        document.lang = lang;
        if let Some(title) = &render.title {
            document.title = Some(title.clone());
        }
        document
    }

    fn write(&self, document: &PdfDocument) -> Result<Pdf, DocumentsError> {
        write_document(document, &self.fonts(), self.clock.as_ref())
    }
}

impl PdfEngine for NativeEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Native
    }

    fn render<'a>(
        &'a self,
        render: &'a Render,
    ) -> navree_kernel::BoxFuture<'a, Result<Pdf, DocumentsError>> {
        Box::pin(NativeEngine::render(self, render))
    }
}

impl PdfDocument {
    /// The PDF, laid out with the bundled fonts (blocking: from
    /// milliseconds to seconds). [`NativeEngine::render_document`] and
    /// [`Documents::render_document`](crate::Documents::render_document)
    /// use the configured fonts and the app's clock instead.
    pub fn build(&self) -> Result<Pdf, DocumentsError> {
        write_document(self, &FontBook::bundled(), &SystemClock)
    }
}

/// Lays out and writes `document`.
fn write_document(
    document: &PdfDocument,
    fonts: &FontBook,
    clock: &dyn Clock,
) -> Result<Pdf, DocumentsError> {
    let now = clock.now();
    let date = now.format("%Y-%m-%d").to_string();
    let laid = layout::lay_out(document, fonts, &date).map_err(|error| match error {
        LayoutError::Image {
            source: Some(file),
            message,
        } => DocumentsError::Template {
            file,
            line: None,
            column: None,
            message: format!("the image does not decode: {message}"),
        },
        LayoutError::Image {
            source: None,
            message,
        } => DocumentsError::Engine {
            engine: ENGINE,
            message: format!("an image does not decode: {message}"),
        },
        LayoutError::NoFonts => DocumentsError::EngineUnavailable {
            engine: ENGINE,
            reason: "no fonts: documents.native.bundled_fonts is off and no font directory \
                     has fonts"
                .into(),
        },
    })?;
    let info = write::Info {
        title: document.title.clone(),
        author: document.author.clone(),
        lang: document.lang.clone(),
        rtl: document.direction == Some(Direction::Rtl),
        created: Some(now),
    };
    let bytes = write::write(&laid, fonts, &info).map_err(|message| DocumentsError::Engine {
        engine: ENGINE,
        message,
    })?;
    Ok(Pdf::from_engine(bytes))
}

/// The bytes of image `src` for the document at `path`: a path in the
/// template tree, relative to the document or (with a leading `/`) to the
/// root. Other origins are never fetched.
fn image_source(templates: &Templates, path: &str, src: &str) -> Option<(Bytes, String)> {
    let src = src.split(['?', '#']).next().unwrap_or_default().trim();
    if src.is_empty() || src.starts_with("//") || src.contains("://") || src.contains(':') {
        return None;
    }
    let mut segments: Vec<&str> = if src.starts_with('/') {
        Vec::new()
    } else {
        let mut dir: Vec<&str> = path.split('/').collect();
        dir.pop();
        dir
    };
    for segment in src.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    let resolved = normalize(&segments.join("/"))?;
    let bytes = templates.read(&resolved).ok().flatten()?;
    Some((bytes, resolved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_resolve_inside_the_template_tree_only() {
        let templates = Templates::from_files([
            ("images/logo.png", &b"logo"[..]),
            ("invoices/stamp.png", &b"stamp"[..]),
        ]);
        let found = |path: &str, src: &str| image_source(&templates, path, src).map(|(_, p)| p);
        assert_eq!(found("invoices/show.html", "/images/logo.png").as_deref(), Some("images/logo.png"));
        assert_eq!(found("invoices/show.html", "../images/logo.png").as_deref(), Some("images/logo.png"));
        assert_eq!(found("invoices/show.html", "stamp.png?v=2").as_deref(), Some("invoices/stamp.png"));
        assert_eq!(found("inline.html", "images/logo.png").as_deref(), Some("images/logo.png"));
        assert_eq!(found("invoices/show.html", "../../etc/passwd"), None);
        assert_eq!(found("invoices/show.html", "https://example.com/logo.png"), None);
        assert_eq!(found("invoices/show.html", "//example.com/logo.png"), None);
        assert_eq!(found("invoices/show.html", "C:/logo.png"), None);
        assert_eq!(found("invoices/show.html", "missing.png"), None);
    }
}
