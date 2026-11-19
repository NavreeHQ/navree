//! Ignored end-to-end plugin lifecycle tests against a generated app.
//!
//! Run them with:
//!
//! ```text
//! CARGO_TARGET_DIR=target/builds/cli cargo test -p navree-cli --test plugins -- --ignored
//! ```

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::forward::ProcessRunner;
use navree_cli::names::{AppName, Name};
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};
use navree_cli::plugin::add::{self, AddOptions};
use navree_cli::plugin::index::{Index, IndexKind};
use navree_cli::plugin::list::{self, ListOptions};
use navree_cli::plugin::make::{MakePluginOptions, make_plugin};
use navree_cli::plugin::manifest;
use navree_cli::plugin::remove::{self, RemoveOptions};

use crate::support;

const SQLITE: &str = "navree/sqlite";

/// Proof plugins under the repository root; populated as proof crates land.
const PROOF_PLUGINS: &[&str] = &["plugins/notices", "plugins/dakiya", "plugins/godam"];

/// The workspace root. `std::path::absolute`, not `canonicalize`, which
/// gives verbatim `\\?\C:\..` paths on Windows.
fn workspace() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")).unwrap()
}

fn facade_dir(workspace: &Path) -> PathBuf {
    std::env::var_os("NAVREE_FACADE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("crates").join("navree"))
}

fn build_target(workspace: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => workspace.join(dir),
        None => workspace.join("target"),
    }
    .join("plugins-app")
}

fn sccache_available() -> bool {
    Command::new("sccache")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn app_env(target: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "CARGO_TARGET_DIR".to_owned(),
            target.to_string_lossy().into_owned(),
        ),
        (
            "APP_DATABASE__URL".to_owned(),
            "sqlite://storage/demo.db?mode=rwc".to_owned(),
        ),
    ];
    if sccache_available() {
        env.push(("RUSTC_WRAPPER".to_owned(), "sccache".to_owned()));
    }
    env
}

/// Runs cargo in `dir` with the shared build folder and sccache when installed.
fn cargo(dir: &Path, target: &Path, args: &[&str]) -> bool {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .env("APP_DATABASE__URL", "sqlite://storage/demo.db?mode=rwc")
        .env_remove("RUSTFLAGS");
    if sccache_available() {
        command.env("RUSTC_WRAPPER", "sccache");
    }
    eprintln!("running cargo {} in {}", args.join(" "), dir.display());
    command.status().expect("cargo runs").success()
}

fn cargo_capture(dir: &Path, target: &Path, args: &[&str]) -> (bool, String) {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .env("APP_DATABASE__URL", "sqlite://storage/demo.db?mode=rwc")
        .env_remove("RUSTFLAGS")
        .stderr(std::process::Stdio::inherit());
    if sccache_available() {
        command.env("RUSTC_WRAPPER", "sccache");
    }
    eprintln!("running cargo {} in {}", args.join(" "), dir.display());
    let out = command.output().expect("cargo runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn plain_slash_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        raw.into_owned()
    };
    stripped.replace('\\', "/")
}

fn src_calls_plugin_info(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if src_calls_plugin_info(&path) {
                return true;
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(text) = fs::read_to_string(&path)
        {
            for line in text.lines() {
                let code = line.split("//").next().unwrap_or("");
                if code.contains("plugin_info!") {
                    return true;
                }
            }
        }
    }
    false
}

/// Short names of the index's `kind = "module"` entries whose crate `src/` names `plugin_info!`.
fn first_party_plugins() -> Vec<String> {
    let ws = workspace();
    Index::embedded()
        .packages
        .iter()
        .filter(|entry| entry.kind == IndexKind::Module)
        .filter(|entry| src_calls_plugin_info(&ws.join("crates").join(&entry.krate).join("src")))
        .map(|entry| entry.name.clone())
        .collect()
}

fn copy_tree(src: &Path, dst: &Path) {
    let in_tests = src.file_name().is_some_and(|n| n == "tests");
    fs::create_dir_all(dst).expect("create dir");
    for entry in fs::read_dir(src).expect("read dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        if in_tests && name.to_string_lossy().ends_with("_page.rs") {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        let file_type = entry.file_type().expect("file type");
        if file_type.is_dir() {
            copy_tree(&src_path, &dst_path);
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path).expect("copy file");
        }
    }
}

