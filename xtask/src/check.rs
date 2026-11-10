//! `release check`: what must hold before anything is published.
//!
//! - Every published crate has the workspace's version, and so has every
//!   npm package and app `package.json` that states one.
//! - Every entry of `[workspace.dependencies]` that names a published
//!   workspace crate requires exactly the workspace's version (`=1.2.3`).
//! - Every published crate requires the workspace crates it depends on at
//!   exactly that version; a dev-dependency either does too or is path-only
//!   (Cargo drops it when it packages the crate).
//! - No published crate depends on a workspace crate that is not published.
//! - No cycle: a publish order exists ([`crate::order`]).
//! - `CHANGELOG.md` has a section for the version.
//! - `Cargo.lock` is current.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::changelog;
use crate::metadata::{DepKind, Workspace, cargo, workspace_version};
use crate::order;
use crate::prepare::versioned_package_jsons;

/// The problems the static checks found in `workspace` for `version`,
/// with the root manifest's text (for `[workspace.dependencies]`).
pub fn problems(workspace: &Workspace, version: &str, root_manifest: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let pinned = format!("={version}");

    for package in workspace.publishable() {
        if package.version != version {
            problems.push(format!(
                "{} is at {}, the workspace at {version}",
                package.name, package.version
            ));
        }
        for dep in workspace.internal_deps(package) {
            let target = workspace.member(&dep.name).expect("internal dependency");
            let what = match dep.kind {
                DepKind::Normal => "dependency",
                DepKind::Build => "build dependency",
                DepKind::Dev => "dev-dependency",
            };
            if !target.is_publishable() {
                if dep.kind != DepKind::Dev || dep.is_versioned() {
                    problems.push(format!(
                        "{}: {what} {} is not published (a dev-dependency on it must be path-only)",
                        package.name, dep.name
                    ));
                }
                continue;
            }
            let ok = if dep.kind == DepKind::Dev {
                !dep.is_versioned() || dep.req == pinned
            } else {
                dep.req == pinned
            };
            if !ok {
                problems.push(format!(
                    "{}: {what} {} requires `{}`, not `{pinned}`{}",
                    package.name,
                    dep.name,
                    dep.req,
                    if dep.is_versioned() {
                        ""
                    } else {
                        " (a path without a version cannot be published)"
                    }
                ));
            }
        }
    }

    problems.extend(pin_problems(workspace, version, root_manifest));

    let publishable: Vec<String> = workspace.publishable().map(|p| p.name.clone()).collect();
    if let Err(cycles) = order::layers(workspace, &publishable) {
        problems.push(order::describe_cycles(&cycles));
    }
    problems
}

/// The entries of `[workspace.dependencies]` that name a published
/// workspace crate without requiring exactly `version`.
fn pin_problems(workspace: &Workspace, version: &str, root_manifest: &str) -> Vec<String> {
    let pinned = format!("={version}");
    let Ok(doc) = root_manifest.parse::<toml_edit::DocumentMut>() else {
        return vec!["Cargo.toml does not parse".to_owned()];
    };
    let Some(deps) = doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table_like())
    else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    for (name, entry) in deps.iter() {
        let package = entry
            .get("package")
            .and_then(|p| p.as_str())
            .unwrap_or(name);
        let Some(member) = workspace.member(package) else {
            continue;
        };
        if !member.is_publishable() {
            continue;
        }
        match entry.get("version").and_then(|v| v.as_str()) {
            Some(req) if req == pinned => {}
            Some(req) => problems.push(format!(
                "[workspace.dependencies] {name} requires `{req}`, not `{pinned}`"
            )),
            None => problems.push(format!(
                "[workspace.dependencies] {name} has no version (write `version = \"{pinned}\"`)"
            )),
        }
    }
    problems
}

/// The `package.json` files whose version is not `version`.
pub fn package_json_problems(root: &Path, version: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for file in versioned_package_jsons(root) {
        let found = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|json| json["version"].as_str().map(str::to_owned))
            .unwrap_or_default();
        if found != version {
            let shown = file.strip_prefix(root).unwrap_or(&file);
            problems.push(format!(
                "{} is at {found}, the workspace at {version}",
                shown.display()
            ));
        }
    }
    problems
}

/// What `release check` reports.
pub struct Report {
    /// The workspace's version.
    pub version: String,
    /// How many crates are published.
    pub crates: usize,
    /// How many layers the publish order has.
    pub layers: usize,
    /// Everything that is wrong; empty when the release may go ahead.
    pub problems: Vec<String>,
}

/// Runs every check on the workspace at `root`.
pub fn run(root: &Path) -> Result<Report, String> {
    let workspace = Workspace::load(root)?;
    let version = workspace_version(root)?;
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let mut problems = problems(&workspace, &version, &manifest);
    problems.extend(package_json_problems(root, &version));

    let changelog_text = std::fs::read_to_string(root.join("CHANGELOG.md")).unwrap_or_default();
    if !changelog::has_section(&changelog_text, &version) {
        problems.push(format!("CHANGELOG.md has no section for {version}"));
    }

    let locked = Command::new(cargo())
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !locked.status.success() {
        problems.push(format!(
            "Cargo.lock is not current (run `cargo update --workspace`):\n{}",
            String::from_utf8_lossy(&locked.stderr).trim()
        ));
    }

    let publishable: Vec<String> = workspace.publishable().map(|p| p.name.clone()).collect();
    let layers = order::layers(&workspace, &publishable).map_or(0, |layers| layers.len());
    Ok(Report {
        version,
        crates: publishable.len(),
        layers,
        problems,
    })
}
