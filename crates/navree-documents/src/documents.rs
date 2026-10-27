//! [`Documents`]: the service apps render documents with.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use navree_contracts::Dispatcher;
use navree_storage::{Disk, Storage};
use serde::Serialize;

use crate::engine::{EngineKind, PdfEngine};
use crate::fake::FakeState;
use crate::queue::{DocumentStatus, QueuedDocument, RenderDocument, StoredDocument};
use crate::render::{Render, TemplateSource};
use crate::templates::{HTML_EXTENSION, TYPST_EXTENSION, Templates};
use crate::{DocumentsError, DocumentsOptions, Pdf};

/// Renders documents: PDF from Typst or HTML templates, stored or queued.
///
/// `DocumentsModule` provides and exports it (`Arc<Documents>`); outside
/// an app, build one with [`Documents::new`]. It is cheap to clone.
///
/// ```
/// use navree_documents::{Documents, Templates};
/// use serde_json::json;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_documents::DocumentsError> {
/// let documents = Documents::new(Templates::from_files([(
///     "certificates/classic.typ",
///     "#let data = json(\"/data.json\")\n= Certificate\n#data.name completed #data.course.",
/// )]));
/// let pdf = documents
///     .pdf("certificates/classic", json!({ "name": "Ada Lovelace", "course": "Engines" }))
///     .await?;
/// assert!(pdf.text()?.contains("Ada Lovelace completed Engines."));
/// # Ok(())
/// # }
/// ```
///
/// **Which engine.** A render names its engine, or its template picks it:
/// inline Typst and `name.typ` go to the Typst engine, inline HTML and
/// `name.html` to Chrome. When a name has both files, `documents.engine`
/// decides (Typst by default).
#[derive(Clone)]
pub struct Documents {
    inner: Arc<Inner>,
}

#[derive(Clone)]
struct Inner {
    templates: Templates,
    default_engine: EngineKind,
    engines: Vec<Arc<dyn PdfEngine>>,
    storage: Option<Arc<Storage>>,
    disk: Option<String>,
    dispatcher: Option<Arc<dyn Dispatcher>>,
    /// Set for the fake: renders and queued renders are recorded, and any
    /// template name is accepted when the fake has no templates.
    fake: Option<Arc<FakeState>>,
    /// The configured Typst engine, for its font list.
    #[cfg(feature = "typst")]
    typst: Option<crate::TypstEngine>,
}

impl std::fmt::Debug for Documents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Documents")
            .field("templates", &self.inner.templates)
            .field("default_engine", &self.inner.default_engine)
            .field(
                "engines",
                &self.inner.engines.iter().map(|e| e.kind()).collect::<Vec<_>>(),
            )
            .field("disk", &self.inner.disk)
            .field("queued", &self.inner.dispatcher.is_some())
            .finish_non_exhaustive()
    }
}

impl Documents {
    /// Documents over `templates` with the engines this build has: Typst
    /// with the bundled fonts (feature `typst`) and Chrome with the default
    /// settings (feature `chrome`; nothing starts until the first HTML
    /// render). No storage and no queue: [`save`](Documents::save) needs
    /// [`storage`](Documents::storage).
    pub fn new(templates: Templates) -> Self {
        Self::from_options(&DocumentsOptions::default(), templates)
    }

    /// Documents configured by `options` over `templates` (what
    /// `DocumentsModule` builds, with the configured directories and the
    /// files modules embed).
    pub fn from_options(options: &DocumentsOptions, templates: Templates) -> Self {
        Self::build(options, templates, None)
    }

    /// [`from_options`](Documents::from_options), with the Typst engine on
    /// `clock` (the module passes the app's `dyn Clock`).
    pub(crate) fn build(
        options: &DocumentsOptions,
        templates: Templates,
        clock: Option<Arc<dyn navree_contracts::Clock>>,
    ) -> Self {
        #[allow(unused_mut)]
        let mut engines: Vec<Arc<dyn PdfEngine>> = Vec::new();
        #[cfg(feature = "typst")]
        let typst = {
            let mut typst = crate::TypstEngine::new(templates.clone())
                .bundled_fonts(options.typst.bundled_fonts)
                .font_dirs(options.typst.font_dirs.clone())
                .packages_dir(options.typst.packages_dir.clone());
            if let Some(clock) = clock {
                typst = typst.clock(clock);
            }
            engines.push(Arc::new(typst.clone()));
            Some(typst)
        };
        #[cfg(not(feature = "typst"))]
        let _ = clock;
        #[cfg(feature = "chrome")]
        engines.push(Arc::new(crate::ChromeEngine::new(
            templates.clone(),
            options.chrome.clone(),
        )));
        Self {
            inner: Arc::new(Inner {
                templates,
                default_engine: options.engine,
                engines,
                storage: None,
                disk: options.disk.clone(),
                dispatcher: None,
                fake: None,
                #[cfg(feature = "typst")]
                typst,
            }),
        }
    }

