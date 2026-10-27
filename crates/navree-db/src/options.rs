//! [`DbOptions`]: the `database` configuration section, with
//! [`MySqlOptions`] (and its [`IsolationLevel`]) for MySQL and MariaDB.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use navree_config::{Config, ConfigError, ConfigSection};
use serde::{Deserialize, Deserializer};

/// The settings of `DatabaseModule`: the `database` configuration section.
/// Every key is optional.
///
/// ```toml
/// [database]
/// url = "postgres://navree:navree@127.0.0.1:5432/my_app"   # TimescaleDB (the default) or PostgreSQL;
///                                                          # mysql://, mariadb:// or sqlite://storage/app.db?mode=rwc
/// max_connections = 10
/// min_connections = 0
/// connect_timeout_secs = 8
/// log_queries = false      # every statement at debug level (target navree_db::query)
/// slow_query_ms = 200      # statements slower than this at warn level; 0 turns it off
/// migrate_on_boot = false  # run pending migrations when the app boots
///
/// [database.mysql]         # MySQL and MariaDB only
/// strict = true            # every connection runs in strict SQL mode
/// # modes = ["STRICT_ALL_TABLES", "NO_ENGINE_SUBSTITUTION"]  # the exact SQL mode instead
/// # isolation_level = "read_committed"                      # the server's (REPEATABLE READ) when unset
/// ```
///
/// When `database.slow_query_ms` is not set, `log.slow_query_ms` (the
/// logger's setting) applies. Environment variables map as usual:
/// `APP_DATABASE__URL` → `database.url`, `APP_DATABASE__MYSQL__STRICT` →
/// `database.mysql.strict`, `APP_DATABASE__MYSQL__MODES=A,B` →
/// `database.mysql.modes`.
///
/// The URL picks the database: `postgres://` or `postgresql://` for
/// TimescaleDB, Navree's default, and plain PostgreSQL (the `postgres`
/// feature), `mysql://` or `mariadb://` (the `mysql` feature; MariaDB speaks
/// MySQL's protocol, so both connect through the same driver), or `sqlite:`
/// (built in). `navree new` writes a TimescaleDB URL, the one Dock's
/// `postgres` service serves; without any, the default is a SQLite file
/// (`sqlite://storage/app.db?mode=rwc`), so an app boots with nothing
/// installed.
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
    /// The connection URL (`postgres://...` for TimescaleDB or PostgreSQL,
    /// `mysql://...`, `mariadb://...` or `sqlite://path?mode=rwc`); default
    /// `sqlite://storage/app.db?mode=rwc`.
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
/// section. They shape the session of every connection the pool opens.
///
/// Every MySQL connection also gets SQLx's session settings: the time zone
/// `+00:00` (so `TIMESTAMP` values read and write as UTC), the `utf8mb4`
/// character set, and `PIPES_AS_CONCAT` and `NO_ENGINE_SUBSTITUTION` added
/// to the SQL mode. URL parameters change those (`?timezone=`,
/// `?charset=`, `?collation=`), along with `?ssl-mode=` and `?socket=`.
///
/// ```
/// use navree_db::{DbOptions, IsolationLevel, MySqlOptions};
///
/// // READ COMMITTED, PostgreSQL's default: InnoDB then locks records but
/// // not the gaps between them, as it does under REPEATABLE READ.
/// let options = DbOptions {
///     url: "mysql://app:secret@127.0.0.1:3306/crm".into(),
///     mysql: MySqlOptions {
///         isolation_level: Some(IsolationLevel::ReadCommitted),
///         ..MySqlOptions::default()
///     },
///     ..DbOptions::default()
/// };
/// assert!(options.mysql.strict);
///
/// // Keep a legacy schema's lenient behaviour.
/// let legacy = MySqlOptions {
///     strict: false,
///     ..MySqlOptions::default()
/// };
/// assert_eq!(legacy.modes, None);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MySqlOptions {
    /// Strict SQL mode for every connection (default `true`): the session
    /// adds `ONLY_FULL_GROUP_BY`, `STRICT_TRANS_TABLES`, `NO_ZERO_IN_DATE`,
    /// `NO_ZERO_DATE` and `ERROR_FOR_DIVISION_BY_ZERO` to the server's SQL
    /// mode, so a value that does not fit its column (a string too long, a
    /// number out of range, a zero date) is an error instead of being
    /// silently cut or replaced, and a query selecting a column it neither
    /// groups by nor aggregates is refused, as on PostgreSQL. `false`
    /// removes `STRICT_TRANS_TABLES` and `STRICT_ALL_TABLES` from the
    /// session instead, whatever the server's default. Ignored when
    /// [`modes`](MySqlOptions::modes) is set.
    pub strict: bool,
    /// The session's exact SQL mode, replacing the server's (and what
    /// `strict` and SQLx would add): `["STRICT_ALL_TABLES",
    /// "NO_ENGINE_SUBSTITUTION"]`, or from the environment a
    /// comma-separated string (`APP_DATABASE__MYSQL__MODES=A,B`). An empty
    /// list clears the SQL mode. `None` (the default) leaves the mode to
    /// [`strict`](MySqlOptions::strict). Each entry must be a mode name:
    /// letters, digits and underscores.
    #[serde(deserialize_with = "modes_from_list_or_string")]
    pub modes: Option<Vec<String>>,
    /// The transaction isolation level of every connection
    /// (`"read_committed"`, `"REPEATABLE READ"`, ...). `None` (the default)
    /// keeps the server's, which is `REPEATABLE READ` unless configured
    /// otherwise.
    pub isolation_level: Option<IsolationLevel>,
}

