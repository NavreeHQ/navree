//! [`TypstEngine`]: PDF from Typst templates, compiled in process.

mod fonts;
mod world;

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};

use navree_contracts::{Clock, SystemClock};
use typst::diag::{Severity, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Dict, Smart, Str, Value};
use typst::syntax::Source;
use typst::text::{FontFamily, FontList, Lang, Region, TextElem};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;

use self::fonts::FontSet;
use self::world::{DATA_PATH, INLINE_PATH, RenderWorld, project_file};
use crate::engine::{EngineKind, PdfEngine};
use crate::fonts::DEFAULT_FAMILIES;
use crate::render::{Render, TemplateSource};
use crate::templates::{TYPST_EXTENSION, Templates};
use crate::{DocumentsError, Pdf};

/// How many compilations a cached intermediate result survives unused.
const CACHE_MAX_AGE: usize = 10;

/// PDF from Typst templates, compiled in process by the embedded Typst
/// compiler: no external program, no network.
///
/// - **Templates** are `.typ` files in the template directories (or
///   embedded by modules). `#import`, `#include`, `image(..)`, `read(..)`
///   and `json(..)` resolve inside the template tree: `/`-paths from its
///   root, other paths relative to the file.
/// - **Data**: the render's data is the file `/data.json`, so a template
///   starts with `#let data = json("/data.json")`. A sample `data.json` at
///   the root of the template directory lets the Typst CLI or editor
///   preview the template; at render time the render's data replaces it.
/// - **Locale**: with a locale, text defaults to that language (and its
///   direction: right to left for Arabic), and `sys.inputs` holds
///   `locale` and `dir` (`"rtl"` or `"ltr"`).
/// - **Fonts**: the bundled Noto Sans, Noto Sans Arabic and Noto Sans
///   Devanagari (text falls back through them in that order), plus every
///   font under `documents.typst.font_dirs`.
/// - **Packages**: `#import "@preview/name:1.0.0"` reads
///   `<packages_dir>/preview/name/1.0.0/`; nothing is downloaded.
///
/// Compilation runs on Tokio's blocking pool. The fonts load on the first
/// render, and the compiler keeps intermediate results across renders, so
/// rendering the same template again is faster.
///
/// ```
/// use navree_documents::{Render, Templates, TypstEngine};
/// use serde_json::json;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_documents::DocumentsError> {
/// let engine = TypstEngine::new(Templates::from_files([(
///     "certificates/classic.typ",
///     r#"#let data = json("/data.json")
/// = Certificate of completion
/// #data.name completed #data.course."#,
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
pub struct TypstEngine {
    templates: Templates,
    bundled_fonts: bool,
    font_dirs: Vec<PathBuf>,
    packages_dir: Option<PathBuf>,
    clock: Arc<dyn Clock>,
    state: Arc<State>,
}

/// What is built once and shared by every render.
#[derive(Default)]
struct State {
    fonts: OnceLock<Arc<FontSet>>,
    libraries: Mutex<HashMap<Option<String>, Arc<LazyHash<Library>>>>,
}

impl std::fmt::Debug for TypstEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypstEngine")
            .field("templates", &self.templates)
            .field("bundled_fonts", &self.bundled_fonts)
            .field("font_dirs", &self.font_dirs)
            .field("packages_dir", &self.packages_dir)
            .finish_non_exhaustive()
    }
}

impl TypstEngine {
    /// The engine over `templates`, with the bundled fonts and the system
    /// clock.
    pub fn new(templates: Templates) -> Self {
        Self {
            templates,
            bundled_fonts: true,
            font_dirs: Vec::new(),
            packages_dir: None,
            clock: Arc::new(SystemClock),
            state: Arc::default(),
        }
    }

    /// Whether to load the bundled Noto fonts (default true).
    pub fn bundled_fonts(mut self, on: bool) -> Self {
        self.bundled_fonts = on;
        self.state = Arc::default();
        self
    }

