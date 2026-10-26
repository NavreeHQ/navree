//! [`ApiOptions`]: the `api` configuration section.

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer};

/// OpenAPI document and docs UI settings (configuration section `api`;
/// every key optional).
///
/// ```toml
/// [api]
/// title = "CRM API"
/// version = "1.0.0"
/// servers = ["https://crm.example.com"]
/// docs_ui = false
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ApiOptions {
    /// The document title, `info.title` (default `Navree API`).
    pub title: String,
    /// The API version, `info.version` (default `1.0.0`).
    pub version: String,
    /// Server URLs listed in the document (default none: clients use the
    /// URL they loaded the document from). From the environment, a
    /// comma-separated list (`APP_API__SERVERS=https://a.example,https://b.example`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub servers: Vec<String>,
    /// Serve the Scalar docs UI at `{api_prefix}/docs` (default `true`).
    pub docs_ui: bool,
}

impl Default for ApiOptions {
    fn default() -> Self {
        Self {
            title: "Navree API".to_owned(),
            version: "1.0.0".to_owned(),
            servers: Vec::new(),
            docs_ui: true,
        }
    }
}

impl ConfigSection for ApiOptions {
    const KEY: &'static str = "api";

    fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        if self.title.trim().is_empty() {
            errors.push("api.title must not be empty".to_owned());
        }
        if self.version.trim().is_empty() {
            errors.push("api.version must not be empty".to_owned());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// A list, or one comma-separated string (what an environment variable
/// holds); entries are trimmed and empty ones dropped.
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum List {
        Items(Vec<String>),
        Joined(String),
    }
    let items = match List::deserialize(deserializer)? {
        List::Items(items) => items,
        List::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}
