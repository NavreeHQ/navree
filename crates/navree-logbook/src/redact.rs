//! Masking secrets in what Logbook stores: headers, query strings and
//! bodies.

use http::HeaderMap;
use navree_config::Config;
use serde::Deserialize;
use serde_json::Value;

/// What a masked value is stored as.
pub const REDACTED: &str = "[redacted]";

/// Headers never stored at all.
const NEVER_STORED: [&str; 4] = [
    "authorization",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

/// Masks values whose names contain a fragment of `log.redact`
/// (ASCII case-insensitively); without one, the shared list of secret words
/// (`navree_contracts::Redactor::DEFAULT_FRAGMENTS`).
#[derive(Clone, Debug)]
pub(crate) struct Redactor {
    fragments: Vec<String>,
}

impl Default for Redactor {
    fn default() -> Self {
        Redactor::new(
            navree_contracts::Redactor::DEFAULT_FRAGMENTS
                .iter()
                .map(|f| (*f).to_owned())
                .collect(),
        )
    }
}

/// The `log` section's `redact` key.
#[derive(Deserialize)]
struct LogRedact {
    #[serde(default)]
    redact: Option<Value>,
}

impl Redactor {
    pub(crate) fn new(fragments: Vec<String>) -> Redactor {
        Redactor {
            fragments: fragments
                .into_iter()
                .map(|f| f.trim().to_ascii_lowercase())
                .filter(|f| !f.is_empty())
                .collect(),
        }
    }

    /// The fragments of `log.redact` (a list, or a comma-separated string),
    /// or the defaults.
    pub(crate) fn from_config(config: Option<&Config>) -> Redactor {
        let configured = config
            .and_then(|config| config.section::<LogRedact>("log").ok())
            .and_then(|log| log.redact)
            .and_then(|value| match value {
                Value::String(list) => Some(list.split(',').map(str::to_owned).collect()),
                Value::Array(items) => Some(
                    items
                        .into_iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect(),
                ),
                _ => None,
            });
        match configured {
            Some(fragments) => Redactor::new(fragments),
            None => Redactor::default(),
        }
    }

    /// Whether a value named `name` is a secret.
    pub(crate) fn is_secret(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.fragments.iter().any(|f| name.contains(f.as_str()))
    }

    /// The headers worth storing: never `Authorization`, `Cookie` or
    /// `Set-Cookie`; secret-looking values masked.
    pub(crate) fn headers(&self, headers: &HeaderMap) -> Vec<(String, String)> {
        headers
            .iter()
            .filter(|(name, _)| !NEVER_STORED.contains(&name.as_str()))
            .map(|(name, value)| {
                let value = if self.is_secret(name.as_str()) {
                    REDACTED.to_owned()
                } else {
                    value
                        .to_str()
                        .map_or_else(|_| "[binary]".to_owned(), str::to_owned)
                };
                (name.as_str().to_owned(), value)
            })
            .collect()
    }

    /// A query string (or form body) with secret-looking values masked.
    pub(crate) fn query(&self, query: &str) -> String {
        let pairs: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| {
                let value = if self.is_secret(&key) {
                    REDACTED.to_owned()
                } else {
                    value.into_owned()
                };
                (key.into_owned(), value)
            })
            .collect();
        if pairs.iter().all(|(key, _)| !self.is_secret(key)) {
            return query.to_owned();
        }
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs)
            .finish()
    }

    /// Masks, in place, the values of every key that looks secret.
    pub(crate) fn json(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map.iter_mut() {
                    if self.is_secret(key) {
                        *value = Value::String(REDACTED.to_owned());
                    } else {
                        self.json(value);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.json(item)),
            _ => {}
        }
    }

    /// A text body with secrets masked: JSON by key, forms by field name.
    pub(crate) fn body(&self, content_type: Option<&str>, text: &str) -> String {
        let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
        if content_type.contains("json") {
            if let Ok(mut value) = serde_json::from_str::<Value>(text) {
                self.json(&mut value);
                return serde_json::to_string(&value).unwrap_or_else(|_| text.to_owned());
            }
        } else if content_type.starts_with("application/x-www-form-urlencoded") {
            return self.query(text);
        }
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use serde_json::json;

    use super::*;

    #[test]
    fn credentials_are_never_stored_and_secrets_are_masked() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer x"));
        headers.insert("cookie", HeaderValue::from_static("session=1"));
        headers.insert("x-api-token", HeaderValue::from_static("abc"));
        headers.insert("accept", HeaderValue::from_static("application/json"));
        let stored = Redactor::default().headers(&headers);
        assert_eq!(
            stored,
            [
                ("x-api-token".to_owned(), REDACTED.to_owned()),
                ("accept".to_owned(), "application/json".to_owned()),
            ]
        );
    }

    #[test]
    fn json_and_form_bodies_are_masked_by_key() {
        let redactor = Redactor::default();
        let body = redactor.body(
            Some("application/json"),
            r#"{"email":"a@b.c","password":"hunter2","nested":[{"api_token":"t"}]}"#,
        );
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            value,
            json!({"email": "a@b.c", "password": REDACTED, "nested": [{"api_token": REDACTED}]})
        );
        assert_eq!(
            redactor.body(
                Some("application/x-www-form-urlencoded"),
                "name=ada&password=x"
            ),
            "name=ada&password=%5Bredacted%5D"
        );
        assert_eq!(redactor.query("page=2&q=deals"), "page=2&q=deals");
        assert_eq!(
            redactor.body(Some("text/plain"), "password=x"),
            "password=x"
        );
    }

    #[test]
    fn the_defaults_are_the_shared_list_of_secret_words() {
        let redactor = Redactor::default();
        assert_eq!(
            redactor.fragments.len(),
            navree_contracts::Redactor::DEFAULT_FRAGMENTS.len()
        );
        for name in ["db_password", "api_key", "connection_string", "private_key"] {
            assert!(redactor.is_secret(name), "{name}");
        }
        assert!(!redactor.is_secret("email"));
    }

    #[test]
    fn log_redact_replaces_the_defaults() {
        let config =
            Config::load(&navree_config::ConfigSources::new().set("log.redact", "ssn, card"))
                .unwrap();
        let redactor = Redactor::from_config(Some(&config));
        assert!(redactor.is_secret("customer_SSN"));
        assert!(!redactor.is_secret("password"));
        assert!(Redactor::from_config(None).is_secret("password"));
    }
}
