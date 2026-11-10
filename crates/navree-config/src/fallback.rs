//! Environment fallbacks: the variable a field names with
//! `#[config(env_fallback = "REDIS_URL")]` fills it when nothing else sets
//! it. `config:show --fallbacks` and the reference list the variables the
//! fields declare, and module code reads a fallback through
//! [`Config::fallback`], [`env_fallback`] or [`table_env_fallback`], so the
//! variable it reads is the one they list.

use std::fmt;

use crate::{ConfigSection, ConfigTable, FieldSchema};

/// The variable field `field` of section `T` falls back to: `field` is the
/// key within the section (`url`, `chrome.path`). `None` when the field
/// declares none, or `T` does not describe itself (a section written by
/// hand).
///
/// ```
/// use navree_config::{ConfigSection, env_fallback};
/// use serde::Deserialize;
///
/// #[derive(Default, Deserialize, ConfigSection)]
/// #[config(key = "cache")]
/// #[serde(default)]
/// struct Cache {
///     /// The Redis address.
///     #[config(env_fallback = "REDIS_URL")]
///     url: Option<String>,
/// }
///
/// assert_eq!(env_fallback::<Cache>("url").as_deref(), Some("REDIS_URL"));
/// assert_eq!(env_fallback::<Cache>("prefix"), None);
/// ```
pub fn env_fallback<T: ConfigSection>(field: &str) -> Option<String> {
    find(&T::schema()?.fields, field)?.env_fallback.clone()
}

/// The variable field `field` of table `T` falls back to, for a table that
/// is not a section of its own (an entry of a map, such as one AI provider
/// of `ai.providers`): `field` is the key within the table (`region`).
pub fn table_env_fallback<T: ConfigTable>(field: &str) -> Option<String> {
    find(&T::fields(), field)?.env_fallback.clone()
}

/// The field at dotted `path` among `fields`.
fn find(fields: &[FieldSchema], path: &str) -> Option<FieldSchema> {
    let (name, rest) = match path.split_once('.') {
        Some((name, rest)) => (name, Some(rest)),
        None => (path, None),
    };
    let field = fields.iter().find(|field| field.name == name)?;
    match rest {
        Some(rest) => find(&field.fields, rest),
        None => Some(field.clone()),
    }
}

/// A field's value read from the variable it falls back to
/// ([`Config::fallback`](crate::Config::fallback)). `Debug` shows the
/// variable, never the value: it may be a credential.
#[derive(Clone, PartialEq, Eq)]
pub struct Fallback {
    /// The variable (`REDIS_URL`), for messages about the value.
    pub variable: String,
    /// Its value, as set; never blank.
    pub value: String,
}

impl fmt::Debug for Fallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fallback")
            .field("variable", &self.variable)
            .finish_non_exhaustive()
    }
}
