//! Turning [`ConfigSources`] into layers (one tree per source) and stacking
//! them. This is where files are read, parsed and size-checked, environment
//! variables are selected and mapped to keys, and nesting is limited.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::sources::{DEFAULT_ENV_PREFIX, Source};
use crate::value::{Kind, Origin, Value, from_json, from_toml, insert_at, merge};
use crate::{ConfigError, ConfigSources, parse_dotenv};

/// Origin recorded on values that come from environment variables.
const ENVIRONMENT: &str = "environment";

/// Origin recorded on values from [`ConfigSources::set`].
const SET: &str = "ConfigSources::set";

/// What a load refuses to go past.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// The size of a configuration file, in bytes.
    pub(crate) max_file_bytes: u64,
    /// The nesting of tables and arrays, the root table included.
    pub(crate) max_depth: usize,
}

/// Loads and stacks every source of `sources`, then its `set` values.
pub(crate) fn load(sources: &ConfigSources) -> Result<Value, ConfigError> {
    let limits = sources.limits();
    let mut root = Value::empty_table();
    for source in &sources.sources {
        if let Some(layer) = source_layer(source, &limits)? {
            merge(&mut root, layer);
        }
    }
    for (key, value) in &sources.overrides {
        if let Some(layer) = set_layer(key, value, &limits)? {
            merge(&mut root, layer);
        }
    }
    Ok(root)
}

/// The tree contributed by one source; `None` for a missing file.
fn source_layer(source: &Source, limits: &Limits) -> Result<Option<Value>, ConfigError> {
    let layer = match source {
        Source::Dotenv(path) => match read_optional(path, limits.max_file_bytes)? {
            Some(src) => dotenv_layer(path, &src, limits)?,
            None => return Ok(None),
        },
        Source::File(path) => match read_optional(path, limits.max_file_bytes)? {
            Some(src) if is_json(path) => json_layer(path, &src, limits)?,
            Some(src) => toml_layer(path, &src, limits)?,
            None => return Ok(None),
        },
        Source::Env { prefix } => {
            let vars = select_env(prefix, std::env::vars_os());
            env_layer(
                prefix,
                vars.iter().map(|(k, v)| (k.as_str(), v.as_str())),
                &Origin::from(ENVIRONMENT),
                limits,
                &|name| format!("environment variable `{}`", shorten(name)),
            )?
        }
        Source::EnvVars { prefix, vars } => env_layer(
            prefix,
            vars.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            &Origin::from(ENVIRONMENT),
            limits,
            &|name| format!("environment variable `{}`", shorten(name)),
        )?,
    };
    Ok(Some(layer))
}

/// The tree for one `set(key, value)`; `None` when it would replace the root
/// with something other than a table.
fn set_layer(
    key: &str,
    value: &serde_json::Value,
    limits: &Limits,
) -> Result<Option<Value>, ConfigError> {
    let too_deep = || ConfigError::TooDeep {
        what: format!("`ConfigSources::set` key `{}`", shorten(key)),
        limit: limits.max_depth,
    };
    let path: Vec<String> = key
        .split('.')
        .filter(|s| !s.is_empty())
        .take(limits.max_depth + 1)
        .map(str::to_owned)
        .collect();
    if path.len() > limits.max_depth {
        return Err(too_deep());
    }
    let origin = Origin::from(SET);
    // The value sits below the root and the tables of `path`.
    let value = from_json(value.clone(), &origin, path.len() + 1, limits.max_depth)
        .map_err(|_| too_deep())?;
    let mut layer = Value::empty_table();
    insert_at(&mut layer, &path, value);
    Ok(layer.is_table().then_some(layer))
}

