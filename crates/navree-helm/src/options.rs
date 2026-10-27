//! [`HelmOptions`]: the `helm` configuration section.

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// The panel's settings (configuration section `helm`; every key is
/// optional).
///
/// ```toml
/// [helm]
/// path = "/admin"               # where the panel is served
/// title = "Acme CRM"            # the header and the browser tab
/// logo = "/brand/logo.svg"      # a URL, served by your app or a CDN
/// accent = "#3dff8f"            # buttons, links and focus rings
/// per_page = 25                 # rows per page on lists
/// metrics_cache_secs = 60       # metric results cached per user and range
/// dashboard = true              # metric cards on the home page
/// session_minutes = 480         # how long a panel sign-in lasts
/// ```
///
/// From the environment: `APP_HELM__TITLE="Acme CRM"`,
/// `APP_HELM__PER_PAGE=50`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HelmOptions {
    /// Where the panel is served (`/admin`).
    pub path: String,
    /// The name in the header and the browser tab (`Admin`).
    pub title: String,
    /// A logo URL for the header and the sign-in screen: a path on your app
    /// or an `https://` URL.
    pub logo: Option<String>,
    /// The accent color of buttons and links, `#rgb` or `#rrggbb`
    /// (`#3dff8f`).
    pub accent: String,
    /// Rows per page on lists (25, at most 100).
    pub per_page: u64,
    /// How long metric results are cached per user and range, in seconds
    /// (60; 0 turns caching off).
    pub metrics_cache_secs: u64,
    /// Whether the home page shows the metric cards marked `on_dashboard`
    /// (true).
    pub dashboard: bool,
    /// How long a sign-in to the panel lasts, in minutes (480).
    pub session_minutes: u64,
}

impl Default for HelmOptions {
    fn default() -> Self {
        Self {
            path: "/admin".to_owned(),
            title: "Admin".to_owned(),
            logo: None,
            accent: "#3dff8f".to_owned(),
            per_page: 25,
            metrics_cache_secs: 60,
            dashboard: true,
            session_minutes: 480,
        }
    }
}

impl HelmOptions {
    /// Every problem with the options, each naming its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let path = self.path.trim_end_matches('/');
        if !path.starts_with('/')
            || path.len() < 2
            || path.contains("//")
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'))
        {
            errors.push(format!(
                "helm.path must be a path such as /admin (letters, digits, -, _ and /), not `{}`",
                self.path
            ));
        }
        if self.title.trim().is_empty() {
            errors.push("helm.title must not be empty".to_owned());
        }
        if let Some(logo) = &self.logo
            && !(logo.starts_with('/') && !logo.starts_with("//")
                || logo.starts_with("https://")
                || logo.starts_with("http://"))
        {
            errors.push(
                "helm.logo must be a path on your app (/brand/logo.svg) or an http(s) URL"
                    .to_owned(),
            );
        }
        if !is_hex_color(&self.accent) {
            errors.push(format!(
                "helm.accent must be a hex color such as #3dff8f, not `{}`",
                self.accent
            ));
        }
        if self.per_page == 0 || self.per_page > navree_db::MAX_PER_PAGE {
            errors.push(format!(
                "helm.per_page must be between 1 and {}",
                navree_db::MAX_PER_PAGE
            ));
        }
        if self.session_minutes == 0 {
            errors.push("helm.session_minutes must be at least 1".to_owned());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The path without a trailing slash (`/admin`).
    pub fn base(&self) -> &str {
        self.path.trim_end_matches('/')
    }
}

impl ConfigSection for HelmOptions {
    const KEY: &'static str = "helm";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// `#rgb` or `#rrggbb`.
fn is_hex_color(color: &str) -> bool {
    let Some(hex) = color.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};

    use super::*;

    #[test]
    fn defaults_match_the_docs() {
        let options = HelmOptions::default();
        assert_eq!(options.path, "/admin");
        assert_eq!(options.title, "Admin");
        assert_eq!(options.logo, None);
        assert_eq!(options.accent, "#3dff8f");
        assert_eq!(options.per_page, 25);
        assert_eq!(options.metrics_cache_secs, 60);
        assert!(options.dashboard);
        assert_eq!(options.check(), Ok(()));
    }

    #[test]
    fn environment_variables_set_the_section() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_HELM__TITLE", "Acme CRM"),
                ("APP_HELM__PER_PAGE", "50"),
                ("APP_HELM__DASHBOARD", "false"),
            ],
        );
        let options: HelmOptions = Config::load(&sources).unwrap().typed().unwrap();
        assert_eq!(options.title, "Acme CRM");
        assert_eq!(options.per_page, 50);
        assert!(!options.dashboard);
        assert_eq!(options.path, "/admin");
    }

    #[test]
    fn invalid_options_name_their_keys() {
        let options = HelmOptions {
            path: "admin".into(),
            accent: "green".into(),
            per_page: 0,
            logo: Some("javascript:alert(1)".into()),
            ..HelmOptions::default()
        };
        let errors = options.check().unwrap_err();
        for key in ["helm.path", "helm.accent", "helm.per_page", "helm.logo"] {
            assert!(
                errors.iter().any(|e| e.starts_with(key)),
                "{key}: {errors:?}"
            );
        }
        assert_eq!(
            HelmOptions {
                path: "/back-office/".into(),
                ..HelmOptions::default()
            }
            .base(),
            "/back-office"
        );
    }
}
