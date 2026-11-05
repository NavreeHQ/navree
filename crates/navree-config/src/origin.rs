//! Where a configuration value came from.
//!
//! The loader records the origin of every value it reads. [`ValueOrigin`] is
//! the public face of it (see [`Config::origin`](crate::Config::origin)); the
//! crate-private [`Origin`] is what the tree stores: the short label that
//! type errors quote, the origin as data, and the rank of the source in the
//! load order.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Where the value of a configuration key came from.
///
/// ```
/// use navree_config::{Config, ConfigSources, ValueOrigin};
///
/// let config = Config::load(
///     &ConfigSources::new().env_vars("APP_", [("APP_DATABASE__URL", "sqlite://app.db")]),
/// )?;
/// assert_eq!(
///     config.origin("database.url"),
///     Some(ValueOrigin::Environment { variable: "APP_DATABASE__URL".into() })
/// );
/// assert_eq!(config.origin("database.url").unwrap().to_string(), "environment (APP_DATABASE__URL)");
/// # Ok::<(), navree_config::ConfigError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueOrigin {
    /// No source sets the key: the default of the section applies.
    Default,
    /// An entry of a `.env` file.
    DotEnv {
        /// The file, as given to [`ConfigSources::dotenv`](crate::ConfigSources::dotenv).
        file: String,
        /// The 1-based line of the entry.
        line: usize,
        /// The variable name (`APP_DATABASE__URL`).
        variable: String,
    },
    /// A TOML or JSON file.
    File {
        /// The file, as given to [`ConfigSources::file`](crate::ConfigSources::file).
        file: String,
    },
    /// A process environment variable.
    Environment {
        /// The variable name (`APP_DATABASE__URL`).
        variable: String,
    },
    /// [`ConfigSources::set`](crate::ConfigSources::set).
    Set,
}

impl ValueOrigin {
    /// Whether the value came from a `.env` file.
    pub fn is_dotenv(&self) -> bool {
        matches!(self, ValueOrigin::DotEnv { .. })
    }

    /// Whether the value came from a TOML or JSON file.
    pub fn is_file(&self) -> bool {
        matches!(self, ValueOrigin::File { .. })
    }
}

/// `default`, `.env:12 (APP_MAIL__FROM)`, `config/app.toml`,
/// `environment (APP_MAIL__TRANSPORT)` or `ConfigSources::set`.
impl fmt::Display for ValueOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueOrigin::Default => f.write_str("default"),
            ValueOrigin::DotEnv {
                file,
                line,
                variable,
            } => write!(f, "{file}:{line} ({variable})"),
            ValueOrigin::File { file } => f.write_str(file),
            ValueOrigin::Environment { variable } => write!(f, "environment ({variable})"),
            ValueOrigin::Set => f.write_str("ConfigSources::set"),
        }
    }
}

/// What a value of the tree remembers about its source. Cheap to clone; a
/// layer's values share the label, the entries of an environment layer each
/// carry their variable.
#[derive(Clone)]
pub(crate) struct Origin(Arc<Data>);

struct Data {
    /// What type errors quote: a file path, `environment` or
    /// `ConfigSources::set`.
    label: String,
    source: ValueOrigin,
    /// The position of the source in the load order; `u32::MAX` for `set`.
    rank: u32,
}

impl Origin {
    pub(crate) fn new(label: impl Into<String>, source: ValueOrigin, rank: u32) -> Self {
        Origin(Arc::new(Data {
            label: label.into(),
            source,
            rank,
        }))
    }

    /// The origin as data.
    pub(crate) fn source(&self) -> &ValueOrigin {
        &self.0.source
    }

    /// Where the source sits in the load order: a higher rank wins.
    pub(crate) fn rank(&self) -> u32 {
        self.0.rank
    }
}

/// The label.
impl Deref for Origin {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0.label
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.label)
    }
}

impl fmt::Debug for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Origin").field(&self.0.label).finish()
    }
}

/// A label with no source behind it, for tests of the tree.
#[cfg(test)]
impl From<&str> for Origin {
    fn from(label: &str) -> Self {
        Origin::new(
            label,
            ValueOrigin::File {
                file: label.to_owned(),
            },
            0,
        )
    }
}

/// A key that a `.env` file and a config file both set, with different
/// values.
///
/// The source that ranks higher wins and the other has no effect (see
/// [`ConfigSources::DOTENV_RANKS_ABOVE_FILES`](crate::ConfigSources::DOTENV_RANKS_ABOVE_FILES);
/// the real environment ranks above both). Each entry names the key and both
/// sources, never a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shadowed {
    /// The configuration key (`database.url`).
    pub key: String,
    /// The source whose value has no effect.
    pub hidden: ValueOrigin,
    /// The source whose value is used.
    pub winner: ValueOrigin,
}

impl Shadowed {
    /// The warning shown at boot and by `config:check`: the key, both
    /// sources and what to do.
    pub fn message(&self) -> String {
        let (used, unused) = if self.winner.is_dotenv() {
            ("`.env`", "config file")
        } else {
            ("config file", "`.env`")
        };
        format!(
            "`{key}` is set in both places, `.env` and a config file; the {used} value is used ({winner}) and the {unused} value has no effect ({hidden}). Remove one of the two.",
            key = self.key,
            winner = self.winner,
            hidden = self.hidden,
        )
    }
}

impl fmt::Display for Shadowed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}
