//! [`Page`] and [`Element`]: actions, assertions, waits and reading the
//! page. Every step waits, up to the timeout, for the page to be ready.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wildmatch::WildMatch;

use crate::browser::BrowserInner;
use crate::cdp::{CdpError, PageLog};
use crate::error::{FailureFiles, StepFailure};
use crate::settings::absolute;
use crate::{BrowserError, Target, Viewport, artifacts, keys};

/// The page script (`page.js`).
const LIB: &str = include_str!("page.js");
/// Answered by the short page expression when the script is not installed
/// in the current document yet.
const MISSING: &str = "navree-browser: script missing";
/// How often a waiting step looks again.
const POLL: Duration = Duration::from_millis(40);
/// The least time one look at the page may take, even when the step's
/// timeout is shorter: a busy page is reported as slow, not as failing.
const PROBE_MIN: Duration = Duration::from_secs(2);
/// The least time a navigation may take, whatever the step timeout.
const NAVIGATION_MIN: Duration = Duration::from_secs(30);
/// How long capturing the failure files may take.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// One tab of a [`Browser`](crate::Browser), and what it reported.
pub(crate) struct PageCore {
    browser: Arc<BrowserInner>,
    session: String,
    target_id: String,
    log: Arc<Mutex<PageLog>>,
    viewport: Mutex<Viewport>,
}

impl PageCore {
    async fn call(&self, method: &str, params: Value) -> Result<Value, CdpError> {
        self.browser
            .chrome
            .connection
            .call(Some(&self.session), method, params)
            .await
    }

    fn log(&self) -> MutexGuard<'_, PageLog> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn apply_viewport(&self, v: &Viewport) -> Result<(), CdpError> {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": v.width(),
                "height": v.height(),
                "deviceScaleFactor": v.scale(),
                "mobile": v.is_mobile(),
                "screenWidth": v.width(),
                "screenHeight": v.height(),
            }),
        )
        .await?;
        let touch = if v.has_touch() {
            json!({ "enabled": true, "maxTouchPoints": 5 })
        } else {
            json!({ "enabled": false })
        };
        self.call("Emulation.setTouchEmulationEnabled", touch).await?;
        let chrome = &self.browser.chrome;
        self.call(
            "Emulation.setUserAgentOverride",
            json!({ "userAgent": v.user_agent(&chrome.version, &chrome.user_agent) }),
        )
        .await?;
        Ok(())
    }
}

impl Drop for PageCore {
    fn drop(&mut self) {
        let connection = &self.browser.chrome.connection;
        connection.fire(
            None,
            "Target.closeTarget",
            json!({ "targetId": self.target_id }),
        );
        connection.forget(&self.session);
    }
}

/// Opens a tab in the browser's context with `viewport`.
pub(crate) async fn open(
    browser: Arc<BrowserInner>,
    viewport: Viewport,
) -> Result<Page, BrowserError> {
    let refused = |method: &str, e: CdpError| BrowserError::Connection(format!("{method}: {e}"));
    let connection = &browser.chrome.connection;
    let created = connection
        .call(
            None,
            "Target.createTarget",
            json!({ "url": "about:blank", "browserContextId": browser.context }),
        )
        .await
        .map_err(|e| refused("Target.createTarget", e))?;
    let target_id = created["targetId"].as_str().unwrap_or_default().to_owned();
    let attached = connection
        .call(
            None,
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
        )
        .await
        .map_err(|e| refused("Target.attachToTarget", e))?;
    let session = attached["sessionId"].as_str().unwrap_or_default().to_owned();
    let log = connection.log(&session);
    log.lock().unwrap_or_else(|e| e.into_inner()).main_frame = Some(target_id.clone());
    let timeout = browser.settings.timeout;
    let core = Arc::new(PageCore {
        browser,
        session,
        target_id,
        log,
        viewport: Mutex::new(viewport.clone()),
    });
    let enable = [
        ("Page.enable", json!({})),
        ("Runtime.enable", json!({})),
        ("Network.enable", json!({})),
        ("Log.enable", json!({})),
        ("Page.setLifecycleEventsEnabled", json!({ "enabled": true })),
        ("Emulation.setFocusEmulationEnabled", json!({ "enabled": true })),
    ];
    let calls = enable.iter().map(|(method, params)| {
        let core = &core;
        async move {
            core.call(method, params.clone())
                .await
                .map_err(|e| refused(method, e))
        }
    });
    futures_util::future::try_join_all(calls).await?;
    core.apply_viewport(&viewport)
        .await
        .map_err(|e| refused("Emulation", e))?;
    Ok(Page {
        scope: Scope {
            core,
            chain: Vec::new(),
            timeout,
        },
    })
}

/// A cookie of the page, from [`Page::cookie`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Cookie {
    /// The name.
    pub name: String,
    /// The value.
    pub value: String,
    /// The domain it is sent to.
    pub domain: String,
    /// The path it is sent to.
    pub path: String,
    /// When it expires, in seconds since the Unix epoch (`None`: when the
    /// browser closes).
    #[serde(default, deserialize_with = "expiry")]
    pub expires: Option<f64>,
    /// Hidden from scripts.
    pub http_only: bool,
    /// Sent over HTTPS only.
    pub secure: bool,
    /// `Strict`, `Lax` or `None` (absent when not set).
    #[serde(default)]
    pub same_site: Option<String>,
}

fn expiry<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    let value = <Option<f64> as serde::Deserialize>::deserialize(d)?;
    Ok(value.filter(|seconds| *seconds > 0.0))
}

/// A request the page made, from [`Page::wait_for_request`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageRequest {
    /// `GET`, `POST`, ...
    pub method: String,
    /// The full URL.
    pub url: String,
    /// The response status (`None` when the request failed).
    pub status: Option<u16>,
    /// Why it failed (`net::ERR_CONNECTION_REFUSED`, ...).
    pub failure: Option<String>,
}

/// Why a step did not succeed.
enum Fail {
    /// It waited until the timeout; why it was not ready.
    Timeout(String),
    /// It failed without waiting (or waiting cannot help).
    Now(String),
}

/// A script evaluation that did not give a value.
enum EvalError {
    /// The page navigated or reloaded meanwhile: look again.
    Lost,
    Fail(Fail),
}

impl From<Fail> for EvalError {
    fn from(fail: Fail) -> EvalError {
        EvalError::Fail(fail)
    }
}

/// Once-off evaluations: a navigation meanwhile is a failure.
fn once(error: EvalError) -> Fail {
    match error {
        EvalError::Lost => Fail::Now("the page navigated away meanwhile".to_owned()),
        EvalError::Fail(fail) => fail,
    }
}

/// The result of one look at the page.
enum Probe<T> {
    Ready(T),
    Wait(String),
}

/// A page, or an element of it ([`Element`]): where targets are looked up.
#[derive(Clone)]
pub(crate) struct Scope {
    core: Arc<PageCore>,
    /// The targets narrowing the search, outermost first (empty: the page).
    chain: Vec<Target>,
    timeout: Duration,
}

