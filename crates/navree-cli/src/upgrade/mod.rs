//! `navree upgrade [--check] [PATH]`: moves an app's or crate's Rust files and
//! `Cargo.toml` off `chrono` (and `navree_orm::Timestamp`) onto Navree's own
//! dates and times (`navree::datetime` / `navree_datetime` and `HasTimestamps`).

pub mod rewrite;
pub mod rules;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use toml_edit::{DocumentMut, InlineTable, Item, Value};

pub use rewrite::{FileContext, RewriteOutcome, TargetCrateOrDefault, UpgradeNote, rewrite_source};
pub use rules::TargetCrate;

/// Summary of one file changed (or that would change under `--check`) by [`run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileReport {
    /// Full path of the file.
    pub path: PathBuf,
    /// Slash-separated path relative to the `root` passed to [`run`].
    pub relative: String,
    /// Number of rewrites applied in this file (`1` for a `Cargo.toml` dependency update).
    pub rewrites: usize,
    /// Manual `// navree upgrade:` notes added to this file.
    pub notes: Vec<UpgradeNote>,
}

/// Summary returned by [`run`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Whether [`run`] was invoked in `--check` mode (nothing written to disk).
    pub check_only: bool,
    /// Files modified (or that would be modified under `--check`), sorted by relative path.
    pub files: Vec<FileReport>,
    /// Total rewrites across all files in [`files`](Self::files).
    pub rewrites: usize,
    /// Total `// navree upgrade:` notes added across all files.
    pub notes: usize,
}

impl Report {
    /// Whether no file needed any change.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Relative slash-normalized paths of every changed (or would-change) file.
    pub fn relative_paths(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.relative.as_str()).collect()
    }
}

#[derive(Clone, Debug)]
struct PackageMeta {
    manifest_path: PathBuf,
    name: String,
    target: TargetCrate,
    is_navree_orm: bool,
    depends_on_navree: bool,
}

#[derive(Default)]
struct PackageTally {
    any_chrono_left: bool,
    src_uses_datetime: bool,
    dev_uses_datetime: bool,
}

/// Walks `root` (a directory or a single `.rs` file), rewrites `chrono` and
/// `navree_orm::Timestamp` uses onto `navree::datetime` (or `navree_datetime`
/// inside Navree's own library crates), updates `Cargo.toml` when `chrono` is
/// no longer used, and writes changed files unless `check_only` is `true`.
pub fn run(root: &Path, check_only: bool) -> anyhow::Result<Report> {
    anyhow::ensure!(root.exists(), "{} does not exist", root.display());

    if root.is_file() {
        return run_single_file(root, check_only);
    }

    let mut rs_files = Vec::new();
    collect_rs_files(root, &mut rs_files)?;
    rs_files.sort();

    let mut pkg_cache: BTreeMap<PathBuf, Option<PackageMeta>> = BTreeMap::new();
    let mut pkg_tallies: BTreeMap<PathBuf, PackageTally> = BTreeMap::new();
    let mut report = Report {
        check_only,
        ..Report::default()
    };

    for path in &rs_files {
        let pkg = resolve_package_for_file(path, root, &mut pkg_cache);
        let (target, is_navree_orm) = match &pkg {
            Some(meta) => (meta.target, meta.is_navree_orm),
            None => (TargetCrate::Facade, false),
        };

        let original = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;

        let outcome = rewrite::rewrite_source_with(
            &original,
            FileContext {
                target: TargetCrateOrDefault(target),
                is_navree_orm,
            },
        )
        .with_context(|| format!("cannot upgrade {}", path.display()))?;

        if let Some(meta) = &pkg {
            let pkg_dir = meta.manifest_path.parent().unwrap_or(root);
            let tally = pkg_tallies.entry(meta.manifest_path.clone()).or_default();
            if outcome.still_uses_chrono {
                tally.any_chrono_left = true;
            }
            if outcome.uses_datetime {
                if is_dev_target_file(path, pkg_dir) {
                    tally.dev_uses_datetime = true;
                } else {
                    tally.src_uses_datetime = true;
                }
            }
        }

        if outcome.source != original {
            if !check_only {
                std::fs::write(path, &outcome.source)
                    .with_context(|| format!("cannot write {}", path.display()))?;
            }
            report.rewrites += outcome.rewrites;
            report.notes += outcome.notes.len();
            report.files.push(FileReport {
                path: path.clone(),
                relative: relative_slash(root, path),
                rewrites: outcome.rewrites,
                notes: outcome.notes,
            });
        }
    }

    // Update manifests residing at or under `root` whose `.rs` files were scanned.
    let mut manifests = Vec::new();
    collect_manifests(root, &mut manifests)?;
    manifests.sort();

    for manifest_path in manifests {
        if !manifest_path.starts_with(root) {
            continue;
        }
        let Some(meta) = read_package_meta(&manifest_path) else {
            continue;
        };
        let tally = pkg_tallies.remove(&manifest_path).unwrap_or_default();
        let original = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("cannot read {}", manifest_path.display()))?;
        if let Some(updated) = update_cargo_toml(&original, &meta, &tally)
            && updated != original
        {
            if !check_only {
                std::fs::write(&manifest_path, &updated)
                    .with_context(|| format!("cannot write {}", manifest_path.display()))?;
            }
            report.rewrites += 1;
            report.files.push(FileReport {
                path: manifest_path.clone(),
                relative: relative_slash(root, &manifest_path),
                rewrites: 1,
                notes: Vec::new(),
            });
        }
    }

    report.files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(report)
}

