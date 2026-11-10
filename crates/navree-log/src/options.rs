//! [`LogOptions`]: the `log` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use navree_config::{ConfigSection, ConfigTable};
use navree_contracts::Redactor;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

use crate::LogError;
use crate::filter::Levels;

/// The settings of `LoggerModule`: the `log` configuration section. Every
/// key is optional.
///
/// ```toml
/// [log]
/// format = "json"            # "pretty" by default in debug builds
/// level = "info"
/// levels = { navree_queue = "debug" }
/// file = { path = "storage/logs/navree.log", rotation = "daily", keep_days = 14 }
/// redact = ["password", "token", "ssn"]   # replaces the shared list of secret words
/// slow_query_ms = 200
/// ```
///
/// Environment variables are strings, so `redact` also accepts a
/// comma-separated list (`APP_LOG__REDACT=password,ssn`) and `levels` a
/// directive list (`APP_LOG__LEVELS=navree::request=debug,sqlx=warn`);
/// `APP_LOG__LEVELS__NAVREE_QUEUE=debug` sets a single target. Level and
/// format names are case-insensitive.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "log", docs = "/docs/basics/logging/", validate = Self::check_values)]
#[serde(default)]
pub struct LogOptions {
    /// How lines are written; [`LogFormat::default`] depends on the build.
    #[config(one_of = "pretty, json")]
    pub format: LogFormat,
    /// The level of every target without its own entry in `levels`:
    /// `trace`, `debug`, `info` (default), `warn`, `error` or `off`.
    pub level: String,
    /// Per-target levels; a target also covers its submodules
    /// (`navree_db` covers `navree_db::pool`).
    #[serde(deserialize_with = "levels_map_or_string")]
    pub levels: BTreeMap<String, String>,
    /// A rotating log file written in addition to stdout (none by default).
    #[config(table)]
    pub file: Option<FileLog>,
    /// Field names containing any of these fragments (ASCII
    /// case-insensitively) are written as `[redacted]`. Replaces the
    /// defaults, the shared list of secret words
    /// ([`Redactor::DEFAULT_FRAGMENTS`]).
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub redact: Vec<String>,
    /// Queries slower than this are logged (read by the database module).
    pub slow_query_ms: u64,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            format: LogFormat::default(),
            level: "info".to_owned(),
            levels: BTreeMap::new(),
            file: None,
            redact: Redactor::DEFAULT_FRAGMENTS.map(String::from).to_vec(),
            slow_query_ms: 200,
        }
    }
}

impl LogOptions {
    /// Checks levels, targets and file settings; returns the level filter.
    pub(crate) fn check(&self) -> Result<Levels, LogError> {
        let levels = Levels::new(&self.level, &self.levels)?;
        if let Some(file) = &self.file {
            file.check()?;
        }
        Ok(levels)
    }
}

impl LogOptions {
    /// Checks the values (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map(drop).map_err(|error| error.to_string())
    }
}

/// How log lines are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogFormat {
    /// Human-readable single lines (`timestamp LEVEL target: message key=value`),
    /// coloured when stdout is a terminal and `NO_COLOR` is unset.
    Pretty,
    /// One JSON object per line:
    /// `{"timestamp","level","target","fields","span","spans"}`.
    Json,
}

impl Default for LogFormat {
    /// [`Pretty`](LogFormat::Pretty) in debug builds,
    /// [`Json`](LogFormat::Json) in release builds.
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Pretty
        } else {
            Self::Json
        }
    }
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            _ => Err(format!("unknown log format `{s}`: expected pretty or json")),
        }
    }
}

impl<'de> Deserialize<'de> for LogFormat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// A rotating log file (`tracing-appender`).
///
/// `storage/logs/navree.log` with daily rotation writes
/// `storage/logs/navree.2026-10-25.log`; with [`Rotation::Never`] it writes
/// `storage/logs/navree.log`. Relative paths are relative to the working
/// directory; the directory is created when missing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct FileLog {
    /// The file path; the date is inserted before the extension.
    pub path: PathBuf,
    /// When a new file is started.
    #[config(one_of = "daily, hourly, never")]
    pub rotation: Rotation,
    /// How many days of rotated files are kept (at least 1); older files
    /// are deleted when a new one starts.
    pub keep_days: u32,
}

impl Default for FileLog {
    /// `storage/logs/navree.log`, rotated daily, kept 14 days.
    fn default() -> Self {
        Self {
            path: PathBuf::from("storage/logs/navree.log"),
            rotation: Rotation::Daily,
            keep_days: 14,
        }
    }
}

impl FileLog {
    fn check(&self) -> Result<(), LogError> {
        if self.path.file_name().is_none() {
            return Err(LogError::InvalidFile {
                message: format!("`path` must name a file, got `{}`", self.path.display()),
            });
        }
        if self.keep_days == 0 {
            return Err(LogError::InvalidFile {
                message: "`keep_days` must be at least 1".to_owned(),
            });
        }
        Ok(())
    }
}

/// When a log file is rotated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rotation {
    /// A new file every day (UTC).
    #[default]
    Daily,
    /// A new file every hour.
    Hourly,
    /// A single file, never rotated or deleted.
    Never,
}

impl FromStr for Rotation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "daily" => Ok(Self::Daily),
            "hourly" => Ok(Self::Hourly),
            "never" => Ok(Self::Never),
            _ => Err(format!(
                "unknown log rotation `{s}`: expected daily, hourly or never"
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Rotation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// A `target = level` table, or a `target=level,target=level` string.
fn levels_map_or_string<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct LevelsVisitor;

    impl<'de> Visitor<'de> for LevelsVisitor {
        type Value = BTreeMap<String, String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a table of target = level, or a string like `target=level,other=level`")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            let mut levels = BTreeMap::new();
            for item in s.split(',').map(str::trim).filter(|item| !item.is_empty()) {
                let (target, level) = item
                    .split_once('=')
                    .ok_or_else(|| E::custom(format!("`{item}` is not `target=level`")))?;
                levels.insert(target.trim().to_owned(), level.trim().to_owned());
            }
            Ok(levels)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut levels = BTreeMap::new();
            while let Some((target, level)) = map.next_entry::<String, String>()? {
                levels.insert(target, level);
            }
            Ok(levels)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(BTreeMap::new())
        }
    }

    d.deserialize_any(LevelsVisitor)
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = LogOptions::schema().expect("schema");
        assert_eq!(schema.key, "log");
        assert_eq!(schema.docs.as_deref(), Some("/docs/basics/logging/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(field("format").ty, FieldType::choice(&["pretty", "json"]));
        assert_eq!(field("redact").ty, FieldType::list(FieldType::String));
        assert_eq!(field("file").ty, FieldType::Table);
        let leaves: Vec<String> = schema.leaves().into_iter().map(|(path, _)| path).collect();
        assert!(
            leaves.contains(&"log.file.keep_days".to_owned()),
            "{leaves:?}"
        );
        let defaults = LogOptions::defaults().expect("defaults");
        assert_eq!(defaults["level"], "info");
        assert_eq!(defaults["slow_query_ms"], 200);
    }

    #[test]
    fn redaction_starts_from_the_shared_list_of_secret_words() {
        let defaults = LogOptions::default();
        assert_eq!(defaults.redact, Redactor::DEFAULT_FRAGMENTS);
        let published = LogOptions::defaults().expect("defaults");
        assert_eq!(
            published["redact"].as_array().map(Vec::len),
            Some(Redactor::DEFAULT_FRAGMENTS.len())
        );
    }
}
