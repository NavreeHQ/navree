//! `navree plugin:list` and the plugin and library-name warnings printed by `navree check`.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::forward::Runner;
use crate::plugin::files::{AppFiles, Snapshot, Written, find_app, read_src};
use crate::plugin::grants;
use crate::plugin::index::{Index, IndexKind};
use crate::plugin::manifest::{self, AppFacts, ModulePath, shown};
use crate::plugin::metadata::{self, Metadata, Package};
use crate::plugin::wiring::wired_in;

/// What `navree plugin:list` was asked.
#[derive(Clone, Debug, Default)]
pub struct ListOptions {
    /// Print the listing as a JSON array.
    pub json: bool,
}

/// One row of `plugin:list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    pub package: String,
    pub version: String,
    pub first_party: bool,
    pub module: Option<String>,
    pub file: Option<String>,
    pub foreign_navree_lib: Option<String>,
}

#[derive(Serialize)]
struct JsonRow {
    package: String,
    version: String,
    first_party: bool,
    module: Option<String>,
    wired: bool,
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn foreign_navree_library(pkg: &Package) -> Option<&str> {
    let lib = pkg.lib_name.as_deref()?;
    let expected = pkg.name.replace('-', "_");
    (lib.starts_with("navree") && lib != expected).then_some(lib)
}

fn library_mark_text(package: &str, lib: &str) -> String {
    let expected = shown(&package.replace('-', "_"));
    let lib_shown = shown(lib);
    format!("library is `{lib_shown}` (not `{expected}`)")
}

/// Warning lines for resolved packages whose library target name starts with `navree`
/// while differing from the package's own crate name.
pub(crate) fn navree_library_warnings(metadata: &Metadata) -> Vec<String> {
    let mut flagged: Vec<(&str, &str, &str)> = metadata
        .resolved_packages()
        .into_iter()
        .filter_map(|pkg| {
            let lib = foreign_navree_library(pkg)?;
            Some((pkg.name.as_str(), pkg.version.as_str(), lib))
        })
        .collect();
    flagged.sort_unstable();
    flagged.dedup();
    flagged
        .into_iter()
        .map(|(name, version, lib)| {
            let pkg_shown = shown(name);
            let ver_shown = shown(version);
            let lib_shown = shown(lib);
            let expected = shown(&name.replace('-', "_"));
            format!(
                "warning  {pkg_shown} {ver_shown} names its library `{lib_shown}` (not `{expected}`), like a `navree` crate: remove or replace the dependency"
            )
        })
        .collect()
}

/// Computes the rows of `plugin:list`. A `Metadata` with no root package (a workspace
/// run at its virtual root) lists nothing.
pub(crate) fn listed(metadata: &Metadata, index: &Index, src: &[(String, String)]) -> Vec<Listed> {
    listed_with_roles(metadata, index, src, BTreeSet::new(), None)
}

/// Computes the rows of `plugin:list` using the workspace roles and demo guest role
/// configured by the app.
pub(crate) fn listed_with_roles(
    metadata: &Metadata,
    index: &Index,
    src: &[(String, String)],
    roles: BTreeSet<String>,
    demo_guest_role: Option<String>,
) -> Vec<Listed> {
    if metadata.root_dir().is_none() {
        return Vec::new();
    }

    let navree_pkg = metadata.navree();
    let navree_version = navree_pkg
        .map(|n| n.version.clone())
        .unwrap_or_else(|| "1.4.0".to_owned());
    let navree_features: BTreeSet<String> = match navree_pkg {
        Some(n) if !n.features.is_empty() => n.features.keys().cloned().collect(),
        _ => index.manifest_features.iter().cloned().collect(),
    };
    let facts = AppFacts {
        first_party: false,
        navree_version: navree_version.clone(),
        navree_features,
        allowed_features: index.manifest_features.iter().cloned().collect(),
        reserved: index.reserved.iter().cloned().collect(),
        roles,
        demo_guest_role,
        granted: Vec::new(),
    };

    let mut seen_ids = BTreeSet::new();
    let mut community = Vec::new();

    for pkg in metadata.plugin_dependencies() {
        if index.find(&pkg.name).is_some_and(|e| e.krate == pkg.name) {
            continue;
        }
        seen_ids.insert(pkg.id.clone());

        if let Some(lib) = foreign_navree_library(pkg) {
            community.push(Listed {
                package: pkg.name.clone(),
                version: pkg.version.clone(),
                first_party: false,
                module: None,
                file: None,
                foreign_navree_lib: Some(lib.to_owned()),
            });
            continue;
        }

        let crate_name = pkg.name.replace('-', "_");
        if pkg.lib_name.as_deref() != Some(crate_name.as_str()) {
            community.push(Listed {
                package: pkg.name.clone(),
                version: pkg.version.clone(),
                first_party: false,
                module: None,
                file: None,
                foreign_navree_lib: None,
            });
            continue;
        }

        let Some(ref table) = pkg.plugin_table else {
            continue;
        };
        match manifest::read(&pkg.name, table, &facts) {
            Ok(checked) => {
                let full_module = format!("{crate_name}::{}", checked.module.as_str());
                let crate_mention = format!("{crate_name}::");
                let file = wired_in(src, checked.module.type_name(), &crate_mention);
                community.push(Listed {
                    package: pkg.name.clone(),
                    version: pkg.version.clone(),
                    first_party: false,
                    module: Some(full_module),
                    file,
                    foreign_navree_lib: None,
                });
            }
            Err(_) => {
                community.push(Listed {
                    package: pkg.name.clone(),
                    version: pkg.version.clone(),
                    first_party: false,
                    module: None,
                    file: None,
                    foreign_navree_lib: None,
                });
            }
        }
    }

    for pkg in metadata.resolved_packages() {
        if seen_ids.contains(&pkg.id) {
            continue;
        }
        if let Some(lib) = foreign_navree_library(pkg) {
            seen_ids.insert(pkg.id.clone());
            community.push(Listed {
                package: pkg.name.clone(),
                version: pkg.version.clone(),
                first_party: false,
                module: None,
                file: None,
                foreign_navree_lib: Some(lib.to_owned()),
            });
        }
    }

    community.sort_by(|a, b| {
        a.package
            .cmp(&b.package)
            .then_with(|| a.version.cmp(&b.version))
    });

    let enabled = metadata.navree_enabled();
    let mut rows = community;
    for entry in &index.packages {
        if entry.kind != IndexKind::Module || !enabled.contains(&entry.feature) {
            continue;
        }
        let mod_expr = entry
            .module
            .as_deref()
            .expect("first-party module entry has module");
        let parsed = ModulePath::parse(mod_expr).expect("index module path parses");
        let crate_mention = format!("navree::{}", entry.feature.replace('-', "_"));
        let file = wired_in(src, parsed.type_name(), &crate_mention);
        rows.push(Listed {
            package: entry.name.clone(),
            version: navree_version.clone(),
            first_party: true,
            module: Some(mod_expr.to_owned()),
            file,
            foreign_navree_lib: None,
        });
    }

    rows
}

/// Warning lines for each installed community plugin that nothing under `src/` wires.
pub(crate) fn unwired_warnings(listed: &[Listed]) -> Vec<String> {
    listed
        .iter()
        .filter(|row| !row.first_party && row.foreign_navree_lib.is_none() && row.file.is_none())
        .map(|row| {
            let pkg = shown(&row.package);
            format!(
                "warning  {pkg} declares a plugin, but nothing under src/ wires it: run navree plugin:add {pkg}, or remove the dependency"
            )
        })
        .collect()
}

fn pad_right(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count >= width {
        text.to_owned()
    } else {
        format!("{text}{}", " ".repeat(width - count))
    }
}

/// Lists the plugins this app depends on and whether each is wired under `src/`.
pub fn run(
    cwd: &Path,
    options: &ListOptions,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let process_env: Vec<(String, String)> = std::env::vars().collect();
    run_with_env(cwd, options, &process_env, runner, out)
}

/// Lists the plugins this app depends on using `process_env` when scanning configured roles.
pub fn run_with_env(
    cwd: &Path,
    options: &ListOptions,
    process_env: &[(String, String)],
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let roots = find_app(cwd).map_err(|msg| anyhow::anyhow!("{msg}"))?;
    let snapshot = Snapshot::take(&roots)?;

    let (ok, stdout, stderr) = runner.capture_both(&metadata::invocation(), &roots.package)?;
    if !ok {
        let _ = snapshot.restore(
            &roots,
            &Default::default(),
            &Written::default(),
            snapshot.cargo_toml_bytes(),
        );
        anyhow::bail!(
            "cargo metadata failed: {}; nothing was changed",
            metadata::cargo_error_line(&stderr)
        );
    }
    let meta = match Metadata::parse(&stdout) {
        Ok(parsed) => parsed,
        Err(err) => {
            let _ = snapshot.restore(
                &roots,
                &Default::default(),
                &Written::default(),
                snapshot.cargo_toml_bytes(),
            );
            anyhow::bail!(
                "cargo metadata failed: {}; nothing was changed",
                shown(&err)
            );
        }
    };

    let rows = match meta.root_dir() {
        Some(pkg_dir) => {
            let app_roots = find_app(pkg_dir).unwrap_or_else(|_| roots.clone());
            if let Ok(files) = AppFiles::read(&app_roots) {
                let role_sources = grants::scan(&files, process_env);
                listed_with_roles(
                    &meta,
                    Index::embedded(),
                    &files.src,
                    role_sources.roles(),
                    role_sources.demo_guest_role(),
                )
            } else {
                let src = read_src(pkg_dir)?;
                listed(&meta, Index::embedded(), &src)
            }
        }
        None => Vec::new(),
    };

    if options.json {
        let json_rows: Vec<JsonRow> = rows
            .iter()
            .map(|row| {
                let (library, warning) = match &row.foreign_navree_lib {
                    Some(lib) => (Some(shown(lib)), Some(library_mark_text(&row.package, lib))),
                    None => (None, None),
                };
                JsonRow {
                    package: shown(&row.package),
                    version: shown(&row.version),
                    first_party: row.first_party,
                    module: row.module.as_deref().map(shown),
                    wired: row.file.is_some(),
                    file: row.file.clone(),
                    library,
                    warning,
                }
            })
            .collect();
        let encoded = serde_json::to_string(&json_rows)?;
        writeln!(out, "{encoded}")?;
        return Ok(());
    }

    if rows.is_empty() {
        writeln!(out, "No plugins: no dependency of this app declares one.")?;
        return Ok(());
    }

    let pkg_width = rows
        .iter()
        .map(|row| shown(&row.package).chars().count())
        .max()
        .unwrap_or(0);
    let ver_width = rows
        .iter()
        .map(|row| shown(&row.version).chars().count())
        .max()
        .unwrap_or(0);

    for row in &rows {
        let pkg_shown = shown(&row.package);
        let ver_shown = shown(&row.version);
        let pkg_col = pad_right(&pkg_shown, pkg_width);
        let ver_col = pad_right(&ver_shown, ver_width);
        let (status, detail) = if let Some(ref lib) = row.foreign_navree_lib {
            ("warning", library_mark_text(&row.package, lib))
        } else if let Some(ref file) = row.file {
            ("wired", file.clone())
        } else {
            ("not wired", format!("run: navree plugin:add {pkg_shown}"))
        };
        writeln!(out, "{pkg_col}  {ver_col}  {status:<9}  {detail}")?;
    }

    Ok(())
}
