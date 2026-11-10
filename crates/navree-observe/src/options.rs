//! [`ObserveOptions`]: the `observe` configuration section.

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;

/// Observability settings (configuration section `observe`; every key optional).
///
/// ```toml
/// [observe]
/// metrics = true
/// metrics_token = "a-long-random-string"   # or APP_OBSERVE__METRICS_TOKEN
/// ```
#[derive(Clone, Debug, Deserialize, ConfigSection)]
#[config(
    key = "observe",
    docs = "/docs/digging-deeper/health-and-metrics/",
    validate = Self::check
)]
#[serde(default)]
pub struct ObserveOptions {
    /// Install the Prometheus recorder and serve `GET /metrics` (default `true`).
    pub metrics: bool,
    /// When set, `GET /metrics` requires `Authorization: Bearer <token>`.
    pub metrics_token: Option<Secret<String>>,
}

impl Default for ObserveOptions {
    fn default() -> Self {
        Self {
            metrics: true,
            metrics_token: None,
        }
    }
}

impl ObserveOptions {
    /// Checks the options; the message never echoes the token.
    pub(crate) fn check(&self) -> Result<(), String> {
        match &self.metrics_token {
            Some(token) if token.expose().trim().is_empty() => Err(
                "observe.metrics_token must not be empty (remove it to serve /metrics without a token)"
                    .to_owned(),
            ),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = ObserveOptions::schema().expect("schema");
        assert_eq!(schema.key, "observe");
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        assert_eq!(schema.secret_paths(), ["observe.metrics_token"]);
        assert!(schema.fields.iter().any(|f| f.name == "metrics"));
        let defaults = ObserveOptions::defaults().expect("defaults");
        assert_eq!(defaults["metrics"], true);
        assert!(defaults["metrics_token"].is_null());
    }

    #[test]
    fn debug_never_prints_the_token() {
        let options = ObserveOptions {
            metrics_token: Some(Secret::new("scrape-me-not".to_owned())),
            ..ObserveOptions::default()
        };
        assert!(!format!("{options:?}").contains("scrape-me-not"));
    }
}
