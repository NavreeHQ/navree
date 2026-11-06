//! [`DocumentsOptions`]: the `documents` configuration section.

use std::path::PathBuf;

use navree_config::{ConfigSection, ConfigTable};
use serde::{Deserialize, Serialize};

use crate::EngineKind;
use crate::print::PrintOptions;

/// The settings of `DocumentsModule`: the `documents` configuration
/// section. Every key is optional.
///
/// ```toml
/// [documents]
/// templates = ["resources/documents"]  # or APP_DOCUMENTS__TEMPLATES=a,b
/// engine = "native"         # the engine renders use unless they name one: native or chrome
/// disk = "documents"        # storage disk for queued renders (default: the default disk)
///
/// [documents.native]
/// bundled_fonts = true      # Noto Sans, Serif and Mono; Noto Sans Arabic, Hebrew and Devanagari
/// font_dirs = ["resources/fonts"]
/// timeout_secs = 60
/// max_image_pixels = 25000000   # the most pixels one PNG, GIF or WebP may have (width x height)
///
/// [documents.native.print]
/// paper = "a4"
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ConfigSection)]
#[config(key = "documents", docs = "/docs/packages/documents/", validate = Self::check)]
#[serde(default)]
pub struct DocumentsOptions {
    /// Template directories, searched in order (default
    /// `["resources/documents"]`). From the environment, a comma-separated
    /// list.
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    pub templates: Vec<PathBuf>,
    /// The engine renders use unless they name one (default `native`;
    /// `chrome` in a build without the `native` feature).
    #[config(one_of = "native, chrome")]
    pub engine: EngineKind,
    /// The storage disk queued renders are written to (default: the
    /// default disk).
    pub disk: Option<String>,
    /// The native engine.
    #[config(table)]
    pub native: NativeOptions,
    /// The Chrome engine.
    #[config(table)]
    pub chrome: ChromeOptions,
}

impl Default for DocumentsOptions {
    fn default() -> Self {
        Self {
            templates: vec![PathBuf::from("resources/documents")],
            engine: if cfg!(feature = "native") {
                EngineKind::Native
            } else {
                EngineKind::Chrome
            },
            disk: None,
            native: NativeOptions::default(),
            chrome: ChromeOptions::default(),
        }
    }
}

impl DocumentsOptions {
    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
        if self.native.max_image_pixels == 0 {
            return Err("`native.max_image_pixels` must be at least 1".to_owned());
        }
        Ok(())
    }
}

/// The native engine's settings (`documents.native`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct NativeOptions {
    /// Load the bundled Noto fonts (default true).
    pub bundled_fonts: bool,
    /// More font directories, searched recursively (default none).
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    pub font_dirs: Vec<PathBuf>,
    /// Time limit for one render, in seconds (default 60).
    pub timeout_secs: u64,
    /// The most pixels (width times height) one PNG, GIF or WebP image in a
    /// document may have (default 25,000,000: 5000 by 5000, or an A3 page
    /// at 600 dots per inch). A larger image is
    /// [`DocumentsError::TooLarge`] before it is decoded. A JPEG is not
    /// counted: it goes into the PDF as it is, without being decoded. At
    /// least 1; 0 fails the boot.
    ///
    /// [`DocumentsError::TooLarge`]: crate::DocumentsError::TooLarge
    pub max_image_pixels: u64,
    /// The page settings renders use unless they bring their own (an
    /// `@page` rule in the template wins while `prefer_css_page_size`
    /// is on).
    #[config(table)]
    pub print: PrintOptions,
}

impl Default for NativeOptions {
    fn default() -> Self {
        Self {
            bundled_fonts: true,
            font_dirs: Vec::new(),
            timeout_secs: 60,
            max_image_pixels: 25_000_000,
            print: PrintOptions::default(),
        }
    }
}

