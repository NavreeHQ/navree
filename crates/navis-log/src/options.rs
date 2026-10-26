//! [`LogOptions`]: the `log` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use navis_config::ConfigSection;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::LogError;
use crate::filter::Levels;

/// The settings of `LoggerModule`: the `log` configuration section. Every
/// key is optional.
///
/// ```toml
/// [log]
/// format = "json"            # "pretty" by default in debug builds
/// level = "info"
/// levels = { navis_queue = "debug" }
/// file = { path = "storage/logs/navis.log", rotation = "daily", keep_days = 14 }
/// redact = ["password", "token", "authorization", "cookie", "secret"]
/// slow_query_ms = 200
/// ```
///
/// Environment variables are strings, so `redact` also accepts a
/// comma-separated list (`APP_LOG__REDACT=password,ssn`) and `levels` a
/// directive list (`APP_LOG__LEVELS=navis::request=debug,sqlx=warn`);
/// `APP_LOG__LEVELS__NAVIS_QUEUE=debug` sets a single target. Level and
/// format names are case-insensitive.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct LogOptions {
    /// How lines are written; [`LogFormat::default`] depends on the build.
    pub format: LogFormat,
    /// The level of every target without its own entry in `levels`:
    /// `trace`, `debug`, `info` (default), `warn`, `error` or `off`.
    pub level: String,
    /// Per-target levels; a target also covers its submodules
    /// (`navis_db` covers `navis_db::pool`).
    #[serde(deserialize_with = "levels_map_or_string")]
    pub levels: BTreeMap<String, String>,
    /// A rotating log file written in addition to stdout (none by default).
    pub file: Option<FileLog>,
    /// Field names containing any of these fragments (ASCII
    /// case-insensitively) are written as `[redacted]`. Replaces the
    /// defaults: `password`, `token`, `authorization`, `cookie`, `secret`.
    #[serde(deserialize_with = "list_or_comma_string")]
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
            redact: ["password", "token", "authorization", "cookie", "secret"]
                .map(String::from)
                .to_vec(),
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

impl ConfigSection for LogOptions {
    const KEY: &'static str = "log";

    fn validate(&self) -> Result<(), String> {
        self.check().map(drop).map_err(|error| error.to_string())
    }
}

/// How log lines are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// `storage/logs/navis.log` with daily rotation writes
/// `storage/logs/navis.2026-10-25.log`; with [`Rotation::Never`] it writes
/// `storage/logs/navis.log`. Relative paths are relative to the working
/// directory; the directory is created when missing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct FileLog {
    /// The file path; the date is inserted before the extension.
    pub path: PathBuf,
    /// When a new file is started.
    pub rotation: Rotation,
    /// How many days of rotated files are kept (at least 1); older files
    /// are deleted when a new one starts.
    pub keep_days: u32,
}

impl Default for FileLog {
    /// `storage/logs/navis.log`, rotated daily, kept 14 days.
    fn default() -> Self {
        Self {
            path: PathBuf::from("storage/logs/navis.log"),
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

/// A list of strings, or one comma-separated string (blank items dropped).
fn list_or_comma_string<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    struct ListVisitor;

    impl<'de> Visitor<'de> for ListVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list of strings or a comma-separated string")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            Ok(s.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect())
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                items.push(item);
            }
            Ok(items)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
    }

    d.deserialize_any(ListVisitor)
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