    /// Directories to load more fonts from (`.ttf`, `.otf`, `.ttc`,
    /// `.otc`, searched recursively).
    pub fn font_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.font_dirs = dirs;
        self.state = Arc::default();
        self
    }

    /// The directory of local Typst packages (`<namespace>/<name>/<version>`).
    pub fn packages_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.packages_dir = dir;
        self
    }

    /// The clock for `datetime.today()` and the PDF's creation date.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The templates.
    pub fn templates(&self) -> &Templates {
        &self.templates
    }

    /// The font families templates can use, sorted (loads the fonts:
    /// blocking).
    pub fn font_families(&self) -> Vec<String> {
        self.fonts().families()
    }

    /// Renders `render` on Tokio's blocking pool.
    pub async fn render(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        let engine = self.clone();
        let render = render.clone();
        tokio::task::spawn_blocking(move || engine.render_blocking(&render))
            .await
            .map_err(|error| DocumentsError::Engine {
                engine: "typst",
                message: format!("the compiler stopped: {error}"),
            })?
    }

    /// Renders `render` on the current thread (it blocks: compiling takes
    /// from milliseconds to seconds).
    pub fn render_blocking(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        if let Some(error) = &render.data_error {
            return Err(DocumentsError::Data(error.clone()));
        }
        let (main_path, text) = match &render.source {
            TemplateSource::Template(name) => {
                let path = Templates::template_path(name, TYPST_EXTENSION).ok_or_else(|| {
                    DocumentsError::TemplateNotFound {
                        name: name.clone(),
                        tried: Vec::new(),
                    }
                })?;
                let bytes = self
                    .templates
                    .read(&path)
                    .map_err(|error| DocumentsError::Template {
                        file: path.clone(),
                        line: None,
                        column: None,
                        message: error.to_string(),
                    })?
                    .ok_or_else(|| DocumentsError::TemplateNotFound {
                        name: name.clone(),
                        tried: vec![path.clone()],
                    })?;
                let text =
                    String::from_utf8(bytes.to_vec()).map_err(|_| DocumentsError::Template {
                        file: path.clone(),
                        line: None,
                        column: None,
                        message: "the file is not valid UTF-8".into(),
                    })?;
                (format!("/{path}"), text)
            }
            TemplateSource::Typst(source) => (INLINE_PATH.to_owned(), source.clone()),
            TemplateSource::Html(_) => {
                return Err(DocumentsError::EngineUnavailable {
                    engine: "typst",
                    reason: "HTML is printed by the chrome engine".into(),
                });
            }
        };
        let text = text
            .strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(text);
        let main = project_file(&main_path).ok_or_else(|| DocumentsError::TemplateNotFound {
            name: render.name().to_owned(),
            tried: vec![main_path.clone()],
        })?;
        let data = serde_json::to_vec(&render.data)
            .map_err(|error| DocumentsError::Data(error.to_string()))?;
        let world = RenderWorld {
            library: self.library(render.locale.as_deref()),
            fonts: self.fonts(),
            templates: self.templates.clone(),
            packages_dir: self.packages_dir.clone(),
            clock: self.clock.clone(),
            main,
            main_source: Source::new(main, text),
            data_id: project_file(DATA_PATH),
            data: Bytes::new(data),
            sources: Mutex::default(),
        };
        let Warned { output, warnings } = typst::compile::<PagedDocument>(&world);
        for warning in &warnings {
            tracing::debug!(
                template = render.name(),
                warning = %warning.message,
                "documents: typst warning"
            );
        }
        let mut document = output.map_err(|errors| template_error(&world, &errors))?;
        if let Some(title) = &render.title {
            document.info_mut().title = Some(title.as_str().into());
        }
        let options = typst_pdf::PdfOptions {
            ident: Smart::Auto,
            timestamp: timestamp(self.clock.as_ref()),
            ..typst_pdf::PdfOptions::default()
        };
        let bytes = typst_pdf::pdf(&document, &options)
            .map_err(|errors| template_error(&world, &errors))?;
        typst::comemo::evict(CACHE_MAX_AGE);
        Ok(Pdf::from_engine(bytes))
    }

    fn fonts(&self) -> Arc<FontSet> {
        self.state
            .fonts
            .get_or_init(|| Arc::new(FontSet::load(self.bundled_fonts, &self.font_dirs)))
            .clone()
    }

    /// The standard library with Navree's defaults, one per locale.
    fn library(&self, locale: Option<&str>) -> Arc<LazyHash<Library>> {
        let key = locale.map(str::to_owned);
        let mut libraries = self
            .state
            .libraries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(library) = libraries.get(&key) {
            return library.clone();
        }
        let library = Arc::new(LazyHash::new(build_library(locale)));
        // Locales come from requests: keep the cache small.
        if libraries.len() < 64 {
            libraries.insert(key, library.clone());
        }
        library
    }
}

