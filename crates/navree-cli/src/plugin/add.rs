//! Installing a plugin into an app (`navree plugin:add`).

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::forward::{Runner, forward};
use crate::plugin::block;
use crate::plugin::cargo_toml::{self, DepTable, Source};
use crate::plugin::files::{AppFiles, AppRoots, Snapshot, Written, find_app, write_all};
use crate::plugin::grants;
use crate::plugin::index::{Index, IndexEntry, IndexKind};
use crate::plugin::manifest::{self, AppFacts, ModulePath, shown};
use crate::plugin::metadata::{self, Metadata};
use crate::plugin::policy;
use crate::plugin::wiring::{self, ShellHint, Wiring};

/// What `navree plugin:add` was asked.
#[derive(Clone, Debug, Default)]
pub struct AddOptions {
    /// First-party short name (`billing`) or community crate name (`navree-plugin-khata`);
    /// optional when `--path` is given.
    pub package: Option<String>,
    /// Cargo version requirement (`^1.2`) for a community crate.
    pub version: Option<String>,
    /// Local plugin crate folder inside the app.
    pub path: Option<PathBuf>,
    /// Skip inserting the `.module(..)` line into `src/app.rs` and print it instead.
    pub no_wire: bool,
    /// Run `config:publish --plugin` and `plugins --json` for a community crate too.
    pub publish_config: bool,
    /// The app's Dock settings, passed when running the app.
    pub app_env: Vec<(String, String)>,
    /// The process environment, scanned for role lists and page sources.
    pub process_env: Vec<(String, String)>,
}

/// How a community package reaches the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum How {
    /// A community crate already a dependency from the same source, asked with no `--version`:
    /// `cargo add` is not run again.
    Present,
    /// `cargo add NAME`, or `cargo add NAME@REQ`, from the registry.
    Registry { requirement: Option<String> },
    /// `cargo add --path DIR`; `dir` is relative to the package folder, with `/`.
    Path { dir: String },
}

/// What package selection decided before Cargo runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    FirstParty(IndexEntry),
    Community { package: String, how: How },
}

/// Chooses the package to install. The `Err` is the refusal text without `error: `.
/// `cwd` is the folder the command ran in, which `--path` is resolved against.
pub(crate) fn choose(
    roots: &AppRoots,
    files: &AppFiles,
    options: &AddOptions,
    cwd: &Path,
    index: &Index,
) -> Result<Choice, String> {
    if let Some(given) = options.package.as_deref() {
        check_package_name(given)?;
    }

    if let Some(requirement) = options.version.as_deref()
        && !is_version_requirement(requirement)
    {
        return Err(format!(
            "--version `{}` is not a Cargo version requirement",
            shown(requirement)
        ));
    }

    if let Some(raw_path) = options.path.as_deref() {
        return choose_path(roots, files, options, cwd, index, raw_path);
    }

    let Some(given) = options.package.as_deref() else {
        return Err("name a package to install, or pass --path".to_owned());
    };

    if let Some(entry) = index.find(given) {
        return match entry.kind {
            IndexKind::Tool => {
                let cmd = entry.tool_command.as_deref().unwrap_or("");
                Err(format!(
                    "`{}` is a tool, not a plugin: use `{cmd}`",
                    entry.name
                ))
            }
            IndexKind::Feature => Err(format!(
                "`{}` is a feature of navree, not a plugin: add \"{}\" to navree's features in Cargo.toml",
                entry.name, entry.feature
            )),
            IndexKind::Module | IndexKind::Dev => {
                if options.version.is_some() {
                    Err(
                        "--version is for community crates; first-party packages follow navree's version"
                            .to_owned(),
                    )
                } else {
                    Ok(Choice::FirstParty(entry.clone()))
                }
            }
        };
    }

    if let Some((existing_name, existing_source)) = dependency_entry(&files.cargo_toml, given) {
        if existing_source != Source::Registry {
            return Err(format!(
                "{existing_name} is already a dependency of this app from {}; run navree plugin:remove {existing_name} first to add it from the registry",
                describe_source(&existing_source)
            ));
        }
        if options.version.is_none() {
            return Ok(Choice::Community {
                package: existing_name,
                how: How::Present,
            });
        }
        return Ok(Choice::Community {
            package: existing_name,
            how: How::Registry {
                requirement: options.version.clone(),
            },
        });
    }

    Ok(Choice::Community {
        package: given.to_owned(),
        how: How::Registry {
            requirement: options.version.clone(),
        },
    })
}

fn choose_path(
    roots: &AppRoots,
    files: &AppFiles,
    options: &AddOptions,
    cwd: &Path,
    index: &Index,
    raw_path: &Path,
) -> Result<Choice, String> {
    let joined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };
    let shown_dir = std::path::absolute(&joined).unwrap_or_else(|_| joined.clone());
    let shown_root = std::path::absolute(&roots.package).unwrap_or_else(|_| roots.package.clone());

    let outside_err = || {
        format!(
            "--path {} is outside the app ({}); keep the plugin's folder inside the app",
            shown_dir.display(),
            shown_root.display()
        )
    };

    if !joined.exists() {
        if !is_lexical_strict_subpath(&shown_dir, &shown_root) {
            return Err(outside_err());
        }
        return Err(format!("--path {} does not exist", shown_dir.display()));
    }

    let Ok(canon_dir) = fs::canonicalize(&joined) else {
        return Err(format!("--path {} does not exist", shown_dir.display()));
    };
    let Ok(canon_root) = fs::canonicalize(&roots.package) else {
        return Err(outside_err());
    };
    if !canon_dir.starts_with(&canon_root) || canon_dir == canon_root {
        return Err(outside_err());
    }

    let rel_path = canon_dir
        .strip_prefix(&canon_root)
        .map_err(|_| outside_err())?;
    let mut segments = Vec::new();
    for comp in rel_path.components() {
        if let Component::Normal(seg) = comp {
            let Some(text) = seg.to_str() else {
                return Err(outside_err());
            };
            segments.push(text);
        }
    }
    if segments.is_empty() {
        return Err(outside_err());
    }
    let rel_dir = segments.join("/");

    let manifest_path = canon_dir.join("Cargo.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let parsed: toml::Table = manifest_text
        .parse()
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let Some(found) = parsed
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|pkg| pkg.get("name"))
        .and_then(toml::Value::as_str)
    else {
        return Err(format!(
            "{} holds no Cargo package (missing [package] name)",
            shown_dir.display()
        ));
    };

    check_package_name(found)?;

    if let Some(given) = options.package.as_deref()
        && !same_package_name(given, found)
    {
        return Err(format!(
            "{} holds the package {}, not {}",
            shown_dir.display(),
            shown(found),
            shown(given)
        ));
    }

    let is_first_party_name = index.find(found).is_some()
        || index
            .packages
            .iter()
            .any(|entry| same_package_name(&entry.krate, found));
    if is_first_party_name {
        return Err(format!(
            "{} is the name of a first-party package; give the local crate a name of its own",
            shown(found)
        ));
    }

    if let Some((existing_name, existing_source)) = dependency_entry(&files.cargo_toml, found) {
        let same_path = matches!(
            &existing_source,
            Source::Path(existing_dir) if normalize_rel_dir(existing_dir) == rel_dir
        );
        if !same_path {
            return Err(format!(
                "{existing_name} is already a dependency of this app from {}; run navree plugin:remove {existing_name} first to add it from the folder {rel_dir}",
                describe_source(&existing_source)
            ));
        }
        if options.version.is_none() {
            return Ok(Choice::Community {
                package: existing_name,
                how: How::Present,
            });
        }
    }

    Ok(Choice::Community {
        package: found.to_owned(),
        how: How::Path { dir: rel_dir },
    })
}

fn check_package_name(name: &str) -> Result<(), String> {
    if !is_package_name(name) {
        return Err(format!(
            "`{}` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters",
            shown(name)
        ));
    }
    if name == "navree" {
        return Err("`navree` is the framework, not a plugin".to_owned());
    }
    Ok(())
}

fn is_package_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'-' | b'_')
        })
}

fn is_version_requirement(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= 64
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'*' | b'^' | b'~' | b'=' | b'<' | b'>' | b',' | b'+' | b'-' | b' '
                )
        })
}

