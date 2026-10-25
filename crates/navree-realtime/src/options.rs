//! [`RealtimeOptions`]: the `realtime` configuration section.

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::{Deserialize, Deserializer};

/// Settings of the realtime server (configuration section `realtime`; every
/// key optional).
///
/// ```toml
/// [realtime]
/// app_id = "crm"
/// key = "navree-crm-key"          # the key in `ws://host/app/{key}` and in Echo's config
/// secret = "change-me"          # signs private and presence subscriptions
/// allowed_origins = ["http://localhost:3000"]
/// activity_timeout_secs = 120
/// ```
///
/// `Debug` redacts the secret.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct RealtimeOptions {
    /// The application id (default `navree`).
    pub app_id: String,
    /// The public application key clients connect with (default `navree`):
    /// letters, digits, `-`, `_` and `.`.
    pub key: String,
    /// The secret that signs private and presence channel subscriptions.
    /// Empty (the default) means a random secret per process, which is
    /// enough for this in-process server: it both signs and verifies.
    pub secret: Secret<String>,
    /// Exact origins (`https://app.example.com`) whose pages may connect;
    /// `*` allows any. Empty (the default) means the `http.cors`
    /// allowed origins. A request without an `Origin` header (not a
    /// browser) or from the server's own origin is always accepted. From the
    /// environment, a comma-separated list.
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub allowed_origins: Vec<String>,
    /// Seconds of silence after which the server pings a client (default
    /// 120, sent to clients as `activity_timeout`). A client that then stays
    /// silent for another `min(activity_timeout, 30 s)` is disconnected.
    pub activity_timeout_secs: u64,
}

impl Default for RealtimeOptions {
    fn default() -> Self {
        Self {
            app_id: "navree".to_owned(),
            key: "navree".to_owned(),
            secret: Secret::default(),
            allowed_origins: Vec::new(),
            activity_timeout_secs: 120,
        }
    }
}

impl ConfigSection for RealtimeOptions {
    const KEY: &'static str = "realtime";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

impl RealtimeOptions {
    /// Every problem with these options (empty when valid). Messages name
    /// the key and never echo the secret.
    pub(crate) fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.app_id.trim().is_empty() {
            errors.push("realtime.app_id must not be empty".to_owned());
        }
        let key_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
        if self.key.is_empty() || !self.key.chars().all(key_char) {
            errors.push(format!(
                "realtime.key must be letters, digits, `-`, `_` or `.` (got `{}`)",
                self.key
            ));
        }
        if self.activity_timeout_secs == 0 {
            errors.push("realtime.activity_timeout_secs must be at least 1".to_owned());
        }
        for origin in &self.allowed_origins {
            let valid = origin == "*"
                || ((origin.starts_with("http://") || origin.starts_with("https://"))
                    && http::HeaderValue::from_str(origin).is_ok());
            if !valid {
                errors.push(format!(
                    "realtime.allowed_origins: `{origin}` is not an origin like https://app.example.com (or `*`)"
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
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
    enum Origins {
        List(Vec<String>),
        Joined(String),
    }
    let origins = match Origins::deserialize(deserializer)? {
        Origins::List(origins) => origins,
        Origins::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(origins
        .into_iter()
        .map(|origin| origin.trim().to_owned())
        .filter(|origin| !origin.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};

    use super::*;

    fn load(sources: ConfigSources) -> Result<RealtimeOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<RealtimeOptions>()
    }

    #[test]
    fn missing_section_gives_the_defaults() {
        let options = load(ConfigSources::new()).unwrap();
        assert_eq!(options.app_id, "navree");
        assert_eq!(options.key, "navree");
        assert!(options.secret.expose().is_empty());
        assert!(options.allowed_origins.is_empty());
        assert_eq!(options.activity_timeout_secs, 120);
    }

    #[test]
    fn section_realtime_is_read_and_origins_may_be_comma_separated() {
        let options = load(
            ConfigSources::new()
                .set(
                    "realtime",
                    serde_json::json!({"app_id": "crm", "key": "navree-crm-key", "secret": "s3cret"}),
                )
                .env_vars(
                    "APP_",
                    [
                        (
                            "APP_REALTIME__ALLOWED_ORIGINS",
                            "http://localhost:3000, https://crm.example.com,",
                        ),
                        ("APP_REALTIME__ACTIVITY_TIMEOUT_SECS", "30"),
                    ],
                ),
        )
        .unwrap();
        assert_eq!(options.app_id, "crm");
        assert_eq!(options.key, "navree-crm-key");
        assert_eq!(options.secret.expose(), "s3cret");
        assert_eq!(
            options.allowed_origins,
            ["http://localhost:3000", "https://crm.example.com"]
        );
        assert_eq!(options.activity_timeout_secs, 30);
        assert!(
            !format!("{options:?}").contains("s3cret"),
            "secret redacted"
        );
    }

    #[test]
    fn invalid_options_fail_validation_naming_the_key() {
        for (section, expected) in [
            (serde_json::json!({"key": ""}), "realtime.key"),
            (serde_json::json!({"key": "has space"}), "realtime.key"),
            (serde_json::json!({"key": "a:b"}), "realtime.key"),
            (serde_json::json!({"app_id": ""}), "realtime.app_id"),
            (
                serde_json::json!({"activity_timeout_secs": 0}),
                "realtime.activity_timeout_secs",
            ),
            (
                serde_json::json!({"allowed_origins": ["localhost:3000"]}),
                "realtime.allowed_origins",
            ),
        ] {
            let error = load(ConfigSources::new().set("realtime", section.clone()))
                .expect_err(&section.to_string())
                .to_string();
            assert!(error.contains(expected), "{section}: {error}");
        }
        let star = load(
            ConfigSources::new().set("realtime", serde_json::json!({"allowed_origins": ["*"]})),
        )
        .unwrap();
        assert_eq!(star.allowed_origins, ["*"]);
    }
}
