//! [`ViewOptions`].

use std::path::{Path, PathBuf};

use navree_config::ConfigSection;
use serde::Deserialize;

/// Where [`Views`](crate::Views) finds template files, and whether it reloads
/// them. Configuration section `view`:
///
/// ```toml
/// [view]
/// dirs = ["resources/views", "vendor/views"]  # or APP_VIEW__DIRS=a,b
/// reload = false                              # APP_VIEW__RELOAD
/// ```
///
/// Relative directories are resolved against the process working directory.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "view", docs = "/docs/digging-deeper/views/")]
#[serde(default)]
pub struct ViewOptions {
    /// Template directories, searched in order: the first one holding a
    /// template name wins, and any file wins over an embedded template.
    /// A directory that does not exist is skipped. Default:
    /// `["resources/views"]`. From the environment, a comma-separated list.
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    pub dirs: Vec<PathBuf>,
    /// Re-read template files when they change, appear or disappear (checked
    /// on every lookup, which costs a `stat` per lookup). Off: every template
    /// is read once, when `Views` is built. Default: on in debug builds, off
    /// in release builds.
    pub reload: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            dirs: vec![PathBuf::from("resources/views")],
            reload: cfg!(debug_assertions),
        }
    }
}

/// The default options with `dir` as the only template directory.
impl From<PathBuf> for ViewOptions {
    fn from(dir: PathBuf) -> Self {
        Self {
            dirs: vec![dir],
            ..Self::default()
        }
    }
}

/// The default options with `dir` as the only template directory.
impl From<&Path> for ViewOptions {
    fn from(dir: &Path) -> Self {
        Self::from(dir.to_path_buf())
    }
}

/// The default options with `dir` as the only template directory.
impl From<&str> for ViewOptions {
    fn from(dir: &str) -> Self {
        Self::from(PathBuf::from(dir))
    }
}

/// The default options with `dir` as the only template directory.
impl From<String> for ViewOptions {
    fn from(dir: String) -> Self {
        Self::from(PathBuf::from(dir))
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = ViewOptions::schema().expect("schema");
        assert_eq!(schema.key, "view");
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let dirs = schema
            .fields
            .iter()
            .find(|f| f.name == "dirs")
            .expect("dirs");
        assert_eq!(dirs.ty, FieldType::list(FieldType::String));
        assert!(schema.fields.iter().any(|f| f.name == "reload"));
        let defaults = ViewOptions::defaults().expect("defaults");
        assert_eq!(defaults["dirs"][0], "resources/views");
    }

    #[test]
    fn dirs_come_from_a_list_or_one_comma_separated_string() {
        let list: ViewOptions =
            serde_json::from_value(serde_json::json!({ "dirs": ["a", "b"] })).expect("list");
        assert_eq!(list.dirs, [PathBuf::from("a"), PathBuf::from("b")]);
        let joined: ViewOptions =
            serde_json::from_value(serde_json::json!({ "dirs": "a, b,,c" })).expect("joined");
        assert_eq!(
            joined.dirs,
            [PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")]
        );
    }
}
