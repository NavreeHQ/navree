//! `navree plugin:remove`: unwiring a plugin's module line and removing its crate
//! while leaving configuration, grants, page sources, environment names, features and data.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::forward::Runner;
use crate::plugin::cargo_toml::{Source, dependency_source};
use crate::plugin::files::{AppFiles, AppRoots, Snapshot, Written, find_app, write_all};
use crate::plugin::grants;
use crate::plugin::index::{Index, IndexKind};
use crate::plugin::manifest::{self, AppFacts, Manifest, shown};
use crate::plugin::metadata::{self, Metadata, Package};
use crate::plugin::wiring::{module_line, unwire};

/// What `navree plugin:remove` was asked.
#[derive(Clone, Debug, Default)]
pub struct RemoveOptions {
    /// Package name or first-party short name (`navree-plugin-announcements`, `billing`).
    pub package: String,
}

fn canonical_name(name: &str) -> String {
    name.replace('-', "_")
}

fn find_resolved_dependency<'a>(
    metadata: &'a Metadata,
    roots: &AppRoots,
    package: &str,
    source: &Source,
) -> Option<&'a Package> {
    match source {
        Source::Path(rel_dir) => metadata
            .dependency_at(&roots.package.join(rel_dir))
            .or_else(|| {
                let wanted = canonical_name(package);
                metadata
                    .resolved_packages()
                    .into_iter()
                    .find(|pkg| pkg.source.is_none() && canonical_name(&pkg.name) == wanted)
            }),
        Source::Registry | Source::Git(_) => metadata.dependency(package),
        Source::Workspace => {
            let wanted = canonical_name(package);
            metadata
                .resolved_packages()
                .into_iter()
                .find(|pkg| canonical_name(&pkg.name) == wanted)
                .or_else(|| {
                    metadata
                        .packages()
                        .iter()
                        .find(|pkg| canonical_name(&pkg.name) == wanted)
                })
        }
    }
}

fn line_has_ui_policy_mark(line: &str, package: &str) -> bool {
    let needle = format!("plugin:add {package}");
    let mut rest = line;
    while let Some(pos) = rest.find(&needle) {
        let after = &rest[pos + needle.len()..];
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            return true;
        }
        rest = after;
    }
    false
}

fn env_present(example: Option<&str>, name: &str) -> bool {
    let Some(raw) = example else {
        return false;
    };
    for line in raw.lines() {
        let trimmed = line.trim_start();
        let candidate = trimmed.strip_prefix("# ").unwrap_or(trimmed);
        if let Some((key, _)) = candidate.split_once('=')
            && key == name
        {
            return true;
        }
    }
    false
}

fn navree_kept_features(
    cargo_toml: &str,
    metadata: &Metadata,
    manifest_features: &[String],
) -> Vec<String> {
    if manifest_features.is_empty() {
        return Vec::new();
    }
    let enabled = metadata.navree_enabled();
    let mut in_cargo = BTreeSet::new();
    let mut defaults_on = true;
    if let Ok(parsed) = cargo_toml.parse::<toml::Table>()
        && let Some(dep) = parsed.get("dependencies").and_then(|d| d.get("navree"))
    {
        if let Some(tbl) = dep.as_table() {
            if tbl.get("default-features").and_then(toml::Value::as_bool) == Some(false) {
                defaults_on = false;
            }
            if let Some(arr) = tbl.get("features").and_then(toml::Value::as_array) {
                for item in arr.iter().filter_map(toml::Value::as_str) {
                    in_cargo.insert(item.to_owned());
                }
            }
        }
    }
    manifest_features
        .iter()
        .filter(|feat| {
            enabled.contains(feat.as_str())
                || in_cargo.contains(feat.as_str())
                || (defaults_on && enabled.is_empty() && in_cargo.is_empty())
        })
        .cloned()
        .collect()
}

