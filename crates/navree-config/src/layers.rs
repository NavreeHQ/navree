//! Turning [`ConfigSources`] into layers (one tree per source) and stacking
//! them. This is where files are read, parsed and size-checked, environment
//! variables are selected and mapped to keys, and nesting is limited.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::origin::{Origin, ValueOrigin};
use crate::sources::{DEFAULT_ENV_PREFIX, Source};
use crate::value::{Kind, Value, from_json, from_toml, insert_at, merge};
use crate::{ConfigError, ConfigSources, Shadowed};

/// Label recorded on values that come from environment variables.
const ENVIRONMENT: &str = "environment";

/// Label recorded on values from [`ConfigSources::set`].
const SET: &str = "ConfigSources::set";

/// The `app` settings that also have a short name at the top level:
/// `APP_URL` is `url`, and means `app.url`.
pub(crate) const APP_ALIASES: [&str; 6] = ["name", "url", "version", "env", "key", "previous_keys"];

/// A loaded tree and what loading it noticed.
pub(crate) struct Loaded {
    pub(crate) root: Value,
    /// The `app` table with its short names resolved (see [`app_view`]).
    pub(crate) app: Option<Value>,
    /// Keys that `.env` and a config file both set with different values.
    pub(crate) shadows: Vec<Shadowed>,
}

/// What a load refuses to go past.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// The size of a configuration file, in bytes.
    pub(crate) max_file_bytes: u64,
    /// The nesting of tables and arrays, the root table included.
    pub(crate) max_depth: usize,
}

/// Loads and stacks every source of `sources`, then its `set` values, and
/// notes the keys that `.env` and a config file both set with different
/// values.
pub(crate) fn load(sources: &ConfigSources) -> Result<Loaded, ConfigError> {
    let limits = sources.limits();
    let mut root = Value::empty_table();
    let mut shadows = Vec::new();
    for (rank, source) in sources.sources.iter().enumerate() {
        let rank = u32::try_from(rank).unwrap_or(u32::MAX - 1);
        if let Some(layer) = source_layer(source, &limits, rank)? {
            match source {
                Source::File(_) => find_shadows(&root, &layer, Other::Dotenv, &mut shadows),
                // A key that `config/app.toml` and a section file both set.
                Source::Dotenv(_) | Source::Section { .. } => {
                    find_shadows(&root, &layer, Other::File, &mut shadows)
                }
                _ => {}
            }
            merge(&mut root, layer);
        }
    }
    for (key, value) in &sources.overrides {
        if let Some(layer) = set_layer(key, value, &limits)? {
            merge(&mut root, layer);
        }
    }
    let app = app_view(&root);
    // A value that a later source (the real environment, `set`) overrides
    // again leaves nothing to say about the two files: keep the entries whose
    // winner still decides the key.
    shadows.retain(|shadow| {
        let decided = match (&app, shadow.key.strip_prefix("app.")) {
            (Some(app), Some(rest)) => app.lookup(rest),
            _ => root.lookup(&shadow.key),
        };
        decided.is_none_or(|value| {
            value.is_table()
                || value
                    .origin
                    .as_ref()
                    .is_some_and(|origin| *origin.source() == shadow.winner)
        })
    });
    Ok(Loaded { root, app, shadows })
}

/// The tree contributed by one source; `None` for a missing file. `rank` is
/// the position of the source in the load order.
fn source_layer(source: &Source, limits: &Limits, rank: u32) -> Result<Option<Value>, ConfigError> {
    let layer = match source {
        Source::Dotenv(path) => match read_optional(path, limits.max_file_bytes)? {
            Some(src) => dotenv_layer(path, &src, limits, rank)?,
            None => return Ok(None),
        },
        Source::File(path) => match read_optional(path, limits.max_file_bytes)? {
            Some(src) if is_json(path) => json_layer(path, &src, limits, rank)?,
            Some(src) => toml_layer(path, &src, limits, rank)?,
            None => return Ok(None),
        },
        Source::Section { path, key } => match read_optional(path, limits.max_file_bytes)? {
            Some(src) => {
                let table = if is_json(path) {
                    json_layer(path, &src, limits, rank)?
                } else {
                    toml_layer(path, &src, limits, rank)?
                };
                let segments: Vec<String> = key.split('.').map(str::to_owned).collect();
                let mut tree = Value::empty_table();
                insert_at(&mut tree, &segments, table);
                tree
            }
            None => return Ok(None),
        },
        Source::Env { prefix } => {
            let vars = select_env(prefix, std::env::vars_os());
            environment_layer(prefix, &vars, limits, rank)?
        }
        Source::EnvVars { prefix, vars } => environment_layer(prefix, vars, limits, rank)?,
    };
    Ok(Some(layer))
}

