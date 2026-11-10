//! The run's settings: `NAVREE_BROWSER_*` environment variables, overridden
//! by [`BrowserBuilder`](crate::BrowserBuilder) values.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::BrowserError;

/// The default time each action, assertion and wait may take.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// The default window size.
pub(crate) const DEFAULT_WINDOW: (u32, u32) = (1440, 900);

/// What a [`Browser`](crate::Browser) runs with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    /// The browser binary (`None`: find one).
    pub(crate) chrome: Option<PathBuf>,
    pub(crate) headless: bool,
    pub(crate) slow_mo: Duration,
    pub(crate) timeout: Duration,
    pub(crate) window: (u32, u32),
    /// The root of the failure files and screenshots.
    pub(crate) artifacts: PathBuf,
}

impl Settings {
    /// The settings from `NAVREE_BROWSER_*` variables read through `env`;
    /// invalid values are errors naming the variable.
    pub(crate) fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, BrowserError> {
        let var = |name: &str| {
            env(name)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let headless = match var("NAVREE_BROWSER_HEADLESS") {
            None => true,
            Some(value) => parse_bool(&value).ok_or_else(|| {
                BrowserError::Config(format!(
                    "NAVREE_BROWSER_HEADLESS must be true or false, not {value:?}"
                ))
            })?,
        };
        let millis = |name: &str, default: u64, zero_ok: bool| -> Result<u64, BrowserError> {
            match var(name) {
                None => Ok(default),
                Some(value) => match value.parse::<u64>() {
                    Ok(ms) if ms > 0 || zero_ok => Ok(ms),
                    _ => Err(BrowserError::Config(format!(
                        "{name} must be a {}number of milliseconds, not {value:?}",
                        if zero_ok { "" } else { "positive " }
                    ))),
                },
            }
        };
        let slow_mo = millis("NAVREE_BROWSER_SLOW_MO", 0, true)?;
        let timeout = millis(
            "NAVREE_BROWSER_TIMEOUT_MS",
            DEFAULT_TIMEOUT.as_millis() as u64,
            false,
        )?;
        let artifacts = match var("NAVREE_BROWSER_ARTIFACTS") {
            Some(dir) => absolute(Path::new(&dir)),
            None => default_artifacts(),
        };
        Ok(Settings {
            chrome: var("NAVREE_BROWSER_CHROME").map(PathBuf::from),
            headless,
            slow_mo: Duration::from_millis(slow_mo),
            timeout: Duration::from_millis(timeout),
            window: DEFAULT_WINDOW,
            artifacts,
        })
    }

    /// The settings from the process environment.
    pub(crate) fn from_process_env() -> Result<Settings, BrowserError> {
        Settings::from_env(&|name| std::env::var(name).ok())
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// `path` against the current directory.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// `<cargo target dir>/navree-browser`: the target directory is the
/// nearest ancestor of the test executable holding Cargo's
/// `.rustc_info.json` or `CACHEDIR.TAG` (so `CARGO_TARGET_DIR` and
/// workspaces are followed), else `target` in the current directory.
pub(crate) fn default_artifacts() -> PathBuf {
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| target_dir_of(&exe))
        .unwrap_or_else(|| absolute(Path::new("target")));
    target.join("navree-browser")
}

/// The Cargo target directory containing `exe`: the nearest ancestor with
/// Cargo's marker files, else the parent of `<profile>/deps`.
pub(crate) fn target_dir_of(exe: &Path) -> Option<PathBuf> {
    let marked = exe
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(".rustc_info.json").is_file() || dir.join("CACHEDIR.TAG").is_file());
    if let Some(dir) = marked {
        return Some(dir.to_path_buf());
    }
    let deps = exe.parent()?;
    if deps.file_name()? != "deps" {
        return None;
    }
    deps.parent()?.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn settings(vars: &[(&str, &str)]) -> Result<Settings, BrowserError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Settings::from_env(&move |name| map.get(name).cloned())
    }

    #[test]
    fn defaults_match_the_reference_table() {
        let s = settings(&[]).unwrap();
        assert!(s.headless);
        assert_eq!(s.slow_mo, Duration::ZERO);
        assert_eq!(s.timeout, Duration::from_secs(5));
        assert_eq!(s.window, (1440, 900));
        assert_eq!(s.chrome, None);
        assert!(s.artifacts.ends_with("navree-browser"), "{:?}", s.artifacts);
    }

    #[test]
    fn variables_set_each_option() {
        let s = settings(&[
            ("NAVREE_BROWSER_HEADLESS", "false"),
            ("NAVREE_BROWSER_SLOW_MO", "250"),
            ("NAVREE_BROWSER_TIMEOUT_MS", "20000"),
            ("NAVREE_BROWSER_ARTIFACTS", "out/browser"),
            ("NAVREE_BROWSER_CHROME", "/opt/chrome/chrome"),
        ])
        .unwrap();
        assert!(!s.headless);
        assert_eq!(s.slow_mo, Duration::from_millis(250));
        assert_eq!(s.timeout, Duration::from_secs(20));
        assert!(s.artifacts.is_absolute());
        assert!(s.artifacts.ends_with("out/browser"));
        assert_eq!(s.chrome, Some(PathBuf::from("/opt/chrome/chrome")));
        assert!(
            !settings(&[("NAVREE_BROWSER_HEADLESS", "0")])
                .unwrap()
                .headless
        );
        assert!(
            settings(&[("NAVREE_BROWSER_HEADLESS", " TRUE ")])
                .unwrap()
                .headless
        );
    }

    #[test]
    fn invalid_values_name_the_variable() {
        for (name, value) in [
            ("NAVREE_BROWSER_HEADLESS", "maybe"),
            ("NAVREE_BROWSER_SLOW_MO", "fast"),
            ("NAVREE_BROWSER_TIMEOUT_MS", "0"),
            ("NAVREE_BROWSER_TIMEOUT_MS", "-5"),
        ] {
            let error = settings(&[(name, value)]).unwrap_err();
            assert!(matches!(error, BrowserError::Config(_)), "{error}");
            assert!(error.to_string().contains(name), "{error}");
        }
    }

    #[test]
    fn the_target_dir_is_found_from_the_executable() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let target = dir.path().join("custom-target");
        std::fs::create_dir_all(target.join("debug/deps")).unwrap();
        std::fs::write(target.join(".rustc_info.json"), "{}").unwrap();
        let exe = target.join("debug/deps/browser-1234.exe");
        assert_eq!(target_dir_of(&exe), Some(target));

        // Without marker files: the parent of `<profile>/deps`.
        let bare = dir.path().join("bare");
        let exe = bare.join("x86_64-unknown-linux-gnu/debug/deps/browser-1");
        assert_eq!(
            target_dir_of(&exe),
            Some(bare.join("x86_64-unknown-linux-gnu"))
        );
        assert_eq!(target_dir_of(&dir.path().join("elsewhere/x.exe")), None);
    }
}
