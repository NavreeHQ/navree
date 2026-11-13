//! A registry's sparse index: which versions of a crate it has.
//!
//! The index is how a publish resumes: a crate whose version is in the
//! index is never published again.

use std::path::{Path, PathBuf};

use crate::process::{Stdin, curl};

/// The sparse index of crates.io.
pub const CRATES_IO_INDEX: &str = "https://index.crates.io/";

/// Which crate versions a registry has.
pub trait Index {
    /// Whether the registry has `version` of `name` (yanked or not: a
    /// yanked version can never be published again either).
    fn has(&mut self, name: &str, version: &str) -> Result<bool, String>;

    /// Whether the registry has any version of `name`: a crate it has none of
    /// is published for the first time, which trusted publishing cannot do.
    fn known(&mut self, name: &str) -> Result<bool, String>;
}

/// The path of a crate's file in an index: `1/a`, `2/ab`, `3/a/abc`,
/// `na/vr/navree-kernel`.
pub fn index_path(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
}

/// Whether an index file (one JSON object per line) lists `version`.
pub fn lists_version(file: &str, version: &str) -> bool {
    file.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|entry| entry["vers"].as_str() == Some(version))
}

/// A sparse index over HTTP.
#[derive(Debug, Clone)]
pub struct SparseIndex {
    base: String,
}

impl SparseIndex {
    /// The index at `url` (`sparse+https://...` or `https://...`).
    pub fn new(url: &str) -> Result<Self, String> {
        let base = url.strip_prefix("sparse+").unwrap_or(url);
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(format!(
                "{url}: only sparse indexes are read (`sparse+https://...`)"
            ));
        }
        let mut base = base.to_owned();
        if !base.ends_with('/') {
            base.push('/');
        }
        Ok(SparseIndex { base })
    }

    /// The index of `registry` (`None` is crates.io): the URL Cargo would
    /// use, from `CARGO_REGISTRIES_<NAME>_INDEX` or a `[registries]` table
    /// of a Cargo configuration file.
    pub fn for_registry(registry: Option<&str>, cwd: &Path) -> Result<Self, String> {
        match registry {
            None | Some("crates-io") => Self::new(CRATES_IO_INDEX),
            Some(name) => {
                let url = registry_index_url(name, cwd)
                    .ok_or_else(|| format!("no index for registry `{name}`: set {} or add [registries.{name}] to a Cargo configuration file", env_name(name, "INDEX")))?;
                Self::new(&url)
            }
        }
    }

    /// The index's base URL, ending in `/`.
    pub fn base(&self) -> &str {
        &self.base
    }
}

impl SparseIndex {
    /// The index file of `name`, or `None` when the index has no such crate.
    fn file(&self, name: &str) -> Result<Option<String>, String> {
        let url = format!("{}{}", self.base, index_path(name));
        let mut last = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(5 * attempt));
            }
            match curl("GET", &url, &[], Stdin::None) {
                Ok(response) if response.status == 200 => return Ok(Some(response.body)),
                // An index answers 404 (or 410, or 451) for a crate it does not have.
                Ok(response) if matches!(response.status, 404 | 410 | 451) => return Ok(None),
                Ok(response) => last = format!("{url}: HTTP {}", response.status),
                Err(error) => last = error,
            }
        }
        Err(last)
    }
}

impl Index for SparseIndex {
    fn has(&mut self, name: &str, version: &str) -> Result<bool, String> {
        Ok(self
            .file(name)?
            .is_some_and(|file| lists_version(&file, version)))
    }

    fn known(&mut self, name: &str) -> Result<bool, String> {
        Ok(self
            .file(name)?
            .is_some_and(|file| file.lines().any(|line| !line.trim().is_empty())))
    }
}

/// `CARGO_REGISTRIES_<NAME>_<KEY>` for registry `name`.
pub fn env_name(name: &str, key: &str) -> String {
    format!(
        "CARGO_REGISTRIES_{}_{key}",
        name.to_ascii_uppercase().replace('-', "_")
    )
}

/// The index URL of registry `name`: the environment first, then the Cargo
/// configuration files from `cwd` up, then Cargo's home.
fn registry_index_url(name: &str, cwd: &Path) -> Option<String> {
    if let Ok(url) = std::env::var(env_name(name, "INDEX")) {
        return Some(url);
    }
    let mut files: Vec<PathBuf> = cwd
        .ancestors()
        .flat_map(|dir| [dir.join(".cargo/config.toml"), dir.join(".cargo/config")])
        .collect();
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".cargo"))
        });
    if let Some(home) = home {
        files.push(home.join("config.toml"));
        files.push(home.join("config"));
    }
    files.iter().find_map(|file| {
        let text = std::fs::read_to_string(file).ok()?;
        let doc: toml_edit::DocumentMut = text.parse().ok()?;
        doc.get("registries")?
            .get(name)?
            .get("index")?
            .as_str()
            .map(str::to_owned)
    })
}
