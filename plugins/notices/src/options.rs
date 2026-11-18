//! Configuration for the `notices` plugin (`[notices]`).

use navree::config::{Config, ConfigError, ConfigSection};
use navree::kernel::AppContext;
use serde::{Deserialize, Serialize};

/// Settings read from the `[notices]` table of the app configuration.
#[derive(Clone, Debug, Deserialize, Serialize, ConfigSection)]
#[config(key = "notices")]
#[serde(default)]
pub struct NoticesOptions {
    /// Whether the `notices` plugin is active.
    pub enabled: bool,
    /// How many days a notice stays on the board before expiry removes it.
    pub keep_days: u32,
    /// Sender address used for daily notice digests (`APP_NOTICES__DIGEST_FROM`).
    pub digest_from: String,
}

impl Default for NoticesOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_days: 30,
            digest_from: "notices@ludhiana-society.example".to_owned(),
        }
    }
}

/// Reads `[notices]` from `app`'s configuration, or returns the defaults when
/// the app has no `ConfigModule`.
pub(crate) async fn load(app: &AppContext) -> Result<NoticesOptions, ConfigError> {
    match app.try_get::<Config>().await {
        Some(config) => config.section_or_default::<NoticesOptions>(NoticesOptions::KEY),
        None => Ok(NoticesOptions::default()),
    }
}
