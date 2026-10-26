//! [`Config`]: the merged configuration tree.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use config::{Map, Value, ValueKind};
use serde::de::DeserializeOwned;

use crate::error::redact_values;
use crate::sources::{DEFAULT_ENV_PREFIX, Source};
use crate::{ConfigError, ConfigSection, ConfigSources, parse_dotenv};

/// Origin recorded on values that come from environment variables.
const ENVIRONMENT: &str = "environment";

/// Origin recorded on values from [`ConfigSources::set`].
const SET: &str = "ConfigSources::set";

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
/// use navis_config::{Config, ConfigSources};
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
/// # Ok::<(), navis_config::ConfigError>(())
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
        let mut root = empty_table();
        for source in &sources.sources {
            if let Some(layer) = source_layer(source)? {
                merge(&mut root, layer);
            }
        }
        for (key, value) in &sources.overrides {
            if let Some(layer) = set_layer(key, value) {
                merge(&mut root, layer);
            }
        }
        Ok(Config {
            root: Arc::new(root),
        })
    }

    /// Deserializes the section at dotted path `key` (`""` is the whole tree).
    /// A missing section is [`ConfigError::MissingSection`].
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        match self.lookup(key) {
            Some(value) => deserialize(key, value.clone()),
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
            Some(value) => deserialize(key, value.clone()),
            None => Ok(T::default()),
        }
    }

    /// Reads section [`T::KEY`](ConfigSection::KEY) (an empty table when
    /// absent, so `#[serde(default)]` fields apply) and validates it. This is
    /// what [`ModuleBuilderConfigExt::config`](crate::ModuleBuilderConfigExt::config)
    /// runs at boot.
    pub fn typed<T: ConfigSection>(&self) -> Result<T, ConfigError> {
        let section: T = match self.lookup(T::KEY) {
            Some(value) => deserialize(T::KEY, value.clone())?,
            None => deserialize(T::KEY, empty_table())?,
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
        self.lookup(key)?.clone().into_string().ok()
    }

    /// The value at dotted path `key` as a boolean (`true`/`false`,
    /// `yes`/`no`, `on`/`off`, `1`/`0`, case-insensitive, or a number);
    /// `None` when absent or not a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.lookup(key)?.clone().into_bool().ok()
    }

    /// The whole tree as JSON, safe to log: the value of every key whose name
    /// contains (case-insensitively) `password`, `secret`, `token`, `key`,
    /// `authorization` or `cookie` is replaced, subtree and all, by
    /// `"[redacted]"`, and the password of any `scheme://user:password@host`
    /// URL is masked. Other values are shown as they are, so keep secrets
    /// under secret-looking keys.
    pub fn redacted(&self) -> serde_json::Value {
        redacted_json(&self.root)
    }

    /// The value at dotted path `key`, if present.
    fn lookup(&self, key: &str) -> Option<&Value> {
        let mut node = &*self.root;
        for segment in key.split('.').filter(|s| !s.is_empty()) {
            match &node.kind {
                ValueKind::Table(map) => node = map.get(segment)?,
                _ => return None,
            }
        }
        Some(node)
    }
}

/// Prints the [redacted](Config::redacted) tree.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Config").field(&self.redacted()).finish()
    }
}

/// What masked values are replaced with.
const REDACTED: &str = "[redacted]";

/// Key fragments that mark a value as secret.
const SECRET_KEY_PARTS: [&str; 6] = [
    "password",
    "secret",
    "token",
    "key",
    "authorization",
    "cookie",
];

fn is_secret_key(key: &str) -> bool {
    let key = key.to_lowercase();
    SECRET_KEY_PARTS.iter().any(|part| key.contains(part))
}