impl Default for MySqlOptions {
    fn default() -> Self {
        Self {
            strict: true,
            modes: None,
            isolation_level: None,
        }
    }
}

impl MySqlOptions {
    /// What is wrong with these settings, one message per problem.
    fn problems(&self) -> Vec<String> {
        self.modes
            .iter()
            .flatten()
            .filter(|mode| !is_mode_name(mode))
            .map(|mode| {
                format!(
                    "mysql.modes: `{mode}` is not an SQL mode name (letters, digits and underscores, e.g. STRICT_ALL_TABLES)"
                )
            })
            .collect()
    }
}

/// Whether `mode` can be an SQL mode name (`STRICT_TRANS_TABLES`, `DB2`):
/// an ASCII letter, then letters, digits and underscores.
fn is_mode_name(mode: &str) -> bool {
    mode.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && mode.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `modes` as a list (`["A", "B"]`) or a comma-separated string (`"A,B"`,
/// the form an environment variable takes); entries are trimmed and empty
/// ones dropped.
fn modes_from_list_or_string<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Modes {
        List(Vec<String>),
        Text(String),
    }

    let modes = match Option::<Modes>::deserialize(deserializer)? {
        None => return Ok(None),
        Some(Modes::List(list)) => list,
        Some(Modes::Text(text)) => text.split(',').map(str::to_owned).collect(),
    };
    Ok(Some(
        modes
            .iter()
            .map(|mode| mode.trim())
            .filter(|mode| !mode.is_empty())
            .map(str::to_owned)
            .collect(),
    ))
}

/// A transaction isolation level: what one transaction sees of others'
/// writes (`database.mysql.isolation_level`).
///
/// Parsed without regard to case, with words joined by underscores,
/// hyphens or spaces: `"read_committed"`, `"READ-COMMITTED"` and
/// `"READ COMMITTED"` are all [`ReadCommitted`](IsolationLevel::ReadCommitted).
///
/// ```
/// use navree_db::IsolationLevel;
///
/// let level: IsolationLevel = "read committed".parse()?;
/// assert_eq!(level, IsolationLevel::ReadCommitted);
/// assert_eq!(level.as_sql(), "READ COMMITTED");
/// assert!("snapshot".parse::<IsolationLevel>().is_err());
/// # Ok::<(), String>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IsolationLevel {
    /// Reads see uncommitted writes of other transactions (dirty reads).
    ReadUncommitted,
    /// Every statement sees the writes committed before it started.
    ReadCommitted,
    /// Every read of a transaction sees one snapshot, taken at its first
    /// read; locking reads (`FOR UPDATE`) see the latest commits. MySQL's
    /// default.
    RepeatableRead,
    /// Like `RepeatableRead`, with every plain read taking shared locks.
    Serializable,
}

impl IsolationLevel {
    /// The level as SQL spells it: `READ UNCOMMITTED`, `READ COMMITTED`,
    /// `REPEATABLE READ` or `SERIALIZABLE`.
    pub fn as_sql(self) -> &'static str {
        match self {
            IsolationLevel::ReadUncommitted => "READ UNCOMMITTED",
            IsolationLevel::ReadCommitted => "READ COMMITTED",
            IsolationLevel::RepeatableRead => "REPEATABLE READ",
            IsolationLevel::Serializable => "SERIALIZABLE",
        }
    }
}

impl fmt::Display for IsolationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_sql())
    }
}

