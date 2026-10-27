//! Finding, launching and reaching Chrome.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::options::ChromeOptions;

/// The environment variable naming the Chrome executable (also what
/// `navree browser:install` sets for Chrome for Testing).
pub const CHROME_ENV: &str = "NAVREE_BROWSER_CHROME";

/// The Chrome executable: `configured`, else `env` (the value of
/// [`CHROME_ENV`]), else the first usual install location that exists.
pub(crate) fn find_chrome(configured: Option<&Path>, env: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = configured {
        return Some(path.to_path_buf());
    }
    if let Some(path) = env.filter(|p| !p.as_os_str().is_empty()) {
        return Some(path);
    }
    candidates().into_iter().find(|path| path.is_file())
}

/// Where Chrome, Chromium and Edge usually live.
fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        let roots = [
            std::env::var_os("ProgramFiles"),
            std::env::var_os("ProgramFiles(x86)"),
            std::env::var_os("LOCALAPPDATA"),
        ];
        for root in roots.into_iter().flatten() {
            let root = PathBuf::from(root);
            paths.push(root.join("Google/Chrome/Application/chrome.exe"));
            paths.push(root.join("Chromium/Application/chrome.exe"));
            paths.push(root.join("Microsoft/Edge/Application/msedge.exe"));
        }
    } else if cfg!(target_os = "macos") {
        paths.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
        paths.push("/Applications/Chromium.app/Contents/MacOS/Chromium".into());
        paths.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
    } else {
        for dir in std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default()
        {
            for name in [
                "chrome-headless-shell",
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
            ] {
                paths.push(dir.join(name));
            }
        }
    }
    paths
}

/// Chrome's command-line flags: headless, no first-run or background
/// work, a throwaway profile and a DevTools port picked by the OS.
pub(crate) fn launch_args(options: &ChromeOptions, profile: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "--headless",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-default-apps",
        "--disable-sync",
        "--disable-dev-shm-usage",
        "--mute-audio",
        "--hide-scrollbars",
        "--font-render-hinting=none",
        "--remote-debugging-port=0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.push(format!("--user-data-dir={}", profile.display()));
    if options.no_sandbox {
        args.push("--no-sandbox".into());
    }
    args.extend(options.args.iter().cloned());
    args.push("about:blank".into());
    args
}

/// The browser WebSocket URL in a line Chrome prints at start.
pub(crate) fn devtools_url(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("DevTools listening on ")?;
    rest.starts_with("ws://").then(|| rest.trim().to_owned())
}

/// A Chrome process this engine started, with its profile directory
/// (removed when dropped, after the process is killed).
pub(crate) struct LaunchedChrome {
    pub(crate) child: Child,
    pub(crate) ws_url: String,
    _profile: tempfile::TempDir,
}

/// Starts Chrome and waits (up to `timeout`) for its DevTools URL.
pub(crate) async fn launch(
    path: &Path,
    options: &ChromeOptions,
    timeout: Duration,
) -> Result<LaunchedChrome, String> {
    let profile = tempfile::Builder::new()
        .prefix("navree-chrome-")
        .tempdir()
        .map_err(|error| format!("cannot create a Chrome profile directory: {error}"))?;
    let mut child = Command::new(path)
        .args(launch_args(options, profile.path()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", path.display()))?;
    let stderr = child.stderr.take().ok_or("Chrome has no stderr")?;
    let mut lines = BufReader::new(stderr).lines();
    let found = tokio::time::timeout(timeout, async {
        let mut seen = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = devtools_url(&line) {
                return Ok(url);
            }
            if seen.len() < 20 {
                seen.push(line);
            }
        }
        Err(format!(
            "Chrome exited before it was ready{}",
            if seen.is_empty() {
                String::new()
            } else {
                format!(": {}", seen.join(" | "))
            }
        ))
    })
    .await;
    let ws_url = match found {
        Ok(Ok(url)) => url,
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            return Err(format!(
                "Chrome did not start within {} s",
                timeout.as_secs()
            ));
        }
    };
    // Keep draining stderr so Chrome never blocks on a full pipe.
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::trace!(target: "navree_documents::chrome", "{line}");
        }
    });
    Ok(LaunchedChrome {
        child,
        ws_url,
        _profile: profile,
    })
}

