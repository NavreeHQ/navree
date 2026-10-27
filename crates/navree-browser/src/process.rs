//! Starting Chrome and sharing it between the tests of a binary.
//!
//! One Chrome process serves every [`Browser`](crate::Browser) of a test
//! binary that is alive at the same time (each in a browser context of its
//! own). The process stops when the last of them is dropped, so no browser
//! outlives the tests.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::BrowserError;
use crate::cdp::Connection;

/// How long Chrome may take to open its DevTools endpoint.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// A running browser process and its DevTools connection.
pub(crate) struct Chrome {
    pub(crate) connection: Connection,
    /// `156.0.8078.48`.
    pub(crate) version: String,
    /// The browser's own user agent.
    pub(crate) user_agent: String,
    child: Mutex<Option<Child>>,
    profile: PathBuf,
}

/// Processes are shared by binary and headless mode.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    headless: bool,
}

static POOL: LazyLock<tokio::sync::Mutex<HashMap<Key, Weak<Chrome>>>> =
    LazyLock::new(Default::default);

/// The running Chrome for `path` in this mode, started if there is none.
pub(crate) async fn shared(
    path: &Path,
    headless: bool,
    window: (u32, u32),
) -> Result<Arc<Chrome>, BrowserError> {
    let key = Key {
        path: path.to_path_buf(),
        headless,
    };
    let mut pool = POOL.lock().await;
    if let Some(chrome) = pool.get(&key).and_then(Weak::upgrade)
        && !chrome.connection.is_closed()
    {
        return Ok(chrome);
    }
    let chrome = Arc::new(Chrome::launch(path, headless, window).await?);
    pool.insert(key, Arc::downgrade(&chrome));
    Ok(chrome)
}

/// The command-line flags Chrome starts with.
pub(crate) fn chrome_args(
    path: &Path,
    headless: bool,
    window: (u32, u32),
    profile: &Path,
) -> Vec<String> {
    let mut args = vec![
        "--remote-debugging-port=0".to_owned(),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
        "--disable-background-networking".to_owned(),
        "--disable-background-timer-throttling".to_owned(),
        "--disable-backgrounding-occluded-windows".to_owned(),
        "--disable-renderer-backgrounding".to_owned(),
        "--disable-breakpad".to_owned(),
        "--disable-client-side-phishing-detection".to_owned(),
        "--disable-component-update".to_owned(),
        "--disable-default-apps".to_owned(),
        "--disable-dev-shm-usage".to_owned(),
        "--disable-extensions".to_owned(),
        "--disable-hang-monitor".to_owned(),
        "--disable-ipc-flooding-protection".to_owned(),
        "--disable-popup-blocking".to_owned(),
        "--disable-prompt-on-repost".to_owned(),
        "--disable-sync".to_owned(),
        "--disable-search-engine-choice-screen".to_owned(),
        "--disable-features=Translate,MediaRouter,OptimizationHints,DialMediaRouteProvider,CalculateNativeWinOcclusion,AutofillServerCommunication".to_owned(),
        "--force-color-profile=srgb".to_owned(),
        "--metrics-recording-only".to_owned(),
        "--mute-audio".to_owned(),
        "--password-store=basic".to_owned(),
        "--use-mock-keychain".to_owned(),
        format!("--window-size={},{}", window.0, window.1),
    ];
    if headless {
        let shell = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("headless"));
        args.push(if shell { "--headless" } else { "--headless=new" }.to_owned());
        args.push("--hide-scrollbars".to_owned());
    }
    // Containers and recent Ubuntu runners cannot start Chrome's sandbox;
    // tests load their own app's pages.
    if cfg!(target_os = "linux") {
        args.push("--no-sandbox".to_owned());
    }
    args.push("about:blank".to_owned());
    args
}

/// The DevTools WebSocket URL from Chrome's `DevToolsActivePort` file
/// (port, then path) or its `DevTools listening on ws://...` line.
pub(crate) fn devtools_url(port_file: Option<&str>, output: &[String]) -> Option<String> {
    if let Some(text) = port_file {
        let mut lines = text.lines();
        if let (Some(port), Some(path)) = (lines.next(), lines.next())
            && port.trim().parse::<u16>().is_ok()
            && path.trim().starts_with('/')
        {
            return Some(format!("ws://127.0.0.1:{}{}", port.trim(), path.trim()));
        }
    }
    output.iter().find_map(|line| {
        line.split_once("DevTools listening on ")
            .map(|(_, url)| url.trim().to_owned())
    })
}

