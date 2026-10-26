//! [`ViewOptions`].

use std::path::{Path, PathBuf};

use navis_config::ConfigSection;
use serde::{Deserialize, Deserializer};

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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ViewOptions {
    /// Template directories, searched in order: the first one holding a
    /// template name wins, and any file wins over an embedded template.
    /// A directory that does not exist is skipped. Default:
    /// `["resources/views"]`. From the environment, a comma-separated list.
    #[serde(deserialize_with = "dirs")]
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

impl ConfigSection for ViewOptions {
    const KEY: &'static str = "view";
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

/// `dirs` as configured: a list, or one comma-separated string (what an
/// environment variable holds).
fn dirs<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<PathBuf>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Dirs {
        List(Vec<PathBuf>),
        Joined(String),
    }

    Ok(match Dirs::deserialize(deserializer)? {
        Dirs::List(dirs) => dirs,
        Dirs::Joined(joined) => joined
            .split(',')
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .collect(),
    })
}