/// Reads a file; a missing file is `None` (every file source is optional).
/// A file larger than `limit` bytes is an error, and is never read whole.
fn read_optional(path: &Path, limit: u64) -> Result<Option<String>, ConfigError> {
    let io_error = |source| ConfigError::Io {
        path: path.to_owned(),
        source,
    };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "configuration file not found, skipped");
            return Ok(None);
        }
        Err(error) => return Err(io_error(error)),
    };
    let mut bytes = Vec::new();
    // One byte more than the limit is enough to know the file is too large.
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > limit {
        return Err(ConfigError::TooLarge {
            path: path.to_owned(),
            limit,
        });
    }
    String::from_utf8(bytes).map(Some).map_err(|_| {
        io_error(io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        ))
    })
}

/// Whether `path` names a JSON file (extension `.json`, any case).
fn is_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

/// A `.env` file: its entries go through the `APP_` environment mapping.
fn dotenv_layer(path: &Path, src: &str, limits: &Limits) -> Result<Value, ConfigError> {
    let entries = parse_dotenv(src).map_err(|error| match error {
        ConfigError::Dotenv { line, message, .. } => ConfigError::Dotenv {
            path: Some(path.to_owned()),
            line,
            message,
        },
        other => other,
    })?;
    env_layer(
        DEFAULT_ENV_PREFIX,
        entries.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        &Origin::from(path.display().to_string()),
        limits,
        &|name| format!("`.env` entry `{}` in `{}`", shorten(name), path.display()),
    )
}

/// A TOML file. Parse errors keep the position but drop the source excerpt,
/// which may contain secrets.
fn toml_layer(path: &Path, src: &str, limits: &Limits) -> Result<Value, ConfigError> {
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
    let origin = Origin::from(path.display().to_string());
    from_toml(toml::Value::Table(table), &origin, 1, limits.max_depth).map_err(|_| {
        ConfigError::TooDeep {
            what: format!("configuration file `{}`", path.display()),
            limit: limits.max_depth,
        }
    })
}

/// A JSON file: an object at the top. Errors keep the position, not the text.
fn json_layer(path: &Path, src: &str, limits: &Limits) -> Result<Value, ConfigError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let json_error = |line, column, message: String| ConfigError::Json {
        path: path.to_owned(),
        line,
        column,
        message,
    };
    let json: serde_json::Value = serde_json::from_str(src).map_err(|error| {
        // serde_json ends its message with " at line L column C".
        let suffix = format!(" at line {} column {}", error.line(), error.column());
        let text = error.to_string();
        let message = text.strip_suffix(&suffix).unwrap_or(&text).to_owned();
        json_error(error.line(), error.column(), message)
    })?;
    if !json.is_object() {
        return Err(json_error(
            1,
            1,
            "the top level must be an object".to_owned(),
        ));
    }
    let origin = Origin::from(path.display().to_string());
    from_json(json, &origin, 1, limits.max_depth).map_err(|_| ConfigError::TooDeep {
        what: format!("configuration file `{}`", path.display()),
        limit: limits.max_depth,
    })
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