fn run_single_file(file: &Path, check_only: bool) -> anyhow::Result<Report> {
    let parent = file.parent().unwrap_or_else(|| Path::new("."));
    let mut cache = BTreeMap::new();
    let pkg = resolve_package_for_file(file, parent, &mut cache);
    let (target, is_navree_orm) = match &pkg {
        Some(meta) => (meta.target, meta.is_navree_orm),
        None => (TargetCrate::Facade, false),
    };

    let original =
        std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    let outcome = rewrite::rewrite_source_with(
        &original,
        FileContext {
            target: TargetCrateOrDefault(target),
            is_navree_orm,
        },
    )
    .with_context(|| format!("cannot upgrade {}", file.display()))?;

    let mut report = Report {
        check_only,
        ..Report::default()
    };
    if outcome.source != original {
        if !check_only {
            std::fs::write(file, &outcome.source)
                .with_context(|| format!("cannot write {}", file.display()))?;
        }
        report.rewrites = outcome.rewrites;
        report.notes = outcome.notes.len();
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string_lossy().into_owned());
        report.files.push(FileReport {
            path: file.to_path_buf(),
            relative: name,
            rewrites: outcome.rewrites,
            notes: outcome.notes,
        });
    }
    Ok(report)
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if should_skip_dir(name.to_string_lossy().as_ref()) {
                continue;
            }
            collect_rs_files(&path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn collect_manifests(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if should_skip_dir(name.to_string_lossy().as_ref()) {
                continue;
            }
            collect_manifests(&path, out)?;
        } else if file_type.is_file() && entry.file_name() == "Cargo.toml" {
            out.push(path);
        }
    }
    Ok(())
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".next" | "dist" | "vendor" | "templates"
    )
}

fn is_dev_target_file(path: &Path, pkg_dir: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(pkg_dir) else {
        return false;
    };
    let Some(first) = rel.components().next() else {
        return false;
    };
    matches!(
        first.as_os_str().to_string_lossy().as_ref(),
        "tests" | "benches" | "examples"
    )
}

fn resolve_package_for_file(
    file: &Path,
    stop_root: &Path,
    cache: &mut BTreeMap<PathBuf, Option<PackageMeta>>,
) -> Option<PackageMeta> {
    let start_dir = file.parent()?;
    // Walk up from `start_dir` to `stop_root`, and if none has `[package]`, check ancestors above
    // `stop_root` only if `stop_root` is inside a crate (`src/`).
    for ancestor in start_dir.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if candidate.is_file() {
            let meta = cache
                .entry(candidate.clone())
                .or_insert_with(|| read_package_meta(&candidate))
                .clone();
            if meta.is_some() {
                return meta;
            }
            // Stop at a workspace root `Cargo.toml` so standalone fixture folders inside a
            // temp directory never climb out to an unrelated parent repo.
            if ancestor == stop_root || manifest_has_workspace(&candidate) {
                break;
            }
        }
        if ancestor == stop_root {
            // Only keep climbing above `stop_root` when `stop_root` is a subfolder like `src/`
            // inside an app or crate (stop before leaving `stop_root` if no Cargo.toml exists
            // within 3 levels).
            for up in stop_root.ancestors().skip(1).take(3) {
                let cand = up.join("Cargo.toml");
                if cand.is_file() {
                    return cache
                        .entry(cand.clone())
                        .or_insert_with(|| read_package_meta(&cand))
                        .clone();
                }
            }
            break;
        }
    }
    None
}

