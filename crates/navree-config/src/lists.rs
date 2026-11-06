//! Deserializers for a setting that is a list in a file and one
//! comma-separated string in an environment variable.
//!
//! The loader never turns a string into a list on its own (a string stays a
//! string, so a value with a comma in it is not split by surprise). A field
//! that wants `APP_QUEUE__QUEUES=high,default` says so with
//! `#[serde(deserialize_with = "navree_config::comma_list")]`.

use std::fmt::Display;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

/// A list, or one string to split.
#[derive(Deserialize)]
#[serde(untagged)]
enum ListOrString<T> {
    List(Vec<T>),
    Joined(String),
}

/// The items of a comma-separated string: trimmed, empty ones dropped.
fn items(joined: &str) -> impl Iterator<Item = &str> {
    joined
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn strings(value: ListOrString<String>) -> Vec<String> {
    match value {
        ListOrString::List(list) => list
            .into_iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect(),
        ListOrString::Joined(joined) => items(&joined).map(str::to_owned).collect(),
    }
}

/// A list of strings from a list, or from one comma-separated string.
/// Items are trimmed and empty items are dropped, so `"a, b,,"` is `["a",
/// "b"]` and a blank string is the empty list.
///
/// ```
/// use navree_config::{Config, ConfigSources, comma_list};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Cors {
///     #[serde(deserialize_with = "comma_list")]
///     origins: Vec<String>,
/// }
///
/// let config = Config::load(&ConfigSources::new().env_vars(
///     "APP_",
///     [("APP_CORS__ORIGINS", "https://a.test, https://b.test")],
/// ))?;
/// assert_eq!(
///     config.section::<Cors>("cors")?.origins,
///     ["https://a.test", "https://b.test"]
/// );
/// # Ok::<(), navree_config::ConfigError>(())
/// ```
pub fn comma_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    ListOrString::<String>::deserialize(deserializer).map(strings)
}

/// Like [`comma_list`] for a field that may be left out: `Option<Vec<String>>`.
pub fn comma_list_opt<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error> {
    Option::<ListOrString<String>>::deserialize(deserializer).map(|value| value.map(strings))
}

/// A list of values of any type that parses from text (numbers, enums with
/// `FromStr`), from a list or from one comma-separated string. The items of
/// the string are trimmed and parsed one by one; an item that does not parse
/// is an error that does not repeat it.
///
/// ```
/// use navree_config::{Config, ConfigSources, comma_list_of};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Ports {
///     #[serde(deserialize_with = "comma_list_of")]
///     ports: Vec<u16>,
/// }
///
/// let config =
///     Config::load(&ConfigSources::new().env_vars("APP_", [("APP_NET__PORTS", "80, 443")]))?;
/// assert_eq!(config.section::<Ports>("net")?.ports, [80, 443]);
/// # Ok::<(), navree_config::ConfigError>(())
/// ```
pub fn comma_list_of<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
{
    match ListOrString::<T>::deserialize(deserializer)? {
        ListOrString::List(list) => Ok(list),
        ListOrString::Joined(joined) => items(&joined)
            .map(|item| {
                item.parse::<T>()
                    .map_err(|error| D::Error::custom(format!("an item is not valid: {error}")))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, ConfigSources};

    #[derive(Deserialize)]
    struct Holder {
        #[serde(default, deserialize_with = "comma_list")]
        names: Vec<String>,
        #[serde(default, deserialize_with = "comma_list_opt")]
        maybe: Option<Vec<String>>,
        #[serde(default, deserialize_with = "comma_list_of")]
        ports: Vec<u16>,
    }

    fn holder(pairs: &[(&str, &str)]) -> Result<Holder, crate::ConfigError> {
        Config::load(&ConfigSources::new().env_vars("APP_", pairs.iter().copied()))
            .expect("load")
            .section::<Holder>("h")
    }

    #[test]
    fn a_string_is_split_trimmed_and_cleaned() {
        let h = holder(&[("APP_H__NAMES", " a, b ,,c ,")]).expect("valid");
        assert_eq!(h.names, ["a", "b", "c"]);
        let h = holder(&[("APP_H__NAMES", "   ")]).expect("valid");
        assert!(h.names.is_empty());
    }

    #[test]
    fn a_list_from_a_file_is_trimmed_too() {
        let config = Config::load(&ConfigSources::new().set(
            "h",
            serde_json::json!({ "names": [" a ", "", "b"], "ports": [80, 443] }),
        ))
        .expect("load");
        let h = config.section::<Holder>("h").expect("valid");
        assert_eq!(h.names, ["a", "b"]);
        assert_eq!(h.ports, [80, 443]);
        assert_eq!(h.maybe, None);
    }

    #[test]
    fn an_optional_list_is_none_until_it_is_set() {
        assert_eq!(holder(&[]).expect("valid").maybe, None);
        let h = holder(&[("APP_H__MAYBE", "x, y")]).expect("valid");
        assert_eq!(h.maybe, Some(vec!["x".to_owned(), "y".to_owned()]));
    }

    #[test]
    fn typed_items_parse_and_a_bad_one_does_not_echo_the_text() {
        let h = holder(&[("APP_H__PORTS", "80, 443")]).expect("valid");
        assert_eq!(h.ports, [80, 443]);
        let error = holder(&[("APP_H__PORTS", "80, hunter2")])
            .err()
            .expect("not a port");
        let text = error.to_string();
        assert!(text.contains("h"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }
}