impl Chrome {
    async fn launch(path: &Path, headless: bool, window: (u32, u32)) -> Result<Chrome, BrowserError> {
        let launch_error = |reason: String| BrowserError::Launch {
            path: path.to_path_buf(),
            reason,
        };
        let profile =
            std::env::temp_dir().join(format!("navree-browser-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&profile).map_err(|e| {
            BrowserError::io(format!("cannot create {}", profile.display()), e)
        })?;
        let mut child = Command::new(path)
            .args(chrome_args(path, headless, window, &profile))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                remove_dir(&profile);
                launch_error(format!("cannot run it: {e}"))
            })?;
        let output = drain(child.stderr.take());

        let deadline = Instant::now() + START_TIMEOUT;
        let url = loop {
            let port_file = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok();
            let lines: Vec<String> = lock(&output).iter().cloned().collect();
            if let Some(url) = devtools_url(port_file.as_deref(), &lines) {
                break url;
            }
            if let Ok(Some(status)) = child.try_wait() {
                remove_dir(&profile);
                return Err(launch_error(format!(
                    "it exited ({status}) before opening DevTools{}",
                    tail(&output)
                )));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                remove_dir(&profile);
                return Err(launch_error(format!(
                    "it did not open DevTools within {}s{}",
                    START_TIMEOUT.as_secs(),
                    tail(&output)
                )));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };

        let connected = match Connection::connect(url).await {
            Ok(connection) => connection
                .call(None, "Browser.getVersion", json!({}))
                .await
                .map(|info| (connection, info))
                .map_err(|e| launch_error(format!("Browser.getVersion failed: {e}"))),
            Err(error) => Err(error),
        };
        let (connection, info) = match connected {
            Ok(connected) => connected,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                remove_dir(&profile);
                return Err(error);
            }
        };
        let product = info["product"].as_str().unwrap_or_default();
        Ok(Chrome {
            connection,
            version: product
                .split_once('/')
                .map_or(product, |(_, version)| version)
                .to_owned(),
            user_agent: info["userAgent"].as_str().unwrap_or_default().to_owned(),
            child: Mutex::new(Some(child)),
            profile,
        })
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        self.connection.fire(None, "Browser.close", json!({}));
        if let Some(mut child) = lock(&self.child).take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        remove_dir(&self.profile);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reads a child's output on a thread, keeping the last lines.
pub(crate) fn drain(
    stream: Option<impl std::io::Read + Send + 'static>,
) -> Arc<Mutex<VecDeque<String>>> {
    let lines = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stream) = stream {
        let keep = lines.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let mut keep = lock(&keep);
                if keep.len() >= 40 {
                    keep.pop_front();
                }
                keep.push_back(line);
            }
        });
    }
    lines
}

/// The kept output lines, indented, after a colon (empty when none).
pub(crate) fn tail(output: &Mutex<VecDeque<String>>) -> String {
    let lines = lock(output);
    if lines.is_empty() {
        return String::new();
    }
    let mut text = String::from("; its output ends with:");
    for line in lines.iter() {
        text.push_str("\n    ");
        text.push_str(line);
    }
    text
}

/// Removes a profile folder, retrying while the browser's processes let go
/// of its files.
fn remove_dir(dir: &Path) {
    for _ in 0..20 {
        match std::fs::remove_dir_all(dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_devtools_url_comes_from_the_port_file_or_the_output() {
        assert_eq!(
            devtools_url(Some("52417\n/devtools/browser/abc\n"), &[]),
            Some("ws://127.0.0.1:52417/devtools/browser/abc".to_owned())
        );
        // A file still being written is not used.
        assert_eq!(devtools_url(Some("52417"), &[]), None);
        let output = ["".to_owned(), "DevTools listening on ws://127.0.0.1:9222/devtools/browser/x".to_owned()];
        assert_eq!(
            devtools_url(None, &output),
            Some("ws://127.0.0.1:9222/devtools/browser/x".to_owned())
        );
        assert_eq!(devtools_url(None, &[]), None);
    }

    #[test]
    fn chrome_starts_headless_with_a_private_profile_and_the_window_size() {
        let profile = Path::new("/tmp/profile");
        let args = chrome_args(Path::new("/opt/chrome/chrome"), true, (1440, 900), profile);
        assert!(args.contains(&"--remote-debugging-port=0".to_owned()));
        assert!(args.contains(&format!("--user-data-dir={}", profile.display())));
        assert!(args.contains(&"--headless=new".to_owned()));
        assert!(args.contains(&"--window-size=1440,900".to_owned()));
        assert_eq!(args.last().map(String::as_str), Some("about:blank"));

        let shell = chrome_args(
            Path::new("/pw/chrome-headless-shell"),
            true,
            (800, 600),
            profile,
        );
        assert!(shell.contains(&"--headless".to_owned()));
        let shown = chrome_args(Path::new("/opt/chrome/chrome"), false, (800, 600), profile);
        assert!(!shown.iter().any(|a| a.starts_with("--headless")));
    }

    #[test]
    fn output_tails_are_indented() {
        let output = Mutex::new(VecDeque::from(["a".to_owned(), "b".to_owned()]));
        assert_eq!(tail(&output), "; its output ends with:\n    a\n    b");
        assert_eq!(tail(&Mutex::new(VecDeque::new())), "");
    }
}