/// The layer of process environment variables (or an explicit snapshot).
fn environment_layer(
    prefix: &str,
    vars: &[(String, String)],
    limits: &Limits,
    rank: u32,
) -> Result<Value, ConfigError> {
    env_layer(
        prefix,
        vars.iter().map(|(name, value)| {
            let origin = Origin::new(
                ENVIRONMENT,
                ValueOrigin::Environment {
                    variable: name.clone(),
                },
                rank,
            );
            (name.as_str(), value.as_str(), origin)
        }),
        limits,
        &|name| format!("environment variable `{}`", shorten(name)),
    )
}

/// The `app` table with the short names of its settings resolved into it, or
/// `None` when the tree has no `app` table and no short name.
///
/// `APP_URL` loads as the top-level key `url`; the setting is `app.url`. For
/// each short name (`name`, `url`, `version`, `env`, `key`, `previous_keys`)
/// that holds a plain value, `app.<name>` takes the value of the two that
/// comes from the source that ranks higher in the load order; when both come
/// from one source, `app.<name>` (written out in full) wins, except for
/// `env`, where `APP_ENV` wins. The tree itself is not changed: this is the
/// view that `app.*` reads go through.
pub(crate) fn app_view(root: &Value) -> Option<Value> {
    let Kind::Table(top) = &root.kind else {
        return None;
    };
    let app = top.get("app");
    if app.is_some_and(|app| !app.is_table()) {
        return app.cloned();
    }
    let mut view = app.cloned().unwrap_or_else(Value::empty_table);
    let mut found = app.is_some();
    for alias in APP_ALIASES {
        let Some(short) = top.get(alias).filter(|v| !v.is_table()) else {
            continue;
        };
        found = true;
        let explicit = view.lookup(alias).filter(|v| !v.is_table());
        let rank = |value: &Value| value.origin.as_ref().map_or(0, |o| o.rank());
        // Within one source `app.<name>` wins, except for the environment,
        // where `env` (`APP_ENV`) has always come first.
        let tie_goes_to_short = alias == "env";
        let explicit_wins = explicit.is_some_and(|explicit| {
            rank(explicit) > rank(short) || (rank(explicit) == rank(short) && !tie_goes_to_short)
        });
        if !explicit_wins {
            insert_at(&mut view, &[alias.to_owned()], short.clone());
        }
    }
    found.then_some(view)
}

/// Which kind of source the earlier value of a conflict came from.
#[derive(Clone, Copy)]
enum Other {
    Dotenv,
    File,
}

impl Other {
    fn matches(self, origin: &ValueOrigin) -> bool {
        match self {
            Other::Dotenv => origin.is_dotenv(),
            Other::File => origin.is_file(),
        }
    }
}

/// Notes the keys that `layer` (a config file or a `.env` file, merged over
/// the tree so far) sets with a different value than an earlier source of the
/// `other` kind: the earlier value has no effect and the layer's wins. A
/// layer that turns a plain value into a table (or the other way round)
/// replaces the values it covers too, and a short name of an app setting
/// (`APP_URL`) is the same key as `app.url`. Equal values conflict with
/// nothing, so they are not reported. Values are compared, never kept.
fn find_shadows(root: &Value, layer: &Value, other: Other, shadows: &mut Vec<Shadowed>) {
    layer.for_each_leaf("", &mut |key, leaf| {
        let Some(winner) = leaf.origin.as_ref().map(|o| o.source().clone()) else {
            return;
        };
        let mut note = |hidden_key: &str, hidden: &Value| {
            if let Some(origin) = hidden.origin.as_ref().map(|o| o.source())
                && other.matches(origin)
            {
                shadows.push(Shadowed {
                    key: hidden_key.to_owned(),
                    hidden: origin.clone(),
                    winner: winner.clone(),
                });
            }
        };
        // At the key, or below it when the layer replaces a table.
        if let Some(existing) = root.lookup(key) {
            existing.for_each_leaf(key, &mut |hidden_key, hidden| {
                if hidden_key != key || !same_value(hidden, leaf) {
                    note(hidden_key, hidden);
                }
            });
        }
        // `app.<name>` against the short name (`APP_URL`), either way round.
        if let Some(alias) = key.strip_prefix("app.")
            && APP_ALIASES.contains(&alias)
            && let Some(hidden) = root.lookup(alias)
            && !hidden.is_table()
            && !same_value(hidden, leaf)
        {
            note(key, hidden);
        }
        if APP_ALIASES.contains(&key)
            && let Some(hidden) = root.lookup(&format!("app.{key}"))
            && !hidden.is_table()
            && !same_value(hidden, leaf)
        {
            note(&format!("app.{key}"), hidden);
        }
        // Above it, when the layer turns a plain value into a table.
        let segments: Vec<&str> = key.split('.').collect();
        for end in 1..segments.len() {
            let above = segments[..end].join(".");
            if let Some(hidden) = root.lookup(&above)
                && !hidden.is_table()
            {
                note(&above, hidden);
            }
        }
    });
}

