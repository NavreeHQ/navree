//! [`Config`]: the merged configuration tree.

use std::fmt;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::de::{self, DeError, Sink};
use crate::layers::APP_ALIASES;
use crate::origin::{Shadowed, ValueOrigin};
use crate::redact::{is_secret_key, redacted_json, scrub, secret_values};
use crate::usage::{UnusedKey, Usage};
use crate::value::{Kind, Value};
use crate::{ConfigError, ConfigSection, ConfigSources, layers};

/// The merged configuration of an app: one tree built from
/// [`ConfigSources`], read through typed sections.
///
/// [`ConfigModule`](crate::ConfigModule) loads it at boot and exports it
/// globally as a singleton, so providers resolve `Arc<Config>`; most modules
/// use [`ModuleBuilderConfigExt::config`](crate::ModuleBuilderConfigExt::config)
/// instead. Keys are dotted paths (`"database.url"`). Values convert on
/// demand: a section field of type `u16` accepts `8080` from TOML and `"8080"`
/// from the environment.
///
/// `Debug` prints the [redacted](Config::redacted) tree. Cloning is cheap.
///
/// ```
/// use navree_config::{Config, ConfigSources};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Server {
///     port: u16,
/// }
///
/// let config = Config::load(
///     &ConfigSources::new().env_vars("APP_", [("APP_SERVER__PORT", "8080")]),
/// )?;
/// assert_eq!(config.section::<Server>("server")?.port, 8080);
/// assert_eq!(config.get_string("server.port").as_deref(), Some("8080"));
/// # Ok::<(), navree_config::ConfigError>(())
/// ```
#[derive(Clone)]
pub struct Config {
    /// The merged tree; always a table.
    root: Arc<Value>,
    /// The `app` table with the short names of its settings (`APP_URL`)
    /// resolved into it; reads of `app` and `app.*` go through it.
    app: Option<Arc<Value>>,
    /// The `.env` values that a config file hides.
    shadows: Arc<Vec<Shadowed>>,
    /// Which keys have been read so far.
    usage: Arc<Usage>,
}

impl Config {
    /// Loads and merges `sources` (see [`ConfigSources`] for the rules).
    ///
    /// This reads files and the process environment: call it at startup or
    /// on a blocking thread, not on an async worker.
    pub fn load(sources: &ConfigSources) -> Result<Config, ConfigError> {
        let loaded = layers::load(sources)?;
        Ok(Config {
            root: Arc::new(loaded.root),
            app: loaded.app.map(Arc::new),
            shadows: Arc::new(loaded.shadows),
            usage: Arc::new(Usage::default()),
        })
    }

