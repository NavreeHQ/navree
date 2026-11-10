//! `ui-docs`: the website's UI docs, generated from the toolkit.
//!
//! Every documented example is a Rust function in `src/examples/`. This crate
//! renders each one through navree-ui into a preview document, records the
//! function's body as the example's code and its `data-slot` parts for the
//! anatomy diagram, and reads the props tables from the toolkit's source. It
//! writes them to `docs/ui-data/`, which the website reads.
//!
//! ```text
//! cargo run -p ui-docs              # write the files
//! cargo run -p ui-docs -- --check   # fail when they are stale
//! ```
//!
//! `cargo test -p ui-docs` runs the same check.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod api;
pub mod catalog;
mod examples;
pub mod flags;
pub mod icons;
pub mod output;

/// Where the generated files live, from the repository's root.
pub const OUT: &str = "docs/ui-data";

/// What to run when the files are stale.
pub const RERUN: &str = "rerun: cargo run -p ui-docs";

/// The repository's root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A file of the repository, with Windows line endings made Unix ones.
pub fn read_repo(path: &str) -> String {
    let full = repo().join(path);
    std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("{}: {e}", full.display()))
        .replace("\r\n", "\n")
}

/// Every generated file, by its path under [`OUT`].
pub fn generate() -> BTreeMap<String, String> {
    let api = api::read(|file| read_repo(&format!("crates/navree-ui/src/{file}")));
    output::files(&examples::all(), &api)
}

/// The files under [`OUT`] that differ from `files`, are missing, or are left
/// over from an example that no longer exists.
pub fn stale(files: &BTreeMap<String, String>) -> Vec<String> {
    let out = repo().join(OUT);
    let mut stale: Vec<String> = files
        .iter()
        .filter(|(path, text)| {
            std::fs::read_to_string(out.join(path))
                .map(|on_disk| on_disk.replace("\r\n", "\n") != **text)
                .unwrap_or(true)
        })
        .map(|(path, _)| path.clone())
        .collect();
    stale.extend(
        on_disk(&out)
            .into_iter()
            .filter(|path| !files.contains_key(path)),
    );
    stale.sort();
    stale
}

/// Writes `files` under [`OUT`] and removes any file there that isn't one of
/// them.
pub fn write(files: &BTreeMap<String, String>) -> std::io::Result<()> {
    let out = repo().join(OUT);
    for path in on_disk(&out) {
        if !files.contains_key(&path) {
            std::fs::remove_file(out.join(&path))?;
        }
    }
    for (path, text) in files {
        let full = out.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full, text)?;
    }
    Ok(())
}

/// Every file under `dir`, as a path relative to it with `/` separators.
fn on_disk(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(relative) = path.strip_prefix(dir) {
                found.push(
                    relative
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    found
}
