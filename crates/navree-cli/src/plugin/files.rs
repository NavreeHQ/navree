//! The app's roots and files, read once before planning edits.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Where an app's files are.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AppRoots {
    /// The package: its `Cargo.toml` and `src/`.
    pub package: PathBuf,
    /// Where `config/`, `.env`, `.env.*` and `.env.example` are, and where the app runs.
    pub config: PathBuf,
    /// The Cargo workspace root, whose `Cargo.lock` the snapshot takes.
    pub workspace: PathBuf,
}

/// The app's files `plugin:add`, `plugin:remove` and `plugin:list` read, read once. Every path is
/// relative with `/`: to `roots.package` for `Cargo.toml` and `src/`, to `roots.config` for the rest.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AppFiles {
    pub roots: AppRoots,
    pub cargo_toml: String,
    pub app_rs: Option<String>,
    /// Every `.rs` file under `src/`, in path order.
    pub src: Vec<(String, String)>,
    pub env_example: Option<String>,
    /// `config/*.toml`, `config/*.json`, `config/env/*.toml`, by relative path.
    pub config: BTreeMap<String, String>,
    /// `.env` and `.env.*` but `.env.example`, by name.
    pub dot_env: BTreeMap<String, String>,
    pub shell_rs: Option<String>,
}