/// Whether two leaves hold the same value. Environment and `.env` values are
/// text, so a number or a boolean in a file equals its text.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a.scalar_string(), b.scalar_string()) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
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
    let origin = Origin::new(SET, ValueOrigin::Set, u32::MAX);
    // The value sits below the root and the tables of `path`.
    let value =
        from_json(value, &origin, path.len() + 1, limits.max_depth).map_err(|_| too_deep())?;
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
fn dotenv_layer(path: &Path, src: &str, limits: &Limits, rank: u32) -> Result<Value, ConfigError> {
    let (entries, lines) = crate::dotenv::parse_with_lines(src).map_err(|error| match error {
        ConfigError::Dotenv { line, message, .. } => ConfigError::Dotenv {
            path: Some(path.to_owned()),
            line,
            message,
        },
        other => other,
    })?;
    let file = path.display().to_string();
    env_layer(
        DEFAULT_ENV_PREFIX,
        entries.iter().zip(&lines).map(|((name, value), line)| {
            let origin = Origin::new(
                file.clone(),
                ValueOrigin::DotEnv {
                    file: file.clone(),
                    line: *line,
                    variable: name.clone(),
                },
                rank,
            );
            (name.as_str(), value.as_str(), origin)
        }),
        limits,
        &|name| format!("`.env` entry `{}` in `{}`", shorten(name), path.display()),
    )
}

/// The origin of every value of a file layer.
fn file_origin(path: &Path, rank: u32) -> Origin {
    let file = path.display().to_string();
    Origin::new(file.clone(), ValueOrigin::File { file }, rank)
}

/// A TOML file. Parse errors keep the position but drop the source excerpt,
/// which may contain secrets.
fn toml_layer(path: &Path, src: &str, limits: &Limits, rank: u32) -> Result<Value, ConfigError> {
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
    let origin = file_origin(path, rank);
    from_toml(toml::Value::Table(table), &origin, 1, limits.max_depth).map_err(|_| {
        ConfigError::TooDeep {
            what: format!("configuration file `{}`", path.display()),
            limit: limits.max_depth,
        }
    })
}

/// A JSON file: an object at the top. Errors keep the position, not the text.
fn json_layer(path: &Path, src: &str, limits: &Limits, rank: u32) -> Result<Value, ConfigError> {
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
    let origin = file_origin(path, rank);
    from_json(&json, &origin, 1, limits.max_depth).map_err(|_| ConfigError::TooDeep {
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

/// Builds a layer from `(NAME, value, origin)` triples: names starting with
/// `prefix` (ASCII case-insensitive) map to lower-cased paths split on `__`
/// (`APP_DATABASE__URL` → `database.url`); other names are skipped. A name
/// with more segments than the depth limit is an error described by
/// `describe(name)`. Each value records its own `origin`.
fn env_layer<'a>(
    prefix: &str,
    vars: impl IntoIterator<Item = (&'a str, &'a str, Origin)>,
    limits: &Limits,
    describe: &dyn Fn(&str) -> String,
) -> Result<Value, ConfigError> {
    let mut entries: Vec<(Vec<String>, &str, Origin)> = Vec::new();
    for (name, value, origin) in vars {
        let Some(path) = env_path(prefix, name, limits.max_depth) else {
            continue;
        };
        if path.len() > limits.max_depth {
            return Err(ConfigError::TooDeep {
                what: describe(name),
                limit: limits.max_depth,
            });
        }
        entries.push((path, value, origin));
    }
    // Deterministic whatever the input order: parents before children, and a
    // stable sort keeps "later wins" for repeated names.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut layer = Value::empty_table();
    for (path, value, origin) in entries {
        insert_at(
            &mut layer,
            &path,
            Value::new(Some(&origin), Kind::String(value.to_owned())),
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
            forward
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str(), Origin::from("test"))),
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
            [(name.as_str(), "x", Origin::from("test"))],
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
            env_layer("APP_", [(ok.as_str(), "x", test)], &limits(), &|n| n
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
            [("APP_A__B", "x", Origin::from(".env"))],
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
            &serde_json::json!({ "x": { "y": 1 } }),
            &Origin::from("a"),
            1,
            8,
        )
        .unwrap();
        let b = from_json(
            &serde_json::json!({ "x": { "z": { "w": 1 } } }),
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
