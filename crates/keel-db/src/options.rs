//! [`DbOptions`]: the `database` configuration section.

use std::fmt;

use keel_config::{Config, ConfigError, ConfigSection};
use serde::Deserialize;

/// The settings of `DatabaseModule`: the `database` configuration section.
/// Every key is optional.
///
/// ```toml
/// [database]
/// url = "sqlite://storage/app.db?mode=rwc"   # or postgres://user:pass@host/db
/// max_connections = 10
/// min_connections = 0
/// connect_timeout_secs = 8
/// log_queries = false      # every statement at debug level (target keel_db::query)
/// slow_query_ms = 200      # statements slower than this at warn level; 0 turns it off
/// migrate_on_boot = false  # run pending migrations when the app boots
/// ```
///
/// When `database.slow_query_ms` is not set, `log.slow_query_ms` (the
/// logger's setting) applies. Environment variables map as usual:
/// `APP_DATABASE__URL` → `database.url`.
///
/// `Debug` masks the password of the URL.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DbOptions {
    /// The connection URL (`sqlite://path?mode=rwc`, `postgres://...`).
    pub url: String,
    /// The most pooled connections.
    pub max_connections: u32,
    /// The fewest pooled connections kept open.
    pub min_connections: u32,
    /// How long to wait for a new connection, in seconds.
    pub connect_timeout_secs: u64,
    /// Logs every statement (SQL without bound values) at debug level.
    pub log_queries: bool,
    /// Statements slower than this are logged at warn level (0 = never).
    pub slow_query_ms: u64,
    /// Runs pending migrations when the app boots (tests and development).
    pub migrate_on_boot: bool,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self {
            url: "sqlite://storage/app.db?mode=rwc".to_owned(),
            max_connections: 10,
            min_connections: 0,
            connect_timeout_secs: 8,
            log_queries: false,
            slow_query_ms: 200,
            migrate_on_boot: false,
        }
    }
}

impl DbOptions {
    /// Reads the `database` section of `config` and validates it. Without
    /// `database.slow_query_ms`, the logger's `log.slow_query_ms` applies.
    pub fn from_config(config: &Config) -> Result<Self, ConfigError> {
        let mut options = config.typed::<DbOptions>()?;
        if config.get_string("database.slow_query_ms").is_none()
            && let Some(ms) = config
                .get_string("log.slow_query_ms")
                .and_then(|v| v.trim().parse::<u64>().ok())
        {
            options.slow_query_ms = ms;
        }
        Ok(options)
    }

    /// The URL with its password (if any) replaced by `***`, safe to log.
    pub fn redacted_url(&self) -> String {
        redact_url(&self.url)
    }
}

impl ConfigSection for DbOptions {
    const KEY: &'static str = "database";

    fn validate(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if self.url.trim().is_empty() {
            problems.push("url must not be empty".to_owned());
        }
        if self.max_connections == 0 {
            problems.push("max_connections must be at least 1".to_owned());
        }
        if self.min_connections > self.max_connections {
            problems.push("min_connections must not exceed max_connections".to_owned());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

impl fmt::Debug for DbOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbOptions")
            .field("url", &self.redacted_url())
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("log_queries", &self.log_queries)
            .field("slow_query_ms", &self.slow_query_ms)
            .field("migrate_on_boot", &self.migrate_on_boot)
            .finish()
    }
}

/// `scheme://user:secret@host/db` → `scheme://user:***@host/db`.
pub(crate) fn redact_url(url: &str) -> String {
    match password_range(url) {
        Some((start, end)) => format!("{}***{}", &url[..start], &url[end..]),
        None => url.to_owned(),
    }
}

/// The password of `scheme://user:password@host/...`, if any.
pub(crate) fn url_password(url: &str) -> Option<&str> {
    password_range(url)
        .map(|(start, end)| &url[start..end])
        .filter(|password| !password.is_empty())
}

/// The byte range of the password in `scheme://user:password@host/...`.
fn password_range(url: &str) -> Option<(usize, usize)> {
    let authority_start = url.find("://")? + 3;
    let rest = &url[authority_start..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let at = authority.rfind('@')?;
    let colon = authority[..at].find(':')?;
    Some((authority_start + colon + 1, authority_start + at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_url_passwords() {
        assert_eq!(
            redact_url("postgres://keel:hunter2@db:5432/app?sslmode=require"),
            "postgres://keel:***@db:5432/app?sslmode=require"
        );
        assert_eq!(
            redact_url("postgres://keel@db/app"),
            "postgres://keel@db/app"
        );
        assert_eq!(
            redact_url("sqlite://storage/app.db?mode=rwc"),
            "sqlite://storage/app.db?mode=rwc"
        );
        let options = DbOptions {
            url: "postgres://u:topsecret@h/d".into(),
            ..DbOptions::default()
        };
        assert!(!format!("{options:?}").contains("topsecret"));
        assert_eq!(url_password(&options.url), Some("topsecret"));
        assert_eq!(url_password("postgres://u@h/d"), None);
    }

    #[test]
    fn validation_reports_bad_pool_sizes() {
        let options = DbOptions {
            max_connections: 0,
            ..DbOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("max_connections"));
        let options = DbOptions {
            min_connections: 5,
            max_connections: 2,
            ..DbOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("min_connections"));
    }
}