/// A quoted text, shortened for step names.
fn quoted(text: &str) -> String {
    let short: String = text.chars().take(60).collect();
    if short.len() < text.len() {
        format!("{:?}", short + "…")
    } else {
        format!("{text:?}")
    }
}

/// The path (with query) of a URL.
fn path_of(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let with_query = rest.find('/').map_or("/", |i| &rest[i..]);
    let with_query = with_query.split('#').next().unwrap_or(with_query);
    let path = with_query.split('?').next().unwrap_or(with_query);
    (path, with_query)
}

/// Whether `url` matches `glob` (`*` any characters, `?` one): its path,
/// its path and query, or the whole URL.
pub(crate) fn url_matches(glob: &str, url: &str) -> bool {
    let pattern = WildMatch::new(glob);
    let (path, with_query) = path_of(url);
    pattern.matches(path) || pattern.matches(with_query) || pattern.matches(url)
}

impl Scope {
    fn nav_timeout(&self) -> Duration {
        self.timeout.max(NAVIGATION_MIN)
    }

    fn args(&self, target: Option<&Target>, extra: Value) -> Value {
        let mut args = json!({ "chain": self.chain.iter().map(Target::to_json).collect::<Vec<_>>() });
        if let Some(target) = target {
            args["target"] = target.to_json();
        }
        if let Value::Object(extra) = extra {
            for (key, value) in extra {
                args[key] = value;
            }
        }
        args
    }

    fn gone(&self) -> Option<String> {
        self.core.log().gone.clone()
    }

    fn refused(&self, method: &str, error: CdpError) -> Fail {
        if error.is_session_gone() {
            Fail::Now(
                self.gone()
                    .unwrap_or_else(|| "the page was closed".to_owned()),
            )
        } else {
            Fail::Now(format!("the browser refused {method}: {error}"))
        }
    }

    /// Sends a page command. Right after a navigation the page can be
    /// between documents for a moment; such answers are retried.
    async fn cdp(&self, method: &str, params: Value) -> Result<Value, Fail> {
        let mut attempts = 0;
        loop {
            match self.core.call(method, params.clone()).await {
                Ok(answer) => return Ok(answer),
                Err(error) if error.is_between_documents() && attempts < 40 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(error) => return Err(self.refused(method, error)),
            }
        }
    }

    async fn evaluate(&self, expression: String, by_value: bool) -> Result<Value, EvalError> {
        let answer = match self
            .core
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": by_value,
                    "awaitPromise": true,
                    "userGesture": true,
                }),
            )
            .await
        {
            Ok(answer) => answer,
            Err(error) if error.is_context_lost() => return Err(EvalError::Lost),
            Err(error) => return Err(self.refused("Runtime.evaluate", error).into()),
        };
        if let Some(details) = answer.get("exceptionDetails") {
            let text = details["exception"]["description"]
                .as_str()
                .or_else(|| details["text"].as_str())
                .unwrap_or("an exception");
            if text.contains("Execution context was destroyed")
                || text.contains("Promise was collected")
                || text.contains("Inspected target navigated")
            {
                return Err(EvalError::Lost);
            }
            return Err(Fail::Now(format!("the script threw {text}")).into());
        }
        Ok(answer["result"].clone())
    }