/// The Chrome engine's settings (`documents.chrome`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct ChromeOptions {
    /// The Chrome or Chromium executable. Default: the
    /// `NAVREE_BROWSER_CHROME` environment variable, then the usual
    /// install locations.
    #[config(env_fallback = "NAVREE_BROWSER_CHROME")]
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
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub args: Vec<String>,
    /// The page settings renders use unless they bring their own.
    #[config(table)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use navree_config::FieldType;
    use serde_json::json;

    #[test]
    fn the_section_describes_itself() {
        let schema = DocumentsOptions::schema().expect("schema");
        assert_eq!(schema.key, "documents");
        assert_eq!(schema.docs.as_deref(), Some("/docs/packages/documents/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        assert!(schema.secret_paths().is_empty());
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        assert_eq!(field("engine").ty, FieldType::choice(&["native", "chrome"]));
        assert_eq!(field("templates").ty, FieldType::list(FieldType::String));
        assert_eq!(field("native").ty, FieldType::Table);
        assert!(
            field("native")
                .fields
                .iter()
                .any(|field| field.name == "max_image_pixels")
        );
        assert!(
            field("chrome")
                .fields
                .iter()
                .any(|field| field.name == "pool_size")
        );
        let fallbacks = schema.env_fallbacks();
        assert!(
            fallbacks.iter().any(|fallback| {
                fallback.key == "documents.chrome.path"
                    && fallback.variable == "NAVREE_BROWSER_CHROME"
            }),
            "{fallbacks:?}"
        );
        let defaults = DocumentsOptions::defaults().expect("defaults");
        assert_eq!(defaults["templates"], json!(["resources/documents"]));
        assert_eq!(defaults["native"]["timeout_secs"], json!(60));
        assert_eq!(defaults["chrome"]["pool_size"], json!(2));
    }

    #[test]
    fn defaults_match_the_documentation() {
        let options = DocumentsOptions::default();
        assert_eq!(
            options.templates,
            vec![PathBuf::from("resources/documents")]
        );
        let engine = if cfg!(feature = "native") {
            EngineKind::Native
        } else {
            EngineKind::Chrome
        };
        assert_eq!(options.engine, engine);
        assert_eq!(options.disk, None);
        assert!(options.native.bundled_fonts);
        assert_eq!(options.native.timeout_secs, 60);
        assert_eq!(options.native.max_image_pixels, 25_000_000);
        assert_eq!(options.native.print, PrintOptions::default());
        assert_eq!(options.chrome.pool_size, 2);
        assert_eq!(options.chrome.timeout_secs, 30);
        assert!(!options.chrome.no_sandbox);
        assert!(!options.chrome.allow_network);
    }

    #[test]
    fn an_image_pixel_limit_of_zero_is_refused_at_boot() {
        assert_eq!(DocumentsOptions::default().validate(), Ok(()));
        let mut options = DocumentsOptions::default();
        options.native.max_image_pixels = 0;
        let message = options.validate().unwrap_err();
        assert!(message.contains("native.max_image_pixels"), "{message}");
    }

    #[test]
    fn lists_come_as_lists_or_comma_separated_strings() {
        let options: DocumentsOptions = serde_json::from_value(json!({
            "templates": "a, b",
            "engine": "chrome",
            "native": {"font_dirs": ["fonts"], "print": {"landscape": true}},
            "chrome": {"args": "--lang=ar,--disable-gpu", "print": {"paper": "letter"}},
        }))
        .unwrap();
        assert_eq!(
            options.templates,
            vec![PathBuf::from("a"), PathBuf::from("b")]
        );
        assert_eq!(options.engine, EngineKind::Chrome);
        assert_eq!(options.native.font_dirs, vec![PathBuf::from("fonts")]);
        assert!(options.native.bundled_fonts);
        assert!(options.native.print.landscape);
        assert_eq!(options.chrome.args, vec!["--lang=ar", "--disable-gpu"]);
        assert_eq!(options.chrome.print.paper, crate::Paper::Letter);
        assert_eq!(options.chrome.pool_size, 2);
        assert!(serde_json::from_value::<DocumentsOptions>(json!({"engine": "unknown"})).is_err());
    }
}
