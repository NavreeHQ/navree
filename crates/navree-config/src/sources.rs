//! [`ConfigSources`]: the ordered list of places configuration comes from.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::layers::Limits;

/// The prefix of Navree's environment variables (and `.env` entries).
pub(crate) const DEFAULT_ENV_PREFIX: &str = "APP_";

/// The ordered configuration sources of an app, handed to
/// [`ConfigModule::for_root`](crate::ConfigModule::for_root) or
/// [`Config::load`](crate::Config::load).
///
/// Sources are merged in the order they were added, later over earlier:
/// tables merge key by key, any other value (string, number, array) is
/// replaced whole. [`set`](ConfigSources::set) values go on top of every
/// source whatever the call order. Every file source is optional: a missing
/// file is skipped, a malformed one is an error.
///
/// Merging is associative as long as the layers agree on which paths are
/// tables. A layer that turns a table into a scalar or an array, or back,
/// replaces the whole subtree, and the replacement forgets what it replaced:
/// with `x = { p = 1 }`, then `x = 5`, then `x = { q = 2 }`, the result is
/// `x = { q = 2 }`, not `x = { p = 1, q = 2 }`. Layers are always merged
/// left to right, so a load is not affected; this only matters if you merge
/// trees yourself in another order.
///
/// Environment variables (and `.env` entries) map to keys by stripping the
/// prefix, splitting on `__` and lower-casing: `APP_DATABASE__URL` →
/// `database.url`, `APP_LOG__LEVELS__NAVREE_QUEUE` → `log.levels.navree_queue`.
/// Their values are strings; typed sections convert them (`"10"` fills a
/// `u32`, `"true"`/`"yes"`/`"1"` fill a `bool`). Only variables under the
/// prefix are read: the others are never looked at, converted or kept.
///
/// Files are limited in size ([`max_file_size`](ConfigSources::max_file_size),
/// 1 MiB) and in nesting ([`max_depth`](ConfigSources::max_depth), 32
/// levels), and so are environment variable names and `set` keys, so a
/// hostile or broken file cannot exhaust memory or the stack.
///
/// ```
/// use navree_config::ConfigSources;
///
/// // What `ConfigSources::defaults()` builds, plus a test override:
/// let sources = ConfigSources::new()
///     .file("config/app.toml")
///     .dotenv(".env")
///     .env()
///     .set("database.url", "sqlite://test.db?mode=rwc");
/// ```
#[derive(Clone, Default)]
pub struct ConfigSources {
    pub(crate) sources: Vec<Source>,
    /// `set` values: applied after every source, in call order.
    pub(crate) overrides: Vec<(String, serde_json::Value)>,
    max_file_bytes: Option<u64>,
    max_depth: Option<usize>,
}

/// One configuration source.
#[derive(Clone)]
pub(crate) enum Source {
    /// A `.env` file, mapped with the `APP_` prefix.
    Dotenv(PathBuf),
    /// A TOML file.
    File(PathBuf),
    /// A TOML or JSON file whose keys all belong to one section: its top
    /// level is the table at `key`.
    Section { path: PathBuf, key: String },
    /// The process environment (read at load time), mapped with `prefix`.
    Env { prefix: String },
    /// An explicit environment snapshot, mapped with `prefix`.
    EnvVars {
        prefix: String,
        vars: Vec<(String, String)>,
    },
}

impl ConfigSources {
    /// The default [`max_file_size`](ConfigSources::max_file_size): 1 MiB.
    pub const DEFAULT_MAX_FILE_SIZE: u64 = 1024 * 1024;

    /// The default [`max_depth`](ConfigSources::max_depth): 32 levels.
    pub const DEFAULT_MAX_DEPTH: usize = 32;

    /// The greatest [`max_depth`](ConfigSources::max_depth) accepted.
    pub const HARD_MAX_DEPTH: usize = 128;

    /// No sources: an empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `.env` ranks above `config/app.toml` in
    /// [`defaults`](ConfigSources::defaults) (`true`), or below it (`false`).
    /// The real environment ranks above both either way.
    ///
    /// This constant is the one place that sets the order of the two files
    /// in `defaults` and `defaults_in`. The warning about a key that both
    /// set with different values does not depend on it.
    pub const DOTENV_RANKS_ABOVE_FILES: bool = true;

    /// The conventional sources, relative to the working directory, lowest
    /// first:
    ///
    /// 1. `config/app.toml`;
    /// 2. a file per section, `config/<section>.toml` (see
    ///    [`section_file`](ConfigSources::section_file)), in name order;
    /// 3. the file of the environment, `config/env/<env>.toml`, when it
    ///    exists (see [`defaults_in`](ConfigSources::defaults_in));
    /// 4. `.env`;
    /// 5. `APP_` environment variables.
    ///
    /// `.env` ranks above the files, as the order of the two is
    /// [`DOTENV_RANKS_ABOVE_FILES`](ConfigSources::DOTENV_RANKS_ABOVE_FILES).
    pub fn defaults() -> Self {
        Self::defaults_in("")
    }