/// Calls the page script's `func`. The script is installed once per
    /// document (under a symbol on `globalThis`); later calls send only the
    /// call.
    async fn lib(&self, func: &str, args: Value) -> Result<Value, EvalError> {
        let key = format!("navree-browser:{}:{}", env!("CARGO_PKG_VERSION"), LIB.len());
        let call = format!(
            "(() => {{ const n = globalThis[Symbol.for({key:?})]; return n ? n.{func}({args}) : {MISSING:?}; }})()"
        );
        let result = self.evaluate(call, true).await?;
        let value = result.get("value").cloned().unwrap_or(Value::Null);
        if value != MISSING {
            return Ok(value);
        }
        let install = format!("(globalThis[Symbol.for({key:?})] = ({LIB})()).{func}({args})");
        let result = self.evaluate(install, true).await?;
        Ok(result.get("value").cloned().unwrap_or(Value::Null))
    }

    /// Looks at the page until `probe` is ready or `timeout` passes.
    async fn wait<T, F, Fut>(&self, timeout: Duration, mut probe: F) -> Result<T, Fail>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<Probe<T>, EvalError>>,
    {
        let deadline = Instant::now() + timeout;
        let mut reason = String::new();
        loop {
            if let Some(gone) = self.gone() {
                return Err(Fail::Now(gone));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left.max(PROBE_MIN), probe()).await {
                Ok(Ok(Probe::Ready(value))) => return Ok(value),
                Ok(Ok(Probe::Wait(why))) => reason = why,
                Ok(Err(EvalError::Lost)) => {
                    if reason.is_empty() {
                        reason = "the page kept navigating".to_owned();
                    }
                }
                Ok(Err(EvalError::Fail(fail))) => return Err(fail),
                Err(_) => {
                    if reason.is_empty() {
                        reason = "the page did not answer".to_owned();
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Fail::Timeout(reason));
            }
            tokio::time::sleep(POLL.min(deadline - now)).await;
        }
    }

    /// Runs the page function `func` once and judges its answer.
    async fn probe(
        &self,
        func: &str,
        args: Value,
        target: Option<&Target>,
    ) -> Result<Probe<Value>, EvalError> {
        let answer = self.lib(func, args).await?;
        self.judge(answer, target)
    }

    /// `{ wait }` answers become reasons, `{ fatal }` answers failures.
    fn judge(&self, answer: Value, target: Option<&Target>) -> Result<Probe<Value>, EvalError> {
        let shown = target.map_or_else(|| "the element".to_owned(), Target::to_string);
        if let Some(code) = answer["fatal"].as_str() {
            let message = match code {
                "not-checkable" => format!("{shown} is not a checkbox, radio button or switch"),
                "radio" => format!("{shown} is a radio button, which cannot be unchecked"),
                "not-file" => format!("{shown} is not a file input"),
                other => other.to_owned(),
            };
            return Err(Fail::Now(message).into());
        }
        let Some(code) = answer["wait"].as_str() else {
            return Ok(Probe::Ready(answer));
        };
        let reason = match code {
            "missing" => format!("no element matches {shown}"),
            "hidden" => format!("{shown} is not visible"),
            "scope" => {
                let index = answer["index"].as_u64().unwrap_or(0) as usize;
                match self.chain.get(index) {
                    Some(scope) => format!("no visible element matches {scope} (the scope)"),
                    None => "the scope is not visible".to_owned(),
                }
            }
            "disabled" => format!("{shown} is disabled"),
            "readonly" => format!("{shown} is read-only"),
            "moving" => format!("{shown} is still moving"),
            "offscreen" => format!("{shown} is outside the viewport"),
            "covered" => format!(
                "{shown} is covered by {}",
                answer["by"].as_str().unwrap_or("another element")
            ),
            "unfocused" => format!("{shown} does not take the focus"),
            "option" => format!(
                "{shown} has no option {:?} (its options: {})",
                answer["option"].as_str().unwrap_or_default(),
                answer["options"]
                    .as_array()
                    .map(|o| {
                        o.iter()
                            .filter_map(Value::as_str)
                            .map(|s| format!("{s:?}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default()
            ),
            "option-disabled" => format!(
                "the option {:?} of {shown} is disabled",
                answer["option"].as_str().unwrap_or_default()
            ),
            "apart" => "the two elements do not fit in the viewport together".to_owned(),
            other => other.to_owned(),
        };
        Ok(Probe::Wait(reason))
    }

    /// Waits until the page function `func` answers without `{ wait }`.
    async fn ready(&self, func: &str, args: Value, target: &Target) -> Result<Value, Fail> {
        self.wait(self.timeout, || {
            let args = args.clone();
            async move { self.probe(func, args, Some(target)).await }
        })
        .await
    }

    /// Runs a step: on failure, saves the failure files and describes it.
    async fn step<T>(
        &self,
        name: String,
        action: bool,
        work: impl Future<Output = Result<T, Fail>>,
    ) -> Result<T, BrowserError> {
        let started = Instant::now();
        match work.await {
            Ok(value) => {
                let slow = self.core.browser.settings.slow_mo;
                if action && !slow.is_zero() {
                    tokio::time::sleep(slow).await;
                }
                Ok(value)
            }
            Err(Fail::Timeout(reason)) => {
                Err(self.failure(name, reason, Some(started.elapsed())).await)
            }
            Err(Fail::Now(reason)) => Err(self.failure(name, reason, None).await),
        }
    }

    async fn failure(&self, step: String, reason: String, waited: Option<Duration>) -> BrowserError {
        let url = Some(self.core.log().url.clone()).filter(|u| !u.is_empty());
        let files = self.capture_failure().await;
        BrowserError::Step(Box::new(StepFailure {
            step,
            reason,
            url,
            waited,
            files: Some(files),
        }))
    }

    /// The screenshot, HTML and console log of a failure.
    async fn capture_failure(&self) -> FailureFiles {
        let artifacts = &self.core.browser.artifacts;
        let number = artifacts.next_number();
        let alive = self.gone().is_none();
        let mut files = FailureFiles::default();
        if alive
            && let Ok(Ok(png)) = tokio::time::timeout(CAPTURE_TIMEOUT, self.png(false)).await
        {
            files.screenshot = artifacts.write(&format!("{number}-failure.png"), &png).ok();
        }
        if alive
            && let Ok(Ok(html)) =
                tokio::time::timeout(CAPTURE_TIMEOUT, self.lib("html", json!({}))).await
            && let Some(html) = html.as_str()
        {
            files.html = artifacts
                .write(&format!("{number}-failure.html"), html.as_bytes())
                .ok();
        }
        let mut console = self.core.log().lines.join("\n");
        console.push('\n');
        files.console = artifacts
            .write(&format!("{number}-console.log"), console.as_bytes())
            .ok();
        files
    }

    async fn png(&self, full_page: bool) -> Result<Vec<u8>, Fail> {
        let mut params = json!({ "format": "png" });
        if full_page {
            let metrics = self.cdp("Page.getLayoutMetrics", json!({})).await?;
            let size = &metrics["cssContentSize"];
            params["captureBeyondViewport"] = json!(true);
            params["clip"] = json!({
                "x": 0,
                "y": 0,
                "width": size["width"].as_f64().unwrap_or(1.0).max(1.0),
                "height": size["height"].as_f64().unwrap_or(1.0).max(1.0),
                "scale": 1,
            });
        }
        let shot = self.cdp("Page.captureScreenshot", params).await?;
        let data = shot["data"].as_str().unwrap_or_default();
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| Fail::Now(format!("the screenshot could not be decoded: {e}")))
    }

    // -- mouse and keyboard -------------------------------------------------

    async fn mouse(&self, kind: &str, x: f64, y: f64, buttons: u32) -> Result<(), Fail> {
        let button = if kind == "mouseMoved" && buttons == 0 {
            "none"
        } else {
            "left"
        };
        let mut params = json!({ "type": kind, "x": x, "y": y, "button": button, "buttons": buttons });
        if kind != "mouseMoved" {
            params["clickCount"] = json!(1);
        }
        self.cdp("Input.dispatchMouseEvent", params).await.map(drop)
    }

    async fn click_at(&self, x: f64, y: f64) -> Result<(), Fail> {
        self.mouse("mouseMoved", x, y, 0).await?;
        self.mouse("mousePressed", x, y, 1).await?;
        self.mouse("mouseReleased", x, y, 0).await
    }

    async fn key_events(&self, events: Vec<Value>) -> Result<(), Fail> {
        for event in events {
            self.cdp("Input.dispatchKeyEvent", event).await?;
        }
        Ok(())
    }

    async fn type_text(&self, text: &str) -> Result<(), Fail> {
        for c in text.chars() {
            match keys::character(c) {
                Some(key) => self.key_events(keys::press(&key, 0)).await?,
                None => {
                    self.cdp("Input.insertText", json!({ "text": c.to_string() }))
                        .await?;
                }
            }
        }
        Ok(())
    }

    fn point_of(answer: &Value) -> (f64, f64) {
        (
            answer["x"].as_f64().unwrap_or_default(),
            answer["y"].as_f64().unwrap_or_default(),
        )
    }

    // -- actions -------------------------------------------------------------

    async fn do_visit(&self, url: &str) -> Result<(), Fail> {
        let timeout = self.nav_timeout();
        let deadline = Instant::now() + timeout;
        let navigate = self.cdp("Page.navigate", json!({ "url": url }));
        let answer = match tokio::time::timeout(timeout, navigate).await {
            Ok(answer) => answer?,
            Err(_) => return Err(Fail::Timeout(format!("{url} did not answer"))),
        };
        if let Some(error) = answer["errorText"].as_str().filter(|e| !e.is_empty()) {
            return Err(Fail::Now(format!("{url} could not be loaded ({error})")));
        }
        let Some(loader) = answer["loaderId"].as_str() else {
            return Ok(()); // a same-document navigation
        };
        let left = deadline.saturating_duration_since(Instant::now());
        self.wait(left, || async move {
            let loaded = self.core.log().loaded.iter().any(|l| l == loader);
            Ok(if loaded {
                Probe::Ready(())
            } else {
                Probe::Wait(format!("{url} did not finish loading"))
            })
        })
        .await
    }

    async fn do_back(&self) -> Result<(), Fail> {
        let history = self.cdp("Page.getNavigationHistory", json!({})).await?;
        let index = history["currentIndex"].as_i64().unwrap_or(0);
        if index <= 0 {
            return Err(Fail::Now(
                "there is no earlier page in this tab's history".to_owned(),
            ));
        }
        let entry = &history["entries"][(index - 1) as usize];
        let url = entry["url"].as_str().unwrap_or_default().to_owned();
        self.cdp(
            "Page.navigateToHistoryEntry",
            json!({ "entryId": entry["id"] }),
        )
        .await?;
        let url = &url;
        self.wait(self.nav_timeout(), || async move {
            let history = self.cdp("Page.getNavigationHistory", json!({})).await?;
            let location = self.lib("location", json!({})).await?;
            let done = history["currentIndex"].as_i64() == Some(index - 1)
                && location["ready"] == "complete"
                && location["href"].as_str() == Some(url.as_str());
            Ok(if done {
                Probe::Ready(())
            } else {
                Probe::Wait(format!("{url} did not finish loading"))
            })
        })
        .await
    }

    async fn do_refresh(&self) -> Result<(), Fail> {
        let before = self.core.log().loader.clone();
        self.cdp("Page.reload", json!({})).await?;
        let before = &before;
        self.wait(self.nav_timeout(), || async move {
            let log = self.core.log();
            let done = log.loader != *before
                && log
                    .loader
                    .as_ref()
                    .is_some_and(|loader| log.loaded.contains(loader));
            Ok(if done {
                Probe::Ready(())
            } else {
                Probe::Wait("the page did not finish reloading".to_owned())
            })
        })
        .await
    }

    async fn do_resize(&self, width: u32, height: u32) -> Result<(), Fail> {
        if width == 0 || height == 0 {
            return Err(Fail::Now("the size must be above zero".to_owned()));
        }
        let viewport = {
            let mut current = self.core.viewport.lock().unwrap_or_else(|e| e.into_inner());
            *current = current.clone().with_size(width, height);
            current.clone()
        };
        self.core
            .apply_viewport(&viewport)
            .await
            .map_err(|e| self.refused("Emulation.setDeviceMetricsOverride", e))
    }

    async fn do_click(&self, target: &Target, mode: &str) -> Result<(), Fail> {
        let args = self.args(Some(target), json!({ "mode": mode, "enabled": true }));
        let point = self.ready("point", args, target).await?;
        let (x, y) = Self::point_of(&point);
        self.click_at(x, y).await
    }

    async fn do_type(&self, target: &Target, text: &str, clear: bool) -> Result<(), Fail> {
        let focus = self
            .ready("focus", self.args(Some(target), json!({ "clear": clear })), target)
            .await?;
        if focus["set"] == true {
            let set = self
                .lib("setValue", self.args(Some(target), json!({ "value": text })))
                .await
                .map_err(once)?;
            return match self.judge(set, Some(target)).map_err(once)? {
                Probe::Ready(_) => Ok(()),
                Probe::Wait(reason) => Err(Fail::Now(reason)),
            };
        }
        if clear && focus["hasText"] == true {
            let backspace = keys::key("Backspace").expect("Backspace is a key");
            self.key_events(keys::press(&backspace, 0)).await?;
            self.lib("emptied", self.args(Some(target), json!({})))
                .await
                .map_err(once)?;
        }
        self.type_text(text).await
    }

    async fn do_select(&self, target: &Target, option: &str) -> Result<(), Fail> {
        let answer = self
            .ready("select", self.args(Some(target), json!({ "option": option })), target)
            .await?;
        if answer["custom"] != true {
            return Ok(());
        }
        // A custom select (a combobox button): open it, then pick the option.
        let (x, y) = Self::point_of(&answer);
        self.click_at(x, y).await?;
        let page = Scope {
            chain: Vec::new(),
            ..self.clone()
        };
        let choice = Target::new(option);
        let args = page.args(Some(&choice), json!({ "mode": "option", "enabled": true }));
        let point = page.ready("point", args, &choice).await?;
        let (x, y) = Self::point_of(&point);
        page.click_at(x, y).await
    }

    async fn do_check(&self, target: &Target, checked: bool) -> Result<(), Fail> {
        let args = self.args(Some(target), json!({ "checked": checked }));
        let answer = self.ready("checkbox", args, target).await?;
        if answer["done"] == true {
            return Ok(());
        }
        let (x, y) = Self::point_of(&answer);
        self.click_at(x, y).await?;
        let args = self.args(Some(target), json!({}));
        let still = if checked { "unchecked" } else { "checked" };
        self.wait(self.timeout, || {
            let args = args.clone();
            async move {
                let state = self.lib("checked", args).await?;
                Ok(if state["checked"] == checked {
                    Probe::Ready(())
                } else {
                    Probe::Wait(format!("{target} is still {still} after the click"))
                })
            }
        })
        .await
    }

    async fn do_attach(&self, target: &Target, file: &Path) -> Result<(), Fail> {
        let file = absolute(file);
        if !file.is_file() {
            return Err(Fail::Now(format!("{} is not a file", file.display())));
        }
        let args = self.args(Some(target), json!({}));
        self.ready("fileState", args.clone(), target).await?;
        let element = self
            .evaluate(format!("({LIB})().fileInput({args})"), false)
            .await
            .map_err(once)?;
        let Some(object) = element["objectId"].as_str() else {
            return Err(Fail::Now(format!("{target} is not a file input")));
        };
        let result = self
            .cdp(
                "DOM.setFileInputFiles",
                json!({ "files": [file], "objectId": object }),
            )
            .await;
        let _ = self
            .cdp("Runtime.releaseObject", json!({ "objectId": object }))
            .await;
        result.map(drop)
    }

    async fn do_keys(&self, combo: &str) -> Result<(), Fail> {
        let events = keys::combination(combo).map_err(Fail::Now)?;
        self.key_events(events).await
    }

    async fn do_drag(&self, from: &Target, to: &Target) -> Result<(), Fail> {
        let args = self.args(None, json!({ "from": from.to_json(), "to": to.to_json() }));
        let answer = self
            .wait(self.timeout, || {
                let args = args.clone();
                async move {
                    let answer = self.lib("drag", args).await?;
                    let which = if answer["which"] == "to" { to } else { from };
                    self.judge(answer, Some(which))
                }
            })
            .await?;
        let start = (
            answer["from"][0].as_f64().unwrap_or_default(),
            answer["from"][1].as_f64().unwrap_or_default(),
        );
        let end = (
            answer["to"][0].as_f64().unwrap_or_default(),
            answer["to"][1].as_f64().unwrap_or_default(),
        );
        self.core.log().drag = None;
        self.cdp("Input.setInterceptDrags", json!({ "enabled": true }))
            .await?;
        let result = self.drag_moves(start, end).await;
        let _ = self
            .cdp("Input.setInterceptDrags", json!({ "enabled": false }))
            .await;
        result
    }

    /// Presses at `start`, moves to `end` in steps (pointer-based drag and
    /// drop needs the intermediate moves), releases. A native HTML drag
    /// the browser intercepted is finished with drag events.
    async fn drag_moves(&self, start: (f64, f64), end: (f64, f64)) -> Result<(), Fail> {
        let (ax, ay) = start;
        let (bx, by) = end;
        self.mouse("mouseMoved", ax, ay, 0).await?;
        self.mouse("mousePressed", ax, ay, 1).await?;
        let nudge = if bx >= ax { 6.0 } else { -6.0 };
        self.mouse("mouseMoved", ax + nudge, ay, 1).await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        const STEPS: u32 = 12;
        for step in 1..=STEPS {
            let t = f64::from(step) / f64::from(STEPS);
            let (x, y) = (ax + (bx - ax) * t, ay + (by - ay) * t);
            let intercepted = self.core.log().drag.take();
            if let Some(data) = intercepted {
                return self.native_drop(data, (x, y), end).await;
            }
            self.mouse("mouseMoved", x, y, 1).await?;
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
        self.mouse("mouseMoved", bx, by, 1).await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let intercepted = self.core.log().drag.take();
        if let Some(data) = intercepted {
            return self.native_drop(data, end, end).await;
        }
        self.mouse("mouseReleased", bx, by, 0).await
    }

    async fn native_drop(&self, data: Value, at: (f64, f64), end: (f64, f64)) -> Result<(), Fail> {
        for (kind, (x, y)) in [("dragEnter", at), ("dragOver", end), ("drop", end)] {
            self.cdp(
                "Input.dispatchDragEvent",
                json!({ "type": kind, "x": x, "y": y, "data": data, "modifiers": 0 }),
            )
            .await?;
        }
        self.mouse("mouseReleased", end.0, end.1, 0).await
    }

    // -- assertions and waits -------------------------------------------------

    async fn do_see(&self, text: &str, want: bool) -> Result<(), Fail> {
        let args = self.args(None, json!({ "text": text }));
        self.wait(self.timeout, || {
            let args = args.clone();
            async move {
                let answer = self.probe("see", args, None).await?;
                Ok(match answer {
                    Probe::Ready(answer) if answer["seen"] == want => Probe::Ready(()),
                    Probe::Ready(_) if want => Probe::Wait("the text is not visible".to_owned()),
                    Probe::Ready(_) => Probe::Wait("the text is visible".to_owned()),
                    Probe::Wait(reason) => Probe::Wait(reason),
                })
            }
        })
        .await
    }

    async fn do_see_in(&self, target: &Target, text: &str) -> Result<(), Fail> {
        let args = self.args(Some(target), json!({ "text": text }));
        self.wait(self.timeout, || {
            let args = args.clone();
            async move {
                Ok(match self.probe("seeIn", args, Some(target)).await? {
                    Probe::Ready(answer) if answer["seen"] == true => Probe::Ready(()),
                    Probe::Ready(_) => Probe::Wait(format!("{target} does not show the text")),
                    Probe::Wait(reason) => Probe::Wait(reason),
                })
            }
        })
        .await
    }

    /// Waits until `check` accepts the page's location.
    async fn location_until(
        &self,
        check: impl Fn(&Value) -> Result<(), String>,
    ) -> Result<(), Fail> {
        let check = &check;
        self.wait(self.timeout, || async move {
            let location = self.lib("location", json!({})).await?;
            Ok(match check(&location) {
                Ok(()) => Probe::Ready(()),
                Err(reason) => Probe::Wait(reason),
            })
        })
        .await
    }

    /// Waits until the number of visible matches satisfies `check`.
    async fn count_until(
        &self,
        target: &Target,
        check: impl Fn(u64) -> bool,
        reason: impl Fn(u64, u64) -> String,
    ) -> Result<u64, Fail> {
        let args = self.args(Some(target), json!({}));
        let (check, reason) = (&check, &reason);
        self.wait(self.timeout, || {
            let args = args.clone();
            async move {
                Ok(match self.probe("count", args, Some(target)).await? {
                    Probe::Ready(answer) => {
                        let count = answer["count"].as_u64().unwrap_or(0);
                        let total = answer["total"].as_u64().unwrap_or(0);
                        if check(count) {
                            Probe::Ready(count)
                        } else {
                            Probe::Wait(reason(count, total))
                        }
                    }
                    Probe::Wait(why) => Probe::Wait(why),
                })
            }
        })
        .await
    }

    async fn visible(&self, target: &Target) -> Result<u64, Fail> {
        self.count_until(
            target,
            |count| count > 0,
            |_, total| {
                if total > 0 {
                    format!("{target} is not visible")
                } else {
                    format!("no element matches {target}")
                }
            },
        )
        .await
    }

    async fn missing(&self, target: &Target) -> Result<(), Fail> {
        self.count_until(
            target,
            |count| count == 0,
            |count, _| format!("{target} is still visible ({count} shown)"),
        )
        .await
        .map(drop)
    }

    async fn do_wait_for_request(&self, glob: &str) -> Result<PageRequest, Fail> {
        self.wait(self.timeout, || async move {
            let mut log = self.core.log();
            let found = log
                .requests
                .iter_mut()
                .find(|r| !r.seen && url_matches(glob, &r.url));
            Ok(match found {
                Some(request) if request.done => {
                    request.seen = true;
                    Probe::Ready(PageRequest {
                        method: request.method.clone(),
                        url: request.url.clone(),
                        status: request.status,
                        failure: request.failure.clone(),
                    })
                }
                Some(request) => Probe::Wait(format!(
                    "the request {} {} has not finished",
                    request.method, request.url
                )),
                None => Probe::Wait(format!("the page made no request matching {glob:?}")),
            })
        })
        .await
    }

    // -- reading ---------------------------------------------------------------

    async fn do_script(&self, script: &str) -> Result<Value, Fail> {
        let result = self
            .evaluate(format!("(async () => {{\n{script}\n}})()"), true)
            .await
            .map_err(once)?;
        Ok(result.get("value").cloned().unwrap_or(Value::Null))
    }

    async fn location(&self) -> Result<Value, Fail> {
        self.wait(self.timeout, || async move {
            Ok(Probe::Ready(self.lib("location", json!({})).await?))
        })
        .await
    }

    async fn do_screenshot(&self, name: &str, full_page: bool) -> Result<PathBuf, Fail> {
        let png = self.png(full_page).await?;
        let artifacts = &self.core.browser.artifacts;
        let file = format!("{}-{}.png", artifacts.next_number(), artifacts::file_stem(name));
        artifacts
            .write(&file, &png)
            .map_err(|e| Fail::Now(e.to_string()))
    }

    fn element(&self, target: Target) -> Element {
        let mut chain = self.chain.clone();
        chain.push(target);
        Element {
            scope: Scope {
                chain,
                ..self.clone()
            },
        }
    }
}

macro_rules! scope_api {
    ($ty:ident) => {
        impl $ty {
            /// The same page or element with another timeout for its steps:
            /// `page.with_timeout(Duration::from_secs(20)).assert_see("Export ready")`.
            pub fn with_timeout(&self, timeout: Duration) -> $ty {
                let mut copy = self.clone();
                copy.scope.timeout = timeout;
                copy
            }

            /// How long each step waits.
            pub fn timeout(&self) -> Duration {
                self.scope.timeout
            }

            // -- navigation ---------------------------------------------------

            /// Loads `url` in this tab and waits for its `load` event (up to
            /// 30 s, or the timeout if longer).
            pub async fn visit(&self, url: impl AsRef<str>) -> Result<(), BrowserError> {
                let url = url.as_ref();
                let s = &self.scope;
                s.step(format!("visit({})", quoted(url)), true, s.do_visit(url)).await
            }

            /// Goes back one page in the tab's history and waits for it.
            pub async fn back(&self) -> Result<(), BrowserError> {
                let s = &self.scope;
                s.step("back()".to_owned(), true, s.do_back()).await
            }

            /// Reloads the page and waits for its `load` event.
            pub async fn refresh(&self) -> Result<(), BrowserError> {
                let s = &self.scope;
                s.step("refresh()".to_owned(), true, s.do_refresh()).await
            }

            /// Resizes the viewport to `width` by `height` CSS pixels, keeping
            /// its pixel ratio, touch support and user agent.
            pub async fn resize(&self, width: u32, height: u32) -> Result<(), BrowserError> {
                let s = &self.scope;
                s.step(format!("resize({width}, {height})"), true, s.do_resize(width, height))
                    .await
            }

            // -- actions ------------------------------------------------------

            /// Clears the field `target`, then types `text` one key at a time.
            pub async fn type_into(
                &self,
                target: impl Into<Target>,
                text: impl AsRef<str>,
            ) -> Result<(), BrowserError> {
                let (target, text) = (target.into(), text.as_ref());
                let s = &self.scope;
                let name = format!("type_into({target}, {})", quoted(text));
                s.step(name, true, s.do_type(&target, text, true)).await
            }

            /// Types `text` at the end of the field `target`, without clearing it.
            pub async fn append(
                &self,
                target: impl Into<Target>,
                text: impl AsRef<str>,
            ) -> Result<(), BrowserError> {
                let (target, text) = (target.into(), text.as_ref());
                let s = &self.scope;
                let name = format!("append({target}, {})", quoted(text));
                s.step(name, true, s.do_type(&target, text, false)).await
            }

            /// Selects the option `option` (its text or value) of the select
            /// `target`; a custom select (a combobox) is opened and the option
            /// with that name clicked.
            pub async fn select(
                &self,
                target: impl Into<Target>,
                option: impl AsRef<str>,
            ) -> Result<(), BrowserError> {
                let (target, option) = (target.into(), option.as_ref());
                let s = &self.scope;
                let name = format!("select({target}, {})", quoted(option));
                s.step(name, true, s.do_select(&target, option)).await
            }

            /// Checks the checkbox, switch or radio button `target` (clicking it
            /// or its label) unless it is checked.
            pub async fn check(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("check({target})"), true, s.do_check(&target, true)).await
            }

            /// Unchecks the checkbox or switch `target` unless it is unchecked.
            pub async fn uncheck(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("uncheck({target})"), true, s.do_check(&target, false))
                    .await
            }

            /// Puts the file at `path` (relative to the current directory) in
            /// the file input `target`, visible or not.
            pub async fn attach(
                &self,
                target: impl Into<Target>,
                path: impl AsRef<Path>,
            ) -> Result<(), BrowserError> {
                let (target, path) = (target.into(), path.as_ref());
                let s = &self.scope;
                let name = format!("attach({target}, {:?})", path.display().to_string());
                s.step(name, true, s.do_attach(&target, path)).await
            }

            /// Clicks the button or link `target` (by text or accessible name,
            /// or any target) with real mouse events at its center.
            pub async fn press(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("press({target})"), true, s.do_click(&target, "press")).await
            }

            /// Clicks any element `target` with real mouse events at its center.
            pub async fn click(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("click({target})"), true, s.do_click(&target, "any")).await
            }

            /// Presses a key or combination on the focused element: `"Enter"`,
            /// `"Escape"`, `"Tab"`, `"ArrowDown"`, `"Control+K"`, `"Shift+Tab"`.
            pub async fn keys(&self, keys: impl AsRef<str>) -> Result<(), BrowserError> {
                let keys = keys.as_ref();
                let s = &self.scope;
                s.step(format!("keys({})", quoted(keys)), true, s.do_keys(keys)).await
            }

            /// Drags `from` onto `to` with the mouse, in steps (pointer-based
            /// drag and drop and native HTML drag and drop both work).
            pub async fn drag(
                &self,
                from: impl Into<Target>,
                to: impl Into<Target>,
            ) -> Result<(), BrowserError> {
                let (from, to) = (from.into(), to.into());
                let s = &self.scope;
                s.step(format!("drag({from}, {to})"), true, s.do_drag(&from, &to)).await
            }

            /// Scrolls `target` to the middle of the viewport.
            pub async fn scroll_to(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                let args = s.args(Some(&target), json!({}));
                s.step(
                    format!("scroll_to({target})"),
                    true,
                    async { s.ready("scrollTo", args, &target).await.map(drop) },
                )
                .await
            }

            // -- assertions ---------------------------------------------------

            /// Waits until `text` is part of the visible text (of the page, or
            /// of this element). Text in hidden elements does not count.
            pub async fn assert_see(&self, text: impl AsRef<str>) -> Result<(), BrowserError> {
                let text = text.as_ref();
                let s = &self.scope;
                s.step(format!("assert_see({})", quoted(text)), false, s.do_see(text, true))
                    .await
            }

            /// Waits until `text` is not part of the visible text.
            pub async fn assert_dont_see(&self, text: impl AsRef<str>) -> Result<(), BrowserError> {
                let text = text.as_ref();
                let s = &self.scope;
                let name = format!("assert_dont_see({})", quoted(text));
                s.step(name, false, s.do_see(text, false)).await
            }

            /// Waits until a visible match of `target` shows `text`.
            pub async fn assert_see_in(
                &self,
                target: impl Into<Target>,
                text: impl AsRef<str>,
            ) -> Result<(), BrowserError> {
                let (target, text) = (target.into(), text.as_ref());
                let s = &self.scope;
                let name = format!("assert_see_in({target}, {})", quoted(text));
                s.step(name, false, s.do_see_in(&target, text)).await
            }

            /// Waits until the URL's path is `path` (`"/deals"`).
            pub async fn assert_path_is(&self, path: impl AsRef<str>) -> Result<(), BrowserError> {
                let path = path.as_ref();
                let s = &self.scope;
                let check = |location: &Value| {
                    let actual = location["path"].as_str().unwrap_or_default();
                    if actual == path {
                        Ok(())
                    } else {
                        Err(format!("the path is {actual:?}"))
                    }
                };
                s.step(format!("assert_path_is({})", quoted(path)), false, s.location_until(check))
                    .await
            }

            /// Waits until the query string has `key` with `value` (one of its
            /// values, decoded).
            pub async fn assert_query_has(
                &self,
                key: impl AsRef<str>,
                value: impl AsRef<str>,
            ) -> Result<(), BrowserError> {
                let (key, value) = (key.as_ref(), value.as_ref());
                let s = &self.scope;
                let check = |location: &Value| {
                    let found = location["query"].as_array().is_some_and(|pairs| {
                        pairs.iter().any(|pair| pair[0] == key && pair[1] == value)
                    });
                    if found {
                        Ok(())
                    } else {
                        Err(format!(
                            "the query string is {:?}",
                            location["search"].as_str().unwrap_or_default()
                        ))
                    }
                };
                let name = format!("assert_query_has({}, {})", quoted(key), quoted(value));
                s.step(name, false, s.location_until(check)).await
            }

            /// Waits until the document's title is `title`.
            pub async fn assert_title(&self, title: impl AsRef<str>) -> Result<(), BrowserError> {
                let title = title.as_ref();
                let s = &self.scope;
                let check = |location: &Value| {
                    let actual = location["title"].as_str().unwrap_or_default().trim();
                    if actual == title.trim() {
                        Ok(())
                    } else {
                        Err(format!("the title is {actual:?}"))
                    }
                };
                s.step(format!("assert_title({})", quoted(title)), false, s.location_until(check))
                    .await
            }

            /// Waits until the field `target` has the value `value` (for a
            /// select: its value or the selected option's text).
            pub async fn assert_value(
                &self,
                target: impl Into<Target>,
                value: impl AsRef<str>,
            ) -> Result<(), BrowserError> {
                let (target, value) = (target.into(), value.as_ref());
                let s = &self.scope;
                let args = s.args(Some(&target), json!({}));
                let name = format!("assert_value({target}, {})", quoted(value));
                let target = &target;
                s.step(name, false, async {
                    s.wait(s.timeout, || {
                        let args = args.clone();
                        async move {
                            Ok(match s.probe("value", args, Some(target)).await? {
                                Probe::Ready(answer)
                                    if answer["value"] == value || answer["shown"] == value =>
                                {
                                    Probe::Ready(())
                                }
                                Probe::Ready(answer) => Probe::Wait(format!(
                                    "its value is {:?}",
                                    answer["value"].as_str().unwrap_or_default()
                                )),
                                Probe::Wait(reason) => Probe::Wait(reason),
                            })
                        }
                    })
                    .await
                })
                .await
            }

            /// Waits until `target` is visible.
            pub async fn assert_visible(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("assert_visible({target})"), false, async {
                    s.visible(&target).await.map(drop)
                })
                .await
            }

            /// Waits until no visible element matches `target`.
            pub async fn assert_missing(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("assert_missing({target})"), false, s.missing(&target))
                    .await
            }

            /// Waits until exactly `count` visible elements match `target`.
            pub async fn assert_count(
                &self,
                target: impl Into<Target>,
                count: usize,
            ) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                let want = count as u64;
                s.step(format!("assert_count({target}, {count})"), false, async {
                    s.count_until(
                        &target,
                        |n| n == want,
                        |n, _| format!("{n} visible elements match"),
                    )
                    .await
                    .map(drop)
                })
                .await
            }

            /// Waits until `target` is visible and enabled.
            pub async fn assert_enabled(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                let args = s.args(Some(&target), json!({}));
                let name = format!("assert_enabled({target})");
                let target = &target;
                s.step(name, false, async {
                    s.wait(s.timeout, || {
                        let args = args.clone();
                        async move {
                            Ok(match s.probe("enabled", args, Some(target)).await? {
                                Probe::Ready(answer) if answer["enabled"] == true => Probe::Ready(()),
                                Probe::Ready(_) => Probe::Wait(format!("{target} is disabled")),
                                Probe::Wait(reason) => Probe::Wait(reason),
                            })
                        }
                    })
                    .await
                })
                .await
            }

            /// Fails if the page logged a `console.error` or threw an uncaught
            /// exception since it opened (failed requests are only logged).
            pub async fn assert_no_console_errors(&self) -> Result<(), BrowserError> {
                let s = &self.scope;
                // A round trip to the page first: console events sent before
                // its answer are in the log when it arrives.
                let _ = tokio::time::timeout(PROBE_MIN, s.lib("location", json!({}))).await;
                let errors = s.core.log().errors.clone();
                s.step("assert_no_console_errors()".to_owned(), false, async {
                    if errors.is_empty() {
                        Ok(())
                    } else {
                        Err(Fail::Now(format!(
                            "{} console error{}: {}",
                            errors.len(),
                            if errors.len() == 1 { "" } else { "s" },
                            errors.join("; ")
                        )))
                    }
                })
                .await
            }

            // -- waits --------------------------------------------------------

            /// Waits until `target` is visible.
            pub async fn wait_for(&self, target: impl Into<Target>) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("wait_for({target})"), false, async {
                    s.visible(&target).await.map(drop)
                })
                .await
            }

            /// Waits until no visible element matches `target`.
            pub async fn wait_until_missing(
                &self,
                target: impl Into<Target>,
            ) -> Result<(), BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("wait_until_missing({target})"), false, s.missing(&target))
                    .await
            }

            /// Waits until the URL matches `glob` (`*` matches any characters):
            /// its path (`"/deals/*"`), path and query, or the whole URL.
            pub async fn wait_for_url(&self, glob: impl AsRef<str>) -> Result<(), BrowserError> {
                let glob = glob.as_ref();
                let s = &self.scope;
                let check = |location: &Value| {
                    let href = location["href"].as_str().unwrap_or_default();
                    if url_matches(glob, href) {
                        Ok(())
                    } else {
                        Err(format!("the URL is {href}"))
                    }
                };
                s.step(format!("wait_for_url({})", quoted(glob)), false, s.location_until(check))
                    .await
            }

            /// Waits until the page has made a request whose URL matches `glob`
            /// (its path, path and query, or whole URL) and its response has
            /// arrived. Requests made by earlier steps count; each request
            /// satisfies one wait.
            pub async fn wait_for_request(
                &self,
                glob: impl AsRef<str>,
            ) -> Result<PageRequest, BrowserError> {
                let glob = glob.as_ref();
                let s = &self.scope;
                let name = format!("wait_for_request({})", quoted(glob));
                s.step(name, false, s.do_wait_for_request(glob)).await
            }

            // -- reading ------------------------------------------------------

            /// The visible text of the first visible match of `target`, trimmed.
            pub async fn text(&self, target: impl Into<Target>) -> Result<String, BrowserError> {
                let target = target.into();
                let s = &self.scope;
                let args = s.args(Some(&target), json!({}));
                s.step(format!("text({target})"), false, async {
                    let answer = s.ready("text", args, &target).await?;
                    Ok(answer["text"].as_str().unwrap_or_default().to_owned())
                })
                .await
            }

            /// The value of the field `target`.
            pub async fn value(&self, target: impl Into<Target>) -> Result<String, BrowserError> {
                let target = target.into();
                let s = &self.scope;
                let args = s.args(Some(&target), json!({}));
                s.step(format!("value({target})"), false, async {
                    let answer = s.ready("value", args, &target).await?;
                    Ok(answer["value"].as_str().unwrap_or_default().to_owned())
                })
                .await
            }

            /// Every visible match of `target` now (without waiting for one), as
            /// elements found again by position each time they are used.
            pub async fn all(&self, target: impl Into<Target>) -> Result<Vec<Element>, BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("all({target})"), false, async {
                    let count = s
                        .count_until(&target, |_| true, |_, _| String::new())
                        .await?;
                    Ok(if target.has_nth() {
                        (0..count.min(1)).map(|_| s.element(target.clone())).collect()
                    } else {
                        (0..count as usize)
                            .map(|i| s.element(target.clone().nth(i)))
                            .collect()
                    })
                })
                .await
            }

            /// Waits for `target` and returns it as an [`Element`] whose
            /// methods look inside it.
            pub async fn within(&self, target: impl Into<Target>) -> Result<Element, BrowserError> {
                let target = target.into();
                let s = &self.scope;
                s.step(format!("within({target})"), false, async {
                    s.visible(&target).await?;
                    Ok(s.element(target.clone()))
                })
                .await
            }

            /// Runs `script` (a function body: `return document.title`) in the
            /// page, awaits it, and deserializes what it returns.
            pub async fn script<T: DeserializeOwned>(
                &self,
                script: impl AsRef<str>,
            ) -> Result<T, BrowserError> {
                let script = script.as_ref();
                let s = &self.scope;
                s.step(format!("script({})", quoted(script)), false, async {
                    let value = s.do_script(script).await?;
                    serde_json::from_value(value.clone()).map_err(|e| {
                        Fail::Now(format!(
                            "its result {value} is not a {}: {e}",
                            std::any::type_name::<T>()
                        ))
                    })
                })
                .await
            }

            /// The cookie `name` the browser holds for the page's URL.
            pub async fn cookie(&self, name: impl AsRef<str>) -> Result<Option<Cookie>, BrowserError> {
                let name = name.as_ref();
                let s = &self.scope;
                s.step(format!("cookie({})", quoted(name)), false, async {
                    let location = s.location().await?;
                    let href = location["href"].as_str().unwrap_or_default();
                    let answer = s.cdp("Network.getCookies", json!({ "urls": [href] })).await?;
                    let cookies: Vec<Cookie> =
                        serde_json::from_value(answer["cookies"].clone()).unwrap_or_default();
                    Ok(cookies.into_iter().find(|c| c.name == name))
                })
                .await
            }

            /// The page's URL.
            pub async fn url(&self) -> Result<String, BrowserError> {
                let s = &self.scope;
                s.step("url()".to_owned(), false, async {
                    let location = s.location().await?;
                    Ok(location["href"].as_str().unwrap_or_default().to_owned())
                })
                .await
            }

            // -- screenshots --------------------------------------------------

            /// Saves a screenshot of the viewport as
            /// `<artifacts>/<test name>/NN-<name>.png` and returns its path.
            pub async fn screenshot(&self, name: impl AsRef<str>) -> Result<PathBuf, BrowserError> {
                let name = name.as_ref();
                let s = &self.scope;
                s.step(format!("screenshot({})", quoted(name)), false, s.do_screenshot(name, false))
                    .await
            }

            /// Saves a screenshot of the whole page, beyond the viewport.
            pub async fn screenshot_full_page(
                &self,
                name: impl AsRef<str>,
            ) -> Result<PathBuf, BrowserError> {
                let name = name.as_ref();
                let s = &self.scope;
                let step = format!("screenshot_full_page({})", quoted(name));
                s.step(step, false, s.do_screenshot(name, true)).await
            }
        }
    };
}

