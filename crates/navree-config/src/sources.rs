//! [`ConfigSources`]: the ordered list of places configuration comes from.

use std::fmt;
use std::path::PathBuf;

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
/// Environment variables (and `.env` entries) map to keys by stripping the
/// prefix, splitting on `__` and lower-casing: `APP_DATABASE__URL` →
/// `database.url`, `APP_LOG__LEVELS__NAVREE_QUEUE` → `log.levels.navree_queue`.
/// Their values are strings; typed sections convert them (`"10"` fills a
/// `u32`, `"true"`/`"yes"`/`"1"` fill a `bool`).
///
/// ```
/// use navree_config::ConfigSources;
///
/// // What `ConfigSources::defaults()` builds, plus a test override:
/// let sources = ConfigSources::new()
///     .dotenv(".env")
///     .file("config/app.toml")
///     .env()
///     .set("database.url", "sqlite://test.db?mode=rwc");
/// ```
#[derive(Clone, Default)]
pub struct ConfigSources {
    pub(crate) sources: Vec<Source>,
    /// `set` values: applied after every source, in call order.
    pub(crate) overrides: Vec<(String, serde_json::Value)>,
}

/// One configuration source.
#[derive(Clone)]
pub(crate) enum Source {
    /// A `.env` file, mapped with the `APP_` prefix.
    Dotenv(PathBuf),
    /// A TOML file.
    File(PathBuf),
    /// The process environment (read at load time), mapped with `prefix`.
    Env { prefix: String },
    /// An explicit environment snapshot, mapped with `prefix`.
    EnvVars {
        prefix: String,
        vars: Vec<(String, String)>,
    },
}

impl ConfigSources {
    /// No sources: an empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// The conventional sources, relative to the working directory: `.env`,
    /// then `config/app.toml`, then `APP_` environment variables.
    pub fn defaults() -> Self {
        Self::defaults_in("")
    }

    /// The conventional sources of the app in `dir`: `dir/.env`, then
    /// `dir/config/app.toml`, then `APP_` environment variables. What
    /// [`defaults`](ConfigSources::defaults) reads, for tools that look at
    /// an app from another directory (Navigator, `navree mcp`).
    pub fn defaults_in(dir: impl AsRef<std::path::Path>) -> Self {
        let dir = dir.as_ref();
        Self::new()
            .dotenv(dir.join(".env"))
            .file(dir.join("config/app.toml"))
            .env()
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

    /// A TOML file; its tables become nested keys as written (keys keep
    /// their case).
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.sources.push(Source::File(path.into()));
        self
    }

    /// The process environment variables whose names start with `prefix`
    /// (ASCII case-insensitively, as Windows treats variable names), read
    /// when the configuration is loaded. `"MYAPP"` and `"MYAPP_"` are the same
    /// prefix; `""` takes every variable. Variables that are not valid
    /// Unicode are skipped.
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
            Source::Env { prefix } => f.debug_struct("Env").field("prefix", prefix).finish(),
            Source::EnvVars { prefix, vars } => f
                .debug_struct("EnvVars")
                .field("prefix", prefix)
                .field("names", &vars.iter().map(|(k, _)| k).collect::<Vec<_>>())
                .finish(),
        }
    }
}

/// `APP` and `APP_` both mean the prefix `APP_`; an empty prefix stays empty.
fn normalize_prefix(prefix: &str) -> String {
    if prefix.is_empty() || prefix.ends_with('_') {
        prefix.to_owned()
    } else {
        format!("{prefix}_")
    }
}