fn manifest_has_workspace(manifest_path: &Path) -> bool {
    std::fs::read_to_string(manifest_path)
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok())
        .is_some_and(|t| t.contains_key("workspace"))
}

fn read_package_meta(manifest_path: &Path) -> Option<PackageMeta> {
    let raw = std::fs::read_to_string(manifest_path).ok()?;
    let parsed: toml::Table = raw.parse().ok()?;
    let pkg = parsed.get("package")?.as_table()?;
    let name = pkg.get("name")?.as_str()?.to_owned();

    let has_dep = |crate_name: &str| {
        ["dependencies", "dev-dependencies", "build-dependencies"]
            .iter()
            .any(|section| {
                parsed
                    .get(*section)
                    .and_then(toml::Value::as_table)
                    .is_some_and(|tbl| tbl.keys().any(|k| k.replace('_', "-") == crate_name))
            })
    };

    let has_direct_dt = has_dep("navree-datetime");
    let depends_on_navree = has_dep("navree");
    let is_internal_lib_crate = name == "navree-datetime"
        || (name.starts_with("navree-")
            && !name.starts_with("navree-plugin-")
            && name != "navree-cli");

    let target = if has_direct_dt || (!depends_on_navree && is_internal_lib_crate) {
        TargetCrate::Direct
    } else {
        TargetCrate::Facade
    };

    Some(PackageMeta {
        manifest_path: manifest_path.to_path_buf(),
        is_navree_orm: name == "navree-orm",
        name,
        target,
        depends_on_navree,
    })
}

fn update_cargo_toml(cargo_toml: &str, meta: &PackageMeta, tally: &PackageTally) -> Option<String> {
    if meta.name == "navree-datetime" {
        return None;
    }
    let mut doc: DocumentMut = cargo_toml.parse().ok()?;
    let mut changed = false;

    // For Navree's own library crates using `navree_datetime`, add `navree-datetime = { workspace = true }`
    // where `chrono = { workspace = true }` was used.
    if meta.target == TargetCrate::Direct && (tally.src_uses_datetime || tally.dev_uses_datetime) {
        let uses_workspace = uses_workspace_chrono(&doc);
        if uses_workspace {
            let section = if tally.src_uses_datetime {
                "dependencies"
            } else {
                "dev-dependencies"
            };
            let already_in_deps = table_has_dep(&doc, "dependencies", "navree-datetime")
                || (section == "dev-dependencies"
                    && table_has_dep(&doc, "dev-dependencies", "navree-datetime"));
            if !already_in_deps
                && let Some(tbl) = doc.get_mut(section).and_then(Item::as_table_like_mut)
            {
                let mut inline = InlineTable::new();
                inline.insert("workspace", Value::from(true));
                tbl.insert("navree-datetime", Item::Value(Value::InlineTable(inline)));
                changed = true;
            }
        }
    }

    // Remove `chrono` when no `.rs` file in the package references `chrono` anymore, AND the
    // package now has its datetime dependency (`navree` or `navree-datetime`).
    let can_drop_chrono = !tally.any_chrono_left
        && (meta.depends_on_navree
            || table_has_dep(&doc, "dependencies", "navree-datetime")
            || table_has_dep(&doc, "dev-dependencies", "navree-datetime")
            || (!tally.src_uses_datetime && !tally.dev_uses_datetime));

    if can_drop_chrono {
        for section in ["dependencies", "dev-dependencies"] {
            if let Some(tbl) = doc.get_mut(section).and_then(Item::as_table_like_mut)
                && tbl.remove("chrono").is_some()
            {
                changed = true;
            }
        }
    }

    changed.then(|| doc.to_string())
}

fn table_has_dep(doc: &DocumentMut, section: &str, dep: &str) -> bool {
    doc.get(section)
        .and_then(Item::as_table_like)
        .is_some_and(|tbl| {
            tbl.iter()
                .any(|(k, v)| !v.is_none() && k.replace('_', "-") == dep)
        })
}

fn uses_workspace_chrono(doc: &DocumentMut) -> bool {
    ["dependencies", "dev-dependencies"].iter().any(|section| {
        doc.get(section)
            .and_then(Item::as_table_like)
            .and_then(|tbl| tbl.get("chrono"))
            .and_then(Item::as_table_like)
            .and_then(|t| t.get("workspace"))
            .and_then(Item::as_bool)
            == Some(true)
    })
}

fn relative_slash(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