    /// The conventional sources of the app in `dir`, what
    /// [`defaults`](ConfigSources::defaults) reads, for tools that look at
    /// an app from another directory (Navigator, `navree mcp`).
    ///
    /// The section files are the `*.toml` and `*.json` files in
    /// `dir/config` other than `app`, found when this is called. A file
    /// called `mail.toml` holds the keys of `[mail]` without the header, and
    /// `database.mongodb.toml` those of `[database.mongodb]`. A key set in
    /// both `config/app.toml` and a section file is reported (see
    /// [`Config::shadowed`](crate::Config::shadowed)); the section file wins.
    ///
    /// The environment file `dir/config/env/<env>.toml` is a whole tree laid
    /// over the files above it, so it is the place for what differs in
    /// `production` or `testing`. `<env>` is the first of these that names
    /// one, and `local` when none does: the real `APP_ENV`, `APP_ENV` in
    /// `dir/.env`, `app.env` in `dir/config/app.toml`. A missing file is
    /// skipped.
    pub fn defaults_in(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        let dotenv = dir.join(".env");
        let files = default_files(dir);
        let mut sources = Self::new();
        if Self::DOTENV_RANKS_ABOVE_FILES {
            sources.sources.extend(files);
            sources = sources.dotenv(dotenv);
        } else {
            sources = sources.dotenv(dotenv);
            sources.sources.extend(files);
        }
        sources.env()
    }

    /// A TOML file (or a JSON file, when the name ends in `.json`) whose
    /// keys all belong to the section `key`: with
    /// `section_file("mail", "config/mail.toml")`, `from = "a@b.test"` in the
    /// file is `mail.from`. `key` may be dotted (`database.mongodb`). Like
    /// every file source it is optional.
    pub fn section_file(mut self, key: &str, path: impl Into<PathBuf>) -> Self {
        self.sources.push(Source::Section {
            path: path.into(),
            key: key.to_owned(),
        });
        self
    }

    /// `APP_` environment variables (see [`env_prefixed`](ConfigSources::env_prefixed)).
    pub fn env(self) -> Self {
        self.env_prefixed(DEFAULT_ENV_PREFIX)
    }

    /// A `.env` file. Its entries are mapped like `APP_` environment
    /// variables (entries without the prefix are ignored) and never touch the
    /// process environment. Format: see [`parse_dotenv`](crate::parse_dotenv).
    pub fn dotenv(mut self, path: impl Into<PathBuf>) -> Self {
        self.sources.push(Source::Dotenv(path.into()));
        self
    }

    /// A TOML file, or a JSON file when the name ends in `.json` (any case;
    /// the top level must be an object). Its tables become nested keys as
    /// written (keys keep their case), and a JSON `null` is a value that
    /// replaces whatever an earlier source set there.
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.sources.push(Source::File(path.into()));
        self
    }

    /// The process environment variables whose names start with `prefix`
    /// (ASCII case-insensitively, as Windows treats variable names), read
    /// when the configuration is loaded. `"MYAPP"` and `"MYAPP_"` are the same
    /// prefix; `""` takes every variable, so use it only when you mean that.
    /// Variables outside the prefix are never read into the configuration;
    /// a variable under it whose value is not valid Unicode is skipped, with
    /// a warning that names it. When two names map to the same key (`APP_X`
    /// and `app_x`), the one that sorts last wins, on every run.
    pub fn env_prefixed(mut self, prefix: &str) -> Self {
        self.sources.push(Source::Env {
            prefix: normalize_prefix(prefix),
        });
        self
    }

    /// An explicit set of environment variables, mapped like
    /// [`env_prefixed`](ConfigSources::env_prefixed). Tests use it to
    /// simulate the environment instead of mutating the process environment.
    pub fn env_vars<I, K, V>(mut self, prefix: &str, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.sources.push(Source::EnvVars {
            prefix: normalize_prefix(prefix),
            vars: vars
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        });
        self
    }

    /// Sets the dotted path `key` to `value`, above every other source
    /// whatever the call order (later `set` calls win over earlier ones). An
    /// object merges into the existing table instead of replacing it. Used by
    /// tests and by apps that compute settings in code.
    pub fn set(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.overrides.push((key.to_owned(), value.into()));
        self
    }

    /// The largest configuration file, in bytes (`.env`, TOML or JSON). A
    /// larger file fails the load with
    /// [`ConfigError::TooLarge`](crate::ConfigError::TooLarge) and is never
    /// read whole. The default is [`DEFAULT_MAX_FILE_SIZE`](ConfigSources::DEFAULT_MAX_FILE_SIZE).
    pub fn max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_bytes = Some(bytes);
        self
    }

    /// The deepest nesting of tables and arrays, the root table included
    /// (`a.b.c = 1` is three levels), in files, environment variable names
    /// and [`set`](ConfigSources::set) keys and values. Deeper input fails
    /// the load with [`ConfigError::TooDeep`](crate::ConfigError::TooDeep).
    /// The default is [`DEFAULT_MAX_DEPTH`](ConfigSources::DEFAULT_MAX_DEPTH);
    /// values are clamped to `1..=`[`HARD_MAX_DEPTH`](ConfigSources::HARD_MAX_DEPTH).
    pub fn max_depth(mut self, levels: usize) -> Self {
        self.max_depth = Some(levels.clamp(1, Self::HARD_MAX_DEPTH));
        self
    }

    /// The limits a load of these sources observes.
    pub(crate) fn limits(&self) -> Limits {
        Limits {
            max_file_bytes: self.max_file_bytes.unwrap_or(Self::DEFAULT_MAX_FILE_SIZE),
            max_depth: self.max_depth.unwrap_or(Self::DEFAULT_MAX_DEPTH),
        }
    }
}

