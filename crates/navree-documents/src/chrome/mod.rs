//! [`ChromeEngine`]: PDF from HTML templates, printed by headless Chrome
//! over the DevTools protocol.

mod cdp;
mod launch;
mod page;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use navree_view::{ViewError, ViewOptions, Views};
use serde_json::{Value, json};
use tokio::sync::{Mutex, OnceCell, Semaphore};

pub use self::launch::CHROME_ENV;
use self::cdp::Cdp;
use self::launch::{LaunchedChrome, find_chrome, launch, resolve_endpoint};
pub use self::page::DOCUMENT_ORIGIN;
use self::page::PageJob;
use crate::engine::{EngineKind, PdfEngine};
use crate::options::ChromeOptions;
use crate::render::{Render, TemplateSource};
use crate::templates::{HTML_EXTENSION, Templates};
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
    views: OnceCell<Arc<Views>>,
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
                views: OnceCell::new(),
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
        let print = render
            .print
            .as_ref()
            .unwrap_or(&options.print)
            .to_cdp();
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
        let launched = launch(&path, options, timeout)
            .await
            .map_err(unavailable)?;
        let cdp = Cdp::connect(&launched.ws_url)
            .await
            .map_err(unavailable)?;
        tracing::info!(target: "navree_documents::chrome", path = %path.display(), "started Chrome");
        Ok(Browser {
            cdp: Arc::new(cdp),
            process: Some(launched),
        })
    }

    /// The document's path in the template tree and its HTML.
    async fn document(&self, render: &Render) -> Result<(String, String), DocumentsError> {
        if let Some(error) = &render.data_error {
            return Err(DocumentsError::Data(error.clone()));
        }
        match &render.source {
            TemplateSource::Html(html) => Ok(("inline.html".to_owned(), html.clone())),
            TemplateSource::Typst(_) => Err(DocumentsError::EngineUnavailable {
                engine: ENGINE,
                reason: "Typst markup is compiled by the typst engine".into(),
            }),
            TemplateSource::Template(name) => {
                let path = Templates::template_path(name, HTML_EXTENSION).ok_or_else(|| {
                    DocumentsError::TemplateNotFound {
                        name: name.clone(),
                        tried: Vec::new(),
                    }
                })?;
                let views = self.views().await?;
                let context = template_context(render);
                let lookup = path.clone();
                let html = tokio::task::spawn_blocking(move || views.render(&lookup, context))
                    .await
                    .map_err(|error| engine_error(format!("the template renderer stopped: {error}")))?
                    .map_err(|error| view_error(name, error))?;
                Ok((path, html))
            }
        }
    }

    /// The HTML templates, read on the first HTML render and re-read when
    /// files change.
    async fn views(&self) -> Result<Arc<Views>, DocumentsError> {
        self.inner
            .views
            .get_or_try_init(|| async {
                let templates = self.inner.templates.clone();
                let views = tokio::task::spawn_blocking(move || {
                    let options = ViewOptions {
                        dirs: templates.view_dirs(),
                        reload: true,
                    };
                    Views::with_templates(&options, templates.embedded_html())
                })
                .await
                .map_err(|error| engine_error(format!("loading the templates stopped: {error}")))?
                .map_err(|error| view_error("", error))?;
                Ok::<_, DocumentsError>(Arc::new(views))
            })
            .await
            .cloned()
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

/// The variables of an HTML template: the data's fields (or `data` for
/// data that is not an object), plus `locale` and `dir`.
pub(crate) fn template_context(render: &Render) -> Value {
    let locale = Value::String(render.locale.clone().unwrap_or_else(|| "en".into()));
    let dir = Value::String(render.direction().into());
    match &render.data {
        Value::Object(map) => {
            let mut map = map.clone();
            map.entry("locale").or_insert(locale);
            map.entry("dir").or_insert(dir);
            Value::Object(map)
        }
        Value::Null => json!({ "locale": locale, "dir": dir }),
        other => json!({ "data": other, "locale": locale, "dir": dir }),
    }
}

fn view_error(name: &str, error: ViewError) -> DocumentsError {
    match error {
        ViewError::NotFound { tried, .. } => DocumentsError::TemplateNotFound {
            name: name.to_owned(),
            tried,
        },
        ViewError::Template { name, source } => {
            let file = source.name().map(str::to_owned).unwrap_or(name);
            let message = match source.detail() {
                Some(detail) => format!("{}: {detail}", source.kind()),
                None => source.kind().to_string(),
            };
            DocumentsError::Template {
                file,
                line: source.line(),
                column: None,
                message,
            }
        }
        ViewError::Io { path, source } => DocumentsError::Template {
            file: path.display().to_string(),
            line: None,
            column: None,
            message: source.to_string(),
        },
        other => DocumentsError::Template {
            file: name.to_owned(),
            line: None,
            column: None,
            message: other.to_string(),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_variables_add_locale_and_direction() {
        let render = Render::template("x").data(json!({"name": "Ada"})).locale("ar");
        assert_eq!(
            template_context(&render),
            json!({"name": "Ada", "locale": "ar", "dir": "rtl"})
        );
        let render = Render::template("x").data(json!({"locale": "custom"}));
        assert_eq!(
            template_context(&render),
            json!({"locale": "custom", "dir": "ltr"})
        );
        assert_eq!(
            template_context(&Render::template("x")),
            json!({"locale": "en", "dir": "ltr"})
        );
        assert_eq!(
            template_context(&Render::template("x").data([1, 2])),
            json!({"data": [1, 2], "locale": "en", "dir": "ltr"})
        );
    }
}