impl FromStr for IsolationLevel {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let words: Vec<String> = text
            .split([' ', '_', '-'])
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        match words.join("_").as_str() {
            "read_uncommitted" => Ok(IsolationLevel::ReadUncommitted),
            "read_committed" => Ok(IsolationLevel::ReadCommitted),
            "repeatable_read" => Ok(IsolationLevel::RepeatableRead),
            "serializable" => Ok(IsolationLevel::Serializable),
            _ => Err(format!(
                "unknown isolation level `{text}`; expected read_uncommitted, read_committed, repeatable_read or serializable"
            )),
        }
    }
}

impl<'de> Deserialize<'de> for IsolationLevel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
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
        problems.extend(self.mysql.problems());
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
    let valid = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
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
        assert_eq!(options.mysql.modes, None);
        assert_eq!(options.mysql.isolation_level, None);
        assert!(format!("{options:?}").contains("strict: true"));
    }

    fn mysql_section(json: serde_json::Value) -> Result<MySqlOptions, serde_json::Error> {
        serde_json::from_value(json)
    }

    #[test]
    fn mysql_modes_come_as_a_list_or_a_comma_separated_string() {
        let listed = mysql_section(serde_json::json!({
            "modes": ["STRICT_ALL_TABLES", " NO_ENGINE_SUBSTITUTION "]
        }))
        .unwrap();
        assert_eq!(
            listed.modes,
            Some(vec![
                "STRICT_ALL_TABLES".to_owned(),
                "NO_ENGINE_SUBSTITUTION".to_owned()
            ])
        );
        // An environment variable is a string.
        let from_env = mysql_section(serde_json::json!({
            "modes": "STRICT_ALL_TABLES, NO_ENGINE_SUBSTITUTION,"
        }))
        .unwrap();
        assert_eq!(from_env.modes, listed.modes);
        // Empty: clear the SQL mode.
        assert_eq!(
            mysql_section(serde_json::json!({"modes": ""}))
                .unwrap()
                .modes,
            Some(vec![])
        );
        assert_eq!(
            mysql_section(serde_json::json!({"modes": null}))
                .unwrap()
                .modes,
            None
        );
        assert!(mysql_section(serde_json::json!({})).unwrap().strict);
    }

    #[test]
    fn mysql_modes_must_be_mode_names() {
        let options = DbOptions {
            mysql: MySqlOptions {
                modes: Some(vec![
                    "STRICT_ALL_TABLES".to_owned(),
                    "DB2".to_owned(),
                    "NO_ZERO_DATE'; DROP TABLE users; --".to_owned(),
                    "2FAST".to_owned(),
                ]),
                ..MySqlOptions::default()
            },
            ..DbOptions::default()
        };
        let problems = options.validate().unwrap_err();
        assert!(
            problems.contains(
                "mysql.modes: `NO_ZERO_DATE'; DROP TABLE users; --` is not an SQL mode name"
            ),
            "{problems}"
        );
        assert!(
            problems.contains("mysql.modes: `2FAST` is not an SQL mode name"),
            "{problems}"
        );
        assert!(!problems.contains("`DB2`"), "{problems}");
        assert!(!problems.contains("`STRICT_ALL_TABLES`"), "{problems}");
    }

    #[test]
    fn isolation_levels_parse_in_any_spelling() {
        for (text, level) in [
            ("read_uncommitted", IsolationLevel::ReadUncommitted),
            ("READ COMMITTED", IsolationLevel::ReadCommitted),
            ("read-committed", IsolationLevel::ReadCommitted),
            ("Repeatable Read", IsolationLevel::RepeatableRead),
            ("  serializable ", IsolationLevel::Serializable),
        ] {
            assert_eq!(text.parse::<IsolationLevel>(), Ok(level), "{text:?}");
        }
        let error = "snapshot".parse::<IsolationLevel>().unwrap_err();
        assert_eq!(
            error,
            "unknown isolation level `snapshot`; expected read_uncommitted, read_committed, repeatable_read or serializable"
        );
        assert_eq!(
            IsolationLevel::RepeatableRead.to_string(),
            "REPEATABLE READ"
        );
        assert_eq!(IsolationLevel::ReadUncommitted.as_sql(), "READ UNCOMMITTED");
    }

    #[test]
    fn isolation_levels_deserialize_with_a_precise_error() {
        let section = mysql_section(serde_json::json!({"isolation_level": "READ COMMITTED"}));
        assert_eq!(
            section.unwrap().isolation_level,
            Some(IsolationLevel::ReadCommitted)
        );
        let error = mysql_section(serde_json::json!({"isolation_level": "chaos"}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown isolation level `chaos`"), "{error}");
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
