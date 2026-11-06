//! `config:show`: the effective configuration, with its origins, secrets
//! masked. The command is registered by [`ConfigModule`](crate::ConfigModule);
//! the pieces are public so tools and tests can print the same view.
//!
//! There is no way to print a secret: a key that looks secret (see
//! [`Config::redacted`](crate::Config::redacted)) shows as `[redacted] (set)`
//! or `(unset)`, and the password of a URL is masked. The command has no
//! option that reveals them.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::Config;
use crate::origin::ValueOrigin;
use crate::redact::{REDACTED, is_secret_key, mask_urls, redacted_json};
use crate::value::Value;

/// What `config:show` prints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShowOptions {
    /// Only the keys of this section (`mail`, `database.mongodb`).
    pub section: Option<String>,
    /// Also the defaults that no source overrides.
    pub defaults: bool,
    /// Add the origin of every value.
    pub origin: bool,
    /// Print JSON instead of a table.
    pub json: bool,
}

const USAGE: &str = "usage: config:show [section] [--defaults] [--origin] [--json]";

impl ShowOptions {
    /// Reads the command line after `config:show`. An unknown option is an
    /// error; there is no option that shows secrets.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = ShowOptions::default();
        for arg in args {
            match arg.as_str() {
                "--defaults" => options.defaults = true,
                "--origin" => options.origin = true,
                "--json" => options.json = true,
                option if option.starts_with('-') => {
                    return Err(format!("unknown option `{option}`; {USAGE}"));
                }
                section => {
                    if options.section.replace(section.to_owned()).is_some() {
                        return Err(format!("only one section can be shown; {USAGE}"));
                    }
                }
            }
        }
        Ok(options)
    }
}

/// How a value is shown.
#[derive(Clone, Debug, PartialEq)]
enum Shown {
    /// A value that is safe to print (URL passwords masked).
    Value(Json),
    /// A secret that is set.
    SecretSet,
    /// A key without a value.
    Unset,
}

impl Shown {
    fn text(&self) -> String {
        match self {
            Shown::Value(Json::Null) | Shown::Unset => "(unset)".to_owned(),
            Shown::Value(value) => value.to_string(),
            Shown::SecretSet => format!("{REDACTED} (set)"),
        }
    }

    fn json(&self) -> Json {
        match self {
            Shown::Value(value) => value.clone(),
            Shown::SecretSet => Json::String(REDACTED.to_owned()),
            Shown::Unset => Json::Null,
        }
    }
}

struct Row {
    shown: Shown,
    origin: ValueOrigin,
}

/// Renders the effective configuration of `config`, with `defaults` (a JSON
/// object by section key, empty when none are published) under the keys no
/// source sets when `options.defaults` is on.
pub fn render(config: &Config, defaults: &Json, options: &ShowOptions) -> String {
    let rows = rows(config, defaults, options);
    if options.json {
        return render_json(&rows, options);
    }
    if rows.is_empty() {
        return match &options.section {
            Some(section) => format!("nothing is set under `{section}`\n"),
            None => "nothing is set\n".to_owned(),
        };
    }
    let key_width = rows.keys().map(String::len).max().unwrap_or(0);
    let texts: Vec<String> = rows.values().map(|row| row.shown.text()).collect();
    let value_width = texts.iter().map(String::len).max().unwrap_or(0);
    let mut out = String::new();
    for ((key, row), text) in rows.iter().zip(&texts) {
        if options.origin {
            out.push_str(&format!(
                "{key:key_width$}  {text:value_width$}  {}\n",
                row.origin
            ));
        } else {
            out.push_str(&format!("{key:key_width$}  {text}\n"));
        }
    }
    out
}

fn render_json(rows: &BTreeMap<String, Row>, options: &ShowOptions) -> String {
    let json = if options.origin {
        Json::Object(
            rows.iter()
                .map(|(key, row)| {
                    let mut entry = Map::new();
                    entry.insert("value".to_owned(), row.shown.json());
                    entry.insert("origin".to_owned(), Json::String(row.origin.to_string()));
                    (key.clone(), Json::Object(entry))
                })
                .collect(),
        )
    } else {
        let mut tree = Json::Object(Map::new());
        for (key, row) in rows {
            insert(&mut tree, key, row.shown.json());
        }
        tree
    };
    let mut text = serde_json::to_string_pretty(&json).unwrap_or_default();
    text.push('\n');
    text
}