fn join_kept_items(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{}, and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

fn build_kept_line(
    roots: &AppRoots,
    files: &AppFiles,
    metadata: &Metadata,
    package: &str,
    dep_source: &Source,
    manifest: Option<&Manifest>,
) -> String {
    let mut items = Vec::new();

    if let Some(manifest) = manifest {
        for section in &manifest.config {
            let rel = format!("config/{section}.toml");
            if roots.resolve(&rel).is_file() {
                items.push(rel);
            }
        }
    }

    let grant_mark = format!("# plugin:add {package}");
    if files.text("config/workspace.toml").is_some_and(|text| {
        text.lines()
            .any(|line| line.trim_end().ends_with(&grant_mark))
    }) {
        items.push("the grants in config/workspace.toml".to_owned());
    }

    if files.text("config/ui.policy.toml").is_some_and(|text| {
        text.lines()
            .any(|line| line_has_ui_policy_mark(line, package))
    }) {
        items.push("the page sources in config/ui.policy.toml".to_owned());
    }

    if let Some(manifest) = manifest {
        for env_name in &manifest.env {
            if env_present(files.env_example.as_deref(), env_name) {
                items.push(format!("{env_name} in .env.example"));
            }
        }

        let kept_features = navree_kept_features(&files.cargo_toml, metadata, &manifest.features);
        if !kept_features.is_empty() {
            items.push(format!("the features navree: {}", kept_features.join(", ")));
        }
    }

    if let Source::Path(rel_dir) = dep_source {
        let normalized = rel_dir.replace('\\', "/");
        let trimmed = normalized.trim_end_matches('/');
        if !trimmed.is_empty() && roots.package.join(rel_dir).exists() {
            items.push(format!("{trimmed}/"));
        }
    }

    items.push("the plugin's tables and rows".to_owned());
    format!("  kept     {}", join_kept_items(&items))
}

/// Unwires and removes a plugin; leaves configuration, grants, env names, features and data.
pub fn run(
    cwd: &Path,
    options: &RemoveOptions,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let roots = find_app(cwd).map_err(|msg| anyhow::anyhow!("{msg}"))?;

    if options.package == "navree" {
        anyhow::bail!("`navree` is the framework, not a plugin");
    }

    let index = Index::embedded();
    if let Some(entry) = index.find(&options.package) {
        match entry.kind {
            IndexKind::Tool => {
                let cmd = entry.tool_command.as_deref().unwrap_or("navree");
                anyhow::bail!("`{}` is a tool, not a plugin: use `{cmd}`", entry.name);
            }
            IndexKind::Feature => {
                anyhow::bail!(
                    "`{}` is a feature of navree, not a plugin: add \"{}\" to navree's features in Cargo.toml",
                    entry.name,
                    entry.feature
                );
            }
            IndexKind::Dev => {
                writeln!(
                    out,
                    "  kept     the feature navree: {} (other code may use it)",
                    entry.feature
                )?;
                return Ok(());
            }
            IndexKind::Module => {
                let files = AppFiles::read(&roots)?;
                let mod_path = entry
                    .module
                    .as_deref()
                    .expect("first-party module entry has module");
                let line = module_line(mod_path);
                let wire_line = if let Some(updated) =
                    files.app_rs.as_deref().and_then(|app| unwire(app, &line))
                {
                    write_all(&roots, &files, &[("src/app.rs".to_owned(), updated)])
                        .map_err(|err| anyhow::anyhow!("{}", err.message("plugin:remove")))?;
                    format!("  unwired  {line} (src/app.rs)")
                } else {
                    format!("  next     remove {line} from the file that wires it")
                };
                writeln!(out, "{wire_line}")?;
                writeln!(
                    out,
                    "  kept     the feature navree: {} (other code may use it)",
                    entry.feature
                )?;
                return Ok(());
            }
        }
    }

    let files = AppFiles::read(&roots)?;
    let Some(dep_source) = dependency_source(&files.cargo_toml, &options.package) else {
        anyhow::bail!(
            "{} is not a dependency of this app",
            shown(&options.package)
        );
    };

    let snapshot = Snapshot::take(&roots)?;

    let (meta_ok, meta_stdout, meta_stderr) =
        runner.capture_both(&metadata::invocation(), &roots.package)?;
    if !meta_ok {
        let _ = snapshot.restore(&roots, &files, &Written::default());
        anyhow::bail!(
            "cargo metadata failed: {}; nothing was changed",
            metadata::cargo_error_line(&meta_stderr)
        );
    }
    let meta = match Metadata::parse(&meta_stdout) {
        Ok(parsed) => parsed,
        Err(err) => {
            let _ = snapshot.restore(&roots, &files, &Written::default());
            anyhow::bail!(
                "cargo metadata failed: {}; nothing was changed",
                shown(&err)
            );
        }
    };

    let Some(pkg) = find_resolved_dependency(&meta, &roots, &options.package, &dep_source) else {
        let _ = snapshot.restore(&roots, &files, &Written::default());
        anyhow::bail!(
            "{} is not a dependency of this app",
            shown(&options.package)
        );
    };

    let Some(ref plugin_table) = pkg.plugin_table else {
        let _ = snapshot.restore(&roots, &files, &Written::default());
        anyhow::bail!(
            "{} {} declares no plugin ([package.metadata.navree.plugin] is missing); nothing was changed",
            shown(&pkg.name),
            shown(&pkg.version)
        );
    };

    let role_sources = grants::scan(&files, &[]);
    let navree_pkg = meta.navree();
    let navree_features: BTreeSet<String> = match navree_pkg {
        Some(n) if !n.features.is_empty() => n.features.keys().cloned().collect(),
        _ => index.manifest_features.iter().cloned().collect(),
    };
    let facts = AppFacts {
        first_party: false,
        navree_version: navree_pkg
            .map(|n| n.version.clone())
            .unwrap_or_else(|| "1.4.0".to_owned()),
        navree_features,
        allowed_features: index.manifest_features.iter().cloned().collect(),
        reserved: index.reserved.iter().cloned().collect(),
        roles: role_sources.roles(),
        demo_guest_role: role_sources.demo_guest_role(),
        granted: Vec::new(),
    };

    let checked_manifest = manifest::read(&pkg.name, plugin_table, &facts).ok();
    let crate_name = pkg.name.replace('-', "_");
    let lib_matches = pkg.lib_name.as_deref() == Some(crate_name.as_str());

    let (written, wire_line) = match (&checked_manifest, lib_matches) {
        (Some(manifest), true) => {
            let line = module_line(&format!("{crate_name}::{}", manifest.module.as_str()));
            if let Some(updated) = files.app_rs.as_deref().and_then(|app| unwire(app, &line)) {
                match write_all(&roots, &files, &[("src/app.rs".to_owned(), updated)]) {
                    Ok(written) => (written, format!("  unwired  {line} (src/app.rs)")),
                    Err(err) => {
                        let _ = snapshot.restore(&roots, &files, &Written::default());
                        anyhow::bail!("{}", err.message("plugin:remove"));
                    }
                }
            } else {
                (
                    Written::default(),
                    format!("  next     remove {line} from the file that wires it"),
                )
            }
        }
        _ => (
            Written::default(),
            "  next     remove .module(..) from the file that wires it".to_owned(),
        ),
    };

    let (rm_ok, _, rm_stderr) =
        runner.capture_both(&metadata::cargo(&["remove", &pkg.name]), &roots.package)?;
    if !rm_ok {
        let not_put_back = snapshot.restore(&roots, &files, &written);
        let err_line = metadata::cargo_error_line(&rm_stderr);
        if not_put_back.is_empty() {
            anyhow::bail!("cargo remove failed: {err_line}; nothing was changed");
        }
        anyhow::bail!(
            "cargo remove failed: {err_line}; these files could not be put back: {}",
            not_put_back.join(", ")
        );
    }

    let kept_line = build_kept_line(
        &roots,
        &files,
        &meta,
        &pkg.name,
        &dep_source,
        checked_manifest.as_ref(),
    );

    writeln!(out, "{wire_line}")?;
    writeln!(out, "  removed  {} (Cargo.toml)", shown(&pkg.name))?;
    writeln!(out, "{kept_line}")?;
    Ok(())
}