fn rewrite_proof_cargo_toml(
    plugin_toml: &str,
    workspace_toml: &str,
    workspace: &Path,
    facade: &Path,
) -> String {
    let ws_doc: toml_edit::DocumentMut = workspace_toml.parse().expect("parse workspace toml");
    let ws_pkg = ws_doc
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(toml_edit::Item::as_table);
    let ws_deps = ws_doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml_edit::Item::as_table);

    let mut doc: toml_edit::DocumentMut = plugin_toml.parse().expect("parse plugin Cargo.toml");
    if let (Some(pkg), Some(ws_pkg)) = (
        doc.get_mut("package")
            .and_then(toml_edit::Item::as_table_mut),
        ws_pkg,
    ) {
        let inherited: Vec<String> = pkg
            .iter()
            .filter_map(|(key, item)| {
                let is_ws = item
                    .as_table_like()
                    .and_then(|t| t.get("workspace"))
                    .and_then(toml_edit::Item::as_bool)
                    == Some(true);
                is_ws.then(|| key.to_owned())
            })
            .collect();
        for key in inherited {
            if let Some(ws_val) = ws_pkg.get(&key) {
                pkg.insert(&key, ws_val.clone());
            }
        }
    }

    let facade_slash = plain_slash_path(facade);
    let crates_slash = plain_slash_path(&workspace.join("crates"));
    for section in ["dependencies", "dev-dependencies"] {
        let Some(deps) = doc.get_mut(section).and_then(toml_edit::Item::as_table_mut) else {
            continue;
        };
        let keys: Vec<String> = deps.iter().map(|(k, _)| k.to_owned()).collect();
        for key in keys {
            if key == "navree" {
                if let Some(tbl) = deps
                    .get_mut("navree")
                    .and_then(toml_edit::Item::as_table_like_mut)
                    && tbl.contains_key("path")
                {
                    tbl.insert("path", toml_edit::value(facade_slash.as_str()));
                }
                continue;
            }
            if let Some(rel_crate) = deps
                .get(&key)
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|t| t.get("path"))
                .and_then(toml_edit::Item::as_str)
                .and_then(|p| p.strip_prefix("../../crates/"))
                .map(str::to_owned)
            {
                if let Some(tbl) = deps
                    .get_mut(&key)
                    .and_then(toml_edit::Item::as_table_like_mut)
                {
                    let rewritten_path = format!("{crates_slash}/{rel_crate}");
                    tbl.insert("path", toml_edit::value(rewritten_path));
                }
                continue;
            }
            let is_ws = deps
                .get(&key)
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|t| t.get("workspace"))
                .and_then(toml_edit::Item::as_bool)
                == Some(true);
            if is_ws
                && let Some(ws_deps) = ws_deps
                && let Some(ws_item) = ws_deps.get(&key)
            {
                let extra_features = deps
                    .get(&key)
                    .and_then(toml_edit::Item::as_table_like)
                    .and_then(|t| t.get("features"))
                    .cloned();
                let mut replacement = ws_item.clone();
                if let Some(extra) = extra_features
                    && let Some(tbl) = replacement.as_table_like_mut()
                {
                    tbl.insert("features", extra);
                }
                deps.insert(&key, replacement);
            }
        }
    }
    doc.to_string()
}

fn rewrite_repo_test_paths(tests_dir: &Path, workspace: &Path) {
    let Ok(entries) = fs::read_dir(tests_dir) else {
        return;
    };
    let crates_prefix = format!("{}/", plain_slash_path(&workspace.join("crates")));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(text) = fs::read_to_string(&path)
            && text.contains("../../../crates/")
        {
            let updated = text.replace("../../../crates/", &crates_prefix);
            fs::write(&path, updated).expect("rewrite repository test path");
        }
    }
}

