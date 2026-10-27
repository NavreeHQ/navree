//! [`DbOptions`]: the `database` configuration section, with
//! [`MySqlOptions`] for MySQL and MariaDB.

use std::borrow::Cow;
use std::fmt;

use navree_config::{Config, ConfigError, ConfigSection};
use serde::Deserialize;

/// The settings of `DatabaseModule`: the `database` configuration section.
/// Every key is optional.
///
/// ```toml
/// [database]
/// url = "sqlite://storage/app.db?mode=rwc"   # or postgres://, mysql:// or mariadb://user:pass@host/db
/// max_connections = 10
/// min_connections = 0
/// connect_timeout_secs = 8
/// log_queries = false      # every statement at debug level (target navree_db::query)
/// slow_query_ms = 200      # statements slower than this at warn level; 0 turns it off
/// migrate_on_boot = false  # run pending migrations when the app boots
///
/// [database.mysql]         # MySQL and MariaDB only
/// strict = true            # every connection runs in strict SQL mode
/// ```
///
/// When `database.slow_query_ms` is not set, `log.slow_query_ms` (the
/// logger's setting) applies. Environment variables map as usual:
/// `APP_DATABASE__URL` → `database.url`, `APP_DATABASE__MYSQL__STRICT` →
/// `database.mysql.strict`.
///
/// The URL picks the database: `sqlite:` (built in), `postgres://` or
/// `postgresql://` (the `postgres` feature), `mysql://` or `mariadb://`
/// (the `mysql` feature; MariaDB speaks MySQL's protocol, so both connect
/// through the same driver).
///
/// `Debug` masks the password of the URL.
///
/// ```
/// use navree_db::DbOptions;
///
/// let options = DbOptions {
///     url: "mysql://crm:secret@db.internal:3306/crm".into(),
///     ..DbOptions::default()
/// };
/// assert!(options.mysql.strict);
/// assert_eq!(options.redacted_url(), "mysql://crm:***@db.internal:3306/crm");
/// ```
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DbOptions {
    /// The connection URL (`sqlite://path?mode=rwc`, `postgres://...`,
    /// `mysql://...` or `mariadb://...`).
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
    /// MySQL and MariaDB settings (`database.mysql`); other databases
    /// ignore them.
    pub mysql: MySqlOptions,
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
            mysql: MySqlOptions::default(),
        }
    }
}

/// The MySQL and MariaDB settings of [`DbOptions`]: the `database.mysql`
/// section.
///
/// Every MySQL connection also gets SQLx's session settings: the time zone
/// `+00:00` (so `TIMESTAMP` values read and write as UTC), the `utf8mb4`
/// character set, and `PIPES_AS_CONCAT` and `NO_ENGINE_SUBSTITUTION` added
/// to the SQL mode. URL parameters change those (`?timezone=`,
/// `?charset=`, `?collation=`), along with `?ssl-mode=` and `?socket=`.
///
/// ```
/// use navree_db::{DbOptions, MySqlOptions};
///
/// // Keep a legacy schema's lenient behaviour.
/// let options = DbOptions {
///     url: "mysql://app:secret@127.0.0.1:3306/legacy".into(),
///     mysql: MySqlOptions { strict: false },
///     ..DbOptions::default()
/// };
/// assert!(!options.mysql.strict);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MySqlOptions {
    /// Strict SQL mode for every connection (default `true`): the session
    /// adds `STRICT_TRANS_TABLES`, `NO_ZERO_IN_DATE`, `NO_ZERO_DATE` and
    /// `ERROR_FOR_DIVISION_BY_ZERO` to the server's SQL mode, so a value
    /// that does not fit its column (a string too long, a number out of
    /// range, a zero date) is an error instead of being silently cut or
    /// replaced. `false` removes `STRICT_TRANS_TABLES` and
    /// `STRICT_ALL_TABLES` from the session instead, whatever the server's
    /// default.
    pub strict: bool,
}

impl Default for MySqlOptions {
    fn default() -> Self {
        Self { strict: true }
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
            .field("mysql", &self.mysql)
            .finish()
    }
}

/// The URL's scheme, lower-cased (`mysql`, `mariadb`, `postgres`,
/// `sqlite`, ...), if it has one.
pub(crate) fn scheme(url: &str) -> Option<Cow<'_, str>> {
    let (scheme, _) = url.split_once(':')?;
    let valid = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return None;
    }
    Some(if scheme.chars().any(|c| c.is_ascii_uppercase()) {
        Cow::Owned(scheme.to_ascii_lowercase())
    } else {
        Cow::Borrowed(scheme)
    })
}

/// Whether `url` names a MySQL or MariaDB server.
#[cfg(any(feature = "mysql", test))]
pub(crate) fn is_mysql_url(url: &str) -> bool {
    matches!(scheme(url).as_deref(), Some("mysql" | "mariadb"))
}

/// The URL the driver connects with: a `mariadb://` URL becomes
/// `mysql://` (MariaDB speaks MySQL's protocol); every other URL is used as
/// it is.
pub(crate) fn driver_url(url: &str) -> Cow<'_, str> {
    match scheme(url).as_deref() {
        Some("mariadb") => Cow::Owned(format!("mysql{}", &url["mariadb".len()..])),
        _ => Cow::Borrowed(url),
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
            redact_url("postgres://navree:hunter2@db:5432/app?sslmode=require"),
            "postgres://navree:***@db:5432/app?sslmode=require"
        );
        assert_eq!(
            redact_url("postgres://navree@db/app"),
            "postgres://navree@db/app"
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
    fn redacts_mysql_and_mariadb_passwords() {
        assert_eq!(
            redact_url("mysql://navree:hunter2@db:3306/app?ssl-mode=REQUIRED"),
            "mysql://navree:***@db:3306/app?ssl-mode=REQUIRED"
        );
        assert_eq!(
            redact_url("mariadb://navree:hunter2@db/app"),
            "mariadb://navree:***@db/app"
        );
        assert_eq!(url_password("mysql://root@db/app"), None);
    }

    #[test]
    fn mariadb_urls_connect_through_the_mysql_driver() {
        assert_eq!(
            driver_url("mariadb://u:p@db:3306/app?ssl-mode=REQUIRED"),
            "mysql://u:p@db:3306/app?ssl-mode=REQUIRED"
        );
        assert_eq!(driver_url("MariaDB://u@db/app"), "mysql://u@db/app");
        for unchanged in [
            "mysql://u:p@db/app",
            "postgres://u:p@db/app",
            "sqlite://storage/app.db?mode=rwc",
            "mariadb-not-a-scheme",
        ] {
            assert_eq!(driver_url(unchanged), unchanged);
        }
    }

    #[test]
    fn url_schemes_name_the_database() {
        assert_eq!(scheme("mysql://u@db/app").as_deref(), Some("mysql"));
        assert_eq!(scheme("MariaDB://u@db/app").as_deref(), Some("mariadb"));
        assert_eq!(
            scheme("postgresql://u@db/app").as_deref(),
            Some("postgresql")
        );
        assert_eq!(scheme("sqlite::memory:").as_deref(), Some("sqlite"));
        assert_eq!(scheme("no scheme here").as_deref(), None);
        assert_eq!(scheme("://nothing").as_deref(), None);
        assert!(is_mysql_url("mysql://u@db/app"));
        assert!(is_mysql_url("mariadb://u@db/app"));
        assert!(!is_mysql_url("postgres://u@db/app"));
    }

    #[test]
    fn mysql_connections_are_strict_by_default() {
        let options = DbOptions::default();
        assert!(options.mysql.strict);
        assert!(format!("{options:?}").contains("strict: true"));
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
