//! [`DocumentsOptions`]: the `documents` configuration section.

use std::path::PathBuf;

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer, Serialize};

use crate::EngineKind;
use crate::print::PrintOptions;

/// The settings of `DocumentsModule`: the `documents` configuration
/// section. Every key is optional.
///
/// ```toml
/// [documents]
/// templates = ["resources/documents"]  # or APP_DOCUMENTS__TEMPLATES=a,b
/// engine = "typst"          # which engine wins when a name has both a .typ and an .html file
/// disk = "documents"        # storage disk for queued renders (default: the default disk)
///
/// [documents.typst]
/// bundled_fonts = true      # Noto Sans, Noto Sans Arabic, Noto Sans Devanagari
/// font_dirs = ["resources/fonts"]
/// packages_dir = "resources/typst-packages"
/// timeout_secs = 60
///
/// [documents.chrome]
/// path = "/usr/bin/chromium"   # default: NAVREE_BROWSER_CHROME, then the usual install paths
/// endpoint = "ws://chrome:9222/devtools/browser/..."  # use a running Chrome instead
/// pool_size = 2             # pages rendering at once
/// timeout_secs = 30
/// no_sandbox = false        # true when Chrome runs as root in a container
/// allow_network = false     # let documents load from other origins
/// args = []                 # more command-line flags
///
/// [documents.chrome.print]
/// paper = "a4"
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentsOptions {
    /// Template directories, searched in order (default
    /// `["resources/documents"]`). From the environment, a comma-separated
    /// list.
    #[serde(deserialize_with = "paths")]
    pub templates: Vec<PathBuf>,
    /// The engine used when a template name has both a `.typ` and an
    /// `.html` file (default `typst`).
    pub engine: EngineKind,
    /// The storage disk queued renders are written to (default: the
    /// default disk).
    pub disk: Option<String>,
    /// The Typst engine.
    pub typst: TypstOptions,
    /// The Chrome engine.
    pub chrome: ChromeOptions,
}

impl Default for DocumentsOptions {
    fn default() -> Self {
        Self {
            templates: vec![PathBuf::from("resources/documents")],
            engine: EngineKind::Typst,
            disk: None,
            typst: TypstOptions::default(),
            chrome: ChromeOptions::default(),
        }
    }
}

impl ConfigSection for DocumentsOptions {
    const KEY: &'static str = "documents";
}

/// The Typst engine's settings (`documents.typst`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TypstOptions {
    /// Load the bundled Noto fonts (default true).
    pub bundled_fonts: bool,
    /// More font directories, searched recursively (default none).
    #[serde(deserialize_with = "paths")]
    pub font_dirs: Vec<PathBuf>,
    /// Local Typst packages, as `<namespace>/<name>/<version>` (default
    /// none: package imports fail).
    pub packages_dir: Option<PathBuf>,
    /// Time limit for one render, in seconds (default 60).
    pub timeout_secs: u64,
}

impl Default for TypstOptions {
    fn default() -> Self {
        Self {
            bundled_fonts: true,
            font_dirs: Vec::new(),
            packages_dir: None,
            timeout_secs: 60,
        }
    }
}

/// The Chrome engine's settings (`documents.chrome`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromeOptions {
    /// The Chrome or Chromium executable. Default: the
    /// `NAVREE_BROWSER_CHROME` environment variable, then the usual
    /// install locations.
    pub path: Option<PathBuf>,
    /// A running Chrome to use instead of launching one: its browser
    /// WebSocket URL (`ws://host:9222/devtools/browser/<id>`) or its
    /// DevTools HTTP address (`http://host:9222`).
    pub endpoint: Option<String>,
    /// How many pages render at once (default 2). More renders wait.
    pub pool_size: usize,
    /// Time limit for one render, and for Chrome to start, in seconds
    /// (default 30).
    pub timeout_secs: u64,
    /// Run Chrome with `--no-sandbox` (default false). Needed when Chrome
    /// runs as root, as in many containers.
    pub no_sandbox: bool,
    /// Let documents load images, stylesheets and fonts from other origins
    /// (default false: only the template tree is reachable, so a template
    /// cannot make the server fetch an address its data names).
    pub allow_network: bool,
    /// More command-line flags for Chrome.
    #[serde(deserialize_with = "strings")]
    pub args: Vec<String>,
    /// The page settings renders use unless they bring their own.
    pub print: PrintOptions,
}

impl Default for ChromeOptions {
    fn default() -> Self {
        Self {
            path: None,
            endpoint: None,
            pool_size: 2,
            timeout_secs: 30,
            no_sandbox: false,
            allow_network: false,
            args: Vec::new(),
            print: PrintOptions::default(),
        }
    }
}

/// A list of paths, or one comma-separated string (what an environment
/// variable holds).
fn paths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<PathBuf>, D::Error> {
    Ok(strings(deserializer)?
        .into_iter()
        .map(PathBuf::from)
        .collect())
}

/// A list of strings, or one comma-separated string.
fn strings<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrString {
        List(Vec<String>),
        String(String),
    }
    Ok(match ListOrString::deserialize(deserializer)? {
        ListOrString::List(list) => list,
        ListOrString::String(text) => text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_documentation() {
        let options = DocumentsOptions::default();
        assert_eq!(
            options.templates,
            vec![PathBuf::from("resources/documents")]
        );
        assert_eq!(options.engine, EngineKind::Typst);
        assert_eq!(options.disk, None);
        assert!(options.typst.bundled_fonts);
        assert_eq!(options.typst.timeout_secs, 60);
        assert_eq!(options.chrome.pool_size, 2);
        assert_eq!(options.chrome.timeout_secs, 30);
        assert!(!options.chrome.no_sandbox);
        assert!(!options.chrome.allow_network);
    }

    #[test]
    fn lists_come_as_lists_or_comma_separated_strings() {
        let options: DocumentsOptions = serde_json::from_value(json!({
            "templates": "a, b",
            "engine": "chrome",
            "typst": {"font_dirs": ["fonts"]},
            "chrome": {"args": "--lang=ar,--disable-gpu", "print": {"paper": "letter"}},
        }))
        .unwrap();
        assert_eq!(
            options.templates,
            vec![PathBuf::from("a"), PathBuf::from("b")]
        );
        assert_eq!(options.engine, EngineKind::Chrome);
        assert_eq!(options.typst.font_dirs, vec![PathBuf::from("fonts")]);
        assert!(options.typst.bundled_fonts);
        assert_eq!(options.chrome.args, vec!["--lang=ar", "--disable-gpu"]);
        assert_eq!(options.chrome.print.paper, crate::Paper::Letter);
        assert_eq!(options.chrome.pool_size, 2);
        assert!(serde_json::from_value::<DocumentsOptions>(json!({"engine": "unknown"})).is_err());
    }
}