fn copy_proof_plugin(workspace: &Path, facade: &Path, root: &Path, rel: &str) -> (String, String) {
    let src = workspace.join(rel);
    let folder = src
        .file_name()
        .expect("proof plugin folder")
        .to_string_lossy()
        .into_owned();
    let rel_dst = format!("plugins/{folder}");
    let dst = root.join("plugins").join(&folder);
    copy_tree(&src, &dst);
    rewrite_repo_test_paths(&dst.join("tests"), workspace);

    let ws_toml = fs::read_to_string(workspace.join("Cargo.toml")).expect("read workspace toml");
    let plugin_toml = fs::read_to_string(dst.join("Cargo.toml")).expect("read plugin toml");
    let rewritten = rewrite_proof_cargo_toml(&plugin_toml, &ws_toml, workspace, facade);
    let parsed: toml::Table = rewritten.parse().expect("valid rewritten toml");
    let pkg_name = parsed["package"]["name"]
        .as_str()
        .expect("package name")
        .to_owned();
    fs::write(dst.join("Cargo.toml"), rewritten).expect("write rewritten toml");
    (rel_dst, pkg_name)
}

#[test]
#[ignore = "builds the framework in a new app; run with --ignored"]
fn a_new_app_installs_each_proof_plugin_builds_checks_and_lists_it() {
    let workspace = workspace();
    let facade = facade_dir(&workspace);
    let target = build_target(&workspace);
    let env = app_env(&target);

    let parent = tempfile::Builder::new()
        .prefix("navree-plugin-e2e-")
        .tempdir()
        .unwrap();
    let now = Timestamp::now();
    let app = NewApp {
        name: AppName::parse("kesar").unwrap(),
        ui: Ui::Navree,
        kit: None,
        database: Database::Timescaledb,
        navree: NavreeSource::Path(facade.clone()),
    };
    let root = create_app(parent.path(), &app, now).unwrap();
    fs::copy(workspace.join("Cargo.lock"), root.join("Cargo.lock")).unwrap();

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        make_plugin(
            &root,
            &MakePluginOptions {
                name: Name::parse("plugin", "khata").unwrap(),
                path: "plugins".to_owned(),
                wrap: None,
            },
        )
        .expect("make:plugin khata");

        let khata_opts = AddOptions {
            path: Some(PathBuf::from("plugins/khata")),
            publish_config: true,
            app_env: env.clone(),
            ..AddOptions::default()
        };
        let mut out = Vec::new();
        add::run(&root, &khata_opts, &mut ProcessRunner, &mut out)
            .expect("plugin:add --path plugins/khata --publish-config");
        let printed = String::from_utf8(out).unwrap();

        assert!(
            printed.starts_with(
                "  run      cargo run --quiet -- config:publish --plugin khata (builds and runs the app)\n"
            ),
            "unexpected start of output:\n{printed}"
        );
        assert!(
            printed.contains("  added    navree-plugin-khata 0.1.0 (Cargo.toml)\n"),
            "missing added line:\n{printed}"
        );
        assert!(
            printed.contains(
                "  wired    .module(navree_plugin_khata::KhataModule::from_config()) (src/app.rs)\n"
            ),
            "missing wired line:\n{printed}"
        );
        assert!(
            printed.contains("  wrote    config/khata.toml\n"),
            "missing wrote line:\n{printed}"
        );
        assert!(
            printed.contains("\n\nkhata  navree-plugin-khata 0.1.0"),
            "missing blank line and plugin block:\n{printed}"
        );
        assert!(
            printed.ends_with(
                "Installed navree-plugin-khata 0.1.0. It is not an official Navree package: read its code before you deploy it.\n"
            ),
            "missing closing line:\n{printed}"
        );

        let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let khata_dep_line = cargo_toml
            .lines()
            .find(|line| line.trim_start().starts_with("navree-plugin-khata"))
            .expect("navree-plugin-khata line in Cargo.toml");
        assert!(
            khata_dep_line.contains("path = \"plugins/khata\""),
            "expected forward-slash path in `{khata_dep_line}`"
        );

        let before_second = support::files(&root);
        let mut second_out = Vec::new();
        add::run(&root, &khata_opts, &mut ProcessRunner, &mut second_out)
            .expect("second plugin:add khata");
        assert_eq!(
            support::files(&root),
            before_second,
            "second install of khata changed files"
        );

        let mut proof_packages = Vec::new();
        for &proof_rel in PROOF_PLUGINS {
            let (rel_dst, pkg_name) = copy_proof_plugin(&workspace, &facade, &root, proof_rel);
            let opts = AddOptions {
                path: Some(PathBuf::from(&rel_dst)),
                publish_config: true,
                app_env: env.clone(),
                ..AddOptions::default()
            };
            let mut proof_out = Vec::new();
            add::run(&root, &opts, &mut ProcessRunner, &mut proof_out)
                .unwrap_or_else(|e| panic!("install {rel_dst}: {e}"));
            let snap = support::files(&root);
            add::run(&root, &opts, &mut ProcessRunner, &mut Vec::new())
                .unwrap_or_else(|e| panic!("reinstall {rel_dst}: {e}"));
            assert_eq!(
                support::files(&root),
                snap,
                "reinstall of {rel_dst} changed files"
            );
            proof_packages.push(pkg_name);
        }

        assert!(
            cargo(&root, &target, &["test", "--workspace", "--quiet"]),
            "cargo test --workspace failed"
        );
        assert!(
            cargo(
                &root,
                &target,
                &["run", "--quiet", "--features", SQLITE, "--", "check"]
            ),
            "cargo run --features navree/sqlite -- check failed"
        );
        let (plugins_ok, plugins_stdout) =
            cargo_capture(&root, &target, &["run", "--quiet", "--", "plugins"]);
        assert!(plugins_ok, "cargo run -- plugins failed");
        for expected_head in [
            "khata  navree-plugin-khata 0.1.0",
            "notices  navree-plugin-notices 0.1.0  Notice board",
            "dakiya  navree-plugin-dakiya 0.1.0  Dakiya mail",
            "godam  navree-plugin-godam 0.1.0  Godam disks",
        ] {
            assert!(
                plugins_stdout.contains(expected_head),
                "plugins output did not list `{expected_head}`:\n{plugins_stdout}"
            );
        }
        assert!(
            cargo(
                &root,
                &target,
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--quiet",
                    "--",
                    "-D",
                    "warnings",
                ]
            ),
            "cargo clippy --workspace --all-targets -- -D warnings failed"
        );

        let mut list_out = Vec::new();
        list::run(
            &root,
            &ListOptions { json: false },
            &mut ProcessRunner,
            &mut list_out,
        )
        .expect("plugin:list");
        let listed = String::from_utf8(list_out).unwrap();
        assert!(
            listed.contains("navree-plugin-khata  0.1.0  wired      src/app.rs"),
            "unexpected plugin:list output:\n{listed}"
        );

        remove::run(
            &root,
            &RemoveOptions {
                package: "navree-plugin-khata".to_owned(),
            },
            &mut ProcessRunner,
            &mut Vec::new(),
        )
        .expect("plugin:remove navree-plugin-khata");
        for pkg_name in &proof_packages {
            remove::run(
                &root,
                &RemoveOptions {
                    package: pkg_name.clone(),
                },
                &mut ProcessRunner,
                &mut Vec::new(),
            )
            .unwrap_or_else(|e| panic!("plugin:remove {pkg_name}: {e}"));
        }
        assert!(
            cargo(&root, &target, &["check", "--quiet"]),
            "cargo check after plugin:remove failed"
        );

        let first_party = first_party_plugins();
        if first_party.is_empty() {
            eprintln!("no first-party package names itself a plugin yet");
        } else {
            let index = Index::embedded();
            for short in first_party {
                let entry = index.find(&short).expect("indexed first-party package");
                let crate_toml_raw = fs::read_to_string(
                    workspace
                        .join("crates")
                        .join(&entry.krate)
                        .join("Cargo.toml"),
                )
                .unwrap();
                let crate_toml: toml::Table = crate_toml_raw.parse().unwrap();
                let ns = crate_toml
                    .get("package")
                    .and_then(|p| p.get("metadata"))
                    .and_then(|m| m.get("navree"))
                    .and_then(|n| n.get("plugin"))
                    .and_then(|pl| pl.get("namespace"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| manifest::default_namespace(&entry.krate));

                let needs_storage = short == "media";
                if needs_storage {
                    add::run(
                        &root,
                        &AddOptions {
                            package: Some("storage".to_owned()),
                            app_env: env.clone(),
                            ..AddOptions::default()
                        },
                        &mut ProcessRunner,
                        &mut Vec::new(),
                    )
                    .expect("prerequisite plugin:add storage for media");
                }

                let mut fp_out = Vec::new();
                add::run(
                    &root,
                    &AddOptions {
                        package: Some(short.clone()),
                        app_env: env.clone(),
                        ..AddOptions::default()
                    },
                    &mut ProcessRunner,
                    &mut fp_out,
                )
                .unwrap_or_else(|e| panic!("first-party plugin:add {short}: {e}"));
                let fp_printed = String::from_utf8(fp_out).unwrap();
                let wired_by_add = fp_printed.contains("  wired    ");
                assert!(
                    fp_printed.contains(&format!("\n{ns}  {} ", entry.krate)),
                    "first-party {short} missing plugins block:\n{fp_printed}"
                );
                assert!(
                    fp_printed.ends_with(&format!("Installed {short}.\n")),
                    "first-party {short} missing Installed line:\n{fp_printed}"
                );

                let (ok, app_plugins) =
                    cargo_capture(&root, &target, &["run", "--quiet", "--", "plugins"]);
                assert!(ok, "plugins command failed for {short}");
                assert!(
                    app_plugins.contains(&format!("{ns}  {} ", entry.krate)),
                    "plugins command did not list {short}:\n{app_plugins}"
                );

                let mut fp_list = Vec::new();
                list::run(
                    &root,
                    &ListOptions { json: false },
                    &mut ProcessRunner,
                    &mut fp_list,
                )
                .expect("plugin:list for first-party");
                let fp_list_text = String::from_utf8(fp_list).unwrap();
                assert!(
                    fp_list_text.lines().any(|line| {
                        line.split_whitespace().next() == Some(short.as_str())
                            && line.contains("wired")
                    }),
                    "plugin:list did not mark {short} wired:\n{fp_list_text}"
                );

                let mut fp_rm = Vec::new();
                remove::run(
                    &root,
                    &RemoveOptions {
                        package: short.clone(),
                    },
                    &mut ProcessRunner,
                    &mut fp_rm,
                )
                .unwrap_or_else(|e| panic!("first-party plugin:remove {short}: {e}"));
                let fp_rm_text = String::from_utf8(fp_rm).unwrap();
                if wired_by_add {
                    assert!(
                        fp_rm_text.contains("  unwired  "),
                        "plugin:remove {short} did not unwire:\n{fp_rm_text}"
                    );
                } else {
                    assert!(
                        fp_rm_text.contains("  next     remove "),
                        "plugin:remove {short} for pre-wired module unexpected:\n{fp_rm_text}"
                    );
                }

                if needs_storage {
                    remove::run(
                        &root,
                        &RemoveOptions {
                            package: "storage".to_owned(),
                        },
                        &mut ProcessRunner,
                        &mut Vec::new(),
                    )
                    .expect("cleanup plugin:remove storage after media");
                }
            }
        }
    }));

    if let Err(payload) = outcome {
        let kept = parent.keep();
        eprintln!(
            "the generated plugin app failed; kept at {}",
            kept.display()
        );
        resume_unwind(payload);
    }
    if std::env::var_os("NAVREE_KEEP_GENERATED").is_some() {
        eprintln!("kept the generated app at {}", parent.keep().display());
    }
}

