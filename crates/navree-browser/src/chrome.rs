//! Finding a Chrome, Chromium or Edge binary.
//!
//! The order: `NAVREE_BROWSER_CHROME`; Chrome and Chromium installed on the
//! machine; Chrome for Testing in Navree's cache (`navree browser:install`);
//! Playwright's browsers (`ms-playwright`); Microsoft Edge.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::BrowserError;

/// The environment variable naming the browser binary.
pub(crate) const CHROME_ENV: &str = "NAVREE_BROWSER_CHROME";

/// The operating system, for the install locations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    pub(crate) fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// Where to look: the operating system and its environment.
pub(crate) struct Search<'a> {
    pub(crate) os: Os,
    pub(crate) env: &'a dyn Fn(&str) -> Option<OsString>,
    /// Whether the browser runs headless (Playwright's headless shell only
    /// runs headless).
    pub(crate) headless: bool,
}

impl Search<'_> {
    fn var(&self, name: &str) -> Option<PathBuf> {
        (self.env)(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    fn home(&self) -> Option<PathBuf> {
        match self.os {
            Os::Windows => self.var("USERPROFILE").or_else(|| self.var("HOME")),
            _ => self.var("HOME"),
        }
    }

    /// The browser to use, or why there is none.
    pub(crate) fn find(&self) -> Result<PathBuf, BrowserError> {
        if let Some(path) = self.var(CHROME_ENV) {
            return if path.is_file() {
                Ok(path)
            } else {
                Err(BrowserError::ChromeNotFound(format!(
                    "{CHROME_ENV} is {}, which is not a file",
                    path.display()
                )))
            };
        }
        let candidates = self.candidates();
        if let Some(found) = candidates.iter().find(|path| path.is_file()) {
            return Ok(found.clone());
        }
        let mut message = String::from(
            "no Chrome or Chromium found. Set NAVREE_BROWSER_CHROME to a Chrome, Chromium or Edge \
             binary, or run `navree browser:install` to download Chrome for Testing. Looked in:",
        );
        for dir in self.searched() {
            message.push_str("\n  ");
            message.push_str(&dir);
        }
        Err(BrowserError::ChromeNotFound(message))
    }

    /// Every candidate binary, best first (existence not checked).
    pub(crate) fn candidates(&self) -> Vec<PathBuf> {
        let mut out = self.system_chrome();
        if let Some(cache) = cache_dir(self.os, self.env) {
            out.extend(installed(&cache.join("chrome"), self.os));
        }
        if let Some(playwright) = self.playwright_dir() {
            out.extend(playwright_browsers(&playwright, self.os, self.headless));
        }
        out.extend(self.edge());
        out
    }

    /// The places [`find`](Self::find) looked, for the error message.
    fn searched(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.os {
            Os::Windows => {
                for var in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
                    if let Some(dir) = self.var(var) {
                        out.push(format!("{} (Chrome, Chromium, Edge)", dir.display()));
                    }
                }
            }
            Os::Mac => out.push("/Applications and ~/Applications".to_owned()),
            Os::Linux => {
                out.push("PATH (google-chrome, chromium, chromium-browser, ...)".to_owned());
                out.push("/opt/google/chrome, /usr/bin, /snap/bin".to_owned());
            }
        }
        if let Some(cache) = cache_dir(self.os, self.env) {
            out.push(format!(
                "{} (navree browser:install)",
                cache.join("chrome").display()
            ));
        }
        if let Some(playwright) = self.playwright_dir() {
            out.push(format!("{} (Playwright)", playwright.display()));
        }
        out
    }

    fn system_chrome(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        match self.os {
            Os::Windows => {
                for var in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
                    if let Some(dir) = self.var(var) {
                        out.push(dir.join(r"Google\Chrome\Application\chrome.exe"));
                    }
                }
                if let Some(dir) = self.var("LOCALAPPDATA") {
                    out.push(dir.join(r"Chromium\Application\chrome.exe"));
                }
            }
            Os::Mac => {
                let mut roots = vec![PathBuf::from("/Applications")];
                if let Some(home) = self.home() {
                    roots.push(home.join("Applications"));
                }
                for root in &roots {
                    out.push(root.join("Google Chrome.app/Contents/MacOS/Google Chrome"));
                }
                for root in &roots {
                    out.push(root.join("Chromium.app/Contents/MacOS/Chromium"));
                }
            }
            Os::Linux => {
                out.extend(self.on_path(&[
                    "google-chrome",
                    "google-chrome-stable",
                    "chromium",
                    "chromium-browser",
                    "chrome",
                ]));
                for path in [
                    "/opt/google/chrome/chrome",
                    "/usr/bin/google-chrome",
                    "/usr/bin/chromium",
                    "/usr/bin/chromium-browser",
                    "/snap/bin/chromium",
                ] {
                    out.push(PathBuf::from(path));
                }
            }
        }
        out
    }

    fn edge(&self) -> Vec<PathBuf> {
        match self.os {
            Os::Windows => ["PROGRAMFILES(X86)", "PROGRAMFILES"]
                .iter()
                .filter_map(|var| self.var(var))
                .map(|dir| dir.join(r"Microsoft\Edge\Application\msedge.exe"))
                .collect(),
            Os::Mac => vec![PathBuf::from(
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            )],
            Os::Linux => {
                let mut out = self.on_path(&["microsoft-edge", "microsoft-edge-stable"]);
                out.push(PathBuf::from("/opt/microsoft/msedge/msedge"));
                out
            }
        }
    }

    fn on_path(&self, names: &[&str]) -> Vec<PathBuf> {
        let Some(path) = (self.env)("PATH") else {
            return Vec::new();
        };
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        names
            .iter()
            .flat_map(|name| dirs.iter().map(move |dir| dir.join(name)))
            .collect()
    }

    /// `PLAYWRIGHT_BROWSERS_PATH`, else Playwright's default cache.
    fn playwright_dir(&self) -> Option<PathBuf> {
        if let Some(dir) = self.var("PLAYWRIGHT_BROWSERS_PATH")
            && dir.as_os_str() != "0"
        {
            return Some(dir);
        }
        match self.os {
            Os::Windows => self.var("LOCALAPPDATA").map(|d| d.join("ms-playwright")),
            Os::Mac => self.home().map(|h| h.join("Library/Caches/ms-playwright")),
            Os::Linux => self
                .var("XDG_CACHE_HOME")
                .or_else(|| self.home().map(|h| h.join(".cache")))
                .map(|d| d.join("ms-playwright")),
        }
    }
}