    /// The fake's documents: `engine` for every kind, recording into
    /// `state`.
    pub(crate) fn fake(templates: Templates, engine: Arc<dyn PdfEngine>, state: Arc<FakeState>) -> Self {
        Self {
            inner: Arc::new(Inner {
                templates,
                default_engine: EngineKind::Typst,
                engines: vec![engine],
                storage: None,
                disk: None,
                dispatcher: None,
                fake: Some(state),
                #[cfg(feature = "typst")]
                typst: None,
            }),
        }
    }

    /// Uses `engine` for renders of its kind, replacing the built-in one
    /// (a Typst engine with a fixed clock, a Chrome engine with other
    /// settings, your own engine).
    pub fn with_engine(mut self, engine: Arc<dyn PdfEngine>) -> Self {
        let inner = Arc::make_mut(&mut self.inner);
        inner.engines.retain(|e| e.kind() != engine.kind());
        inner.engines.push(engine);
        self
    }

    /// The engine for templates that exist as both `.typ` and `.html`.
    pub fn default_engine(mut self, kind: EngineKind) -> Self {
        Arc::make_mut(&mut self.inner).default_engine = kind;
        self
    }

    /// Stores documents on `storage`: on `disk`, or on the default disk
    /// when `None`.
    pub fn storage(mut self, storage: Arc<Storage>, disk: Option<String>) -> Self {
        let inner = Arc::make_mut(&mut self.inner);
        inner.storage = Some(storage);
        if disk.is_some() {
            inner.disk = disk;
        }
        self
    }

    /// Queues renders through `dispatcher` (navree-queue's, in an app with
    /// `QueueModule`).
    pub fn dispatcher(mut self, dispatcher: Arc<dyn Dispatcher>) -> Self {
        Arc::make_mut(&mut self.inner).dispatcher = Some(dispatcher);
        self
    }

    /// The templates.
    pub fn templates(&self) -> &Templates {
        &self.inner.templates
    }

    /// The engine of `kind`, if this build has it.
    pub fn engine(&self, kind: EngineKind) -> Option<Arc<dyn PdfEngine>> {
        self.inner
            .engines
            .iter()
            .find(|engine| engine.kind() == kind)
            .cloned()
    }

    /// Which engine renders `render`: the one it names, else the one its
    /// template picks. An unknown template name is
    /// [`DocumentsError::TemplateNotFound`], listing the files tried.
    pub fn engine_for(&self, render: &Render) -> Result<EngineKind, DocumentsError> {
        if let Some(kind) = render.engine {
            return Ok(kind);
        }
        let name = match &render.source {
            TemplateSource::Typst(_) => return Ok(EngineKind::Typst),
            TemplateSource::Html(_) => return Ok(EngineKind::Chrome),
            TemplateSource::Template(name) => name,
        };
        match Path::new(name).extension().and_then(|e| e.to_str()) {
            Some(TYPST_EXTENSION) => return Ok(EngineKind::Typst),
            Some(HTML_EXTENSION) => return Ok(EngineKind::Chrome),
            _ => {}
        }
        let (Some(typ), Some(html)) = (
            Templates::template_path(name, TYPST_EXTENSION),
            Templates::template_path(name, HTML_EXTENSION),
        ) else {
            return Err(DocumentsError::TemplateNotFound {
                name: name.clone(),
                tried: Vec::new(),
            });
        };
        let templates = &self.inner.templates;
        match (templates.exists(&typ), templates.exists(&html)) {
            (true, true) => Ok(self.inner.default_engine),
            (true, false) => Ok(EngineKind::Typst),
            (false, true) => Ok(EngineKind::Chrome),
            (false, false) if self.accepts_any_template() => Ok(self.inner.default_engine),
            (false, false) => Err(DocumentsError::TemplateNotFound {
                name: name.clone(),
                tried: vec![typ, html],
            }),
        }
    }

    /// Renders the template `name` with `data` (anything `Serialize`).
    /// Shorthand for [`render`](Documents::render) with
    /// `Render::template(name).data(data)`.
    pub async fn pdf(&self, name: &str, data: impl Serialize) -> Result<Pdf, DocumentsError> {
        self.render(&Render::template(name).data(data)).await
    }

    /// Renders `render` with its engine.
    pub async fn render(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        let kind = self.engine_for(render)?;
        let engine = self.engine_of(kind)?;
        let started = Instant::now();
        let result = engine.render(render).await;
        match &result {
            Ok(pdf) => tracing::debug!(
                target: "navree_documents",
                template = render.name(),
                engine = kind.name(),
                bytes = pdf.len(),
                ms = started.elapsed().as_millis() as u64,
                "rendered a document"
            ),
            Err(error) => tracing::warn!(
                target: "navree_documents",
                template = render.name(),
                engine = kind.name(),
                %error,
                "rendering a document failed"
            ),
        }
        result
    }

