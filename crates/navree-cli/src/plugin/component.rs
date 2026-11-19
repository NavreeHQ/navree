//! Generating a Navree UI component (`navree make:component`).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

use crate::edit::{Edit, declare_module, insert_above_marker};
use crate::names::Name;
use crate::plugin::cargo_toml::{DepTable, turn_on};
use crate::plugin::manifest::{default_namespace, is_namespace, shown};
use crate::scaffold::{FileSet, join};
use crate::templates::render;

/// What `navree make:component` was asked.
#[derive(Clone, Debug)]
pub struct ComponentOptions {
    /// Component name in `snake_case` (`drop_point`).
    pub name: String,
    /// Target crate folder relative to the working directory (`"."` by default).
    pub path: String,
    /// Include an extension script (`assets/EXT.js`) and browser test (`tests/browser_NAME.rs`).
    pub script: bool,
    /// Include a scoped extension stylesheet (`assets/EXT.css`).
    pub sheet: bool,
    /// Also write a `FieldControl` (`NAME_CONTROL`) and its rule test.
    pub field: bool,
    /// Report what would be created and edited without writing anything.
    pub dry_run: bool,
    /// Replace existing generated files.
    pub force: bool,
}

/// What `make:component` did, as the lines to print.
#[derive(Clone, Debug, Default)]
pub struct ComponentReport {
    /// Printed lines in order, each with its two-space indent and verb.
    pub lines: Vec<String>,
    /// Created and edited `.rs` files, which the command formats.
    pub rust_files: Vec<PathBuf>,
    /// With `--dry-run`: the files that would be written, printed after `lines`.
    pub dry_run_files: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ComponentContext {
    name: String,
    pascal: String,
    ext: String,
    screaming: String,
    title: String,
    ns: String,
    crate_name: String,
    script: bool,
    sheet: bool,
    field: bool,
}

fn resolve_crate_dir(cwd: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let normalized = raw_path.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.is_empty() {
        cwd.to_path_buf()
    } else {
        join(cwd, &parts.join("/"))
    }
}

fn read_crate_identity(
    cargo_toml_path: &Path,
    cargo_toml: &str,
) -> anyhow::Result<(String, String)> {
    let parsed: toml::Value = toml::from_str(cargo_toml)
        .with_context(|| format!("cannot parse {}", cargo_toml_path.display()))?;
    let package_table = parsed
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| anyhow::anyhow!("{} has no [package] table", cargo_toml_path.display()))?;
    let package_name = package_table
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{} has no [package].name", cargo_toml_path.display()))?;
    let crate_name = package_name.replace('-', "_");

    let plugin_meta = package_table
        .get("metadata")
        .and_then(toml::Value::as_table)
        .and_then(|meta| meta.get("navree"))
        .and_then(toml::Value::as_table)
        .and_then(|navree| navree.get("plugin"));

    let ns = match plugin_meta {
        Some(plugin_val) => {
            let explicit = plugin_val
                .as_table()
                .and_then(|table| table.get("namespace"))
                .map(|val| {
                    val.as_str().ok_or_else(|| {
                        anyhow::anyhow!(
                            "{}: [package.metadata.navree.plugin].namespace must be a string",
                            cargo_toml_path.display()
                        )
                    })
                })
                .transpose()?;
            let resolved = match explicit {
                Some(ns) => ns.to_owned(),
                None => default_namespace(package_name),
            };
            if !is_namespace(&resolved) {
                bail!(
                    "{}: `{}` is not a valid plugin namespace",
                    cargo_toml_path.display(),
                    shown(&resolved)
                );
            }
            resolved
        }
        None => "app".to_owned(),
    };

    Ok((crate_name, ns))
}

fn write_file(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("cannot write {}", path.display()))
}