    /// Deserializes the section at dotted path `key` (`""` is the whole tree).
    /// A missing section is [`ConfigError::MissingSection`].
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        match self.lookup(key) {
            Some(value) => self.read(key, value),
            None => Err(ConfigError::MissingSection {
                key: key.to_owned(),
            }),
        }
    }

    /// Deserializes the section at `key`, or `T::default()` when it is absent.
    pub fn section_or_default<T: DeserializeOwned + Default>(
        &self,
        key: &str,
    ) -> Result<T, ConfigError> {
        match self.lookup(key) {
            Some(value) => self.read(key, value),
            None => Ok(T::default()),
        }
    }

    /// Reads section [`T::KEY`](ConfigSection::KEY) (an empty table when
    /// absent, so `#[serde(default)]` fields apply) and validates it. This is
    /// what [`ModuleBuilderConfigExt::config`](crate::ModuleBuilderConfigExt::config)
    /// runs at boot.
    pub fn typed<T: ConfigSection>(&self) -> Result<T, ConfigError> {
        let section: T = match self.lookup(T::KEY) {
            Some(value) => self.read(T::KEY, value)?,
            None => self.read(T::KEY, &Value::empty_table())?,
        };
        section
            .validate()
            .map_err(|message| ConfigError::Validation {
                key: T::KEY.to_owned(),
                message,
            })?;
        Ok(section)
    }

    /// The scalar at dotted path `key` as a string (numbers and booleans are
    /// formatted); `None` when absent or a table or array.
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.usage.key(key);
        self.lookup(key)?.scalar_string()
    }

    /// The value at dotted path `key` as a boolean (`true`/`false`,
    /// `yes`/`no`, `on`/`off`, `1`/`0`, case-insensitive, or a number);
    /// `None` when absent or not a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.usage.key(key);
        self.lookup(key)?.to_bool().ok()
    }

    /// Where the value at dotted path `key` came from: a `.env` entry (with
    /// its line and variable), a file, an environment variable or
    /// [`ConfigSources::set`]. `None` when no source sets the key, or when
    /// `key` is a table (ask for one of its keys).
    ///
    /// ```
    /// use navree_config::{Config, ConfigSources, ValueOrigin};
    ///
    /// let config = Config::load(&ConfigSources::new().set("log.level", "debug"))?;
    /// assert_eq!(config.origin("log.level"), Some(ValueOrigin::Set));
    /// assert_eq!(config.origin("log.format"), None);
    /// # Ok::<(), navree_config::ConfigError>(())
    /// ```
    pub fn origin(&self, key: &str) -> Option<ValueOrigin> {
        let value = self.lookup(key)?;
        if value.is_table() {
            return None;
        }
        value.origin.as_ref().map(|origin| origin.source().clone())
    }

    /// Every key some source sets, as dotted paths in sorted order. A list is
    /// one key; tables are walked. The short names of the `app` settings
    /// (`APP_URL`) are listed under `app` (`app.url`).
    pub fn keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        self.display_tree()
            .for_each_leaf("", &mut |key, _| keys.push(key.to_owned()));
        keys
    }

    /// The `.env` values that a config file hides: the key, the `.env` entry
    /// and the file. A file that ranks above `.env` (the order of
    /// [`ConfigSources::defaults`]) wins, so these `.env` values have no
    /// effect. Pairs that hold the same value are not listed, and nothing is
    /// listed when `.env` ranks above the file.
    pub fn shadowed(&self) -> &[Shadowed] {
        &self.shadows
    }

    /// The keys that nothing has read, as far as this `Config` (and its
    /// clones) can tell: a key is read when a section covers it (a field of
    /// the type that read the section) or [`get_string`](Config::get_string)
    /// or [`get_bool`](Config::get_bool) asked for it. Call it once the
    /// modules have built their sections, as the boot does; a key that
    /// something reads later is reported too.
    ///
    /// The short names of the `app` settings (`APP_URL`, `APP_ENV`, ...) are
    /// read by [`Config::app`] and never reported.
    pub fn unused(&self) -> Vec<UnusedKey> {
        self.unused_with(&[])
    }

    /// Like [`unused`](Config::unused), with `claims`: keys, and the subtrees
    /// below them, that something reads in a way the notes cannot see. A
    /// claim that names a section (`mail`) applies where no section read
    /// covers a key; a claim that names a key below one (`observe.tracing`)
    /// applies everywhere.
    pub fn unused_with(&self, claims: &[&str]) -> Vec<UnusedKey> {
        let mut all: Vec<&str> = APP_ALIASES.to_vec();
        all.extend_from_slice(claims);
        crate::usage::unused(&self.root, &self.usage, &all)
    }

    /// The value at `key`, with `app` and `app.*` read through the view that
    /// resolves the short names of the app settings.
    fn lookup(&self, key: &str) -> Option<&Value> {
        match (&self.app, key) {
            (Some(app), "app") => Some(app.as_ref()),
            (Some(app), key) if key.starts_with("app.") => app.lookup(&key["app.".len()..]),
            _ => self.root.lookup(key),
        }
    }

    /// The tree as `config:show` lists it: the short names of the app
    /// settings are gone and `app` holds them. Values are not masked.
    pub(crate) fn display_tree(&self) -> Value {
        let mut tree = (*self.root).clone();
        if let Kind::Table(top) = &mut tree.kind {
            for alias in APP_ALIASES {
                if top.get(alias).is_some_and(|value| !value.is_table()) {
                    top.remove(alias);
                }
            }
            if let Some(app) = &self.app {
                top.insert("app".to_owned(), (**app).clone());
            }
        }
        tree
    }

    /// The whole tree as JSON, safe to log: the value of every key whose name
    /// contains (case-insensitively) `password`, `passwd`, `pwd`, `secret`,
    /// `token`, `key`, `authorization`, `cookie`, `dsn`, `credential` or
    /// `connection_string` is replaced, subtree and all, by `"[redacted]"`;
    /// the password of any `scheme://user:password@host` URL is masked, and
    /// so is the value of a secret-looking query parameter
    /// (`?token=...`). Other values are shown as they are, so keep secrets
    /// under secret-looking keys.
    pub fn redacted(&self) -> serde_json::Value {
        redacted_json(&self.root)
    }
}

/// Prints the [redacted](Config::redacted) tree.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Config").field(&self.redacted()).finish()
    }
}

impl Config {
    /// Deserializes the section at `key` from `value`. Scalars convert
    /// between types on demand, so `"10"` from the environment fills a
    /// `u32`. The error never holds a string value, nor a number or other
    /// text found under a secret-looking key.
    ///
    /// A section that reads cleanly is noted, with the keys its type has no
    /// field for, so [`unused`](Config::unused) can name them.
    fn read<T: DeserializeOwned>(&self, key: &str, value: &Value) -> Result<T, ConfigError> {
        let sink = Sink::default();
        let section = de::deserialize_tracked(value, key, &sink)
            .map_err(|error| section_error(key, value, &error))?;
        // The whole tree (`""`) says nothing about which keys are read.
        if !key.is_empty() {
            self.usage.section(key, sink);
        }
        Ok(section)
    }
}

fn section_error(key: &str, section: &Value, error: &DeError) -> ConfigError {
    let path = match error.key() {
        Some(inner) => format!("{key}.{inner}"),
        None => key.to_owned(),
    };
    let message = crate::error::redact_values(&error.render(is_secret_key(&path)));
    ConfigError::Section {
        key: key.to_owned(),
        message: scrub(message, &secret_values(section)),
    }
}