fn strip_dependencies_navree_and_added(cargo_toml: &str, added_pkg: &str) -> String {
    let mut out = String::new();
    let mut section = "";
    for line in cargo_toml.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed;
        }
        if section == "[dependencies]" {
            if let Some(rest) = trimmed.strip_prefix("navree")
                && rest.trim_start().starts_with('=')
            {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix(added_pkg)
                && rest.trim_start().starts_with('=')
            {
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

#[test]
#[ignore = "builds the framework in a new app; run with --ignored"]
fn plugin_add_on_a_generated_app_keeps_what_the_owner_changed() {
    let workspace = workspace();
    let facade = facade_dir(&workspace);

    let parent = tempfile::Builder::new()
        .prefix("navree-plugin-keep-")
        .tempdir()
        .unwrap();
    let now = Timestamp::now();
    let app = NewApp {
        name: AppName::parse("kesar").unwrap(),
        ui: Ui::Navree,
        kit: None,
        database: Database::Timescaledb,
        navree: NavreeSource::Path(facade),
    };
    let root = create_app(parent.path(), &app, now).unwrap();
    fs::copy(workspace.join("Cargo.lock"), root.join("Cargo.lock")).unwrap();

    // Add owner comments and odd spacing to Cargo.toml.
    let original_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let customized_toml = format!(
        "# Kesar manifest maintained by Simran Kaur's team in Ludhiana.\n{}",
        original_toml
            .replace(
                "serde = { version = \"1\", features = [\"derive\"] }",
                "serde   =   { version = \"1\",   features = [\"derive\"] }   # keep odd spacing"
            )
            .replace(
                "uuid = { version = \"1\", features = [\"serde\", \"v7\"] }",
                "# Keep this comment above uuid.\nuuid = { version = \"1\", features = [\"serde\", \"v7\"] }"
            )
    );
    assert_ne!(customized_toml, original_toml);
    fs::write(root.join("Cargo.toml"), &customized_toml).unwrap();

    make_plugin(
        &root,
        &MakePluginOptions {
            name: Name::parse("plugin", "notices").unwrap(),
            path: "plugins".to_owned(),
            wrap: None,
        },
    )
    .expect("make:plugin notices");

    // Ask for a feature that is off so `turn_on` edits the `navree` line in Cargo.toml too.
    let notices_toml_path = root.join("plugins/notices/Cargo.toml");
    let notices_toml = fs::read_to_string(&notices_toml_path).unwrap().replace(
        "config = [\"notices\"]\n",
        "features = [\"flags\"]\nconfig = [\"notices\"]\n",
    );
    fs::write(&notices_toml_path, notices_toml).unwrap();

    // Fresh copy before installing, for the missing-marker check.
    let fresh_parent = tempfile::Builder::new()
        .prefix("navree-plugin-nomarker-")
        .tempdir()
        .unwrap();
    let fresh_root = fresh_parent.path().join("kesar");
    copy_tree(&root, &fresh_root);

    let opts = AddOptions {
        path: Some(PathBuf::from("plugins/notices")),
        publish_config: false,
        ..AddOptions::default()
    };
    let mut out = Vec::new();
    add::run(&root, &opts, &mut ProcessRunner, &mut out).expect("first plugin:add notices");

    let after_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(after_toml.contains("navree-plugin-notices"));
    assert!(after_toml.contains("\"flags\""));
    assert_eq!(
        strip_dependencies_navree_and_added(&after_toml, "navree-plugin-notices"),
        strip_dependencies_navree_and_added(&customized_toml, "navree-plugin-notices"),
        "bytes outside the navree line and added dependency changed"
    );

    // A second run changes no file.
    let before_second = support::files(&root);
    add::run(&root, &opts, &mut ProcessRunner, &mut Vec::new()).expect("second plugin:add notices");
    assert_eq!(
        support::files(&root),
        before_second,
        "second run changed files"
    );

    // Fresh copy with the marker deleted: prints the module line and leaves `src/app.rs` unchanged.
    let fresh_app_rs_path = fresh_root.join("src/app.rs");
    let fresh_app_rs_with_marker = fs::read_to_string(&fresh_app_rs_path).unwrap();
    let fresh_app_rs_without_marker =
        fresh_app_rs_with_marker.replace("    // navree:modules\n", "");
    assert_ne!(fresh_app_rs_without_marker, fresh_app_rs_with_marker);
    fs::write(&fresh_app_rs_path, &fresh_app_rs_without_marker).unwrap();

    let mut fresh_out = Vec::new();
    add::run(&fresh_root, &opts, &mut ProcessRunner, &mut fresh_out)
        .expect("plugin:add without marker");
    let fresh_printed = String::from_utf8(fresh_out).unwrap();
    assert!(
        fresh_printed.contains(
            "  next     add this line to Navree::app() in src/app.rs: .module(navree_plugin_notices::NoticesModule::from_config())\n"
        ),
        "missing next line when marker is gone:\n{fresh_printed}"
    );
    assert_eq!(
        fs::read_to_string(&fresh_app_rs_path).unwrap(),
        fresh_app_rs_without_marker,
        "src/app.rs changed despite missing marker"
    );
}
