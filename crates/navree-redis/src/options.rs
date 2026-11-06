//! [`RedisOptions`]: the `redis` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use navree_config::{Config, ConfigError, ConfigSection};
use redis::IntoConnectionInfo;
use serde::Deserialize;

/// The name of the connection `redis.url` describes.
pub const DEFAULT_CONNECTION: &str = "default";

/// The environment variable [`RedisOptions::from_config`] (and so
/// `RedisModule::from_config()`) reads when the configuration leaves
/// `redis.url` out. `navree dock` and Harbor set it.
pub const URL_ENV: &str = "REDIS_URL";

/// The settings of `RedisModule`: the `redis` configuration section. Every
/// key is optional.
///
/// ```toml
/// [redis]
/// url = "redis://127.0.0.1:6379"   # rediss:// for TLS; redis://:password@host:6379/2
/// prefix = "navree:"               # starts every key the drivers write
/// connect_timeout_ms = 5000
/// response_timeout_ms = 5000
///
/// [redis.connections.queue]        # a separate server for one driver
/// url = "redis://10.0.0.7:6379"
/// ```
///
/// From the environment: `APP_REDIS__URL`, `APP_REDIS__PREFIX`,
/// `APP_REDIS__CONNECTIONS__QUEUE__URL`. When the configuration leaves
/// `redis.url` out, [`from_config`](RedisOptions::from_config) takes it from
/// `REDIS_URL` ([`URL_ENV`]). `Debug` masks URL passwords.
#[derive(Clone, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "redis", docs = "/docs/digging-deeper/redis/", validate = Self::check_values)]
#[serde(default)]
pub struct RedisOptions {
    /// The default connection's URL (default `redis://127.0.0.1:6379`):
    /// `redis://[user][:password@]host[:port][/db]`, `rediss://…` for TLS
    /// (rustls, the system's root certificates) or `unix:///path/to/redis.sock`.
    /// When nothing sets it, the `REDIS_URL` environment variable does.
    #[config(env_fallback = "REDIS_URL")]
    pub url: String,
    /// Starts every key the Redis drivers write (default `navree:`). Give
    /// each app sharing a Redis server its own prefix.
    pub prefix: String,
    /// How long opening a connection may take, in milliseconds (default
    /// 5000).
    pub connect_timeout_ms: u64,
    /// How long a command may wait for its answer, in milliseconds (default
    /// 5000). Dedicated connections ([`RedisConnection::dedicated`](crate::RedisConnection::dedicated))
    /// have no response timeout, for blocking commands.
    pub response_timeout_ms: u64,
    /// More connections, by name. A driver uses the connection named after
    /// it when there is one (`cache`, which also holds the locks, `queue`,
    /// `session`, `realtime` and `rate_limiter`), the default one otherwise.
    pub connections: BTreeMap<String, RedisConnectionOptions>,
}

/// One named connection: `redis.connections.<name>`.
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RedisConnectionOptions {
    /// The connection's URL, like [`RedisOptions::url`].
    pub url: String,
}

impl Default for RedisOptions {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".to_owned(),
            prefix: "navree:".to_owned(),
            connect_timeout_ms: 5000,
            response_timeout_ms: 5000,
            connections: BTreeMap::new(),
        }
    }
}

impl RedisOptions {
    /// The `redis` section of `config`, validated, with `redis.url` taken
    /// from the `REDIS_URL` environment variable ([`URL_ENV`]) when the
    /// configuration leaves it out. A configured `redis.url` (in a file or
    /// as `APP_REDIS__URL`) wins; a blank `REDIS_URL` is ignored. This is
    /// what `RedisModule::from_config()` reads at boot.
    pub fn from_config(config: &Config) -> Result<RedisOptions, ConfigError> {
        Self::from_config_with_env(config, |name| std::env::var(name).ok())
    }