/// The browser WebSocket URL of `endpoint`: a `ws://` URL as it is, or the
/// `webSocketDebuggerUrl` that `http://host:port/json/version` reports.
pub(crate) async fn resolve_endpoint(endpoint: &str, timeout: Duration) -> Result<String, String> {
    if endpoint.starts_with("ws://") || endpoint.starts_with("wss://") {
        return Ok(endpoint.to_owned());
    }
    let address = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| {
            format!("documents.chrome.endpoint must be ws:// or http://, not {endpoint}")
        })?
        .trim_end_matches('/');
    let host = address.split('/').next().unwrap_or(address);
    let body = tokio::time::timeout(timeout, async {
        let mut stream = tokio::net::TcpStream::connect(host)
            .await
            .map_err(|error| format!("cannot reach {host}: {error}"))?;
        let request = format!(
            "GET /json/version HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        let mut response = Vec::new();
        stream
            .take(1024 * 1024)
            .read_to_end(&mut response)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(response)
    })
    .await
    .map_err(|_| format!("{endpoint} did not answer within {} s", timeout.as_secs()))??;
    let text = String::from_utf8_lossy(&body);
    // The body is one JSON object (also inside a chunked body).
    let json = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .and_then(|body| Some(&body[body.find('{')?..=body.rfind('}')?]))
        .ok_or_else(|| format!("{endpoint}/json/version: not a JSON response"))?;
    let version: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("{endpoint}/json/version: {error}"))?;
    let url = version
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{endpoint}/json/version has no webSocketDebuggerUrl"))?;
    // Chrome reports its own idea of the host; reach it where we asked.
    let path = url
        .strip_prefix("ws://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(_, path)| path)
        .unwrap_or_default();
    Ok(format!("ws://{host}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_devtools_line_gives_the_browser_url() {
        assert_eq!(
            devtools_url("DevTools listening on ws://127.0.0.1:53211/devtools/browser/0b1c\r"),
            Some("ws://127.0.0.1:53211/devtools/browser/0b1c".to_owned())
        );
        assert_eq!(devtools_url("[1026/101010.123:ERROR:gpu] something"), None);
        assert_eq!(devtools_url("DevTools listening on nothing"), None);
    }

    #[test]
    fn the_configured_path_wins_then_the_environment() {
        let configured = PathBuf::from("/opt/chrome/chrome");
        assert_eq!(
            find_chrome(Some(&configured), Some("/env/chrome".into())),
            Some(configured)
        );
        assert_eq!(
            find_chrome(None, Some("/env/chrome".into())),
            Some(PathBuf::from("/env/chrome"))
        );
    }

    #[test]
    fn flags_add_the_profile_the_sandbox_switch_and_extra_args() {
        let options = ChromeOptions {
            no_sandbox: true,
            args: vec!["--lang=ar".into()],
            ..ChromeOptions::default()
        };
        let args = launch_args(&options, Path::new("/tmp/profile"));
        assert!(args.contains(&"--headless".to_owned()));
        assert!(args.contains(&"--remote-debugging-port=0".to_owned()));
        assert!(args.contains(&"--user-data-dir=/tmp/profile".to_owned()));
        assert!(args.contains(&"--no-sandbox".to_owned()));
        assert!(args.contains(&"--lang=ar".to_owned()));
        assert_eq!(args.last().map(String::as_str), Some("about:blank"));
        let plain = launch_args(&ChromeOptions::default(), Path::new("p"));
        assert!(!plain.contains(&"--no-sandbox".to_owned()));
    }
}