/// The environment variables of the process whose names start with `prefix`
/// (ASCII case-insensitively), as sorted `(name, value)` pairs.
///
/// Every other variable is dropped at once: its value is not converted,
/// stored, logged or mapped. A variable under the prefix whose value is not
/// valid Unicode is skipped with a warning naming it. The sort makes two
/// names that map to the same key (`APP_X` and `app_x` on Linux) resolve the
/// same way on every run instead of in the order the system lists them.
pub(crate) fn select_env(
    prefix: &str,
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(String, String)> {
    let mut selected: Vec<(String, String)> = vars
        .into_iter()
        .filter_map(|(name, value)| {
            let name = name.into_string().ok()?;
            if !has_prefix(&name, prefix) {
                return None;
            }
            match value.into_string() {
                Ok(value) => Some((name, value)),
                Err(_) => {
                    tracing::warn!(
                        variable = %name,
                        "environment variable is not valid Unicode and was skipped"
                    );
                    None
                }
            }
        })
        .collect();
    selected.sort();
    selected
}

fn has_prefix(name: &str, prefix: &str) -> bool {
    name.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// Builds a layer from `NAME=value` pairs: names starting with `prefix`
/// (ASCII case-insensitive) map to lower-cased paths split on `__`
/// (`APP_DATABASE__URL` → `database.url`); other names are skipped. A name
/// with more segments than the depth limit is an error described by
/// `describe(name)`. Values record `origin`.
fn env_layer<'a>(
    prefix: &str,
    vars: impl IntoIterator<Item = (&'a str, &'a str)>,
    origin: &Origin,
    limits: &Limits,
    describe: &dyn Fn(&str) -> String,
) -> Result<Value, ConfigError> {
    let mut entries: Vec<(Vec<String>, &str)> = Vec::new();
    for (name, value) in vars {
        let Some(path) = env_path(prefix, name, limits.max_depth) else {
            continue;
        };
        if path.len() > limits.max_depth {
            return Err(ConfigError::TooDeep {
                what: describe(name),
                limit: limits.max_depth,
            });
        }
        entries.push((path, value));
    }
    // Deterministic whatever the input order: parents before children, and a
    // stable sort keeps "later wins" for repeated names.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut layer = Value::empty_table();
    for (path, value) in entries {
        insert_at(
            &mut layer,
            &path,
            Value::new(Some(origin), Kind::String(value.to_owned())),
        );
    }
    Ok(layer)
}

/// The config path of variable `name`, or `None` if it is not under `prefix`
/// or has an empty segment. At most `max + 1` segments are kept, which is
/// enough to tell that a name is too deep.
fn env_path(prefix: &str, name: &str, max: usize) -> Option<Vec<String>> {
    if !has_prefix(name, prefix) {
        return None;
    }
    let rest = &name[prefix.len()..];
    let segments: Vec<String> = rest
        .split("__")
        .take(max + 1)
        .map(str::to_lowercase)
        .collect();
    if segments.iter().any(String::is_empty) {
        return None;
    }
    Some(segments)
}

/// The first 80 characters of a name, for an error message.
fn shorten(name: &str) -> String {
    const LIMIT: usize = 80;
    match name.char_indices().nth(LIMIT) {
        Some((end, _)) => format!("{}…", &name[..end]),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits {
            max_file_bytes: 1024,
            max_depth: 4,
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    #[test]
    fn select_env_keeps_only_the_prefix_and_sorts_by_name() {
        let selected = select_env(
            "APP_",
            vars(&[
                ("PATH", "/bin"),
                ("app_b", "2"),
                ("APP_A", "1"),
                ("APPLICATION", "x"),
                ("OTHER_APP_C", "x"),
            ]),
        );
        assert_eq!(
            selected,
            [
                ("APP_A".to_owned(), "1".to_owned()),
                ("app_b".to_owned(), "2".to_owned())
            ]
        );
    }

    #[test]
    fn a_value_outside_the_prefix_is_never_looked_at() {
        // A value that is not Unicode would be skipped with a warning if it
        // were converted; outside the prefix it is dropped untouched.
        #[cfg(windows)]
        let not_unicode = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xD800])
        };
        #[cfg(unix)]
        let not_unicode = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xFF, 0xFE])
        };
        let mut all = vars(&[("APP_OK", "yes")]);
        all.push((OsString::from("SECRET_TOKEN"), not_unicode.clone()));
        all.push((OsString::from("APP_BAD"), not_unicode));
        assert_eq!(
            select_env("APP_", all),
            [("APP_OK".to_owned(), "yes".to_owned())]
        );
    }

    #[test]
    fn names_that_map_to_one_key_resolve_in_name_order() {
        let forward = select_env("APP_", vars(&[("APP_X", "upper"), ("app_x", "lower")]));
        let backward = select_env("APP_", vars(&[("app_x", "lower"), ("APP_X", "upper")]));
        assert_eq!(forward, backward);
        let layer = env_layer(
            "APP_",
            forward.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            &Origin::from("test"),
            &limits(),
            &|name| name.to_owned(),
        )
        .unwrap();
        // `app_x` sorts after `APP_X`, so it is the later of the two.
        assert_eq!(
            layer.lookup("x").and_then(Value::scalar_string).as_deref(),
            Some("lower")
        );
    }

    #[test]
    fn env_paths_split_on_double_underscores_and_lower_case() {
        let path = |name| env_path("APP_", name, 8);
        assert_eq!(
            path("APP_A__B_C"),
            Some(vec!["a".to_owned(), "b_c".to_owned()])
        );
        assert_eq!(path("app_A"), Some(vec!["a".to_owned()]));
        assert_eq!(path("APP__A"), Some(vec!["_a".to_owned()]));
        assert_eq!(path("APP_A____B"), None);
        assert_eq!(path("APP___A"), None);
        assert_eq!(path("APP_"), None);
        assert_eq!(path("OTHER"), None);
        assert_eq!(path("AP"), None);
        // A prefix that is not on a character boundary is not a match.
        assert_eq!(path("APé_A"), None);
    }

    #[test]
    fn a_name_with_too_many_segments_is_an_error_naming_it() {
        let name = format!("APP_{}", ["a"; 5].join("__"));
        let error = env_layer(
            "APP_",
            [(name.as_str(), "x")],
            &Origin::from("test"),
            &limits(),
            &|name| format!("variable `{name}`"),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "variable `APP_a__a__a__a__a` nests deeper than the limit of 4 levels"
        );
        let ok = format!("APP_{}", ["a"; 4].join("__"));
        let test = Origin::from("test");
        assert!(
            env_layer("APP_", [(ok.as_str(), "x")], &test, &limits(), &|n| n
                .to_owned())
            .is_ok()
        );
    }

    #[test]
    fn a_huge_name_is_cut_before_it_is_split_whole() {
        let name = format!("APP_{}", "a__".repeat(100_000));
        let path = env_path("APP_", &name, 4).expect("under the prefix");
        assert_eq!(path.len(), 5);
    }

    #[test]
    fn set_keys_and_values_count_toward_the_depth() {
        let limits = limits();
        assert!(set_layer("a.b.c.d", &serde_json::json!(1), &limits).is_ok());
        assert!(set_layer("a.b.c.d.e", &serde_json::json!(1), &limits).is_err());
        // Four tables hold a container value one level lower: too deep.
        assert!(set_layer("a.b.c", &serde_json::json!({ "x": 1 }), &limits).is_ok());
        assert!(set_layer("a.b.c.d", &serde_json::json!({ "x": 1 }), &limits).is_err());
        assert!(
            set_layer(
                "",
                &serde_json::json!({ "a": { "b": { "c": 1 } } }),
                &limits
            )
            .is_ok()
        );
        assert!(
            set_layer(
                "",
                &serde_json::json!({ "a": { "b": { "c": { "d": 1 } } } }),
                &limits
            )
            .is_ok()
        );
        assert!(
            set_layer(
                "",
                &serde_json::json!({ "a": { "b": { "c": { "d": { "e": 1 } } } } }),
                &limits
            )
            .is_err()
        );
    }

    #[test]
    fn shorten_cuts_on_a_character_boundary() {
        assert_eq!(shorten("short"), "short");
        let long = "é".repeat(100);
        assert_eq!(shorten(&long), format!("{}…", "é".repeat(80)));
    }

    #[test]
    fn values_record_where_they_came_from() {
        let layer = env_layer(
            "APP_",
            [("APP_A__B", "x")],
            &Origin::from(".env"),
            &limits(),
            &|n| n.to_owned(),
        )
        .unwrap();
        let leaf = layer.lookup("a.b").expect("leaf");
        assert_eq!(leaf.origin.as_deref(), Some(".env"));
    }

    #[test]
    fn merging_never_deepens_the_tree() {
        let a = from_json(
            serde_json::json!({ "x": { "y": 1 } }),
            &Origin::from("a"),
            1,
            8,
        )
        .unwrap();
        let b = from_json(
            serde_json::json!({ "x": { "z": { "w": 1 } } }),
            &Origin::from("b"),
            1,
            8,
        )
        .unwrap();
        let mut merged = a;
        merge(&mut merged, b);
        assert_eq!(merged.depth(), 3);
    }
}