fn same_package_name(left: &str, right: &str) -> bool {
    let norm = |byte: u8| if byte == b'_' { b'-' } else { byte };
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(a, b)| norm(a) == norm(b))
}

fn normalize_rel_dir(dir: &str) -> String {
    let slash = dir.replace('\\', "/");
    let trimmed = slash.trim_start_matches("./").trim_end_matches('/');
    trimmed.to_owned()
}

fn describe_source(source: &Source) -> String {
    match source {
        Source::Registry => "the registry".to_owned(),
        Source::Path(path) => format!("the folder {}", normalize_rel_dir(path)),
        Source::Git(_) => "a git repository".to_owned(),
        Source::Workspace => "the workspace".to_owned(),
    }
}

fn dependency_entry(cargo_toml: &str, package: &str) -> Option<(String, Source)> {
    let parsed: toml::Table = cargo_toml.parse().ok()?;
    let deps = parsed.get("dependencies")?.as_table()?;
    for (key, value) in deps {
        let actual = value
            .as_table()
            .and_then(|tbl| tbl.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(key.as_str());
        if same_package_name(actual, package) {
            let source = crate::plugin::cargo_toml::dependency_source(cargo_toml, package)?;
            return Some((key.clone(), source));
        }
    }
    None
}

fn lexical_clean(path: &Path) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            other => {
                let text = other.as_os_str().to_string_lossy();
                #[cfg(windows)]
                parts.push(text.to_lowercase());
                #[cfg(not(windows))]
                parts.push(text.into_owned());
            }
        }
    }
    parts
}

fn is_lexical_strict_subpath(dir: &Path, root: &Path) -> bool {
    let dir_parts = lexical_clean(dir);
    let root_parts = lexical_clean(root);
    dir_parts.len() > root_parts.len() && dir_parts[..root_parts.len()] == root_parts[..]
}

/// What package resolution and `cargo metadata` settled; `plan` reads it.
#[derive(Clone, Debug)]
pub(crate) struct Chosen {
    /// Name used in printed lines: the package as Cargo wrote it, or a first-party short name.
    pub name: String,
    /// Version Cargo resolved; for a first-party package, `navree`'s version.
    pub version: String,
    /// Package name with `-` turned into `_`.
    pub crate_name: String,
    /// Library target name from `cargo metadata`.
    pub lib_name: Option<String>,
    /// `Some` for a first-party package: its index entry stands for the manifest.
    pub entry: Option<IndexEntry>,
    /// Community crate's plugin table from `cargo metadata`; `None` for a first-party package.
    pub table: Option<serde_json::Value>,
    /// Whether the community dependency was already in `Cargo.toml` before this run.
    pub already_present: bool,
}

/// Planned file edits and output lines for `plugin:add`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Plan {
    pub edits: Vec<(String, String)>,
    pub lines: Vec<String>,
    pub tail: Vec<String>,
    pub run: Option<String>,
}

/// Checks `chosen`'s manifest against `facts` and plans every edit and printed line in memory.
pub(crate) fn plan(
    files: &AppFiles,
    chosen: &Chosen,
    facts: &AppFacts,
    metadata: &Metadata,
    options: &AddOptions,
) -> Result<Plan, String> {
    if let Some(entry) = &chosen.entry {
        return plan_first_party(files, entry, metadata, options);
    }

    let Some(table) = &chosen.table else {
        return Err(format!(
            "{} {} declares no plugin ([package.metadata.navree.plugin] is missing); nothing was changed",
            shown(&chosen.name),
            shown(&chosen.version)
        ));
    };

    let manifest = manifest::read(&chosen.name, table, facts).map_err(|reason| {
        format!(
            "{} {}: [package.metadata.navree.plugin] {reason}; nothing was changed",
            shown(&chosen.name),
            shown(&chosen.version)
        )
    })?;

    if let Some(lib_name) = &chosen.lib_name {
        if lib_name != &chosen.crate_name {
            return Err(format!(
                "{} {}: its library is named `{}`, not `{}`; nothing was changed",
                shown(&chosen.name),
                shown(&chosen.version),
                shown(lib_name),
                shown(&chosen.crate_name)
            ));
        }
    } else {
        return Err(format!(
            "{} {}: its library is named ``, not `{}`; nothing was changed",
            shown(&chosen.name),
            shown(&chosen.version),
            shown(&chosen.crate_name)
        ));
    }

    let mut edits = Vec::new();
    let mut lines = Vec::new();
    let mut tail = Vec::new();

    if chosen.already_present {
        lines.push(format!(
            "  kept     {} {} is already in Cargo.toml",
            shown(&chosen.name),
            shown(&chosen.version)
        ));
    } else {
        lines.push(format!(
            "  added    {} {} (Cargo.toml)",
            shown(&chosen.name),
            shown(&chosen.version)
        ));
    }

    if let Some((new_cargo_toml, feature_line)) =
        plan_features(&files.cargo_toml, metadata, &manifest.features, false)?
    {
        edits.push(("Cargo.toml".to_owned(), new_cargo_toml));
        lines.push(feature_line);
    }

    let full_module = format!("{}::{}", chosen.crate_name, manifest.module.as_str());
    let line = wiring::module_line(&full_module);
    let crate_mention = format!("{}::", chosen.crate_name);
    let wired_outcome = wiring::wire(
        files,
        &line,
        manifest.module.type_name(),
        &crate_mention,
        options.no_wire,
    );
    let (is_wired, needs_migrate) = record_wiring(
        wired_outcome,
        &line,
        manifest.module.type_name(),
        &mut edits,
        &mut lines,
    );

    let (env_edit, env_statuses) =
        wiring::add_env(files.env_example.as_deref(), &chosen.name, &manifest.env);
    if let Some(new_env_example) = env_edit {
        edits.push((".env.example".to_owned(), new_env_example));
    }
    for (var_name, added) in env_statuses {
        if added {
            lines.push(format!("  env      {var_name} (.env.example)"));
        } else {
            lines.push(format!("  kept     {var_name} is already in .env.example"));
        }
    }

    let run_ns = if is_wired && options.publish_config {
        Some(manifest.namespace.clone())
    } else {
        None
    };

    if run_ns.is_none() && !manifest.config.is_empty() {
        lines.push(format!(
            "  next     navree config:publish --plugin {}",
            manifest.namespace
        ));
    }

    let set_in_code = grants::set_in_code(&files.src);
    let role_sources = grants::scan(files, &options.process_env);
    let (ws_edit, grant_lines) = grants::plan(
        &role_sources,
        files
            .config
            .get("config/workspace.toml")
            .map(String::as_str),
        &chosen.name,
        &manifest.grants,
        set_in_code.as_deref(),
    )
    .map_err(|err| format!("cannot read config/workspace.toml: {err}; nothing was changed"))?;
    if let Some(new_ws) = ws_edit {
        edits.push(("config/workspace.toml".to_owned(), new_ws));
    }
    for grant_line in grant_lines {
        tail.push(grant_line.text());
    }

    let (policy_edit, policy_lines) = policy::plan(
        files,
        &options.process_env,
        &chosen.name,
        &manifest.namespace,
        &manifest.policy,
        set_in_code.as_deref(),
    );
    if let Some(new_policy) = policy_edit {
        edits.push(("config/ui.policy.toml".to_owned(), new_policy));
    }
    tail.extend(policy_lines);

    if needs_migrate {
        tail.push("  next     navree migrate".to_owned());
    }

    if manifest.features.iter().any(|feat| feat == "ui") {
        let shell_source = files.shell_rs.as_deref().or_else(|| {
            files
                .src
                .iter()
                .find(|(path, _)| path == "src/ui/shell.rs")
                .map(|(_, text)| text.as_str())
        });
        match wiring::shell_hint(shell_source) {
            ShellHint::Nothing => {}
            ShellHint::Contributed => tail.push(
                "  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs"
                    .to_owned(),
            ),
            ShellHint::ForRequest => tail.push(
                "  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs, and build Desk's page from the AppShell extractor (.context(..), .user_menu(..)) instead of Shell::for_request; see /docs/ui/app-shell/"
                    .to_owned(),
            ),
        }
    }

    if run_ns.is_none() {
        tail.push(format!(
            "  next     navree plugins, to see what {} registers (it builds and runs the app)",
            shown(&chosen.name)
        ));
    }

    tail.push(format!(
        "Installed {} {}. It is not an official Navree package: read its code before you deploy it.",
        shown(&chosen.name),
        shown(&chosen.version)
    ));

    Ok(Plan {
        edits,
        lines,
        tail,
        run: run_ns,
    })
}

