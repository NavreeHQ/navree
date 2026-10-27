//! [`Browser`] and [`BrowserBuilder`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::artifacts::Artifacts;
use crate::page::{self, Page};
use crate::process::{self, Chrome};
use crate::settings::{Settings, absolute};
use crate::{BrowserError, Viewport, chrome};

/// A browser for one test: a browser context (its own cookies, storage
/// and cache) in a headless Chrome shared by the tests of the binary.
///
/// ```no_run
/// # async fn demo() -> Result<(), navree_browser::BrowserError> {
/// use navree_browser::Browser;
///
/// let browser = Browser::launch().await?;
/// let page = browser.visit("http://127.0.0.1:8080/login").await?;
/// page.type_into("Email", "rep1@navree.test").await?;
/// page.press("Sign in").await?;
/// page.assert_see("Pipeline by stage").await?;
/// # Ok(())
/// # }
/// ```
///
/// The context closes when the browser and its pages are dropped; Chrome
/// stops when no browser of the binary uses it any more.
#[derive(Clone)]
pub struct Browser {
    inner: Arc<BrowserInner>,
}

/// What a browser's pages share.
pub(crate) struct BrowserInner {
    pub(crate) chrome: Arc<Chrome>,
    pub(crate) context: String,
    pub(crate) settings: Settings,
    pub(crate) artifacts: Artifacts,
}

impl Drop for BrowserInner {
    fn drop(&mut self) {
        self.chrome.connection.fire(
            None,
            "Target.disposeBrowserContext",
            json!({ "browserContextId": self.context }),
        );
    }
}

impl Browser {
    /// Starts (or joins) headless Chrome with the settings of the
    /// `NAVREE_BROWSER_*` environment variables.
    pub async fn launch() -> Result<Browser, BrowserError> {
        Browser::builder().launch().await
    }

    /// Options in code: `.headless(false)`, `.chrome(path)`,
    /// `.slow_mo(ms)`, `.timeout(duration)`, `.window(w, h)`,
    /// `.artifacts(dir)`, then `.launch()`.
    pub fn builder() -> BrowserBuilder {
        BrowserBuilder::default()
    }

    /// The browser binary navree-browser would start: `NAVREE_BROWSER_CHROME`,
    /// else Chrome or Chromium installed on the machine, Chrome for Testing
    /// from `navree browser:install`, Playwright's Chromium, or Microsoft
    /// Edge. Use it to skip browser tests on a machine without one.
    pub fn find_chrome() -> Result<PathBuf, BrowserError> {
        let headless = Settings::from_process_env()
            .map(|s| s.headless)
            .unwrap_or(true);
        chrome::find(headless)
    }

    /// A new blank page (desktop viewport of the browser's window size).
    pub async fn new_page(&self) -> Result<Page, BrowserError> {
        let (width, height) = self.inner.settings.window;
        page::open(self.inner.clone(), Viewport::new(width, height)).await
    }

    /// A new page at `url`, after its `load` event.
    pub async fn visit(&self, url: impl AsRef<str>) -> Result<Page, BrowserError> {
        let page = self.new_page().await?;
        page.visit(url).await?;
        Ok(page)
    }

    /// A new page with `viewport` (size, pixel ratio, touch, user agent)
    /// at `url`.
    pub async fn visit_with(
        &self,
        url: impl AsRef<str>,
        viewport: Viewport,
    ) -> Result<Page, BrowserError> {
        let page = page::open(self.inner.clone(), viewport).await?;
        page.visit(url).await?;
        Ok(page)
    }

    /// The browser's version, e.g. `156.0.8078.48`.
    pub fn version(&self) -> &str {
        &self.inner.chrome.version
    }

    /// The folder of this test's screenshots and failure files.
    pub fn artifacts_dir(&self) -> &Path {
        self.inner.artifacts.dir()
    }

    /// How long each step waits.
    pub fn timeout(&self) -> Duration {
        self.inner.settings.timeout
    }
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("version", &self.inner.chrome.version)
            .field("context", &self.inner.context)
            .field("headless", &self.inner.settings.headless)
            .finish_non_exhaustive()
    }
}

