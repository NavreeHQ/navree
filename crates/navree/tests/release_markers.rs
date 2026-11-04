//! The first release is 1.0.0, so nothing can have been deprecated, or added, in an
//! earlier version. This test reads every Rust source file in the workspace and fails on
//! a `since` marker that names a 0.x version, as `#[deprecated]` takes.

use std::path::{Path, PathBuf};

/// The folders that hold Rust code of the workspace, relative to its root.
const ROOTS: &[&str] = &["crates", "packages", "apps", "bench", "e2e", "fuzz"];

/// Folders that are never the workspace's own code.
const SKIPPED: &[&str] = &["target", "node_modules", ".git", ".next", "dist"];

fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !SKIPPED.iter().any(|skipped| name == *skipped) {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_marker_names_a_version_before_the_first_release() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for folder in ROOTS {
        sources(&root.join(folder), &mut files);
    }
    assert!(files.len() > 100, "found only {} files", files.len());
    // Written in two parts, so this file does not match itself.
    let marker = format!("since = {}0.", '"');
    let mut found = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (at, line) in text.lines().enumerate() {
            if line.contains(&marker) {
                found.push(format!("{}:{}: {}", file.display(), at + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "these markers name a version before 1.0.0:\n  {}",
        found.join("\n  ")
    );
}