    /// Renders `render` and writes the PDF to `path` on the documents disk
    /// (`documents.disk`, else the default disk).
    pub async fn save(&self, render: &Render, path: &str) -> Result<StoredDocument, DocumentsError> {
        let disk = self.disk()?;
        let pdf = self.render(render).await?;
        let size = pdf.len() as u64;
        disk.put(path, pdf.into_bytes()).await?;
        Ok(StoredDocument::new(disk, path, size))
    }

    /// Queues `render`: a queue worker renders it and writes the PDF to
    /// `path` on the documents disk. Returns at once with a
    /// [`QueuedDocument`] to check with [`status`](Documents::status).
    ///
    /// The job (`documents.render`) goes through the app's `dyn
    /// Dispatcher`, so in a handler it is queued only if the request's
    /// transaction commits. Without a dispatcher (no `QueueModule`), the
    /// document is rendered and stored before this returns.
    pub async fn queue(
        &self,
        render: Render,
        path: impl Into<String>,
    ) -> Result<QueuedDocument, DocumentsError> {
        let path = path.into();
        if let Some(error) = &render.data_error {
            return Err(DocumentsError::Data(error.clone()));
        }
        if let Some(fake) = &self.inner.fake {
            let queued = QueuedDocument::new(self.disk_name(), path, None);
            fake.record_queued(&render, &queued);
            return Ok(queued);
        }
        let disk = self.disk()?;
        // Check the path before anything is queued.
        disk.exists(&path).await?;
        let job = RenderDocument::new(render, disk.name(), &path);
        let id = job.id;
        let Some(dispatcher) = &self.inner.dispatcher else {
            let queued = QueuedDocument::with_id(id, disk.name(), path, None);
            job.run(self).await?;
            return Ok(queued);
        };
        let job_id = dispatcher
            .dispatch(job)
            .await
            .map_err(|error| DocumentsError::Queue(error.to_string()))?;
        Ok(QueuedDocument::with_id(id, disk.name(), path, Some(job_id)))
    }

    /// Whether the queued document is ready: [`DocumentStatus::Ready`] once
    /// the PDF is on the disk, [`DocumentStatus::Failed`] when rendering
    /// failed for good, [`DocumentStatus::Pending`] until then.
    pub async fn status(&self, queued: &QueuedDocument) -> Result<DocumentStatus, DocumentsError> {
        if self.inner.fake.is_some() && self.inner.storage.is_none() {
            return Ok(DocumentStatus::Pending);
        }
        let disk = self.storage_ref()?.disk(&queued.disk)?;
        crate::queue::status(&disk, &queued.path).await
    }

    /// The font families Typst templates can use, sorted (loads the fonts
    /// on a blocking thread). Empty without the `typst` feature.
    pub async fn font_families(&self) -> Result<Vec<String>, DocumentsError> {
        #[cfg(feature = "typst")]
        if let Some(typst) = self.inner.typst.clone() {
            return tokio::task::spawn_blocking(move || typst.font_families())
                .await
                .map_err(|error| DocumentsError::Engine {
                    engine: "typst",
                    message: format!("loading the fonts stopped: {error}"),
                });
        }
        Ok(Vec::new())
    }

    /// Releases what the engines hold (closes Chrome). Rendering again
    /// starts them again; the module calls this when the app stops.
    pub async fn shutdown(&self) {
        for engine in &self.inner.engines {
            engine.shutdown().await;
        }
    }

    /// The documents disk.
    pub fn disk(&self) -> Result<Disk, DocumentsError> {
        let storage = self.storage_ref()?;
        Ok(match &self.inner.disk {
            Some(name) => storage.disk(name)?,
            None => storage.default_disk(),
        })
    }

    /// The disk called `name`.
    pub(crate) fn storage_disk(&self, name: &str) -> Result<Disk, DocumentsError> {
        Ok(self.storage_ref()?.disk(name)?)
    }

    fn disk_name(&self) -> String {
        match (&self.inner.disk, &self.inner.storage) {
            (Some(name), _) => name.clone(),
            (None, Some(storage)) => storage.default_disk().name().to_owned(),
            (None, None) => "default".to_owned(),
        }
    }

    fn storage_ref(&self) -> Result<&Arc<Storage>, DocumentsError> {
        self.inner.storage.as_ref().ok_or_else(|| {
            DocumentsError::Queue(
                "storing documents needs a storage disk: add StorageModule to the app \
                 (or call Documents::storage)"
                    .into(),
            )
        })
    }

    fn engine_of(&self, kind: EngineKind) -> Result<Arc<dyn PdfEngine>, DocumentsError> {
        if self.inner.fake.is_some() {
            if let Some(engine) = self.inner.engines.first() {
                return Ok(engine.clone());
            }
        }
        self.engine(kind)
            .ok_or_else(|| DocumentsError::EngineUnavailable {
                engine: kind.name(),
                reason: format!(
                    "the `{}` feature of navree-documents is off in this build",
                    kind.name()
                ),
            })
    }

    fn accepts_any_template(&self) -> bool {
        self.inner
            .fake
            .as_ref()
            .is_some_and(|fake| fake.accepts_any_template())
    }
}