/// Browser options in code; each one overrides its `NAVREE_BROWSER_*`
/// variable.
#[derive(Clone, Debug, Default)]
#[must_use = "call .launch() to start the browser"]
pub struct BrowserBuilder {
    chrome: Option<PathBuf>,
    headless: Option<bool>,
    slow_mo: Option<u64>,
    timeout: Option<Duration>,
    window: Option<(u32, u32)>,
    artifacts: Option<PathBuf>,
}

impl BrowserBuilder {
    /// Shows the window with `false` (`NAVREE_BROWSER_HEADLESS`).
    pub fn headless(mut self, headless: bool) -> Self {
        self.headless = Some(headless);
        self
    }

    /// The Chrome, Chromium or Edge binary (`NAVREE_BROWSER_CHROME`).
    pub fn chrome(mut self, path: impl Into<PathBuf>) -> Self {
        self.chrome = Some(path.into());
        self
    }

    /// Milliseconds to pause after each action (`NAVREE_BROWSER_SLOW_MO`).
    pub fn slow_mo(mut self, millis: u64) -> Self {
        self.slow_mo = Some(millis);
        self
    }

    /// How long each action, assertion and wait may take
    /// (`NAVREE_BROWSER_TIMEOUT_MS`, default 5 s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The window and default viewport size (default 1440 by 900).
    pub fn window(mut self, width: u32, height: u32) -> Self {
        self.window = Some((width, height));
        self
    }

    /// Where screenshots and failure files go (`NAVREE_BROWSER_ARTIFACTS`,
    /// default `navree-browser` in Cargo's target directory).
    pub fn artifacts(mut self, dir: impl Into<PathBuf>) -> Self {
        self.artifacts = Some(dir.into());
        self
    }

    /// The settings: the environment's, overridden by the builder's.
    pub(crate) fn settings(&self) -> Result<Settings, BrowserError> {
        let mut settings = Settings::from_process_env()?;
        if let Some(chrome) = &self.chrome {
            settings.chrome = Some(chrome.clone());
        }
        if let Some(headless) = self.headless {
            settings.headless = headless;
        }
        if let Some(slow_mo) = self.slow_mo {
            settings.slow_mo = Duration::from_millis(slow_mo);
        }
        if let Some(timeout) = self.timeout {
            if timeout.is_zero() {
                return Err(BrowserError::Config(
                    "Browser::builder().timeout(..) must be longer than zero".to_owned(),
                ));
            }
            settings.timeout = timeout;
        }
        if let Some((width, height)) = self.window {
            if width == 0 || height == 0 {
                return Err(BrowserError::Config(format!(
                    "Browser::builder().window({width}, {height}) needs a size above zero"
                )));
            }
            settings.window = (width, height);
        }
        if let Some(dir) = &self.artifacts {
            settings.artifacts = absolute(dir);
        }
        Ok(settings)
    }

    /// Starts (or joins) Chrome and opens this test's browser context.
    pub async fn launch(self) -> Result<Browser, BrowserError> {
        let settings = self.settings()?;
        let path = match &settings.chrome {
            Some(path) if path.is_file() => path.clone(),
            Some(path) => {
                return Err(BrowserError::ChromeNotFound(format!(
                    "the browser {} does not exist (NAVREE_BROWSER_CHROME or Browser::builder().chrome(..))",
                    path.display()
                )));
            }
            None => chrome::find(settings.headless)?,
        };
        let artifacts = Artifacts::for_current_test(&settings.artifacts);
        let chrome = process::shared(&path, settings.headless, settings.window).await?;
        let context = chrome
            .connection
            .call(None, "Target.createBrowserContext", json!({}))
            .await
            .map_err(|e| BrowserError::Connection(format!("Target.createBrowserContext: {e}")))?;
        let context = context["browserContextId"]
            .as_str()
            .ok_or_else(|| {
                BrowserError::Connection("Target.createBrowserContext returned no id".to_owned())
            })?
            .to_owned();
        Ok(Browser {
            inner: Arc::new(BrowserInner {
                chrome,
                context,
                settings,
                artifacts,
            }),
        })
    }
}
