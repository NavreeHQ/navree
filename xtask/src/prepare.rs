//! `release prepare <version>`: sets one version everywhere.
//!
//! - `[workspace.package] version` in the root `Cargo.toml`, which every
//!   published crate inherits, and with it the CLI's version and the
//!   starter kits' `kit_version` (both read `CARGO_PKG_VERSION`);
//! - the exact requirement (`=1.2.3`) of every workspace crate in
//!   `[workspace.dependencies]`;
//! - a published crate that states its own version;
//! - the `version` of the npm packages in `packages/js/` and of the apps'
//!   `package.json` files that have one;
//! - a section for the version in `CHANGELOG.md`, drafted from the commit
//!   subjects since the last release tag;
//! - the docs that print the version, and a section for it in the release
//!   notes, written from the changelog section ([`docs`]);
//! - `Cargo.lock`, for the workspace's own entries.

use std::path::{Path, PathBuf};
use std::process::Command;

use toml_edit::{DocumentMut, Item, Value};

use crate::changelog;
use crate::docs;
use crate::metadata::{Workspace, cargo};
use crate::process::git;

/// Whether `version` is a version Cargo and npm both take: `1.2.3`, or a
/// pre-release such as `2.0.0-rc.1`.
pub fn is_valid_version(version: &str) -> bool {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let numbers_ok = numbers.len() == 3
        && numbers.iter().all(|n| {
            !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
                && (n.len() == 1 || !n.starts_with('0'))
        });
    let pre_ok = pre.is_none_or(|pre| {
        !pre.is_empty()
            && pre.split('.').all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
    });
    numbers_ok && pre_ok
}

/// Replaces a value, keeping the spaces and comments around it.
fn set_string(value: &mut Value, new: &str) {
    let decor = value.decor().clone();
    *value = Value::from(new);
    *value.decor_mut() = decor;
}

/// The root manifest with `[workspace.package] version` set to `version`
/// and the requirement of every entry of `[workspace.dependencies]` named
/// in `internal` set to `=version`.
pub fn set_root_manifest(text: &str, version: &str, internal: &[String]) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("Cargo.toml does not parse: {e}"))?;
    let package_version = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("package"))
        .and_then(|p| p.get_mut("version"))
        .and_then(Item::as_value_mut)
        .ok_or("Cargo.toml has no [workspace.package] version")?;
    set_string(package_version, version);
    let pinned = format!("={version}");
    if let Some(deps) = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("dependencies"))
        .and_then(Item::as_table_like_mut)
    {
        for (name, entry) in deps.iter_mut() {
            if !internal.iter().any(|n| n == name.get()) {
                continue;
            }
            let Some(table) = entry.as_table_like_mut() else {
                return Err(format!(
                    "[workspace.dependencies] {}: expected {{ path = ..., version = ... }}",
                    name.get()
                ));
            };
            match table.get_mut("version").and_then(Item::as_value_mut) {
                Some(value) => set_string(value, &pinned),
                None => {
                    table.insert("version", toml_edit::value(pinned.clone()));
                    table.fmt();
                }
            }
        }
    }
    Ok(doc.to_string())
}

/// A member manifest with its own `[package] version` (one that does not
/// inherit the workspace's) set to `version`; `None` when it inherits.
pub fn set_member_manifest(text: &str, version: &str) -> Result<Option<String>, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("manifest does not parse: {e}"))?;
    let Some(value) = doc
        .get_mut("package")
        .and_then(|p| p.get_mut("version"))
        .and_then(Item::as_value_mut)
    else {
        return Ok(None);
    };
    if !value.is_str() {
        // `version.workspace = true` is a table.
        return Ok(None);
    }
    set_string(value, version);
    Ok(Some(doc.to_string()))
}

