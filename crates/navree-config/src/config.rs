//! [`Config`]: the merged configuration tree.

use std::fmt;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::de::{self, DeError};
use crate::redact::{is_secret_key, redacted_json, scrub, secret_values};
use crate::value::Value;
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
}

impl Config {
    /// Loads and merges `sources` (see [`ConfigSources`] for the rules).
    ///
    /// This reads files and the process environment: call it at startup or
    /// on a blocking thread, not on an async worker.
    pub fn load(sources: &ConfigSources) -> Result<Config, ConfigError> {
        Ok(Config {
            root: Arc::new(layers::load(sources)?),
        })
    }

    /// Deserializes the section at dotted path `key` (`""` is the whole tree).
    /// A missing section is [`ConfigError::MissingSection`].
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        match self.root.lookup(key) {
            Some(value) => read(key, value),
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
        match self.root.lookup(key) {
            Some(value) => read(key, value),
            None => Ok(T::default()),
        }
    }

    /// Reads section [`T::KEY`](ConfigSection::KEY) (an empty table when
    /// absent, so `#[serde(default)]` fields apply) and validates it. This is
    /// what [`ModuleBuilderConfigExt::config`](crate::ModuleBuilderConfigExt::config)
    /// runs at boot.
    pub fn typed<T: ConfigSection>(&self) -> Result<T, ConfigError> {
        let section: T = match self.root.lookup(T::KEY) {
            Some(value) => read(T::KEY, value)?,
            None => read(T::KEY, &Value::empty_table())?,
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
        self.root.lookup(key)?.scalar_string()
    }

    /// The value at dotted path `key` as a boolean (`true`/`false`,
    /// `yes`/`no`, `on`/`off`, `1`/`0`, case-insensitive, or a number);
    /// `None` when absent or not a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.root.lookup(key)?.to_bool().ok()
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

/// Deserializes the section at `key` from `value`. Scalars convert between
/// types on demand, so `"10"` from the environment fills a `u32`. The error
/// never holds a string value, nor a number or other text found under a
/// secret-looking key.
fn read<T: DeserializeOwned>(key: &str, value: &Value) -> Result<T, ConfigError> {
    de::deserialize(value).map_err(|error| section_error(key, value, &error))
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