/// A tab of a [`Browser`](crate::Browser): actions, assertions, waits and
/// reading. Every action and assertion waits for the page, up to the
/// timeout (5 s by default), and saves a screenshot, the HTML and the
/// console log when it fails.
///
/// Pages are cheap to clone; clones drive the same tab. The tab closes
/// when the last clone is dropped.
#[derive(Clone)]
pub struct Page {
    scope: Scope,
}

/// An element of a page, from [`Page::within`] or [`Page::all`]: the same
/// methods as [`Page`], looking for targets inside it. It is found again
/// (by its targets) each time it is used, so it survives re-renders.
#[derive(Clone)]
pub struct Element {
    scope: Scope,
}

scope_api!(Page);
scope_api!(Element);

impl Element {
    /// The element's own visible text, trimmed.
    pub async fn inner_text(&self) -> Result<String, BrowserError> {
        let s = &self.scope;
        let args = s.args(None, json!({}));
        s.step("inner_text()".to_owned(), false, async {
            let answer = s
                .wait(s.timeout, || {
                    let args = args.clone();
                    async move { s.probe("ownText", args, None).await }
                })
                .await?;
            Ok(answer["text"].as_str().unwrap_or_default().to_owned())
        })
        .await
    }
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page")
            .field("url", &self.scope.core.log().url)
            .field("timeout", &self.scope.timeout)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let chain: Vec<String> = self.scope.chain.iter().map(Target::to_string).collect();
        f.debug_struct("Element")
            .field("targets", &chain)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_globs_match_the_path_the_query_or_the_url() {
        assert!(url_matches("/deals/*", "http://127.0.0.1:3100/deals/0199c1d2"));
        assert!(url_matches(
            "/api/v1/deals/*/move",
            "http://127.0.0.1:3100/api/v1/deals/abc/move"
        ));
        assert!(url_matches("/deals?stage=won", "http://x/deals?stage=won"));
        assert!(url_matches("http://x/*", "http://x/deals"));
        assert!(url_matches("/", "http://x"));
        assert!(!url_matches("/deals", "http://x/companies"));
        assert!(url_matches("/deals", "http://x/deals#top"));
    }

    #[test]
    fn step_texts_are_quoted_and_shortened() {
        assert_eq!(quoted("Deal saved"), "\"Deal saved\"");
        let long = "x".repeat(80);
        assert_eq!(quoted(&long), format!("\"{}…\"", "x".repeat(60)));
    }

    #[test]
    fn cookies_read_the_devtools_shape() {
        let cookie: Cookie = serde_json::from_value(json!({
            "name": "navree_at", "value": "t", "domain": "127.0.0.1", "path": "/",
            "expires": -1, "size": 10, "httpOnly": true, "secure": false, "session": true,
            "sameSite": "Lax", "priority": "Medium"
        }))
        .unwrap();
        assert_eq!(cookie.name, "navree_at");
        assert_eq!(cookie.expires, None);
        assert!(cookie.http_only);
        assert_eq!(cookie.same_site.as_deref(), Some("Lax"));
    }
}
