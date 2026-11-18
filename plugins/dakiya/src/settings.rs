//! Settings for the `dakiya` mail transport (`[mail.dakiya]`).

use std::fmt;

use navree::contracts::Secret;
use serde::{Deserialize, Deserializer};
use url::Url;

/// Settings read from `[mail.dakiya]` when `mail.transport` selects `dakiya`.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DakiyaSettings {
    /// Endpoint where messages are posted (default `https://api.dakiya.example/v1/send`).
    pub url: String,
    /// Bearer credential for the Dakiya HTTP API.
    #[serde(deserialize_with = "text_only")]
    pub api_key: Secret<String>,
    /// Optional display name added to bare sender addresses.
    pub sender_name: Option<String>,
}

impl Default for DakiyaSettings {
    fn default() -> Self {
        Self {
            url: "https://api.dakiya.example/v1/send".to_owned(),
            api_key: Secret::default(),
            sender_name: None,
        }
    }
}

impl fmt::Debug for DakiyaSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let host = Url::parse(&self.url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_owned));
        f.debug_struct("DakiyaSettings")
            .field("host", &host)
            .field("api_key", &self.api_key)
            .field("sender_name", &self.sender_name)
            .finish()
    }
}

/// Reads a string without converting numbers or booleans, and never echoes a non-string value.
fn text_only<'de, D>(deserializer: D) -> Result<Secret<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) => Ok(Secret::new(text)),
        _ => Err(serde::de::Error::custom("mail.dakiya.api_key must be text")),
    }
}