/// Navree's cache directory: `NAVREE_CACHE_DIR`, else
/// `%LOCALAPPDATA%\navree` (Windows), `~/Library/Caches/navree` (macOS) or
/// `$XDG_CACHE_HOME/navree`, `~/.cache/navree` (Linux). `navree
/// browser:install` puts Chrome for Testing under its `chrome/<version>/`.
pub(crate) fn cache_dir(os: Os, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("NAVREE_CACHE_DIR") {
        return Some(dir);
    }
    match os {
        Os::Windows => var("LOCALAPPDATA").map(|d| d.join("navree")),
        Os::Mac => var("HOME").map(|h| h.join("Library/Caches/navree")),
        Os::Linux => var("XDG_CACHE_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".cache")))
            .map(|d| d.join("navree")),
    }
}

/// The Chrome for Testing binary inside an unpacked `chrome-<platform>`
/// archive directory, for each platform name of `os`.
fn chrome_for_testing(dir: &Path, os: Os) -> Vec<PathBuf> {
    match os {
        Os::Windows => vec![
            dir.join("chrome-win64").join("chrome.exe"),
            dir.join("chrome-win32").join("chrome.exe"),
            dir.join("chrome-win").join("chrome.exe"),
        ],
        Os::Mac => ["chrome-mac-arm64", "chrome-mac-x64"]
            .iter()
            .map(|platform| {
                dir.join(platform)
                    .join("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")
            })
            .chain(std::iter::once(
                dir.join("chrome-mac/Chromium.app/Contents/MacOS/Chromium"),
            ))
            .collect(),
        Os::Linux => vec![
            dir.join("chrome-linux64").join("chrome"),
            dir.join("chrome-linux").join("chrome"),
        ],
    }
}

/// Playwright's headless shell inside a `chromium_headless_shell-<rev>` dir.
fn headless_shell(dir: &Path, os: Os) -> Vec<PathBuf> {
    match os {
        Os::Windows => vec![
            dir.join("chrome-headless-shell-win64/chrome-headless-shell.exe"),
            dir.join("chrome-win/headless_shell.exe"),
        ],
        Os::Mac => vec![
            dir.join("chrome-headless-shell-mac-arm64/chrome-headless-shell"),
            dir.join("chrome-headless-shell-mac-x64/chrome-headless-shell"),
            dir.join("chrome-mac/headless_shell"),
        ],
        Os::Linux => vec![
            dir.join("chrome-headless-shell-linux64/chrome-headless-shell"),
            dir.join("chrome-linux/headless_shell"),
        ],
    }
}

/// The browsers installed under `root/<version>/`, newest version first.
fn installed(root: &Path, os: Os) -> Vec<PathBuf> {
    versions(root, "")
        .into_iter()
        .flat_map(|dir| chrome_for_testing(&dir, os))
        .collect()
}

/// Playwright's `chromium-<rev>` browsers (newest first), then its
/// `chromium_headless_shell-<rev>` shells when running headless.
fn playwright_browsers(root: &Path, os: Os, headless: bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = versions(root, "chromium-")
        .into_iter()
        .flat_map(|dir| chrome_for_testing(&dir, os))
        .collect();
    if headless {
        out.extend(
            versions(root, "chromium_headless_shell-")
                .into_iter()
                .flat_map(|dir| headless_shell(&dir, os)),
        );
    }
    out
}

/// The subdirectories of `root` named `<prefix><version>`, highest version
/// first (versions compare as dotted numbers).
fn versions(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<(Vec<u64>, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name.strip_prefix(prefix)?;
            let parts: Option<Vec<u64>> = version.split('.').map(|p| p.parse().ok()).collect();
            Some((parts?, entry.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, path)| path).collect()
}

/// The browser `find_chrome` uses on this machine.
pub(crate) fn find(headless: bool) -> Result<PathBuf, BrowserError> {
    let env = |name: &str| std::env::var_os(name);
    Search {
        os: Os::current(),
        env: &env,
        headless,
    }
    .find()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    fn env_of(vars: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.as_os_str().to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn the_environment_variable_wins_and_must_name_a_file() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let chrome = dir.path().join("my-chrome");
        touch(&chrome);
        let env = env_of(&[(CHROME_ENV, &chrome)]);
        let search = Search {
            os: Os::Linux,
            env: &env,
            headless: true,
        };
        assert_eq!(search.find().unwrap(), chrome);

        let missing = dir.path().join("nope");
        let env = env_of(&[(CHROME_ENV, &missing)]);
        let error = Search {
            os: Os::Linux,
            env: &env,
            headless: true,
        }
        .find()
        .unwrap_err();
        assert!(error.is_chrome_not_found());
        assert!(error.to_string().contains("not a file"), "{error}");
    }

    #[test]
    fn system_chrome_comes_before_the_caches_then_edge_last() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let program_files = dir.path().join("pf");
        let local = dir.path().join("local");
        let env = env_of(&[("PROGRAMFILES", &program_files), ("LOCALAPPDATA", &local)]);
        let search = Search {
            os: Os::Windows,
            env: &env,
            headless: true,
        };

        // Nothing installed: the error lists where it looked.
        let error = search.find().unwrap_err();
        assert!(error.is_chrome_not_found());
        let message = error.to_string();
        assert!(message.contains("navree browser:install"), "{message}");
        assert!(message.contains("NAVREE_BROWSER_CHROME"), "{message}");

        // Edge only.
        let edge = program_files.join(r"Microsoft\Edge\Application\msedge.exe");
        touch(&edge);
        assert_eq!(search.find().unwrap(), edge);

        // Playwright's headless shell beats Edge when headless...
        let shell = local
            .join("ms-playwright/chromium_headless_shell-1248")
            .join("chrome-headless-shell-win64/chrome-headless-shell.exe");
        touch(&shell);
        assert_eq!(search.find().unwrap(), shell);
        // ...but not when the window is shown.
        let headed = Search {
            os: Os::Windows,
            env: &env,
            headless: false,
        };
        assert_eq!(headed.find().unwrap(), edge);

        // Playwright's full Chromium, newest revision first.
        let old = local.join("ms-playwright/chromium-1234/chrome-win64/chrome.exe");
        let new = local.join("ms-playwright/chromium-1248/chrome-win64/chrome.exe");
        touch(&old);
        touch(&new);
        assert_eq!(search.find().unwrap(), new);

        // Navree's cache (navree browser:install) beats Playwright's.
        let cached = local.join("navree/chrome/156.0.8078.47/chrome-win64/chrome.exe");
        let older = local.join("navree/chrome/141.0.7390.54/chrome-win64/chrome.exe");
        touch(&older);
        assert_eq!(search.find().unwrap(), older);
        touch(&cached);
        assert_eq!(search.find().unwrap(), cached);

        // An installed Chrome beats everything.
        let chrome = program_files.join(r"Google\Chrome\Application\chrome.exe");
        touch(&chrome);
        assert_eq!(search.find().unwrap(), chrome);
    }

    #[test]
    fn linux_looks_on_the_path_and_in_the_xdg_cache() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let bin = dir.path().join("bin");
        let cache = dir.path().join("cache");
        let env = env_of(&[("PATH", &bin), ("XDG_CACHE_HOME", &cache)]);
        let search = Search {
            os: Os::Linux,
            env: &env,
            headless: true,
        };
        let cached = cache.join("navree/chrome/156.0.8078.47/chrome-linux64/chrome");
        touch(&cached);
        let candidates = search.candidates();
        let at = |p: &Path| candidates.iter().position(|c| c == p).unwrap();
        assert!(at(&bin.join("google-chrome")) < at(&cached));
        assert_eq!(
            search.candidates().iter().find(|p| p.starts_with(&cache)),
            Some(&cached)
        );
        assert_eq!(
            playwright_browsers(&cache.join("ms-playwright"), Os::Linux, true),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn the_cache_dir_follows_the_platform_and_can_be_moved() {
        let home = PathBuf::from("/home/ada");
        let env = env_of(&[("HOME", &home)]);
        assert_eq!(
            cache_dir(Os::Linux, &env),
            Some(home.join(".cache").join("navree"))
        );
        assert_eq!(
            cache_dir(Os::Mac, &env),
            Some(home.join("Library/Caches/navree"))
        );
        let local = PathBuf::from(r"C:\Users\ada\AppData\Local");
        let env = env_of(&[("LOCALAPPDATA", &local)]);
        assert_eq!(cache_dir(Os::Windows, &env), Some(local.join("navree")));
        let custom = PathBuf::from("/ci/cache");
        let env = env_of(&[("NAVREE_CACHE_DIR", &custom), ("HOME", &home)]);
        assert_eq!(cache_dir(Os::Linux, &env), Some(custom));
    }

    #[test]
    fn mac_apps_and_chrome_for_testing_bundles() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let found = chrome_for_testing(dir.path(), Os::Mac);
        assert!(found[0].ends_with(
            "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
        ));
        let env = env_of(&[("HOME", dir.path())]);
        let search = Search {
            os: Os::Mac,
            env: &env,
            headless: true,
        };
        assert_eq!(
            search.candidates()[0],
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        );
    }

    #[test]
    fn versions_sort_numerically() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        for name in ["9.0.1", "141.0.7390.54", "156.0.8078.47", "notes"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        let names: Vec<String> = versions(dir.path(), "")
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["156.0.8078.47", "141.0.7390.54", "9.0.1"]);
    }
}