/// A `package.json` with its top-level `"version"` set to `version`,
/// keeping the rest of the file as it is; `None` when it has no version.
pub fn set_package_json(text: &str, version: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    json.get("version")?.as_str()?;
    let mut out = String::with_capacity(text.len());
    let mut done = false;
    for line in text.split_inclusive('\n') {
        // The top level is indented by two spaces in every package.json here.
        if !done && line.starts_with("  \"version\":") {
            let comma = if line.trim_end().ends_with(',') {
                ","
            } else {
                ""
            };
            let newline = &line[line.trim_end().len()..];
            out.push_str(&format!("  \"version\": \"{version}\"{comma}{newline}"));
            done = true;
        } else {
            out.push_str(line);
        }
    }
    done.then_some(out)
}

/// The `package.json` files that carry the release version: the npm
/// packages in `packages/js/*` and the apps in `apps/*`, those with a
/// `version`.
pub fn versioned_package_jsons(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for parent in ["packages/js", "apps"] {
        let Ok(entries) = std::fs::read_dir(root.join(parent)) else {
            continue;
        };
        for entry in entries.flatten() {
            let file = entry.path().join("package.json");
            let has_version = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|json| {
                    json.get("version")
                        .is_some_and(serde_json::Value::is_string)
                });
            if has_version {
                files.push(file);
            }
        }
    }
    files.sort();
    files
}

/// The commit subjects since the last `v*` tag, oldest first; empty when
/// there is no tag yet.
fn subjects_since_last_tag(root: &Path) -> Vec<String> {
    let Ok(tag) = git(root, &["describe", "--tags", "--abbrev=0", "--match", "v*"]) else {
        return Vec::new();
    };
    let range = format!("{}..HEAD", tag.trim());
    git(root, &["log", "--reverse", "--format=%s", &range])
        .map(|log| log.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Runs `release prepare`.
pub fn prepare(root: &Path, version: &str, date: &str) -> Result<Vec<PathBuf>, String> {
    if !is_valid_version(version) {
        return Err(format!("{version} is not a version (1.2.3, or 2.0.0-rc.1)"));
    }
    let workspace = Workspace::load(root)?;
    let mut changed = Vec::new();

    // A crate with a version of its own (a protocol at 0.x) keeps it, and so
    // does the requirement that names it.
    let current = crate::metadata::workspace_version(root)?;
    let internal: Vec<String> = workspace
        .members
        .iter()
        .filter(|package| package.version == current)
        .map(|package| package.name.clone())
        .collect();
    let manifest = root.join("Cargo.toml");
    let text = read(&manifest)?;
    let new = set_root_manifest(&text, version, &internal)?;
    if new != text {
        write(&manifest, &new)?;
        changed.push(manifest);
    }

    for package in workspace.publishable() {
        let text = read(&package.manifest_path)?;
        if let Some(new) = set_member_manifest(&text, version)
            .map_err(|e| format!("{}: {e}", package.manifest_path.display()))?
            && new != text
        {
            write(&package.manifest_path, &new)?;
            changed.push(package.manifest_path.clone());
        }
    }

    for file in versioned_package_jsons(root) {
        let text = read(&file)?;
        if let Some(new) = set_package_json(&text, version)
            && new != text
        {
            write(&file, &new)?;
            changed.push(file);
        }
    }

    let changelog_file = root.join("CHANGELOG.md");
    let text = std::fs::read_to_string(&changelog_file).unwrap_or_default();
    if !changelog::has_section(&text, version) {
        let body = changelog::draft(&subjects_since_last_tag(root));
        write(
            &changelog_file,
            &changelog::add_section(&text, version, date, &body),
        )?;
        changed.push(changelog_file.clone());
    }

    // The docs print the version, and the release notes are written from
    // the changelog section (the draft above, or the one a person wrote).
    let text = std::fs::read_to_string(&changelog_file).unwrap_or_default();
    changed.extend(docs::update(root, version, &text)?);

    // The lock file records each workspace crate's version.
    let status = Command::new(cargo())
        .args(["update", "--workspace"])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot run cargo update: {e}"))?;
    if !status.success() {
        return Err("cargo update --workspace failed".to_owned());
    }
    changed.push(root.join("Cargo.lock"));
    Ok(changed)
}

fn read(file: &Path) -> Result<String, String> {
    std::fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))
}

fn write(file: &Path, text: &str) -> Result<(), String> {
    std::fs::write(file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))
}