fn plan_first_party(
    files: &AppFiles,
    entry: &IndexEntry,
    metadata: &Metadata,
    options: &AddOptions,
) -> Result<Plan, String> {
    let mut edits = Vec::new();
    let mut lines = Vec::new();
    let mut tail = Vec::new();

    if entry.kind == IndexKind::Dev {
        let edit = cargo_toml::turn_on(
            &files.cargo_toml,
            DepTable::DevDependencies,
            std::slice::from_ref(&entry.feature),
        )
        .map_err(|err| format!("Cargo.toml cannot be read: {err}; nothing was changed"))?;
        match edit {
            Some(new_cargo_toml) => {
                edits.push(("Cargo.toml".to_owned(), new_cargo_toml));
                lines.push(format!(
                    "  feature  navree: {} ([dev-dependencies] in Cargo.toml)",
                    entry.feature
                ));
            }
            None => {
                lines.push(format!(
                    "  feature  navree: {} is already on",
                    entry.feature
                ));
            }
        }
        tail.push(format!("Installed {}.", entry.name));
        return Ok(Plan {
            edits,
            lines,
            tail,
            run: None,
        });
    }

    match plan_features(
        &files.cargo_toml,
        metadata,
        std::slice::from_ref(&entry.feature),
        true,
    )? {
        Some((new_cargo_toml, feature_line)) => {
            edits.push(("Cargo.toml".to_owned(), new_cargo_toml));
            lines.push(feature_line);
        }
        None => {
            lines.push(format!(
                "  feature  navree: {} is already on",
                entry.feature
            ));
        }
    }

    let full_module = entry
        .module
        .as_deref()
        .ok_or_else(|| format!("first-party `{}` lacks module path", entry.name))?;
    let parsed_module = ModulePath::parse(full_module)?;
    let line = wiring::module_line(full_module);
    let crate_mention = first_two_segments(full_module);
    let wired_outcome = wiring::wire(
        files,
        &line,
        parsed_module.type_name(),
        &crate_mention,
        options.no_wire,
    );
    let (is_wired, needs_migrate) = record_wiring(
        wired_outcome,
        &line,
        parsed_module.type_name(),
        &mut edits,
        &mut lines,
    );

    let ns = entry.namespace();
    let run_ns = if is_wired { Some(ns.clone()) } else { None };

    if run_ns.is_none() {
        lines.push(format!("  next     navree config:publish --plugin {ns}"));
    }

    if needs_migrate {
        tail.push("  next     navree migrate".to_owned());
    }

    if run_ns.is_none() {
        tail.push(format!(
            "  next     navree plugins, to see what {} registers (it builds and runs the app)",
            entry.name
        ));
    }

    tail.push(format!("Installed {}.", entry.name));

    Ok(Plan {
        edits,
        lines,
        tail,
        run: run_ns,
    })
}

fn record_wiring(
    outcome: Wiring,
    line: &str,
    type_name: &str,
    edits: &mut Vec<(String, String)>,
    lines: &mut Vec<String>,
) -> (bool, bool) {
    match outcome {
        Wiring::Added(new_app_rs) => {
            edits.push(("src/app.rs".to_owned(), new_app_rs));
            lines.push(format!("  wired    {line} (src/app.rs)"));
            (true, true)
        }
        Wiring::AlreadyInApp => {
            lines.push(format!("  kept     {line} is already in src/app.rs"));
            (true, false)
        }
        Wiring::WiredIn(file) => {
            lines.push(format!("  kept     {type_name} is already wired in {file}"));
            (true, false)
        }
        Wiring::Print => {
            lines.push(format!(
                "  next     add this line to Navree::app() in src/app.rs: {line}"
            ));
            (false, true)
        }
    }
}

fn first_two_segments(path: &str) -> String {
    let mut parts = path.split("::");
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    format!("{first}::{second}")
}

