//! [`ChromeEngine`]: PDF from HTML templates, printed by headless Chrome
//! over the DevTools protocol.

mod cdp;
mod launch;
mod page;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{Mutex, Semaphore};

use self::cdp::Cdp;
pub use self::launch::CHROME_ENV;
use self::launch::{LaunchedChrome, find_chrome, launch, resolve_endpoint};
pub use self::page::DOCUMENT_ORIGIN;
use self::page::PageJob;
use crate::engine::{EngineKind, PdfEngine};
use crate::options::ChromeOptions;
use crate::render::Render;
use crate::templates::Templates;
use crate::views::HtmlTemplates;
use crate::{DocumentsError, Pdf};

const ENGINE: &str = "chrome";

/// PDF from HTML templates, printed by headless Chrome (or Chromium, or
/// Edge) over the Chrome DevTools Protocol.
///
/// - **Templates** are `.html` files in the template directories (or
///   embedded by modules), rendered with navree-view: the render's data
///   are the template's variables, plus `locale` (`en` when the render
///   has none) and `dir` (`rtl` or `ltr`) unless the data has keys of
///   those names. `{% extends %}` and `{% include %}` take full paths
///   (`"layouts/document.html"`), and the `money` and `date` filters work.
/// - **Assets**: the document loads from a private origin
///   ([`DOCUMENT_ORIGIN`]), so `<img src="images/logo.png">`, stylesheets and web
///   fonts resolve inside the template tree (relative to the document,
///   `/`-paths from the root). Requests to any other origin are blocked
///   unless `documents.chrome.allow_network` is on, so a template can
///   never make the server fetch an address its data names.
/// - **Pages**: paper, margins, orientation, header and footer come from
///   [`PrintOptions`](crate::PrintOptions) (the render's, else
///   `documents.chrome.print`); a CSS `@page { size }` rule wins while
///   `prefer_css_page_size` is on.
/// - **Isolation**: every document prints in its own browser context,
///   disposed of afterwards: no cookies, storage or cache carry over.
/// - **Pool**: one browser serves every render, with at most
///   `documents.chrome.pool_size` pages printing at once; more renders
///   wait. The browser starts on the first render (or connects to
///   `documents.chrome.endpoint`), starts again if it crashes, and is
///   closed by [`shutdown`](ChromeEngine::shutdown) or when the app stops.
///
/// `documents.chrome.timeout_secs` limits each render, waiting for a page
/// included.
#[derive(Clone)]
pub struct ChromeEngine {
    inner: Arc<Inner>,
}

struct Inner {
    templates: Templates,
    options: ChromeOptions,
    /// The value of [`CHROME_ENV`] when the engine was built.
    env_path: Option<PathBuf>,
    html: HtmlTemplates,
    browser: Mutex<Option<Browser>>,
    slots: Semaphore,
}

/// A browser connection, and the process when this engine started it.
struct Browser {
    cdp: Arc<Cdp>,
    process: Option<LaunchedChrome>,
}

impl std::fmt::Debug for ChromeEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChromeEngine")
            .field("templates", &self.inner.templates)
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

impl ChromeEngine {
    /// The engine over `templates` with `options`. Nothing starts until
    /// the first render.
    pub fn new(templates: Templates, options: ChromeOptions) -> Self {
        let slots = options.pool_size.max(1);
        Self {
            inner: Arc::new(Inner {
                templates,
                options,
                env_path: std::env::var_os(CHROME_ENV).map(PathBuf::from),
                html: HtmlTemplates::default(),
                browser: Mutex::new(None),
                slots: Semaphore::new(slots),
            }),
        }
    }

    /// The engine's settings.
    pub fn options(&self) -> &ChromeOptions {
        &self.inner.options
    }

    /// The templates.
    pub fn templates(&self) -> &Templates {
        &self.inner.templates
    }

    /// The Chrome executable the engine would start: `documents.chrome.path`,
    /// else [`CHROME_ENV`], else the first usual install location that
    /// exists. `None` with an `endpoint`, or when no Chrome is found.
    pub fn executable(&self) -> Option<PathBuf> {
        if self.inner.options.endpoint.is_some() {
            return None;
        }
        find_chrome(
            self.inner.options.path.as_deref(),
            self.inner.env_path.clone(),
        )
    }

    /// Whether a browser is running (or connected) for this engine.
    pub async fn is_running(&self) -> bool {
        self.inner
            .browser
            .lock()
            .await
            .as_ref()
            .is_some_and(|browser| browser.cdp.is_alive())
    }

    /// The HTML that `render` prints: the template rendered with its data,
    /// or the inline HTML. No browser is involved, so this is what to
    /// assert on in fast tests.
    pub async fn render_html(&self, render: &Render) -> Result<String, DocumentsError> {
        Ok(self.document(render).await?.1)
    }

