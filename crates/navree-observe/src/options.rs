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
#[derive(Clone, Debug, Deserialize)]
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

impl ConfigSection for ObserveOptions {
    const KEY: &'static str = "observe";

    fn validate(&self) -> Result<(), String> {
        self.check()
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