    /// [`from_config`](RedisOptions::from_config) with `env` in place of the
    /// process environment.
    pub fn from_config_with_env(
        config: &Config,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<RedisOptions, ConfigError> {
        let mut options: RedisOptions = config.typed()?;
        if config.get_string("redis.url").is_none()
            && let Some(url) = env(URL_ENV).filter(|url| !url.trim().is_empty())
        {
            if let Some(problem) = url_problem(&url) {
                return Err(ConfigError::Validation {
                    key: <Self as ConfigSection>::KEY.to_owned(),
                    message: format!("{URL_ENV} {problem}"),
                });
            }
            options.url = url.trim().to_owned();
        }
        Ok(options)
    }

    /// [`connect_timeout_ms`](RedisOptions::connect_timeout_ms) as a `Duration`.
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    /// [`response_timeout_ms`](RedisOptions::response_timeout_ms) as a `Duration`.
    pub fn response_timeout(&self) -> Duration {
        Duration::from_millis(self.response_timeout_ms)
    }

    /// The default URL with its password (if any) replaced by `***`, safe
    /// to log.
    pub fn redacted_url(&self) -> String {
        redact_url(&self.url)
    }

    /// Every problem with these options (empty when valid). Messages name
    /// the key and never echo a URL, which may hold a password.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if let Some(problem) = url_problem(&self.url) {
            errors.push(format!("redis.url {problem}"));
        }
        if self.connect_timeout_ms == 0 {
            errors.push("redis.connect_timeout_ms must be at least 1".to_owned());
        }
        if self.response_timeout_ms == 0 {
            errors.push("redis.response_timeout_ms must be at least 1".to_owned());
        }
        for (name, connection) in &self.connections {
            if name == DEFAULT_CONNECTION {
                errors.push(
                    "redis.connections.default: `default` is the connection of redis.url; pick another name"
                        .to_owned(),
                );
            } else if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            {
                errors.push(format!(
                    "redis.connections: `{name}` is not a connection name (lowercase letters, digits, `_` and `-`)"
                ));
            }
            if let Some(problem) = url_problem(&connection.url) {
                errors.push(format!("redis.connections.{name}.url {problem}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// [`check`](RedisOptions::check) with the problems joined (the boot
    /// calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// Why `url` is not a Redis URL, without echoing it.
fn url_problem(url: &str) -> Option<&'static str> {
    if url.trim().is_empty() {
        return Some("must not be empty");
    }
    match url.trim().into_connection_info() {
        Ok(_) => None,
        Err(_) => {
            Some("is not a Redis URL (redis://host:port/db, rediss:// for TLS, or unix:///path)")
        }
    }
}

/// `url` with the password of `scheme://user:password@host` replaced by
/// `***`.
pub(crate) fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((credentials, host)) => match credentials.split_once(':') {
            Some((user, _password)) => format!("{scheme}://{user}:***@{host}{path}"),
            None => format!("{scheme}://***@{host}{path}"),
        },
        None => url.to_owned(),
    }
}

impl fmt::Debug for RedisOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisOptions")
            .field("url", &self.redacted_url())
            .field("prefix", &self.prefix)
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .field("response_timeout_ms", &self.response_timeout_ms)
            .field("connections", &self.connections)
            .finish()
    }
}