    /// Renders `render` to a PDF.
    pub async fn render(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        let (path, html) = self.document(render).await?;
        let options = &self.inner.options;
        let print = render.print.as_ref().unwrap_or(&options.print).to_cdp();
        let job = PageJob {
            path,
            html,
            locale: render.locale.clone(),
            print,
            templates: self.inner.templates.clone(),
            allow_network: options.allow_network,
        };
        let secs = options.timeout_secs.max(1);
        let printed = tokio::time::timeout(Duration::from_secs(secs), async {
            let _slot = self
                .inner
                .slots
                .acquire()
                .await
                .map_err(|_| engine_error("the engine is shut down"))?;
            let cdp = self.browser().await?;
            page::print(&cdp, job).await.map_err(engine_error)
        })
        .await
        .map_err(|_| DocumentsError::Timeout {
            engine: ENGINE,
            secs,
        })??;
        let pdf = Pdf::from_engine(printed);
        match &render.title {
            Some(title) => pdf.with_metadata(title, None),
            None => Ok(pdf),
        }
    }

    /// Closes the browser this engine started (or disconnects from the
    /// endpoint). The next render starts it again.
    pub async fn shutdown(&self) {
        let browser = self.inner.browser.lock().await.take();
        let Some(browser) = browser else {
            return;
        };
        let Some(mut process) = browser.process else {
            return;
        };
        let close = browser.cdp.call("Browser.close", json!({}), None);
        let _ = tokio::time::timeout(Duration::from_secs(2), close).await;
        drop(browser.cdp);
        let exited = tokio::time::timeout(Duration::from_secs(5), process.child.wait()).await;
        if exited.is_err() {
            let _ = process.child.kill().await;
        }
    }

    /// The browser connection, started (again) when there is none or it
    /// was lost.
    async fn browser(&self) -> Result<Arc<Cdp>, DocumentsError> {
        let mut browser = self.inner.browser.lock().await;
        if let Some(current) = browser.as_ref() {
            if current.cdp.is_alive() {
                return Ok(current.cdp.clone());
            }
            tracing::warn!(target: "navree_documents::chrome", "the browser went away; starting it again");
        }
        // Dropping the old one kills its process.
        *browser = None;
        let started = self.start().await?;
        let cdp = started.cdp.clone();
        *browser = Some(started);
        Ok(cdp)
    }

    async fn start(&self) -> Result<Browser, DocumentsError> {
        let options = &self.inner.options;
        let timeout = Duration::from_secs(options.timeout_secs.max(1));
        if let Some(endpoint) = &options.endpoint {
            let url = resolve_endpoint(endpoint, timeout)
                .await
                .map_err(unavailable)?;
            let cdp = Cdp::connect(&url).await.map_err(unavailable)?;
            tracing::info!(target: "navree_documents::chrome", endpoint, "connected to Chrome");
            return Ok(Browser {
                cdp: Arc::new(cdp),
                process: None,
            });
        }
        let path = self.executable().ok_or_else(|| {
            unavailable(format!(
                "no Chrome executable found: set documents.chrome.path or {CHROME_ENV}, \
                 install Chrome or Chromium, or set documents.chrome.endpoint"
            ))
        })?;
        let launched = launch(&path, options, timeout).await.map_err(unavailable)?;
        let cdp = Cdp::connect(&launched.ws_url).await.map_err(unavailable)?;
        tracing::info!(target: "navree_documents::chrome", path = %path.display(), "started Chrome");
        Ok(Browser {
            cdp: Arc::new(cdp),
            process: Some(launched),
        })
    }

    /// The document's path in the template tree and its HTML, rendered
    /// on Tokio's blocking pool (templates are read on the first HTML
    /// render and re-read when files change).
    async fn document(&self, render: &Render) -> Result<(String, String), DocumentsError> {
        if let Some(error) = &render.data_error {
            return Err(DocumentsError::Data(error.clone()));
        }
        let inner = self.inner.clone();
        let render = render.clone();
        tokio::task::spawn_blocking(move || inner.html.document(&inner.templates, &render))
            .await
            .map_err(|error| engine_error(format!("the template renderer stopped: {error}")))?
    }
}

impl PdfEngine for ChromeEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Chrome
    }

    fn render<'a>(
        &'a self,
        render: &'a Render,
    ) -> navree_kernel::BoxFuture<'a, Result<Pdf, DocumentsError>> {
        Box::pin(ChromeEngine::render(self, render))
    }

    fn shutdown(&self) -> navree_kernel::BoxFuture<'_, ()> {
        Box::pin(ChromeEngine::shutdown(self))
    }
}

fn engine_error(message: impl Into<String>) -> DocumentsError {
    DocumentsError::Engine {
        engine: ENGINE,
        message: message.into(),
    }
}

fn unavailable(reason: impl Into<String>) -> DocumentsError {
    DocumentsError::EngineUnavailable {
        engine: ENGINE,
        reason: reason.into(),
    }
}
