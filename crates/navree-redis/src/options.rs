//! [`RedisOptions`]: the `redis` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use navree_config::ConfigSection;
use redis::IntoConnectionInfo;
use serde::Deserialize;

/// The name of the connection `redis.url` describes.
pub const DEFAULT_CONNECTION: &str = "default";

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
/// `APP_REDIS__CONNECTIONS__QUEUE__URL`. `Debug` masks URL passwords.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RedisOptions {
    /// The default connection's URL (default `redis://127.0.0.1:6379`):
    /// `redis://[user][:password@]host[:port][/db]`, `rediss://…` for TLS
    /// (rustls, the system's root certificates) or `unix:///path/to/redis.sock`.
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
}

impl ConfigSection for RedisOptions {
    const KEY: &'static str = "redis";

    fn validate(&self) -> Result<(), String> {
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
}