/// Shows the sources in order and the `set` keys, never values.
impl fmt::Debug for ConfigSources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigSources")
            .field("sources", &self.sources)
            .field(
                "set",
                &self
                    .overrides
                    .iter()
                    .map(|(key, _)| key)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Dotenv(path) => f.debug_tuple("Dotenv").field(path).finish(),
            Source::File(path) => f.debug_tuple("File").field(path).finish(),
            Source::Section { path, key } => f
                .debug_struct("Section")
                .field("key", key)
                .field("path", path)
                .finish(),
            Source::Env { prefix } => f.debug_struct("Env").field("prefix", prefix).finish(),
            Source::EnvVars { prefix, vars } => f
                .debug_struct("EnvVars")
                .field("prefix", prefix)
                .field("names", &vars.iter().map(|(k, _)| k).collect::<Vec<_>>())
                .finish(),
        }
    }
}

/// The file sources of the conventional setup, lowest first: `app.toml`, the
/// section files, and the file of the environment.
fn default_files(dir: &Path) -> Vec<Source> {
    let config_dir = dir.join("config");
    let mut files = vec![Source::File(dir.join("config/app.toml"))];
    files.extend(
        section_files(&config_dir)
            .into_iter()
            .map(|(key, path)| Source::Section { path, key }),
    );
    if let Some(name) = environment_name(dir) {
        let overlay = config_dir.join("env").join(format!("{name}.toml"));
        if overlay.is_file() {
            files.push(Source::File(overlay));
        }
    }
    files
}

/// The files `config/<section>.toml` and `config/<section>.json` other than
/// `app`, by section key, in name order.
fn section_files(config_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(config_dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            if extension != "toml" && extension != "json" {
                return None;
            }
            let key = path.file_stem()?.to_str()?.to_owned();
            (key != "app" && is_section_key(&key)).then_some((key, path))
        })
        .collect();
    found.sort();
    found
}

/// Whether `key` can be a section key: dotted names of letters, digits, `_`
/// and `-`.
fn is_section_key(key: &str) -> bool {
    !key.is_empty()
        && key.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// The name of the environment the conventional setup loads a file for: the
/// real `APP_ENV`, then `APP_ENV` in `.env`, then `app.env` in
/// `config/app.toml`, else `local`. Lower case; `None` when the name cannot be
/// a file name.
fn environment_name(dir: &Path) -> Option<String> {
    let named = |value: String| Some(value.trim().to_owned()).filter(|value| !value.is_empty());
    let name = std::env::var("APP_ENV")
        .ok()
        .and_then(named)
        .or_else(|| std::env::var("APP_APP__ENV").ok().and_then(named))
        .or_else(|| dotenv_environment(&dir.join(".env")))
        .or_else(|| file_environment(&dir.join("config/app.toml")))
        .unwrap_or_else(|| "local".to_owned())
        .to_ascii_lowercase();
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then_some(name)
}

/// `APP_ENV` (or `APP_APP__ENV`) in a `.env` file; the last entry wins.
fn dotenv_environment(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let entries = crate::parse_dotenv(&text).ok()?;
    entries
        .into_iter()
        .rev()
        .find(|(name, value)| {
            (name == "APP_ENV" || name == "APP_APP__ENV") && !value.trim().is_empty()
        })
        .map(|(_, value)| value)
}

/// `env` (or `app.env`) in a TOML file.
fn file_environment(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table
        .get("env")
        .or_else(|| table.get("app").and_then(|app| app.get("env")))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
}

/// `APP` and `APP_` both mean the prefix `APP_`; an empty prefix stays empty.
fn normalize_prefix(prefix: &str) -> String {
    if prefix.is_empty() || prefix.ends_with('_') {
        prefix.to_owned()
    } else {
        format!("{prefix}_")
    }
}
