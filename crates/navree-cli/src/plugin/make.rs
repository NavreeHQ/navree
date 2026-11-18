//! Scaffolding a community plugin crate (`navree make:plugin`).

use std::path::Path;

use anyhow::{Context, bail};
use serde::Serialize;

use crate::clock::Timestamp;
use crate::make::MakeReport;
use crate::names::Name;
use crate::plugin::cargo_toml::{AppNavree, DepTable, plugin_navree_dependency, turn_on};
use crate::plugin::files::find_app;
use crate::plugin::manifest::{RESERVED_NAMESPACES, shown};
use crate::scaffold::{FileSet, join};
use crate::templates::{TemplateError, render};

/// Maximum length of a plugin name so `navree-plugin-{name}` stays at most 64 characters.
const MAX_PLUGIN_NAME_LEN: usize = 50;

/// What `navree make:plugin` was asked.
#[derive(Clone, Debug)]
pub struct MakePluginOptions {
    /// Validated `snake_case` plugin name (`announcements`).
    pub name: Name,
    /// Parent folder relative to the working directory (`"plugins"` by default).
    pub path: String,
    /// Optional client crate name to wrap in `src/client.rs` (`"reqwest"`).
    pub wrap: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PluginContext {
    name: String,
    package: String,
    crate_name: String,
    pascal: String,
    title: String,
    navree_dependency: String,
    navree_dev_dependency: String,
    wrap: Option<String>,
    wrap_ident: Option<String>,
    year: i64,
}

fn is_wrap_crate_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn wrap_rust_ident(crate_name: &str) -> String {
    let ident = crate_name.replace('-', "_");
    if crate::names::KEYWORDS.contains(&ident.as_str()) {
        format!("r#{ident}")
    } else {
        ident
    }
}

fn dev_navree_dependency(navree_dependency: &str) -> String {
    let trimmed = navree_dependency.trim();
    if let Some(inner) = trimmed
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    {
        let inner = inner.trim();
        if inner.contains("default-features = false") && !inner.contains("features") {
            return format!("{{ {inner}, features = [\"testing\", \"sqlite\"] }}");
        }
    }

    let probe = format!("[dependencies]\nnavree = {trimmed}\n\n[dev-dependencies]\n");
    let required = vec!["testing".to_owned(), "sqlite".to_owned()];
    if let Ok(Some(updated)) = turn_on(&probe, DepTable::DevDependencies, &required)
        && let Some((_, dev_part)) = updated.split_once("[dev-dependencies]")
    {
        for line in dev_part.lines() {
            let line = line.trim();
            if let Some(rhs) = line.strip_prefix("navree") {
                let rhs = rhs.trim_start();
                if let Some(rhs) = rhs.strip_prefix('=') {
                    return rhs.trim().to_owned();
                }
            }
        }
    }

    format!(
        "{{ version = \"1.4.0\", default-features = false, features = [\"testing\", \"sqlite\"] }}"
    )
}

fn target_paths(cwd: &Path, raw_parent: &str, snake: &str) -> (String, std::path::PathBuf) {
    let normalized = raw_parent.replace('\\', "/");
    let raw_path = Path::new(raw_parent);
    if raw_path.is_absolute() {
        let plugin_dir = raw_path.join(snake);
        let shown_dir = match plugin_dir.strip_prefix(cwd) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => {
                let trimmed = normalized.trim_end_matches('/');
                if trimmed.is_empty() {
                    format!("/{snake}")
                } else {
                    format!("{trimmed}/{snake}")
                }
            }
        };
        return (shown_dir, plugin_dir);
    }

    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let shown_dir = if parts.is_empty() {
        snake.to_owned()
    } else {
        format!("{}/{snake}", parts.join("/"))
    };
    let plugin_dir = join(cwd, &shown_dir);
    (shown_dir, plugin_dir)
}