/// `scheme://user:password@host/...` with the password masked; `None` when
/// `s` is not a URL with a password.
fn mask_url_password(s: &str) -> Option<String> {
    let authority_start = s.find("://")? + 3;
    let rest = &s[authority_start..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let at = authority.rfind('@')?;
    let colon = authority[..at].find(':')?;
    Some(format!(
        "{}{}:{REDACTED}{}",
        &s[..authority_start],
        &authority[..colon],
        &rest[at..]
    ))
}

/// Converts a tree to JSON, masking secret-looking keys.
fn redacted_json(value: &Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match &value.kind {
        ValueKind::Nil => Json::Null,
        ValueKind::Boolean(b) => Json::Bool(*b),
        ValueKind::I64(i) => Json::from(*i),
        ValueKind::U64(u) => Json::from(*u),
        ValueKind::I128(i) => {
            i64::try_from(*i).map_or_else(|_| Json::String(i.to_string()), Json::from)
        }
        ValueKind::U128(u) => {
            u64::try_from(*u).map_or_else(|_| Json::String(u.to_string()), Json::from)
        }
        ValueKind::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        ValueKind::String(s) => Json::String(mask_url_password(s).unwrap_or_else(|| s.clone())),
        ValueKind::Array(items) => Json::Array(items.iter().map(redacted_json).collect()),
        ValueKind::Table(map) => Json::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if is_secret_key(key) {
                        Json::String(REDACTED.to_owned())
                    } else {
                        redacted_json(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
    }
}

fn empty_table() -> Value {
    Value::new(None, ValueKind::Table(Map::new()))
}

/// The tree contributed by one source; `None` for a missing file.
fn source_layer(source: &Source) -> Result<Option<Value>, ConfigError> {
    let layer = match source {
        Source::Dotenv(path) => match read_optional(path)? {
            Some(src) => dotenv_layer(path, &src)?,
            None => return Ok(None),
        },
        Source::File(path) => match read_optional(path)? {
            Some(src) => toml_layer(path, &src)?,
            None => return Ok(None),
        },
        Source::Env { prefix } => {
            // `vars_os`: `vars` panics on a variable that is not Unicode.
            let vars: Vec<(String, String)> = std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect();
            env_layer(
                prefix,
                vars.iter().map(|(k, v)| (k.as_str(), v.as_str())),
                ENVIRONMENT,
            )
        }
        Source::EnvVars { prefix, vars } => env_layer(
            prefix,
            vars.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            ENVIRONMENT,
        ),
    };
    Ok(Some(layer))
}

/// The tree for one `set(key, value)`; `None` when it would replace the root
/// with something other than a table.
fn set_layer(key: &str, value: &serde_json::Value) -> Option<Value> {
    let path: Vec<String> = key
        .split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let mut layer = empty_table();
    set_path(&mut layer, &path, from_json(value.clone(), &SET.to_owned()));
    matches!(layer.kind, ValueKind::Table(_)).then_some(layer)
}

/// Deserializes section `key` from `value`. config-rs converts between
/// scalar types on demand, so `"10"` from the environment fills a `u32`.
/// Values quoted in the error are masked: a mistyped value may be a secret.
fn deserialize<T: DeserializeOwned>(key: &str, value: Value) -> Result<T, ConfigError> {
    T::deserialize(value).map_err(|error| ConfigError::Section {
        key: key.to_owned(),
        message: redact_values(&error.to_string()),
    })
}

/// Reads a file; a missing file is `None` (every file source is optional).
fn read_optional(path: &Path) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(src) => Ok(Some(src)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "configuration file not found, skipped");
            Ok(None)
        }
        Err(source) => Err(ConfigError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

/// A `.env` file: its entries go through the `APP_` environment mapping.
fn dotenv_layer(path: &Path, src: &str) -> Result<Value, ConfigError> {
    let entries = parse_dotenv(src).map_err(|error| match error {
        ConfigError::Dotenv { line, message, .. } => ConfigError::Dotenv {
            path: Some(path.to_owned()),
            line,
            message,
        },
        other => other,
    })?;
    Ok(env_layer(
        DEFAULT_ENV_PREFIX,
        entries.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        &path.display().to_string(),
    ))
}

/// A TOML file. Parse errors keep the position but drop the source excerpt,
/// which may contain secrets.
fn toml_layer(path: &Path, src: &str) -> Result<Value, ConfigError> {
    let table: toml::Table = toml::from_str(src).map_err(|error| {
        let (line, column) = error
            .span()
            .map(|span| line_and_column(src, span.start))
            .unwrap_or((1, 1));
        ConfigError::Toml {
            path: path.to_owned(),
            line,
            column,
            message: error.message().trim().to_owned(),
        }
    })?;
    let origin = path.display().to_string();
    Ok(from_toml(toml::Value::Table(table), &origin))
}

/// The 1-based line and column of byte `offset` in `src`.
fn line_and_column(src: &str, offset: usize) -> (usize, usize) {
    let before = src.get(..offset).unwrap_or(src);
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |last| last.chars().count())
        + 1;
    (line, column)
}

fn from_json(value: serde_json::Value, origin: &String) -> Value {
    use serde_json::Value as Json;
    let kind = match value {
        Json::Null => ValueKind::Nil,
        Json::Bool(b) => ValueKind::Boolean(b),
        Json::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => ValueKind::I64(i),
            (None, Some(u)) => ValueKind::U64(u),
            (None, None) => n.as_f64().map_or(ValueKind::Nil, ValueKind::Float),
        },
        Json::String(s) => ValueKind::String(s),
        Json::Array(items) => {
            ValueKind::Array(items.into_iter().map(|v| from_json(v, origin)).collect())
        }
        Json::Object(map) => ValueKind::Table(
            map.into_iter()
                .map(|(k, v)| (k, from_json(v, origin)))
                .collect(),
        ),
    };
    Value::new(Some(origin), kind)
}

fn from_toml(value: toml::Value, origin: &String) -> Value {
    let kind = match value {
        toml::Value::String(s) => ValueKind::String(s),
        toml::Value::Integer(i) => ValueKind::I64(i),
        toml::Value::Float(f) => ValueKind::Float(f),
        toml::Value::Boolean(b) => ValueKind::Boolean(b),
        toml::Value::Datetime(d) => ValueKind::String(d.to_string()),
        toml::Value::Array(items) => {
            ValueKind::Array(items.into_iter().map(|v| from_toml(v, origin)).collect())
        }
        toml::Value::Table(table) => ValueKind::Table(
            table
                .into_iter()
                .map(|(k, v)| (k, from_toml(v, origin)))
                .collect(),
        ),
    };
    Value::new(Some(origin), kind)
}

/// Builds a layer from `NAME=value` pairs: names starting with `prefix`
/// (ASCII case-insensitive) map to lower-cased paths split on `__`
/// (`APP_DATABASE__URL` → `database.url`); other names are skipped.
fn env_layer<'a>(
    prefix: &str,
    vars: impl IntoIterator<Item = (&'a str, &'a str)>,
    origin: &str,
) -> Value {
    let mut entries: Vec<(Vec<String>, &str)> = vars
        .into_iter()
        .filter_map(|(name, value)| Some((env_path(prefix, name)?, value)))
        .collect();
    // Deterministic whatever the input order: parents before children, and a
    // stable sort keeps "later wins" for repeated names.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let origin = origin.to_owned();
    let mut layer = empty_table();
    for (path, value) in entries {
        set_path(&mut layer, &path, Value::new(Some(&origin), value));
    }
    layer
}

