//! [`RealtimeOptions`]: the `realtime` configuration section.

use navree_config::{ConfigSection, ConfigTable};
use navree_contracts::Secret;
use serde::Deserialize;

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
///
/// [realtime.streams]            # live streams to pages (`GET /broadcasting/stream`)
/// names = 32                    # topics and channels one stream may name
/// per_user = 16                 # streams one user may hold open at once
/// lifetime_secs = 3600          # a stream ends after this, and the page resumes
/// replay = 1024                 # broadcasts kept for streams that come back
/// ```
///
/// `Debug` redacts the secret.
#[derive(Clone, Debug, Deserialize, ConfigSection)]
#[config(key = "realtime", docs = "/docs/digging-deeper/broadcasting/", validate = Self::check_values)]
#[serde(default)]
pub struct RealtimeOptions {
    /// The application id (default `navree`).
    pub app_id: String,
    /// The public application key clients connect with (default `navree`):
    /// letters, digits, `-`, `_` and `.`.
    #[config(public)]
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
    #[serde(deserialize_with = "navree_config::comma_list")]
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
    #[config(one_of = "none, redis")]
    pub backplane: BackplaneKind,
    /// The live streams pages listen to (`realtime.streams`).
    #[config(table)]
    pub streams: StreamOptions,
}

/// The caps of the live streams pages listen to (`realtime.streams`; every
/// key optional).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct StreamOptions {
    /// The topics and channels one stream may name together (default 32).
    /// A stream that names more is refused with 400.
    pub names: usize,
    /// The streams one signed-in user may hold open at once (default 16):
    /// one per open tab. One more is refused with 429, which a page tries
    /// again later. Guests are held by the server's connection limits.
    pub per_user: usize,
    /// How long a stream stays open, give or take a tenth (default 3600
    /// seconds). Then it ends, and the page comes back, resumes from its last
    /// id and is authorized again.
    pub lifetime_secs: u64,
    /// How many of the last broadcasts a process keeps once it has served a
    /// stream, so a stream that comes back is told only what it missed
    /// (default 1024). `0` keeps none: a page that comes back fetches
    /// everything it shows once.
    pub replay: usize,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            names: 32,
            per_user: 16,
            lifetime_secs: 3600,
            replay: 1024,
        }
    }
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
            streams: StreamOptions::default(),
        }
    }
}

impl RealtimeOptions {
    /// [`check`](RealtimeOptions::check) with the problems joined (the boot
    /// calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }

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
        if self.streams.names == 0 {
            errors.push("realtime.streams.names must be at least 1".to_owned());
        }
        if self.streams.per_user == 0 {
            errors.push("realtime.streams.per_user must be at least 1".to_owned());
        }
        if self.streams.lifetime_secs == 0 {
            errors.push("realtime.streams.lifetime_secs must be at least 1".to_owned());
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

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources, FieldType};

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
            (
                serde_json::json!({"streams": {"names": 0}}),
                "realtime.streams.names",
            ),
            (
                serde_json::json!({"streams": {"per_user": 0}}),
                "realtime.streams.per_user",
            ),
            (
                serde_json::json!({"streams": {"lifetime_secs": 0}}),
                "realtime.streams.lifetime_secs",
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
    fn stream_caps_have_defaults_and_read_from_the_environment() {
        let defaults = load(ConfigSources::new()).unwrap();
        assert_eq!(defaults.streams, StreamOptions::default());
        assert_eq!(
            (defaults.streams.names, defaults.streams.per_user),
            (32, 16)
        );
        assert_eq!(
            (defaults.streams.lifetime_secs, defaults.streams.replay),
            (3600, 1024)
        );
        let options = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_REALTIME__STREAMS__PER_USER", "4"),
                ("APP_REALTIME__STREAMS__REPLAY", "0"),
            ],
        ))
        .unwrap();
        assert_eq!(options.streams.per_user, 4);
        assert_eq!(options.streams.replay, 0, "resuming can be turned off");
        assert_eq!(options.streams.names, 32);
    }

    #[test]
    fn a_backplane_needs_a_shared_secret() {
        let defaults = load(ConfigSources::new()).unwrap();
        assert_eq!(defaults.backplane, BackplaneKind::None);

        let error =
            load(ConfigSources::new().set("realtime", serde_json::json!({"backplane": "redis"})))
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

    #[test]
    fn the_section_describes_itself() {
        let schema = RealtimeOptions::schema().expect("schema");
        assert_eq!(schema.key, "realtime");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/digging-deeper/broadcasting/")
        );
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        // The secret is a credential; the key clients connect with is not.
        assert_eq!(schema.secret_paths(), ["realtime.secret"]);
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect(name)
        };
        assert!(field("key").public);
        assert!(!field("key").secret);
        assert_eq!(
            field("allowed_origins").ty,
            FieldType::list(FieldType::String)
        );
        assert_eq!(field("backplane").ty, FieldType::choice(&["none", "redis"]));
        let streams = field("streams");
        assert_eq!(streams.ty, FieldType::Table);
        assert!(
            streams
                .fields
                .iter()
                .any(|field| field.name == "lifetime_secs")
        );
        let defaults = RealtimeOptions::defaults().expect("defaults");
        assert_eq!(defaults["app_id"], "navree");
        assert!(defaults["secret"].is_null());
        assert_eq!(defaults["backplane"], "none");
        assert_eq!(defaults["streams"]["replay"], 1024);
    }

    #[test]
    fn the_secret_is_never_printed() {
        let options = load(
            ConfigSources::new().set("realtime", serde_json::json!({"secret": "hush-hush-value"})),
        )
        .unwrap();
        assert_eq!(options.secret.expose(), "hush-hush-value");
        assert!(!format!("{options:?}").contains("hush-hush-value"));
    }
}
