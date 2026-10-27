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
/// key = "navree-crm-key"          # the key in `ws://host/app/{key}` and in the client's config
/// secret = "change-me"          # signs private and presence subscriptions
/// allowed_origins = ["http://localhost:3000"]
/// activity_timeout_secs = 120
/// relay_url = "auto"            # workers hand broadcasts to `serve`
/// backplane = "none"            # or "redis": share broadcasts and presence across processes
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
    /// Where a process that does not serve the WebSockets (`queue:work`,
    /// `schedule:work`, a command) sends its broadcasts: the base URL of
    /// the `serve` process, e.g. `http://127.0.0.1:8080`, whose
    /// `POST /apps/{app_id}/events` delivers them to its connections.
    /// `auto` means `http://` plus `http.addr` (a wildcard address as
    /// loopback). Empty (the default): broadcasts stay in the process,
    /// which only reaches sockets when it serves them itself. Relayed
    /// messages are signed with a key derived from `realtime.secret`, or
    /// from the app key when there is no secret, so every process of the
    /// app must share one of them.
    pub relay_url: String,
    /// What connects the app's processes (default
    /// [`BackplaneKind::None`]). With [`BackplaneKind::Redis`], every
    /// process shares broadcasts, presence members and user
    /// disconnections over Redis; it needs `RedisModule` and a
    /// [`secret`](RealtimeOptions::secret), because a subscription signed
    /// by one process is checked by another.
    pub backplane: BackplaneKind,
}

/// What connects the realtime hubs of an app's processes
/// (`realtime.backplane`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum BackplaneKind {
    /// Nothing (`"none"`, the default): each process reaches its own
    /// sockets; workers use the relay. An app (or test) that provides an
    /// unnamed `dyn Backplane` (`override_value`) uses that one.
    #[default]
    None,
    /// Redis pub/sub (`"redis"`): the `dyn Backplane` provider named
    /// `redis`, which navree-redis's `RedisModule` registers.
    Redis,
}

impl BackplaneKind {
    /// The configuration name: `none` or `redis`.
    pub fn as_str(self) -> &'static str {
        match self {
            BackplaneKind::None => "none",
            BackplaneKind::Redis => "redis",
        }
    }
}

impl Default for RealtimeOptions {
    fn default() -> Self {
        Self {
            app_id: "navree".to_owned(),
            key: "navree".to_owned(),
            secret: Secret::default(),
            allowed_origins: Vec::new(),
            activity_timeout_secs: 120,
            relay_url: String::new(),
            backplane: BackplaneKind::None,
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
        if self.backplane != BackplaneKind::None && self.secret.expose().is_empty() {
            errors.push(format!(
                "realtime.backplane = \"{}\" needs realtime.secret: every process must sign channel subscriptions with the same secret",
                self.backplane.as_str()
            ));
        }
        let relay = self.relay_url.trim();
        if !(relay.is_empty()
            || relay == "auto"
            || ((relay.starts_with("http://") || relay.starts_with("https://"))
                && relay.len() > "https://".len()))
        {
            errors.push(format!(
                "realtime.relay_url must be empty, `auto` or a URL like http://127.0.0.1:8080 (got `{relay}`)"
            ));
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
            (
                serde_json::json!({"relay_url": "127.0.0.1:8080"}),
                "realtime.relay_url",
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

    #[test]
    fn a_backplane_needs_a_shared_secret() {
        let defaults = load(ConfigSources::new()).unwrap();
        assert_eq!(defaults.backplane, BackplaneKind::None);

        let error = load(
            ConfigSources::new().set("realtime", serde_json::json!({"backplane": "redis"})),
        )
        .expect_err("no secret")
        .to_string();
        assert!(
            error.contains("realtime.backplane = \"redis\" needs realtime.secret"),
            "{error}"
        );

        let options = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_REALTIME__BACKPLANE", "redis"),
                ("APP_REALTIME__SECRET", "shared"),
            ],
        ))
        .unwrap();
        assert_eq!(options.backplane, BackplaneKind::Redis);
        assert_eq!(
            [BackplaneKind::None, BackplaneKind::Redis].map(BackplaneKind::as_str),
            ["none", "redis"]
        );
        assert!(
            load(ConfigSources::new().set("realtime", serde_json::json!({"backplane": "pigeon"})))
                .is_err()
        );
    }
}
