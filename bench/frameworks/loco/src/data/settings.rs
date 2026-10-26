use loco_rs::{Error, Result, config::Config};
use serde::Deserialize;

/// The app's own block, `settings:` in `config/<env>.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// The `iss` every accepted access token must carry.
    pub jwt_issuer: String,
}

impl Settings {
    /// # Errors
    /// When `settings:` is missing or does not match this struct.
    pub fn from_config(config: &Config) -> Result<Self> {
        let raw = config
            .settings
            .clone()
            .ok_or_else(|| Error::string("missing `settings:` in config"))?;
        Ok(serde_json::from_value(raw)?)
    }
}
