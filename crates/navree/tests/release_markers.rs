//! The first release is 1.0.0, so nothing can have been deprecated, or added, in an
//! earlier version. This test reads every Rust source file of the workspace, the Rust
//! templates the CLI writes (`*.rs.j2`) and the skills' Markdown, and fails on a
//! `since` marker that names a 0.x version, as `#[deprecated]` takes.

use std::path::{Path, PathBuf};

/// The folders that hold code or code samples of the workspace, relative to its root.
const ROOTS: &[&str] = &[
    "crates", "packages", "apps", "bench", "e2e", "fuzz", "skills",
];

/// Folders that are never the workspace's own code.
const SKIPPED: &[&str] = &["target", "node_modules", ".git", ".next", "dist"];

/// Whether `path` is a file the marker could be written in.
fn is_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name.ends_with(".rs") || name.ends_with(".rs.j2") || name.ends_with(".md")
}

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
        } else if is_source(&path) {
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
    assert!(
        files
            .iter()
            .any(|file| file.to_string_lossy().ends_with(".rs.j2")),
        "no template was read"
    );
    assert!(
        files
            .iter()
            .any(|file| file.to_string_lossy().contains("skills")),
        "no skill was read"
    );
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