/// Generates a Navree UI component in `options.path` (resolved against `cwd`).
///
/// `options.name` is checked with [`Name::parse`]; an invalid name fails with
/// `{name} is not a component name: lower-case letters, digits and _, starting with a letter`.
pub fn make_component(cwd: &Path, options: &ComponentOptions) -> anyhow::Result<ComponentReport> {
    let Ok(name) = Name::parse("component", &options.name) else {
        bail!(
            "{} is not a component name: lower-case letters, digits and _, starting with a letter",
            shown(&options.name)
        );
    };

    let crate_dir = resolve_crate_dir(cwd, &options.path);
    let cargo_toml_path = crate_dir.join("Cargo.toml");
    let cargo_toml = std::fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("cannot read {}", cargo_toml_path.display()))?;
    let (crate_name, ns) = read_crate_identity(&cargo_toml_path, &cargo_toml)?;

    let snake = name.snake().to_owned();
    let ext = name.kebab();
    let screaming = snake.to_ascii_uppercase();
    let ctx = ComponentContext {
        name: snake.clone(),
        pascal: name.pascal(),
        ext: ext.clone(),
        screaming: screaming.clone(),
        title: name.title(),
        ns,
        crate_name,
        script: options.script,
        sheet: options.sheet,
        field: options.field,
    };

    let mut generated = FileSet::new();
    generated.add(
        format!("src/ui/{snake}.rs"),
        render("component/component.rs", &ctx)?,
    );
    if options.script {
        generated.add(
            format!("assets/{ext}.js"),
            render("component/script.js", &ctx)?,
        );
    }
    if options.sheet {
        generated.add(
            format!("assets/{ext}.css"),
            render("component/sheet.css", &ctx)?,
        );
    }
    if options.script {
        generated.add(
            format!("tests/browser_{snake}.rs"),
            render("component/browser_test.rs", &ctx)?,
        );
    }

    let mut target_states = Vec::new();
    for file in generated.iter() {
        let target = join(&crate_dir, &file.path);
        let exists = target.exists() || std::fs::symlink_metadata(&target).is_ok();
        if exists && !options.force {
            bail!("{} exists; use --force to replace it", file.path);
        }
        target_states.push((file, target, exists));
    }

    let ui_mod_path = join(&crate_dir, "src/ui/mod.rs");
    let ui_mod_existed = ui_mod_path.is_file();
    let ui_mod_edit = if ui_mod_existed {
        let existing = std::fs::read_to_string(&ui_mod_path)
            .with_context(|| format!("cannot read {}", ui_mod_path.display()))?;
        match declare_module(&existing, &format!("pub mod {snake};")) {
            Edit::Changed(updated) => Some(updated),
            Edit::Unchanged => None,
        }
    } else {
        Some(format!("//! UI components.\n\npub mod {snake};\n"))
    };

    let has_extension = options.script || options.sheet;
    let lib_rs_path = join(&crate_dir, "src/lib.rs");
    let mut lib_edit = None;
    let mut wired = false;
    let mut missing_wire_marker = false;
    let mut cargo_edit = None;
    let mut main_feature_added = false;
    let mut dev_feature_added = false;

    if has_extension {
        let contribute_stmt = format!("m.contribute(ui::{snake}::{screaming});");
        match std::fs::read_to_string(&lib_rs_path) {
            Ok(lib_src) => match insert_above_marker(&lib_src, "navree:wire", &contribute_stmt) {
                Some(Edit::Changed(after_wire)) => {
                    let final_lib = match declare_module(&after_wire, "pub mod ui;") {
                        Edit::Changed(with_mod) => with_mod,
                        Edit::Unchanged => after_wire,
                    };
                    lib_edit = Some(final_lib);
                    wired = true;
                }
                Some(Edit::Unchanged) => {
                    if let Edit::Changed(with_mod) = declare_module(&lib_src, "pub mod ui;") {
                        lib_edit = Some(with_mod);
                    }
                }
                None => {
                    missing_wire_marker = true;
                    if let Edit::Changed(with_mod) = declare_module(&lib_src, "pub mod ui;") {
                        lib_edit = Some(with_mod);
                    }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing_wire_marker = true;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot read {}", lib_rs_path.display()));
            }
        }

        let main_edit = turn_on(
            &cargo_toml,
            DepTable::Dependencies,
            &["ui-extension-files".to_owned()],
        )
        .map_err(|reason| anyhow::anyhow!("Cargo.toml: {reason}"))?;
        main_feature_added = main_edit.is_some();

        let dev_edit = if options.script {
            let base_toml = main_edit.as_deref().unwrap_or(&cargo_toml);
            turn_on(
                base_toml,
                DepTable::DevDependencies,
                &["browser".to_owned()],
            )
            .map_err(|reason| anyhow::anyhow!("Cargo.toml: {reason}"))?
        } else {
            None
        };
        dev_feature_added = dev_edit.is_some();
        cargo_edit = dev_edit.or(main_edit);
    }

    let mut hints = vec![format!(
        "cargo test the_{snake}_follows_the_component_rules"
    )];
    if has_extension && missing_wire_marker {
        hints.push(format!(
            "add m.contribute(crate::ui::{snake}::{screaming}) to the register of the module that should show it"
        ));
    }

    if options.dry_run {
        let mut lines = Vec::new();
        let mut preview = FileSet::new();
        for (file, _, exists) in &target_states {
            if *exists {
                lines.push(format!("  would replace    {}", file.path));
            } else {
                lines.push(format!("  would create     {}", file.path));
            }
            preview.add(file.path.clone(), file.contents.clone());
        }
        if let Some(contents) = &ui_mod_edit {
            if ui_mod_existed {
                lines.push("  would update     src/ui/mod.rs".to_owned());
            } else {
                lines.push("  would create     src/ui/mod.rs".to_owned());
                preview.add("src/ui/mod.rs", contents.clone());
            }
        }
        if lib_edit.is_some() {
            lines.push("  would update     src/lib.rs".to_owned());
        }
        if cargo_edit.is_some() {
            lines.push("  would update     Cargo.toml".to_owned());
        }
        for hint in &hints {
            lines.push(format!("  next     {hint}"));
        }
        return Ok(ComponentReport {
            lines,
            rust_files: Vec::new(),
            dry_run_files: Some(preview.to_string()),
        });
    }

    let mut lines = Vec::new();
    let mut rust_files = Vec::new();
    for (file, target, exists) in &target_states {
        write_file(target, &file.contents)?;
        if target.extension().is_some_and(|ext| ext == "rs") {
            rust_files.push(target.clone());
        }
        if *exists {
            lines.push(format!("  replaced {}", file.path));
        } else {
            lines.push(format!("  created  {}", file.path));
        }
    }

    if let Some(contents) = &ui_mod_edit {
        write_file(&ui_mod_path, contents)?;
        rust_files.push(ui_mod_path);
        lines.push("  edited   src/ui/mod.rs".to_owned());
    }

    if let Some(contents) = &lib_edit {
        write_file(&lib_rs_path, contents)?;
        rust_files.push(lib_rs_path);
    }
    if wired {
        lines.push(format!(
            "  wired    m.contribute(ui::{snake}::{screaming}) (src/lib.rs)"
        ));
    }

    if let Some(contents) = &cargo_edit {
        write_file(&cargo_toml_path, contents)?;
        if main_feature_added {
            lines.push("  feature  navree: ui-extension-files (Cargo.toml)".to_owned());
        } else if dev_feature_added {
            lines.push("  feature  navree: browser ([dev-dependencies] in Cargo.toml)".to_owned());
        }
    }

    for hint in &hints {
        lines.push(format!("  next     {hint}"));
    }

    Ok(ComponentReport {
        lines,
        rust_files,
        dry_run_files: None,
    })
}