impl PdfEngine for TypstEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Typst
    }

    fn render<'a>(
        &'a self,
        render: &'a Render,
    ) -> navree_kernel::BoxFuture<'a, Result<Pdf, DocumentsError>> {
        Box::pin(TypstEngine::render(self, render))
    }
}

/// The standard library: text in the bundled families, and with a locale,
/// its language, region and `sys.inputs` (`locale`, `dir`).
fn build_library(locale: Option<&str>) -> Library {
    let mut builder = Library::builder();
    if let Some(locale) = locale {
        let direction = if crate::render::is_rtl(locale) {
            "rtl"
        } else {
            "ltr"
        };
        let inputs: Dict = [
            (Str::from("locale"), Value::Str(locale.into())),
            (Str::from("dir"), Value::Str(direction.into())),
        ]
        .into_iter()
        .collect();
        builder = builder.with_inputs(inputs);
    }
    let mut library = builder.build();
    library.styles.set(
        TextElem::font,
        FontList(
            DEFAULT_FAMILIES
                .iter()
                .map(|f| FontFamily::new(f))
                .collect(),
        ),
    );
    if let Some(locale) = locale {
        let mut parts = locale.split(['-', '_']);
        if let Some(lang) = parts.next().and_then(|lang| Lang::from_str(lang).ok()) {
            library.styles.set(TextElem::lang, lang);
        }
        if let Some(region) = parts
            .find(|part| part.len() == 2)
            .and_then(|region| Region::from_str(region).ok())
        {
            library.styles.set(TextElem::region, Some(region));
        }
    }
    library
}

/// The PDF creation date from the clock.
fn timestamp(clock: &dyn Clock) -> Option<typst_pdf::Timestamp> {
    use chrono::{Datelike, Timelike};
    let now = clock.now();
    let datetime = typst::foundations::Datetime::from_ymd_hms(
        now.year(),
        u8::try_from(now.month()).ok()?,
        u8::try_from(now.day()).ok()?,
        u8::try_from(now.hour()).ok()?,
        u8::try_from(now.minute()).ok()?,
        u8::try_from(now.second()).ok()?,
    )?;
    Some(typst_pdf::Timestamp::new_utc(datetime))
}

/// The first error of a failed compilation, with its file, line and
/// column, and the compiler's hints.
fn template_error(world: &RenderWorld, diagnostics: &[SourceDiagnostic]) -> DocumentsError {
    let diagnostic = diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .or_else(|| diagnostics.first());
    let Some(diagnostic) = diagnostic else {
        return DocumentsError::Template {
            file: world.display_path(world.main()),
            line: None,
            column: None,
            message: "compilation failed".into(),
        };
    };
    let id = diagnostic.span.id();
    let file = world.display_path(id.unwrap_or_else(|| world.main()));
    let position = id.and_then(|id| {
        let range = world.range(diagnostic.span)?;
        let source = world.source(id).ok()?;
        let (line, column) = source.lines().byte_to_line_column(range.start)?;
        Some((line + 1, column + 1))
    });
    let mut message = diagnostic.message.to_string();
    for hint in &diagnostic.hints {
        message.push_str("\nhint: ");
        message.push_str(&hint.v);
    }
    if diagnostics.len() > 1 {
        message.push_str(&format!("\n({} more errors)", diagnostics.len() - 1));
    }
    DocumentsError::Template {
        file,
        line: position.map(|(line, _)| line),
        column: position.map(|(_, column)| column),
        message,
    }
}