fn listed_navree_features(cargo_toml: &str) -> BTreeSet<String> {
    let Ok(parsed) = cargo_toml.parse::<toml::Table>() else {
        return BTreeSet::new();
    };
    let Some(navree) = parsed
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|deps| deps.get("navree"))
        .and_then(toml::Value::as_table)
    else {
        return BTreeSet::new();
    };
    navree
        .get("features")
        .and_then(toml::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn plan_features(
    cargo_toml: &str,
    metadata: &Metadata,
    asked: &[String],
    _first_party: bool,
) -> Result<Option<(String, String)>, String> {
    if asked.is_empty() {
        // Validate Cargo.toml syntax even when no feature is asked for.
        cargo_toml
            .parse::<toml_edit::DocumentMut>()
            .map_err(|err| {
                format!(
                    "Cargo.toml cannot be read: {}; nothing was changed",
                    err.to_string().trim()
                )
            })?;
        return Ok(None);
    }

    let mut effective_on = metadata.navree_enabled();
    let listed = listed_navree_features(cargo_toml);
    if let Some(navree_pkg) = metadata.navree() {
        let expanded_listed = cargo_toml::closure(&navree_pkg.features, &listed);
        effective_on.extend(expanded_listed);
    } else {
        effective_on.extend(listed);
    }

    let mut to_write = Vec::new();
    for feat in asked {
        if !effective_on.contains(feat.as_str()) && !to_write.iter().any(|f: &String| f == feat) {
            to_write.push(feat.clone());
        }
    }

    if to_write.is_empty() {
        cargo_toml
            .parse::<toml_edit::DocumentMut>()
            .map_err(|err| {
                format!(
                    "Cargo.toml cannot be read: {}; nothing was changed",
                    err.to_string().trim()
                )
            })?;
        return Ok(None);
    }

    let edited = cargo_toml::turn_on(cargo_toml, DepTable::Dependencies, &to_write)
        .map_err(|err| format!("Cargo.toml cannot be read: {err}; nothing was changed"))?;
    let Some(new_cargo_toml) = edited else {
        return Ok(None);
    };

    let seeds: BTreeSet<String> = to_write.iter().cloned().collect();
    let brought = match metadata.navree() {
        Some(navree_pkg) => cargo_toml::closure(&navree_pkg.features, &seeds),
        None => seeds.clone(),
    };

    let mut named = to_write.clone();
    for feat in brought {
        if !effective_on.contains(&feat) && !named.contains(&feat) {
            named.push(feat);
        }
    }

    let line = format!("  feature  navree: {} (Cargo.toml)", named.join(", "));
    Ok(Some((new_cargo_toml, line)))
}

/// Installs a plugin into the app containing `cwd`; printed lines go to `out`.
pub fn run(
    cwd: &Path,
    options: &AddOptions,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let roots = find_app(cwd).map_err(|msg| anyhow::anyhow!("{msg}"))?;
    let mut files = AppFiles::read(&roots)?;
    let index = Index::embedded();
    let choice =
        choose(&roots, &files, options, cwd, index).map_err(|msg| anyhow::anyhow!("{msg}"))?;
    let snapshot = Snapshot::take(&roots)?;

    let ran_cargo_add = match &choice {
        Choice::FirstParty(_) => false,
        Choice::Community {
            how: How::Present, ..
        } => false,
        Choice::Community { package, how } => {
            let invocation = match how {
                How::Registry {
                    requirement: Some(req),
                } => {
                    let spec = format!("{package}@{req}");
                    metadata::cargo(&["add", &spec])
                }
                How::Registry { requirement: None } => metadata::cargo(&["add", package]),
                How::Path { dir } => metadata::cargo(&["add", "--path", dir]),
                How::Present => unreachable!(),
            };
            match runner.capture_both(&invocation, &roots.package) {
                Ok((true, _, _)) => true,
                Ok((false, _, stderr)) => {
                    let not_put_back = snapshot.restore(&roots, &files, &Written::default());
                    let last = metadata::cargo_error_line(&stderr);
                    return Err(anyhow::anyhow!(
                        "cargo add failed: {last}; {}",
                        rollback_tail(&not_put_back)
                    ));
                }
                Err(err) => {
                    let not_put_back = snapshot.restore(&roots, &files, &Written::default());
                    return Err(anyhow::anyhow!(
                        "cargo add failed: {}; {}",
                        shown(&err.to_string()),
                        rollback_tail(&not_put_back)
                    ));
                }
            }
        }
    };

    if ran_cargo_add {
        match fs::read_to_string(roots.package.join("Cargo.toml")) {
            Ok(updated_toml) => files.cargo_toml = updated_toml,
            Err(err) => {
                let not_put_back = snapshot.restore(&roots, &files, &Written::default());
                return Err(anyhow::anyhow!(
                    "Cargo.toml cannot be read: {err}; {}",
                    rollback_tail(&not_put_back)
                ));
            }
        }
    }

    let metadata_inv = metadata::invocation();
    let metadata_json = match runner.capture_both(&metadata_inv, &roots.package) {
        Ok((true, stdout, _)) => stdout,
        Ok((false, _, stderr)) => {
            let not_put_back = snapshot.restore(&roots, &files, &Written::default());
            let last = metadata::cargo_error_line(&stderr);
            return Err(anyhow::anyhow!(
                "cargo metadata failed: {last}; {}",
                rollback_tail(&not_put_back)
            ));
        }
        Err(err) => {
            let not_put_back = snapshot.restore(&roots, &files, &Written::default());
            return Err(anyhow::anyhow!(
                "cargo metadata failed: {}; {}",
                shown(&err.to_string()),
                rollback_tail(&not_put_back)
            ));
        }
    };

    let metadata = match Metadata::parse(&metadata_json) {
        Ok(parsed) => parsed,
        Err(err) => {
            let not_put_back = snapshot.restore(&roots, &files, &Written::default());
            return Err(anyhow::anyhow!(
                "cargo metadata failed: {}; {}",
                shown(&err),
                rollback_tail(&not_put_back)
            ));
        }
    };

    let chosen = match build_chosen(&roots, &files, &choice, &metadata) {
        Ok(c) => c,
        Err(msg) => {
            let not_put_back = snapshot.restore(&roots, &files, &Written::default());
            return Err(anyhow::anyhow!("{msg}; {}", rollback_tail(&not_put_back)));
        }
    };

    let role_sources = grants::scan(&files, &options.process_env);
    let navree_pkg = metadata.navree();
    let facts = AppFacts {
        first_party: chosen.entry.is_some(),
        navree_version: navree_pkg
            .map(|pkg| pkg.version.clone())
            .unwrap_or_default(),
        navree_features: navree_pkg
            .map(|pkg| pkg.features.keys().cloned().collect())
            .unwrap_or_default(),
        allowed_features: index.manifest_features.iter().cloned().collect(),
        reserved: index.reserved.iter().cloned().collect(),
        roles: role_sources.roles(),
        demo_guest_role: role_sources.demo_guest_role(),
        granted: role_sources.granted(),
    };

    let planned = match plan(&files, &chosen, &facts, &metadata, options) {
        Ok(p) => p,
        Err(msg) => {
            let not_put_back = snapshot.restore(&roots, &files, &Written::default());
            if not_put_back.is_empty() {
                return Err(anyhow::anyhow!("{msg}"));
            }
            let base = msg.strip_suffix("; nothing was changed").unwrap_or(&msg);
            return Err(anyhow::anyhow!("{base}; {}", rollback_tail(&not_put_back)));
        }
    };

    let written = match write_all(&roots, &files, &planned.edits) {
        Ok(w) => w,
        Err(mut failed) => {
            let cargo_toml_on_disk = fs::read(roots.package.join("Cargo.toml")).ok();
            if cargo_toml_on_disk.as_deref() == Some(files.cargo_toml.as_bytes()) {
                let extra = snapshot.restore(&roots, &files, &Written::default());
                for item in extra {
                    if !failed.not_put_back.contains(&item) {
                        failed.not_put_back.push(item);
                    }
                }
            } else {
                let guard = Written {
                    files: vec![("Cargo.toml".to_owned(), Vec::new())],
                };
                let extra = snapshot.restore(&roots, &files, &guard);
                for item in extra {
                    if item != "Cargo.toml" && !failed.not_put_back.contains(&item) {
                        failed.not_put_back.push(item);
                    }
                }
            }
            return Err(anyhow::anyhow!("{}", failed.message("plugin:add")));
        }
    };

    if let Some(ns) = &planned.run {
        writeln!(
            out,
            "  run      cargo run --quiet -- config:publish --plugin {ns} (builds and runs the app)"
        )?;
        let publish_inv = forward("config:publish", &["--plugin".to_owned(), ns.clone()])
            .with_env(options.app_env.clone());
        let publish_out = match runner.capture(&publish_inv, &roots.config) {
            Ok((true, stdout)) => stdout,
            _ => {
                let not_put_back = snapshot.restore(&roots, &files, &written);
                return Err(anyhow::anyhow!(
                    "{}: config:publish --plugin {ns} failed (its output is above); {}",
                    shown(&chosen.name),
                    rollback_tail(&not_put_back)
                ));
            }
        };

        let mut step10_lines = Vec::new();
        for raw_line in publish_out.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(path) = trimmed.strip_prefix("wrote ") {
                step10_lines.push(format!("  wrote    {path}"));
            } else if let Some(path) =
                trimmed.strip_suffix(": exists, kept (use --force to replace it)")
            {
                step10_lines.push(format!("  kept     {path} exists"));
            } else {
                step10_lines.push(trimmed.to_owned());
            }
        }

        let plugins_inv =
            forward("plugins", &["--json".to_owned()]).with_env(options.app_env.clone());
        let block_pkg = match &chosen.entry {
            Some(entry) => entry.krate.as_str(),
            None => chosen.name.as_str(),
        };
        enum Step11 {
            Block(String),
            AbsentInJson,
            Failed,
        }
        let step11 = match runner.capture_both(&plugins_inv, &roots.config) {
            Ok((true, stdout, _)) => {
                if serde_json::from_str::<serde_json::Value>(&stdout).is_ok() {
                    match block::render(&stdout, block_pkg) {
                        Some(rendered) => Step11::Block(rendered),
                        None => Step11::AbsentInJson,
                    }
                } else {
                    Step11::Failed
                }
            }
            _ => Step11::Failed,
        };

        for line in &planned.lines {
            writeln!(out, "{line}")?;
        }
        for line in &step10_lines {
            writeln!(out, "{line}")?;
        }
        let tail_len = planned.tail.len();
        let (before_closing, closing) = if tail_len > 0 {
            (
                &planned.tail[..tail_len - 1],
                Some(&planned.tail[tail_len - 1]),
            )
        } else {
            (&planned.tail[..], None)
        };
        for line in before_closing {
            writeln!(out, "{line}")?;
        }
        match step11 {
            Step11::Block(rendered) => {
                writeln!(out)?;
                writeln!(out, "{rendered}")?;
            }
            Step11::AbsentInJson => {}
            Step11::Failed => {
                writeln!(
                    out,
                    "  next     navree plugins, to see what {} registers (it builds and runs the app)",
                    shown(&chosen.name)
                )?;
            }
        }
        if let Some(last) = closing {
            writeln!(out, "{last}")?;
        }
    } else {
        for line in &planned.lines {
            writeln!(out, "{line}")?;
        }
        for line in &planned.tail {
            writeln!(out, "{line}")?;
        }
    }

    Ok(())
}

fn build_chosen(
    roots: &AppRoots,
    files: &AppFiles,
    choice: &Choice,
    metadata: &Metadata,
) -> Result<Chosen, String> {
    match choice {
        Choice::FirstParty(entry) => {
            let navree_version = metadata
                .navree()
                .map(|pkg| pkg.version.clone())
                .unwrap_or_default();
            let crate_name = entry.krate.replace('-', "_");
            Ok(Chosen {
                name: entry.name.clone(),
                version: navree_version,
                lib_name: Some(crate_name.clone()),
                crate_name,
                entry: Some(entry.clone()),
                table: None,
                already_present: false,
            })
        }
        Choice::Community { package, how } => {
            let pkg_opt = match how {
                How::Path { dir } => metadata.dependency_at(&roots.package.join(dir)),
                How::Registry { .. } => metadata.dependency(package),
                How::Present => match cargo_toml::dependency_source(&files.cargo_toml, package) {
                    Some(Source::Path(rel)) => {
                        metadata.dependency_at(&roots.package.join(normalize_rel_dir(&rel)))
                    }
                    _ => metadata.dependency(package),
                },
            };
            let Some(pkg) = pkg_opt else {
                return Err(format!(
                    "cargo metadata failed: dependency `{}` was not found",
                    shown(package)
                ));
            };
            let already_present = matches!(how, How::Present);
            let name = if already_present {
                package.clone()
            } else {
                pkg.name.clone()
            };
            let crate_name = pkg.name.replace('-', "_");
            Ok(Chosen {
                name,
                version: pkg.version.clone(),
                crate_name,
                lib_name: pkg.lib_name.clone(),
                entry: None,
                table: pkg.plugin_table.clone(),
                already_present,
            })
        }
    }
}