/// Writes `{path}/{name}/` relative to `cwd`; the report's hint is the `plugin:add --path` line.
/// Inside an app (`find_app(cwd)` answers), the scaffold's `navree` dependency comes from the app's.
pub fn make_plugin(cwd: &Path, options: &MakePluginOptions) -> anyhow::Result<MakeReport> {
    let snake = options.name.snake();
    if snake.len() > MAX_PLUGIN_NAME_LEN {
        bail!("`{snake}` is too long: a plugin name is at most 50 characters");
    }
    if RESERVED_NAMESPACES.binary_search(&snake).is_ok() {
        bail!(
            "`{snake}` gives the namespace `{snake}`, which the framework keeps for itself; choose another name"
        );
    }
    if let Some(wrap) = options.wrap.as_deref()
        && !is_wrap_crate_name(wrap)
    {
        bail!("--wrap: `{}` is not a crate name", shown(wrap));
    }

    let (shown_dir, plugin_dir) = target_paths(cwd, &options.path, snake);
    if plugin_dir.exists() || std::fs::symlink_metadata(&plugin_dir).is_ok() {
        bail!("{shown_dir} exists; choose another name or remove the folder");
    }

    let navree_dependency = match find_app(cwd) {
        Ok(roots) => {
            let cargo_toml_path = roots.package.join("Cargo.toml");
            let cargo_toml = std::fs::read_to_string(&cargo_toml_path)
                .with_context(|| format!("cannot read {}", cargo_toml_path.display()))?;
            let workspace_toml = if roots.workspace == roots.package {
                cargo_toml.clone()
            } else {
                std::fs::read_to_string(roots.workspace.join("Cargo.toml")).unwrap_or_default()
            };
            let app = AppNavree {
                dir: &roots.package,
                cargo_toml: &cargo_toml,
                workspace_dir: &roots.workspace,
                workspace_toml: &workspace_toml,
            };
            plugin_navree_dependency(Some(app), &plugin_dir)
        }
        Err(_) => plugin_navree_dependency(None, &plugin_dir),
    };

    let year = Timestamp::now().year;
    let files = plugin_files(
        &options.name,
        &navree_dependency,
        options.wrap.as_deref(),
        year,
    )?;
    let created = files.write(&plugin_dir)?;
    Ok(MakeReport {
        created,
        updated: Vec::new(),
        hints: vec![format!("navree plugin:add --path {shown_dir}")],
    })
}

/// The files, relative to the plugin's folder; `year` is the year of the licence line
/// (`make_plugin` passes `crate::clock::Timestamp::now().year`).
pub fn plugin_files(
    name: &Name,
    navree_dependency: &str,
    wrap: Option<&str>,
    year: i64,
) -> Result<FileSet, TemplateError> {
    let snake = name.snake().to_owned();
    let package = format!("navree-plugin-{}", name.kebab());
    let crate_name = format!("navree_plugin_{snake}");
    let ctx = PluginContext {
        name: snake,
        package,
        crate_name,
        pascal: name.pascal(),
        title: name.title(),
        navree_dependency: navree_dependency.trim().to_owned(),
        navree_dev_dependency: dev_navree_dependency(navree_dependency),
        wrap: wrap.map(str::to_owned),
        wrap_ident: wrap.map(wrap_rust_ident),
        year,
    };

    let mut files = FileSet::new();
    for (template, path) in [
        ("plugin/Cargo.toml", "Cargo.toml"),
        ("plugin/lib.rs", "src/lib.rs"),
        ("plugin/options.rs", "src/options.rs"),
        ("plugin/boots.rs", "tests/boots.rs"),
        ("plugin/README.md", "README.md"),
        ("plugin/LICENSE-MIT", "LICENSE-MIT"),
        ("plugin/LICENSE-APACHE", "LICENSE-APACHE"),
        ("plugin/ci.yml", ".github/workflows/ci.yml"),
    ] {
        files.add(path, render(template, &ctx)?);
    }
    if wrap.is_some() {
        files.add("src/client.rs", render("plugin/client.rs", &ctx)?);
    }
    Ok(files)
}