/// The config path of variable `name`, or `None` if it is not under `prefix`
/// or has an empty segment.
fn env_path(prefix: &str, name: &str) -> Option<Vec<String>> {
    let head = name.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = &name[prefix.len()..];
    let segments: Vec<String> = rest.split("__").map(str::to_lowercase).collect();
    if segments.iter().any(String::is_empty) {
        return None;
    }
    Some(segments)
}

/// Sets `value` at `path` under `node`, turning non-table nodes on the way into tables.
fn set_path(node: &mut Value, path: &[String], value: Value) {
    let Some((head, rest)) = path.split_first() else {
        *node = value;
        return;
    };
    if !matches!(node.kind, ValueKind::Table(_)) {
        *node = empty_table();
    }
    if let ValueKind::Table(map) = &mut node.kind {
        set_path(
            map.entry(head.clone()).or_insert_with(empty_table),
            rest,
            value,
        );
    }
}

/// Deep-merges `layer` over `base`: tables merge key by key; anything else
/// (scalars, arrays, a table replacing a scalar) is replaced by the layer.
fn merge(base: &mut Value, layer: Value) {
    if let ValueKind::Table(base_map) = &mut base.kind
        && matches!(layer.kind, ValueKind::Table(_))
    {
        if let ValueKind::Table(layer_map) = layer.kind {
            for (key, value) in layer_map {
                match base_map.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        return;
    }
    *base = layer;
}