fn rollback_tail(not_put_back: &[String]) -> String {
    if not_put_back.is_empty() {
        "nothing was changed".to_owned()
    } else {
        format!(
            "these files could not be put back: {}",
            not_put_back.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots_at(dir: &Path) -> AppRoots {
        AppRoots {
            package: dir.to_path_buf(),
            config: dir.to_path_buf(),
            workspace: dir.to_path_buf(),
        }
    }

    fn files_with_cargo(roots: &AppRoots, cargo_toml: &str) -> AppFiles {
        AppFiles {
            roots: roots.clone(),
            cargo_toml: cargo_toml.to_owned(),
            ..AppFiles::default()
        }
    }

    fn base_cargo_toml() -> &'static str {
        "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.4.0\"\n"
    }

    fn write_local_plugin(dir: &Path, name: &str) {
        fs::create_dir_all(dir).expect("create plugin dir");
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        )
        .expect("write plugin Cargo.toml");
    }

    #[test]
    fn choose_takes_a_first_party_name_by_either_spelling() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();

        for spelling in ["billing", "navree-billing", "navree_billing"] {
            let choice = choose(
                &roots,
                &files,
                &AddOptions {
                    package: Some(spelling.to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .expect("first-party billing");
            let Choice::FirstParty(entry) = choice else {
                panic!("expected FirstParty for {spelling}");
            };
            assert_eq!(entry.name, "billing");
            assert_eq!(entry.kind, IndexKind::Module);
        }

        let Choice::FirstParty(http_client) = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("http_client".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .expect("http_client") else {
            panic!("expected FirstParty for http_client");
        };
        assert_eq!(http_client.name, "http-client");

        let community = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("navree-plugin-billing".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .expect("community plugin");
        assert_eq!(
            community,
            Choice::Community {
                package: "navree-plugin-billing".to_owned(),
                how: How::Registry { requirement: None },
            }
        );

        let Choice::FirstParty(browser) = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("browser".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .expect("browser") else {
            panic!("expected FirstParty for browser");
        };
        assert_eq!(browser.kind, IndexKind::Dev);
    }

    #[test]
    fn plugin_add_refuses_tools_features_and_versions_of_first_party_packages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();

        for (pkg, expected) in [
            ("dock", "`dock` is a tool, not a plugin: use `navree dock`"),
            (
                "harbor",
                "`harbor` is a tool, not a plugin: use `navree harbor:init`",
            ),
            (
                "navigator",
                "`navigator` is a tool, not a plugin: use `navree mcp`",
            ),
            (
                "otel",
                "`otel` is a feature of navree, not a plugin: add \"otel\" to navree's features in Cargo.toml",
            ),
        ] {
            let err = choose(
                &roots,
                &files,
                &AddOptions {
                    package: Some(pkg.to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err();
            assert_eq!(err, expected);
        }

        let ver_err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("billing".to_owned()),
                version: Some("^1.4".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            ver_err,
            "--version is for community crates; first-party packages follow navree's version"
        );

        let dash_err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("-khata".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            dash_err,
            "`-khata` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters"
        );

        let long_name = "k".repeat(65);
        let long_err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some(long_name.clone()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            long_err,
            format!(
                "`{long_name}` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters"
            )
        );

        let bad_req = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("navree-plugin-khata".to_owned()),
                version: Some("1.0; rm".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            bad_req,
            "--version `1.0; rm` is not a Cargo version requirement"
        );
    }

    #[test]
    fn choose_refuses_the_framework_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();

        let err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("navree".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(err, "`navree` is the framework, not a plugin");

        let local = tmp.path().join("plugins/fake");
        write_local_plugin(&local, "navree");
        let path_err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(PathBuf::from("plugins/fake")),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(path_err, "`navree` is the framework, not a plugin");
    }

    #[test]
    fn choose_reads_underscores_and_hyphens_as_one_in_a_package_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let index = Index::embedded();

        let hyphen_toml = format!("{}navree-plugin-khata = \"1.0\"\n", base_cargo_toml());
        let files = files_with_cargo(&roots, &hyphen_toml);
        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    package: Some("navree_plugin_khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap(),
            Choice::Community {
                package: "navree-plugin-khata".to_owned(),
                how: How::Present,
            }
        );

        let underscore_toml = format!("{}navree_plugin_khata = \"1.0\"\n", base_cargo_toml());
        let files_u = files_with_cargo(&roots, &underscore_toml);
        assert_eq!(
            choose(
                &roots,
                &files_u,
                &AddOptions {
                    package: Some("navree-plugin-khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap(),
            Choice::Community {
                package: "navree_plugin_khata".to_owned(),
                how: How::Present,
            }
        );

        let path_toml = format!(
            "{}navree-plugin-khata = {{ path = \"plugins/khata\" }}\n",
            base_cargo_toml()
        );
        let files_p = files_with_cargo(&roots, &path_toml);
        assert_eq!(
            choose(
                &roots,
                &files_p,
                &AddOptions {
                    package: Some("navree_plugin_khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-plugin-khata is already a dependency of this app from the folder plugins/khata; run navree plugin:remove navree-plugin-khata first to add it from the registry"
        );
    }

    #[test]
    fn choose_refuses_a_registry_name_against_a_path_and_the_reverse() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let index = Index::embedded();
        let plugin_dir = tmp.path().join("plugins/khata");
        write_local_plugin(&plugin_dir, "navree-plugin-khata");

        let path_toml = format!(
            "{}navree-plugin-khata = {{ path = \"plugins/khata\" }}\n",
            base_cargo_toml()
        );
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, &path_toml),
                &AddOptions {
                    package: Some("navree-plugin-khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-plugin-khata is already a dependency of this app from the folder plugins/khata; run navree plugin:remove navree-plugin-khata first to add it from the registry"
        );

        let reg_toml = format!("{}navree-plugin-khata = \"1.0\"\n", base_cargo_toml());
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, &reg_toml),
                &AddOptions {
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-plugin-khata is already a dependency of this app from the registry; run navree plugin:remove navree-plugin-khata first to add it from the folder plugins/khata"
        );

        let shown_plugin = std::path::absolute(&plugin_dir).unwrap();
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, base_cargo_toml()),
                &AddOptions {
                    package: Some("navree-plugin-notices".to_owned()),
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            format!(
                "{} holds the package navree-plugin-khata, not navree-plugin-notices",
                shown_plugin.display()
            )
        );

        let billing_dir = tmp.path().join("plugins/billing");
        write_local_plugin(&billing_dir, "navree-billing");
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, base_cargo_toml()),
                &AddOptions {
                    path: Some(PathBuf::from("plugins/billing")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-billing is the name of a first-party package; give the local crate a name of its own"
        );
    }

    #[test]
    fn choose_gives_the_package_found_at_path_when_none_is_named() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        let plugin_dir = tmp.path().join("plugins/khata");
        write_local_plugin(&plugin_dir, "navree-plugin-khata");

        let expected = Choice::Community {
            package: "navree-plugin-khata".to_owned(),
            how: How::Path {
                dir: "plugins/khata".to_owned(),
            },
        };

        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap(),
            expected
        );

        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(PathBuf::from(".")),
                    ..AddOptions::default()
                },
                &plugin_dir,
                index,
            )
            .unwrap(),
            expected
        );
    }

    #[test]
    fn choose_holds_the_package_found_at_path_to_the_package_name_grammar() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        let plugin_dir = tmp.path().join("plugins/khata");
        write_local_plugin(&plugin_dir, "Khata Plugin");

        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "`Khata Plugin` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters"
        );
    }

    #[cfg(windows)]
    #[test]
    fn choose_takes_a_path_with_other_separators_and_case_inside_the_app() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        write_local_plugin(&tmp.path().join("plugins/khata"), "navree-plugin-khata");

        for raw in [r"plugins\khata", r"PLUGINS\Khata"] {
            assert_eq!(
                choose(
                    &roots,
                    &files,
                    &AddOptions {
                        path: Some(PathBuf::from(raw)),
                        ..AddOptions::default()
                    },
                    tmp.path(),
                    index,
                )
                .unwrap(),
                Choice::Community {
                    package: "navree-plugin-khata".to_owned(),
                    how: How::Path {
                        dir: "plugins/khata".to_owned(),
                    },
                },
                "for {raw}"
            );
        }
    }

    #[test]
    fn choose_refuses_a_path_outside_the_app_that_does_not_exist_or_is_the_app_itself() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let app_dir = outer.path().join("app");
        fs::create_dir_all(&app_dir).expect("create app dir");
        let sibling = outer.path().join("khata");
        write_local_plugin(&sibling, "navree-plugin-khata");

        let roots = roots_at(&app_dir);
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        let shown_root = std::path::absolute(&app_dir).unwrap();

        let other = tempfile::tempdir().expect("other tempdir");
        write_local_plugin(other.path(), "navree-plugin-khata");

        let mut outside_cases: Vec<PathBuf> = vec![
            PathBuf::from("../khata"),
            other.path().to_path_buf(),
            PathBuf::from("."),
        ];
        #[cfg(windows)]
        outside_cases.push(PathBuf::from(r"D:\khata"));

        for bad in outside_cases {
            let joined = if bad.is_absolute() {
                bad.clone()
            } else {
                app_dir.join(&bad)
            };
            let shown_dir = std::path::absolute(&joined).unwrap();
            let err = choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(bad.clone()),
                    ..AddOptions::default()
                },
                &app_dir,
                index,
            )
            .unwrap_err();
            assert_eq!(
                err,
                format!(
                    "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                    shown_dir.display(),
                    shown_root.display()
                ),
                "for {bad:?}"
            );
        }

        let missing = PathBuf::from("plugins/missing");
        let shown_missing = std::path::absolute(app_dir.join(&missing)).unwrap();
        let missing_err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(missing),
                ..AddOptions::default()
            },
            &app_dir,
            index,
        )
        .unwrap_err();
        assert_eq!(
            missing_err,
            format!("--path {} does not exist", shown_missing.display())
        );
    }

    #[cfg(windows)]
    #[test]
    fn plugin_add_refuses_a_junction_that_leaves_the_app() {
        let app_tmp = tempfile::tempdir().expect("app tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_local_plugin(outside.path(), "navree-plugin-khata");

        fs::create_dir_all(app_tmp.path().join("plugins")).expect("create plugins/");
        let junction = app_tmp.path().join("plugins/khata");
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &junction.to_string_lossy(),
                &outside.path().to_string_lossy(),
            ])
            .output()
            .expect("run mklink /J");
        assert!(status.status.success(), "mklink /J failed");

        let roots = roots_at(app_tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let shown_dir = std::path::absolute(&junction).unwrap();
        let shown_root = std::path::absolute(app_tmp.path()).unwrap();

        let err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(PathBuf::from("plugins/khata")),
                ..AddOptions::default()
            },
            app_tmp.path(),
            Index::embedded(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            format!(
                "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                shown_dir.display(),
                shown_root.display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn plugin_add_refuses_a_symbolic_link_that_leaves_the_app() {
        let app_tmp = tempfile::tempdir().expect("app tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_local_plugin(outside.path(), "navree-plugin-khata");

        fs::create_dir_all(app_tmp.path().join("plugins")).expect("create plugins/");
        let link = app_tmp.path().join("plugins/khata");
        std::os::unix::fs::symlink(outside.path(), &link).expect("create symlink");

        let roots = roots_at(app_tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let shown_dir = std::path::absolute(&link).unwrap();
        let shown_root = std::path::absolute(app_tmp.path()).unwrap();

        let err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(PathBuf::from("plugins/khata")),
                ..AddOptions::default()
            },
            app_tmp.path(),
            Index::embedded(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            format!(
                "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                shown_dir.display(),
                shown_root.display()
            )
        );
    }

    const SAMPLE_APP_RS: &str = "\
pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
    // navree:modules
}
";

    fn small_metadata(resolved: &[&str]) -> Metadata {
        let json = serde_json::json!({
            "packages": [
                {
                    "id": "path+file:///kesar#0.1.0",
                    "name": "kesar",
                    "version": "0.1.0",
                    "source": null,
                    "manifest_path": "/kesar/Cargo.toml",
                    "targets": [{ "kind": ["bin"], "name": "kesar" }],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                    "name": "navree",
                    "version": "1.4.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cargo/navree-1.4.0/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree" }],
                    "features": {
                        "access": [],
                        "billing": [],
                        "documents": [],
                        "dock": [],
                        "full": ["access", "schema", "ui", "workspace"],
                        "postgres": [],
                        "schema": [],
                        "sqlite": [],
                        "ui": [],
                        "ui-native-preview": [],
                        "workspace": []
                    },
                    "metadata": null
                }
            ],
            "resolve": {
                "root": "path+file:///kesar#0.1.0",
                "nodes": [
                    {
                        "id": "path+file:///kesar#0.1.0",
                        "deps": [
                            {
                                "name": "navree",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            }
                        ],
                        "features": []
                    },
                    {
                        "id": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                        "deps": [],
                        "features": resolved
                    }
                ]
            }
        });
        Metadata::parse(&json.to_string()).expect("metadata parses")
    }

    fn plan_facts(
        files: &AppFiles,
        process_env: &[(String, String)],
        first_party: bool,
    ) -> AppFacts {
        let index = Index::embedded();
        let role_sources = grants::scan(files, process_env);
        AppFacts {
            first_party,
            navree_version: "1.4.0".to_owned(),
            navree_features: [
                "access",
                "billing",
                "documents",
                "dock",
                "full",
                "postgres",
                "schema",
                "sqlite",
                "ui",
                "ui-native-preview",
                "workspace",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            allowed_features: ["access", "billing", "ui", "workspace"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            reserved: index.reserved.iter().cloned().collect(),
            roles: role_sources.roles(),
            demo_guest_role: role_sources.demo_guest_role(),
            granted: role_sources.granted(),
        }
    }

    fn announcements_chosen(table_toml: &str, already_present: bool) -> Chosen {
        let parsed: toml::Value = toml::from_str(table_toml).expect("valid TOML");
        Chosen {
            name: "navree-plugin-announcements".to_owned(),
            version: "1.0.0".to_owned(),
            crate_name: "navree_plugin_announcements".to_owned(),
            lib_name: Some("navree_plugin_announcements".to_owned()),
            entry: None,
            table: Some(serde_json::to_value(parsed).expect("JSON value")),
            already_present,
        }
    }

    fn memory_app_files(cargo_toml: &str) -> AppFiles {
        AppFiles {
            cargo_toml: cargo_toml.to_owned(),
            app_rs: Some(SAMPLE_APP_RS.to_owned()),
            src: vec![("src/app.rs".to_owned(), SAMPLE_APP_RS.to_owned())],
            env_example: Some("APP_KEY=\n".to_owned()),
            ..AppFiles::default()
        }
    }

    fn apply_plan_edits(files: &mut AppFiles, edits: &[(String, String)]) {
        for (path, content) in edits {
            match path.as_str() {
                "Cargo.toml" => files.cargo_toml = content.clone(),
                "src/app.rs" => {
                    files.app_rs = Some(content.clone());
                    if let Some(entry) = files.src.iter_mut().find(|(p, _)| p == "src/app.rs") {
                        entry.1 = content.clone();
                    } else {
                        files.src.push(("src/app.rs".to_owned(), content.clone()));
                    }
                }
                ".env.example" => files.env_example = Some(content.clone()),
                other if other.starts_with("config/") => {
                    files.config.insert(other.to_owned(), content.clone());
                }
                _ => {}
            }
        }
    }

    #[test]
    fn plugin_add_twice_changes_nothing_the_second_time() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let mut files = memory_app_files(cargo_toml);
        let manifest_toml = "\
title = \"Announcements\"
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\", \"ui\"]
config = [\"announcements\"]
env = [\"APP_ANNOUNCEMENTS__SIGNING_KEY\"]
grants = { member = [\"announcements.read\"] }
policy = { img_src = [\"https://tile.openstreetmap.org\"] }
";
        let metadata = small_metadata(&["postgres"]);
        let options = AddOptions::default();

        let first = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &metadata,
            &options,
        )
        .expect("first plan succeeds");
        assert!(!first.edits.is_empty());
        apply_plan_edits(&mut files, &first.edits);

        let metadata_after = small_metadata(&["postgres", "workspace", "ui"]);
        let second = plan(
            &files,
            &announcements_chosen(manifest_toml, true),
            &plan_facts(&files, &[], false),
            &metadata_after,
            &options,
        )
        .expect("second plan succeeds");
        assert!(
            second.edits.is_empty(),
            "expected no edits, got {:?}",
            second.edits
        );

        for line in second
            .lines
            .iter()
            .chain(second.tail[..second.tail.len() - 1].iter())
        {
            assert!(
                line.starts_with("  kept     ") || line.starts_with("  next     "),
                "unexpected line on second run: {line}"
            );
        }
    }

    #[test]
    fn plugin_add_without_the_marker_prints_the_line() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let no_marker_app = "pub fn app() -> AppBuilder { Navree::app() }\n";
        let files = AppFiles {
            cargo_toml: cargo_toml.to_owned(),
            app_rs: Some(no_marker_app.to_owned()),
            src: vec![("src/app.rs".to_owned(), no_marker_app.to_owned())],
            ..AppFiles::default()
        };
        let manifest_toml = r#"module = "AnnouncementsModule::from_config()""#;
        let options = AddOptions {
            publish_config: true,
            ..AddOptions::default()
        };
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &small_metadata(&["postgres"]),
            &options,
        )
        .expect("plan succeeds");

        assert!(!planned.edits.iter().any(|(path, _)| path == "src/app.rs"));
        assert!(planned.lines.contains(
            &"  next     add this line to Navree::app() in src/app.rs: .module(navree_plugin_announcements::AnnouncementsModule::from_config())"
                .to_owned()
        ));
        assert_eq!(planned.run, None);
        assert_eq!(
            planned.tail[planned.tail.len() - 2],
            "  next     navree plugins, to see what navree-plugin-announcements registers (it builds and runs the app)"
        );
    }

    #[test]
    fn plugin_add_keeps_comments_and_spacing_in_cargo_toml() {
        let odd_cargo_toml = "\
# Application manifest for Kesar
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

# Core dependencies
[dependencies]
navree   =   {   version = \"1.4\" ,features=[\"postgres\"]  } # db
navree-plugin-announcements = \"1.0.0\"

[profile.dev]
debug = false
";
        let files = memory_app_files(odd_cargo_toml);
        let manifest_toml = "\
module = \"AnnouncementsModule::from_config()\"
features = [\"billing\"]
";
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .expect("plan succeeds");

        let (_, edited_toml) = planned
            .edits
            .iter()
            .find(|(path, _)| path == "Cargo.toml")
            .expect("Cargo.toml edited");
        let expected_toml = odd_cargo_toml.replace(
            "navree   =   {   version = \"1.4\" ,features=[\"postgres\"]  } # db",
            "navree   =   {   version = \"1.4\" ,features=[\"postgres\", \"billing\"]  } # db",
        );
        assert_eq!(edited_toml, &expected_toml);
    }

    #[test]
    fn plugin_add_with_a_feature_already_on_adds_no_second_one() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", features = [\"workspace\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let files = memory_app_files(cargo_toml);
        let manifest_toml = "\
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\", \"ui\"]
";
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &small_metadata(&["full", "ui"]),
            &AddOptions::default(),
        )
        .expect("plan succeeds");

        assert!(!planned.edits.iter().any(|(path, _)| path == "Cargo.toml"));
        assert!(
            !planned
                .lines
                .iter()
                .any(|line| line.starts_with("  feature"))
        );

        let billing_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"billing\"] }
";
        let billing_files = memory_app_files(billing_toml);
        let billing_entry = Index::embedded().find("billing").unwrap().clone();
        let billing_chosen = Chosen {
            name: "billing".to_owned(),
            version: "1.4.0".to_owned(),
            crate_name: "navree_billing".to_owned(),
            lib_name: Some("navree_billing".to_owned()),
            entry: Some(billing_entry),
            table: None,
            already_present: false,
        };
        let billing_plan = plan(
            &billing_files,
            &billing_chosen,
            &plan_facts(&billing_files, &[], true),
            &small_metadata(&["billing"]),
            &AddOptions::default(),
        )
        .expect("billing plan succeeds");
        assert!(
            !billing_plan
                .edits
                .iter()
                .any(|(path, _)| path == "Cargo.toml")
        );
        assert!(
            billing_plan
                .lines
                .contains(&"  feature  navree: billing is already on".to_owned())
        );
    }

    #[test]
    fn plugin_add_plans_its_features_on_the_cargo_toml_cargo_add_wrote() {
        let after_cargo_add = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let files = memory_app_files(after_cargo_add);
        let manifest_toml = "\
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\", \"ui\"]
";
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .expect("plan succeeds");

        let (_, edited_toml) = planned
            .edits
            .iter()
            .find(|(path, _)| path == "Cargo.toml")
            .expect("Cargo.toml edited");
        assert!(edited_toml.contains("navree-plugin-announcements = \"1.0.0\""));
        assert!(edited_toml.contains("features = [\"postgres\", \"workspace\", \"ui\"]"));
    }

    #[test]
    fn plugin_add_prints_the_sidebar_hint_only_for_a_manifest_that_lists_ui() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let ui_manifest = "\
module = \"AnnouncementsModule::from_config()\"
features = [\"ui\"]
";
        let no_ui_manifest = "\
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\"]
";

        // Shell without `.contributed()` -> one-line hint.
        let mut plain_shell_files = memory_app_files(cargo_toml);
        plain_shell_files.shell_rs =
            Some("pub fn shell() -> Shell { Shell::new(\"Kesar\") }\n".to_owned());
        let p1 = plan(
            &plain_shell_files,
            &announcements_chosen(ui_manifest, false),
            &plan_facts(&plain_shell_files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .unwrap();
        assert!(p1.tail.contains(
            &"  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs"
                .to_owned()
        ));

        // Shell calling `for_request(` -> extended AppShell hint.
        let mut for_req_files = memory_app_files(cargo_toml);
        for_req_files.shell_rs =
            Some("pub fn page() { Shell::new(\"Kesar\").for_request(&headers); }\n".to_owned());
        let p2 = plan(
            &for_req_files,
            &announcements_chosen(ui_manifest, false),
            &plan_facts(&for_req_files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .unwrap();
        assert!(p2.tail.contains(
            &"  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs, and build Desk's page from the AppShell extractor (.context(..), .user_menu(..)) instead of Shell::for_request; see /docs/ui/app-shell/"
                .to_owned()
        ));

        // Manifest without `ui` -> no sidebar hint.
        let p3 = plan(
            &plain_shell_files,
            &announcements_chosen(no_ui_manifest, false),
            &plan_facts(&plain_shell_files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .unwrap();
        assert!(!p3.tail.iter().any(|l| l.contains("sidebar")));

        // Shell already calling `.contributed()` -> no sidebar hint.
        let mut contributed_files = memory_app_files(cargo_toml);
        contributed_files.shell_rs =
            Some("pub fn shell() -> Shell { Shell::new(\"Kesar\").contributed() }\n".to_owned());
        let p4 = plan(
            &contributed_files,
            &announcements_chosen(ui_manifest, false),
            &plan_facts(&contributed_files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .unwrap();
        assert!(!p4.tail.iter().any(|l| l.contains("sidebar")));
    }

    #[test]
    fn plugin_add_wires_a_module_whose_type_name_another_crate_also_uses() {
        let app_with_khata = "\
pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(navree_plugin_khata::ClientModule::<reqwest::Client,KhataOptions>::new())
    // navree:modules
}
";
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-khata = \"0.1.0\"
navree-plugin-notices = \"0.1.0\"
";
        let files = AppFiles {
            cargo_toml: cargo_toml.to_owned(),
            app_rs: Some(app_with_khata.to_owned()),
            src: vec![("src/app.rs".to_owned(), app_with_khata.to_owned())],
            ..AppFiles::default()
        };
        let notices_table: toml::Value =
            toml::from_str(r#"module = "ClientModule::<reqwest::Client,NoticesOptions>::new()""#)
                .unwrap();
        let chosen = Chosen {
            name: "navree-plugin-notices".to_owned(),
            version: "0.1.0".to_owned(),
            crate_name: "navree_plugin_notices".to_owned(),
            lib_name: Some("navree_plugin_notices".to_owned()),
            entry: None,
            table: Some(serde_json::to_value(notices_table).unwrap()),
            already_present: false,
        };
        let planned = plan(
            &files,
            &chosen,
            &plan_facts(&files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .expect("plan succeeds");

        assert!(planned.edits.iter().any(|(path, _)| path == "src/app.rs"));
        assert!(planned.lines.contains(
            &"  wired    .module(navree_plugin_notices::ClientModule::<reqwest::Client,NoticesOptions>::new()) (src/app.rs)"
                .to_owned()
        ));
        assert!(!planned.lines.iter().any(|l| l.contains("is already wired")));
    }

    #[test]
    fn plugin_add_writes_suggested_grants_as_visible_lines() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let files = memory_app_files(cargo_toml);
        let manifest_toml = "\
module = \"AnnouncementsModule::from_config()\"
grants = { member = [\"announcements.read\"] }
";
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .expect("plan succeeds");

        let (_, ws_toml) = planned
            .edits
            .iter()
            .find(|(path, _)| path == "config/workspace.toml")
            .expect("config/workspace.toml created");
        assert_eq!(
            ws_toml,
            "[roles.member]\npermissions = [\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n"
        );
        assert!(
            planned.tail.contains(
                &"  grant    member: announcements.read (config/workspace.toml)".to_owned()
            )
        );
    }

    #[test]
    fn plugin_add_marks_each_added_permission_inside_an_existing_list() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let mut files = memory_app_files(cargo_toml);
        files.config.insert(
            "config/workspace.toml".to_owned(),
            "[roles.member]\npermissions = [\"deals.read\"]\n".to_owned(),
        );
        let manifest_toml = "\
module = \"AnnouncementsModule::from_config()\"
grants = { member = [\"announcements.read\"] }
";
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &[], false),
            &small_metadata(&["postgres"]),
            &AddOptions::default(),
        )
        .expect("plan succeeds");

        let (_, ws_toml) = planned
            .edits
            .iter()
            .find(|(path, _)| path == "config/workspace.toml")
            .expect("config/workspace.toml edited");
        assert_eq!(
            ws_toml,
            "[roles.member]\npermissions = [\n    \"deals.read\",\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n"
        );
    }

    #[test]
    fn plugin_add_never_replaces_a_role_list_set_elsewhere() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let manifest_toml = "\
module = \"AnnouncementsModule::from_config()\"
grants = { member = [\"announcements.read\"] }
";

        let file_cases = [
            (
                "config/app.toml",
                "[workspace.roles.member]\npermissions = [\"deals.read\"]\n",
                false,
                "config/app.toml",
            ),
            (
                "config/workspace.json",
                r#"{"roles":{"member":{"permissions":["deals.read"]}}}"#,
                false,
                "config/workspace.json",
            ),
            (
                "config/workspace.roles.toml",
                "[member]\npermissions = [\"deals.read\"]\n",
                false,
                "config/workspace.roles.toml",
            ),
            (
                "config/env/production.toml",
                "[workspace.roles.member]\npermissions = [\"deals.read\"]\n",
                false,
                "config/env/production.toml",
            ),
            (
                ".env",
                "APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS=deals.read\n",
                true,
                "APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS",
            ),
        ];

        for (source_path, contents, is_dotenv, expected_source) in file_cases {
            let mut files = memory_app_files(cargo_toml);
            if is_dotenv {
                files
                    .dot_env
                    .insert(source_path.to_owned(), contents.to_owned());
            } else {
                files
                    .config
                    .insert(source_path.to_owned(), contents.to_owned());
            }
            let planned = plan(
                &files,
                &announcements_chosen(manifest_toml, false),
                &plan_facts(&files, &[], false),
                &small_metadata(&["postgres"]),
                &AddOptions::default(),
            )
            .expect("plan succeeds");
            assert!(
                !planned
                    .edits
                    .iter()
                    .any(|(path, _)| path == "config/workspace.toml"),
                "for {source_path}"
            );
            assert!(
                planned.tail.contains(&format!(
                    "  next     add announcements.read to workspace.roles.member.permissions in {expected_source}"
                )),
                "for {source_path}: {:?}",
                planned.tail
            );
        }

        // Process environment case.
        let files = memory_app_files(cargo_toml);
        let proc_env = vec![(
            "APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS".to_owned(),
            "deals.read".to_owned(),
        )];
        let options = AddOptions {
            process_env: proc_env.clone(),
            ..AddOptions::default()
        };
        let planned = plan(
            &files,
            &announcements_chosen(manifest_toml, false),
            &plan_facts(&files, &proc_env, false),
            &small_metadata(&["postgres"]),
            &options,
        )
        .expect("plan succeeds");
        assert!(
            !planned
                .edits
                .iter()
                .any(|(path, _)| path == "config/workspace.toml")
        );
        assert!(planned.tail.contains(
            &"  next     add announcements.read to workspace.roles.member.permissions in APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS"
                .to_owned()
        ));
    }

    #[test]
    fn plugin_add_refuses_grants_in_a_namespace_the_app_already_grants() {
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"
";
        let mut files = memory_app_files(cargo_toml);
        files.config.insert(
            "config/workspace.toml".to_owned(),
            "[roles.member]\npermissions = [\"announcements.view\"]\n".to_owned(),
        );

        let with_grants = "\
module = \"AnnouncementsModule::from_config()\"
grants = { member = [\"announcements.read\"] }
";
        assert_eq!(
            plan(
                &files,
                &announcements_chosen(with_grants, false),
                &plan_facts(&files, &[], false),
                &small_metadata(&["postgres"]),
                &AddOptions::default(),
            )
            .unwrap_err(),
            "navree-plugin-announcements 1.0.0: [package.metadata.navree.plugin] `grants`: the app already grants permissions under `announcements.` (in config/workspace.toml); a plugin suggests grants only in a namespace of its own; nothing was changed"
        );

        let without_grants = r#"module = "AnnouncementsModule::from_config()""#;
        assert_eq!(
            plan(
                &files,
                &announcements_chosen(without_grants, false),
                &plan_facts(&files, &[], false),
                &small_metadata(&["postgres"]),
                &AddOptions::default(),
            )
            .unwrap_err(),
            "navree-plugin-announcements 1.0.0: [package.metadata.navree.plugin] namespace `announcements`: the app already grants permissions under `announcements.` (in config/workspace.toml); a plugin's namespace must be its own; nothing was changed"
        );

        let long_perm = format!("announcements.{}", "a".repeat(115));
        let long_manifest = format!(
            "module = \"AnnouncementsModule::from_config()\"\ngrants = {{ member = [\"{long_perm}\"] }}\n"
        );
        let clean_files = memory_app_files(cargo_toml);
        assert_eq!(
            plan(
                &clean_files,
                &announcements_chosen(&long_manifest, false),
                &plan_facts(&clean_files, &[], false),
                &small_metadata(&["postgres"]),
                &AddOptions::default(),
            )
            .unwrap_err(),
            format!(
                "navree-plugin-announcements 1.0.0: [package.metadata.navree.plugin] `grants.member`: `{}` is longer than 128 characters; nothing was changed",
                shown(&long_perm)
            )
        );

        let mut marked_files = memory_app_files(cargo_toml);
        marked_files.config.insert(
            "config/workspace.toml".to_owned(),
            "[roles.member]\npermissions = [\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n"
                .to_owned(),
        );
        assert!(
            plan(
                &marked_files,
                &announcements_chosen(with_grants, true),
                &plan_facts(&marked_files, &[], false),
                &small_metadata(&["postgres"]),
                &AddOptions::default(),
            )
            .is_ok()
        );
    }
}