/// Sets `value` at dotted `path` in a JSON object tree.
pub(crate) fn insert(tree: &mut Json, path: &str, value: Json) {
    let mut node = tree;
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        if !node.is_object() {
            *node = Json::Object(Map::new());
        }
        let Some(map) = node.as_object_mut() else {
            return;
        };
        if segments.peek().is_none() {
            map.insert(segment.to_owned(), value);
            return;
        }
        node = map
            .entry(segment.to_owned())
            .or_insert_with(|| Json::Object(Map::new()));
    }
}

/// Every key to show, by key: the tree's leaves, then the defaults that no
/// source overrides.
fn rows(config: &Config, defaults: &Json, options: &ShowOptions) -> BTreeMap<String, Row> {
    let wanted = |key: &str| match &options.section {
        Some(section) => key == section || key.starts_with(&format!("{section}.")),
        None => true,
    };
    let mut rows = BTreeMap::new();
    let tree = config.display_tree();
    tree.for_each_leaf("", &mut |key, leaf| {
        if wanted(key) {
            rows.insert(
                key.to_owned(),
                Row {
                    shown: shown_leaf(key, leaf),
                    origin: leaf
                        .origin
                        .as_ref()
                        .map_or(ValueOrigin::Default, |o| o.source().clone()),
                },
            );
        }
    });
    if options.defaults {
        default_leaves(defaults, "", &mut |key, value| {
            if wanted(key) && !rows.contains_key(key) && !below_a_set_key(&rows, key) {
                rows.insert(
                    key.to_owned(),
                    Row {
                        shown: shown_default(key, value),
                        origin: ValueOrigin::Default,
                    },
                );
            }
        });
    }
    rows
}

/// Whether a key above `key` is set to a plain value or a list, which
/// replaces the table the default has at that place.
fn below_a_set_key(rows: &BTreeMap<String, Row>, key: &str) -> bool {
    let mut prefix = String::new();
    for segment in key.split('.') {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(segment);
        if prefix != key && rows.contains_key(&prefix) {
            return true;
        }
    }
    false
}

/// A value of the tree, masked.
fn shown_leaf(key: &str, leaf: &Value) -> Shown {
    if is_secret_key(key) {
        return match redacted_json(leaf) {
            Json::Null => Shown::Unset,
            _ => Shown::SecretSet,
        };
    }
    Shown::Value(redacted_json(leaf))
}

/// A default, masked.
fn shown_default(key: &str, value: &Json) -> Shown {
    if is_secret_key(key) {
        return if value.is_null() {
            Shown::Unset
        } else {
            Shown::SecretSet
        };
    }
    Shown::Value(mask_json(value))
}

/// Masks the URL passwords in the strings of `value`.
fn mask_json(value: &Json) -> Json {
    match value {
        Json::String(text) => Json::String(mask_urls(text, None)),
        Json::Array(items) => Json::Array(items.iter().map(mask_json).collect()),
        Json::Object(map) => Json::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if is_secret_key(key) {
                        Json::String(REDACTED.to_owned())
                    } else {
                        mask_json(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Calls `f(key, value)` for every non-object value of a defaults tree.
fn default_leaves(tree: &Json, path: &str, f: &mut dyn FnMut(&str, &Json)) {
    match tree {
        Json::Object(map) => {
            for (key, value) in map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                default_leaves(value, &child, f);
            }
        }
        other => f(path, other),
    }
}

/// The published defaults of `registrations`, as one JSON object: each
/// section's defaults at its key.
pub fn defaults_tree(registrations: &[crate::SectionRegistration]) -> Json {
    let mut tree = Json::Object(Map::new());
    for registration in registrations {
        if let Some(defaults) = registration.defaults() {
            insert(&mut tree, registration.key(), defaults);
        }
    }
    tree
}