impl fmt::Debug for RedisConnectionOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisConnectionOptions")
            .field("url", &redact_url(&self.url))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<RedisOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<RedisOptions>()
    }

    #[test]
    fn defaults_point_at_a_local_server() {
        let options = load(ConfigSources::new()).unwrap();
        assert_eq!(options, RedisOptions::default());
        assert_eq!(options.url, "redis://127.0.0.1:6379");
        assert_eq!(options.prefix, "navree:");
        assert_eq!(options.connect_timeout(), Duration::from_secs(5));
        assert_eq!(options.response_timeout(), Duration::from_secs(5));
        assert!(options.connections.is_empty());
    }

    #[test]
    fn the_section_and_environment_variables_are_read() {
        let options = load(
            ConfigSources::new()
                .set("redis", json!({ "prefix": "crm:" }))
                .env_vars(
                    "APP_",
                    [
                        (
                            "APP_REDIS__URL",
                            "rediss://default:s3cret@cache.internal:6380/2",
                        ),
                        ("APP_REDIS__RESPONSE_TIMEOUT_MS", "250"),
                        (
                            "APP_REDIS__CONNECTIONS__QUEUE__URL",
                            "redis://10.0.0.7:6379",
                        ),
                    ],
                ),
        )
        .unwrap();
        assert_eq!(options.prefix, "crm:");
        assert_eq!(options.response_timeout_ms, 250);
        assert_eq!(options.connections["queue"].url, "redis://10.0.0.7:6379");
        let debug = format!("{options:?}");
        assert!(!debug.contains("s3cret"), "{debug}");
        assert!(
            debug.contains("rediss://default:***@cache.internal:6380/2"),
            "{debug}"
        );
    }

    #[test]
    fn invalid_options_name_the_key_and_never_the_url() {
        for (section, expected) in [
            (json!({ "url": "" }), "redis.url must not be empty"),
            (
                json!({ "url": "http://:pw@x" }),
                "redis.url is not a Redis URL",
            ),
            (
                json!({ "connect_timeout_ms": 0 }),
                "redis.connect_timeout_ms",
            ),
            (
                json!({ "response_timeout_ms": 0 }),
                "redis.response_timeout_ms",
            ),
            (
                json!({ "connections": { "default": { "url": "redis://x" } } }),
                "`default` is the connection of redis.url",
            ),
            (
                json!({ "connections": { "Jobs": { "url": "redis://x" } } }),
                "`Jobs` is not a connection name",
            ),
            (
                json!({ "connections": { "jobs": { "url": "nope:pw" } } }),
                "redis.connections.jobs.url is not a Redis URL",
            ),
        ] {
            let error = load(ConfigSources::new().set("redis", section.clone()))
                .expect_err(&section.to_string())
                .to_string();
            assert!(error.contains(expected), "{section}: {error}");
            assert!(!error.contains("pw"), "{error}");
        }
    }

    /// An environment holding only `REDIS_URL = value`.
    fn redis_url(value: &'static str) -> impl Fn(&str) -> Option<String> {
        move |name| (name == URL_ENV).then(|| value.to_owned())
    }

    fn config(sources: ConfigSources) -> Config {
        Config::load(&sources).unwrap()
    }

    #[test]
    fn redis_url_fills_a_url_the_configuration_leaves_out() {
        let options = RedisOptions::from_config_with_env(
            &config(ConfigSources::new().set("redis", json!({ "prefix": "crm:" }))),
            redis_url("redis://:s3cret@10.0.0.5:6379/0"),
        )
        .unwrap();
        assert_eq!(URL_ENV, "REDIS_URL");
        assert_eq!(options.url, "redis://:s3cret@10.0.0.5:6379/0");
        assert_eq!(options.prefix, "crm:");

        let options = RedisOptions::from_config_with_env(
            &config(ConfigSources::new()),
            redis_url("redis://dock:6380"),
        )
        .unwrap();
        assert_eq!(options.url, "redis://dock:6380");
    }

    #[test]
    fn a_configured_url_wins_over_redis_url() {
        for sources in [
            ConfigSources::new().set("redis", json!({ "url": "redis://from-toml:6379" })),
            ConfigSources::new().env_vars("APP_", [("APP_REDIS__URL", "redis://from-toml:6379")]),
        ] {
            let options = RedisOptions::from_config_with_env(
                &config(sources),
                redis_url("redis://dock:6380"),
            )
            .unwrap();
            assert_eq!(options.url, "redis://from-toml:6379");
        }
    }

    #[test]
    fn without_redis_url_the_default_applies() {
        for env in [redis_url(""), redis_url("   ")] {
            let options =
                RedisOptions::from_config_with_env(&config(ConfigSources::new()), env).unwrap();
            assert_eq!(options, RedisOptions::default());
        }
        let options =
            RedisOptions::from_config_with_env(&config(ConfigSources::new()), |_| None).unwrap();
        assert_eq!(options, RedisOptions::default());
    }

    #[test]
    fn an_invalid_redis_url_names_the_variable_and_never_the_value() {
        let error = RedisOptions::from_config_with_env(
            &config(ConfigSources::new()),
            redis_url("http://:pw@x"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("REDIS_URL is not a Redis URL"), "{error}");
        assert!(!error.contains("pw"), "{error}");
    }

    #[test]
    fn the_section_is_still_validated() {
        let error = RedisOptions::from_config_with_env(
            &config(ConfigSources::new().set("redis", json!({ "connect_timeout_ms": 0 }))),
            redis_url("redis://dock:6380"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("redis.connect_timeout_ms"), "{error}");
    }

    #[test]
    fn urls_are_redacted() {
        assert_eq!(
            redact_url("redis://user:pw@host:6379/1"),
            "redis://user:***@host:6379/1"
        );
        assert_eq!(redact_url("redis://:pw@host"), "redis://:***@host");
        assert_eq!(redact_url("rediss://token@host"), "rediss://***@host");
        assert_eq!(redact_url("redis://host:6379"), "redis://host:6379");
        assert_eq!(
            redact_url("unix:///tmp/redis.sock"),
            "unix:///tmp/redis.sock"
        );
        assert_eq!(redact_url("not a url"), "not a url");
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = RedisOptions::schema().expect("schema");
        assert_eq!(schema.key, "redis");
        assert_eq!(schema.docs.as_deref(), Some("/docs/digging-deeper/redis/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        for name in [
            "url",
            "prefix",
            "connect_timeout_ms",
            "response_timeout_ms",
            "connections",
        ] {
            assert!(
                schema.fields.iter().any(|field| field.name == name),
                "{name}"
            );
        }
        // `REDIS_URL` is listed as the fallback of `redis.url`, and only that.
        let fallbacks = schema.env_fallbacks();
        assert_eq!(fallbacks.len(), 1, "{fallbacks:?}");
        assert_eq!(fallbacks[0].key, "redis.url");
        assert_eq!(fallbacks[0].variable, URL_ENV);
        let defaults = RedisOptions::defaults().expect("defaults");
        assert_eq!(defaults["url"], "redis://127.0.0.1:6379");
        assert_eq!(defaults["connect_timeout_ms"], 5000);
    }
}
