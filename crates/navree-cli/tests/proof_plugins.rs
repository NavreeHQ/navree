//! End-to-end installation tests for the repository's worked example plugins
//! (`plugins/notices`, `plugins/dakiya`, `plugins/godam`) and first-party
//! packages in a generated app.
//!
//! Ignored by default because each test compiles the framework in a fresh app;
//! run with:
//!
//! ```text
//! CARGO_TARGET_DIR=target/builds/proof cargo test -p navree-cli --test proof_plugins -- --ignored --test-threads 1
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::forward::ProcessRunner;
use navree_cli::names::AppName;
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};
use navree_cli::plugin::add::{self, AddOptions};
use navree_cli::plugin::list::{self, ListOptions};

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

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, cur: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(cur).expect("read dir") {
            let entry = entry.expect("dir entry");
            if entry.file_name() == "target" {
                continue;
            }
            let path = entry.path();
            let ft = entry.file_type().expect("file type");
            if ft.is_dir() {
                walk(base, &path, out);
            } else if ft.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .expect("subpath")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).expect("read file"));
            }
        }
    }

    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Reads the `navree` dependency source from `app/Cargo.toml` so a copied proof
/// plugin's `navree` entries point at the exact same source as the app's own.
fn app_navree_source_fields(app: &Path) -> Vec<(&'static str, String)> {
    let cargo_toml = fs::read_to_string(app.join("Cargo.toml")).expect("read app Cargo.toml");
    let parsed: toml::Table = cargo_toml.parse().expect("parse app Cargo.toml");
    let dep = parsed
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|deps| deps.get("navree"))
        .expect("app Cargo.toml has [dependencies].navree");

    match dep {
        toml::Value::String(ver) => vec![("version", ver.clone())],
        toml::Value::Table(tbl) => {
            if let Some(raw_path) = tbl.get("path").and_then(toml::Value::as_str) {
                let raw = Path::new(raw_path);
                let resolved = if raw.is_absolute() {
                    raw.to_path_buf()
                } else {
                    std::path::absolute(app.join(raw)).expect("resolve relative navree path")
                };
                vec![("path", plain_slash_path(&resolved))]
            } else if let Some(git) = tbl.get("git").and_then(toml::Value::as_str) {
                let mut fields = vec![("git", git.to_owned())];
                for extra in ["branch", "tag", "rev"] {
                    if let Some(val) = tbl.get(extra).and_then(toml::Value::as_str) {
                        fields.push((extra, val.to_owned()));
                    }
                }
                fields
            } else if let Some(ver) = tbl.get("version").and_then(toml::Value::as_str) {
                let mut fields = vec![("version", ver.to_owned())];
                if let Some(reg) = tbl.get("registry").and_then(toml::Value::as_str) {
                    fields.push(("registry", reg.to_owned()));
                }
                fields
            } else {
                panic!("app `navree` dependency lacks source: {dep:?}");
            }
        }
        other => panic!("unexpected `navree` dependency shape: {other:?}"),
    }
}

fn replace_navree_entry(item: &mut toml_edit::Item, source_fields: &[(&str, String)]) {
    let existing_features: Vec<String> = item
        .as_table_like()
        .and_then(|tbl| tbl.get("features"))
        .and_then(toml_edit::Item::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(toml_edit::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let mut inline = toml_edit::InlineTable::new();
    for (key, val) in source_fields {
        inline.insert(*key, toml_edit::Value::from(val.as_str()));
    }
    inline.insert("default-features", toml_edit::Value::from(false));
    if !existing_features.is_empty() {
        let mut arr = toml_edit::Array::new();
        for feat in existing_features {
            arr.push(feat.as_str());
        }
        inline.insert("features", toml_edit::Value::Array(arr));
    }
    *item = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
}

/// Copies `plugins/{name}` from `repository` under `app/plugins/{name}`,
/// replacing its `[dependencies]` and `[dev-dependencies]` `navree =` entries
/// with `app`'s own `navree` source while keeping each line's `features`.
fn copy_proof_plugin(repository: &Path, name: &str, app: &Path) {
    let src = repository.join("plugins").join(name);
    let dst = app.join("plugins").join(name);
    copy_tree(&src, &dst);

    let source_fields = app_navree_source_fields(app);
    let crates_slash = plain_slash_path(&repository.join("crates"));

    let manifest_path = dst.join("Cargo.toml");
    let raw_toml = fs::read_to_string(&manifest_path).expect("read copied plugin Cargo.toml");
    let mut doc: toml_edit::DocumentMut = raw_toml.parse().expect("parse copied plugin Cargo.toml");

    for section in ["dependencies", "dev-dependencies"] {
        let Some(deps) = doc.get_mut(section).and_then(toml_edit::Item::as_table_mut) else {
            continue;
        };
        if let Some(navree_item) = deps.get_mut("navree") {
            replace_navree_entry(navree_item, &source_fields);
        }
        let keys: Vec<String> = deps.iter().map(|(k, _)| k.to_owned()).collect();
        for key in keys {
            if key == "navree" {
                continue;
            }
            if let Some(rel_crate) = deps
                .get(&key)
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|t| t.get("path"))
                .and_then(toml_edit::Item::as_str)
                .and_then(|p| p.strip_prefix("../../crates/"))
                .map(str::to_owned)
                && let Some(tbl) = deps
                    .get_mut(&key)
                    .and_then(toml_edit::Item::as_table_like_mut)
            {
                let rewritten = format!("{crates_slash}/{rel_crate}");
                tbl.insert("path", toml_edit::value(rewritten));
            }
        }
    }
    fs::write(&manifest_path, doc.to_string()).expect("write rewritten plugin Cargo.toml");

    if let Ok(entries) = fs::read_dir(dst.join("tests")) {
        let crates_prefix = format!("{crates_slash}/");
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
}

fn strip_dependencies_navree_and_plugins(cargo_toml: &str) -> String {
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
            if trimmed.starts_with("navree-plugin-") && trimmed.contains('=') {
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

fn new_generated_app(
    prefix: &str,
    workspace: &Path,
    facade: &Path,
) -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("tempdir");
    let now = Timestamp::now();
    let app = NewApp {
        name: AppName::parse("kesar").unwrap(),
        ui: Ui::Navree,
        kit: None,
        database: Database::Timescaledb,
        navree: NavreeSource::Path(facade.to_path_buf()),
    };
    let root = create_app(parent.path(), &app, now).expect("create_app");
    fs::copy(workspace.join("Cargo.lock"), root.join("Cargo.lock")).expect("copy Cargo.lock");
    (parent, root)
}

fn plugin_block<'a>(plugins_stdout: &'a str, head_prefix: &str) -> Option<&'a str> {
    let start = plugins_stdout.find(head_prefix)?;
    let after_head = &plugins_stdout[start..];
    let first_nl = match after_head.find('\n') {
        Some(pos) => pos + 1,
        None => return Some(after_head),
    };
    let mut end = first_nl;
    for line in after_head[first_nl..].split_inclusive('\n') {
        if line.starts_with(' ') || line.trim().is_empty() {
            end += line.len();
        } else {
            break;
        }
    }
    Some(&after_head[..end])
}

#[test]
#[ignore = "builds the framework in a new app; run with --ignored"]
fn each_proof_plugin_installs_into_an_app_with_edited_files_and_a_second_run_changes_nothing() {
    let workspace = workspace();
    let facade = facade_dir(&workspace);
    let target = build_target(&workspace);
    let env = app_env(&target);

    let (parent, root) = new_generated_app("navree-proof-edited-", &workspace, &facade);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let original_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let customized_toml = format!(
            "# Kesar manifest maintained by Harpreet Sandhu's team in Ludhiana.\n{}",
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

        let nomarker_parent = tempfile::Builder::new()
            .prefix("navree-proof-nomarker-")
            .tempdir()
            .unwrap();
        let nomarker_root = nomarker_parent.path().join("kesar");
        copy_tree(&root, &nomarker_root);
        let nomarker_app_rs_path = nomarker_root.join("src/app.rs");
        let app_rs_with_marker = fs::read_to_string(&nomarker_app_rs_path).unwrap();
        let app_rs_without_marker = app_rs_with_marker.replace("    // navree:modules\n", "");
        assert_ne!(app_rs_without_marker, app_rs_with_marker);
        fs::write(&nomarker_app_rs_path, &app_rs_without_marker).unwrap();

        let expected_facade = plain_slash_path(&facade);
        let cases = [
            (
                "notices",
                "navree-plugin-notices",
                ".module(navree_plugin_notices::NoticesModule::from_config())",
                true,
            ),
            (
                "dakiya",
                "navree-plugin-dakiya",
                ".module(navree_plugin_dakiya::DakiyaModule)",
                false,
            ),
            (
                "godam",
                "navree-plugin-godam",
                ".module(navree_plugin_godam::GodamModule)",
                false,
            ),
        ];

        for (name, pkg, module_line, publish_config) in cases {
            copy_proof_plugin(&workspace, name, &root);

            let copied_toml: toml::Table =
                fs::read_to_string(root.join("plugins").join(name).join("Cargo.toml"))
                    .unwrap()
                    .parse()
                    .unwrap();
            assert_eq!(
                copied_toml["dependencies"]["navree"]["path"].as_str(),
                Some(expected_facade.as_str()),
                "copied {name} [dependencies].navree must point at the app's navree"
            );
            assert_eq!(
                copied_toml["dev-dependencies"]["navree"]["path"].as_str(),
                Some(expected_facade.as_str()),
                "copied {name} [dev-dependencies].navree must point at the app's navree"
            );

            let opts = AddOptions {
                path: Some(PathBuf::from(format!("plugins/{name}"))),
                publish_config,
                app_env: env.clone(),
                ..AddOptions::default()
            };
            let mut out = Vec::new();
            add::run(&root, &opts, &mut ProcessRunner, &mut out)
                .unwrap_or_else(|e| panic!("first plugin:add {name}: {e}"));
            let printed = String::from_utf8(out).unwrap();
            assert!(
                printed.contains(&format!("  added    {pkg} 0.1.0 (Cargo.toml)\n")),
                "{name}: missing added line:\n{printed}"
            );
            assert!(
                printed.contains(&format!("  wired    {module_line} (src/app.rs)\n")),
                "{name}: missing wired line:\n{printed}"
            );
            if publish_config {
                assert!(
                    printed.contains(&format!("  wrote    config/{name}.toml\n")),
                    "{name}: missing wrote config line:\n{printed}"
                );
            }

            let after_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
            assert!(
                after_toml.contains(pkg),
                "{name}: {pkg} missing in Cargo.toml"
            );
            assert_eq!(
                strip_dependencies_navree_and_plugins(&after_toml),
                strip_dependencies_navree_and_plugins(&customized_toml),
                "{name}: bytes outside the navree and plugin dependency lines changed"
            );

            let before_second = files(&root);
            add::run(&root, &opts, &mut ProcessRunner, &mut Vec::new())
                .unwrap_or_else(|e| panic!("second plugin:add {name}: {e}"));
            assert_eq!(
                files(&root),
                before_second,
                "{name}: second plugin:add run changed files"
            );

            copy_proof_plugin(&workspace, name, &nomarker_root);
            let mut nomarker_out = Vec::new();
            add::run(&nomarker_root, &opts, &mut ProcessRunner, &mut nomarker_out)
                .unwrap_or_else(|e| panic!("markerless plugin:add {name}: {e}"));
            let nomarker_printed = String::from_utf8(nomarker_out).unwrap();
            assert!(
                nomarker_printed.contains(&format!(
                    "  next     add this line to Navree::app() in src/app.rs: {module_line}\n"
                )),
                "{name}: missing next line without marker:\n{nomarker_printed}"
            );
            assert_eq!(
                fs::read_to_string(&nomarker_app_rs_path).unwrap(),
                app_rs_without_marker,
                "{name}: src/app.rs changed despite missing marker"
            );
        }

        assert!(
            cargo(&root, &target, &["check", "--quiet"]),
            "cargo check failed on the wired app"
        );

        let (plugins_ok, plugins_stdout) =
            cargo_capture(&root, &target, &["run", "--quiet", "--", "plugins"]);
        assert!(plugins_ok, "cargo run -- plugins failed");
        for expected_head in [
            "notices  navree-plugin-notices 0.1.0  Notice board",
            "dakiya  navree-plugin-dakiya 0.1.0  Dakiya mail",
            "godam  navree-plugin-godam 0.1.0  Godam disks",
        ] {
            assert!(
                plugins_stdout.contains(expected_head),
                "plugins output did not list `{expected_head}`:\n{plugins_stdout}"
            );
        }
        let notices_block = plugin_block(
            &plugins_stdout,
            "notices  navree-plugin-notices 0.1.0  Notice board",
        )
        .expect("notices block in plugins output");
        assert!(
            notices_block.contains("notices:expire"),
            "notices block did not list `notices:expire`:\n{notices_block}"
        );
    }));

    if let Err(payload) = outcome {
        let kept = parent.keep();
        eprintln!(
            "the edited proof-plugin app failed; kept at {}",
            kept.display()
        );
        resume_unwind(payload);
    }
}

#[cfg(windows)]
#[test]
#[ignore = "builds the framework in a new app; run with --ignored"]
fn each_proof_plugin_installs_with_a_windows_style_path() {
    let workspace = workspace();
    let facade = facade_dir(&workspace);
    let target = build_target(&workspace);
    let env = app_env(&target);

    let (parent, root) = new_generated_app("navree-proof-winpath-", &workspace, &facade);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        for name in ["notices", "dakiya", "godam"] {
            copy_proof_plugin(&workspace, name, &root);
        }

        for (win_path, pkg, slash_path) in [
            (
                r"plugins\notices",
                "navree-plugin-notices",
                "plugins/notices",
            ),
            (r"plugins\dakiya", "navree-plugin-dakiya", "plugins/dakiya"),
            (r"PLUGINS\Godam", "navree-plugin-godam", "plugins/godam"),
        ] {
            let opts = AddOptions {
                path: Some(PathBuf::from(win_path)),
                publish_config: false,
                app_env: env.clone(),
                ..AddOptions::default()
            };
            let mut out = Vec::new();
            add::run(&root, &opts, &mut ProcessRunner, &mut out)
                .unwrap_or_else(|e| panic!("plugin:add --path {win_path}: {e}"));
            let printed = String::from_utf8(out).unwrap();
            assert!(
                printed.contains(&format!("  added    {pkg} 0.1.0 (Cargo.toml)\n")),
                "missing added line for {win_path}:\n{printed}"
            );

            let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
            let dep_line = cargo_toml
                .lines()
                .find(|line| line.trim_start().starts_with(pkg))
                .unwrap_or_else(|| panic!("missing {pkg} in Cargo.toml:\n{cargo_toml}"));
            assert!(
                dep_line.contains(&format!("path = \"{slash_path}\"")),
                "expected forward-slash `{slash_path}` in `{dep_line}`"
            );
            assert!(
                !dep_line.contains('\\'),
                "backslash written into Cargo.toml: `{dep_line}`"
            );

            let before_second = files(&root);
            add::run(&root, &opts, &mut ProcessRunner, &mut Vec::new())
                .unwrap_or_else(|e| panic!("second plugin:add --path {win_path}: {e}"));
            assert_eq!(
                files(&root),
                before_second,
                "second Windows-path install of {win_path} changed files"
            );
        }
    }));

    if let Err(payload) = outcome {
        let kept = parent.keep();
        eprintln!(
            "the Windows-path proof-plugin app failed; kept at {}",
            kept.display()
        );
        resume_unwind(payload);
    }
}

#[test]
#[ignore = "builds the framework in a new app; run with --ignored"]
fn first_party_packages_install_into_a_new_app_and_are_listed() {
    let workspace = workspace();
    let facade = facade_dir(&workspace);
    let target = build_target(&workspace);
    let env = app_env(&target);

    let (parent, root) = new_generated_app("navree-proof-firstparty-", &workspace, &facade);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        for short in ["flags", "social"] {
            let mut out = Vec::new();
            add::run(
                &root,
                &AddOptions {
                    package: Some(short.to_owned()),
                    app_env: env.clone(),
                    ..AddOptions::default()
                },
                &mut ProcessRunner,
                &mut out,
            )
            .unwrap_or_else(|e| panic!("plugin:add {short}: {e}"));
            let printed = String::from_utf8(out).unwrap();
            assert!(
                printed.ends_with(&format!("Installed {short}.\n")),
                "plugin:add {short} did not end with `Installed {short}.`:\n{printed}"
            );
        }

        assert!(
            cargo(&root, &target, &["check", "--quiet"]),
            "cargo check failed after installing flags and social"
        );

        let (plugins_ok, plugins_stdout) =
            cargo_capture(&root, &target, &["run", "--quiet", "--", "plugins"]);
        assert!(plugins_ok, "cargo run -- plugins failed");
        assert!(
            plugins_stdout.contains("flags  navree-flags "),
            "plugins output did not list flags:\n{plugins_stdout}"
        );
        assert!(
            plugins_stdout.contains("social  navree-social "),
            "plugins output did not list social:\n{plugins_stdout}"
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
        for short in ["flags", "social"] {
            assert!(
                listed
                    .lines()
                    .any(|line| line.split_whitespace().next() == Some(short)
                        && line.contains("wired")),
                "plugin:list did not say `wired` for {short}:\n{listed}"
            );
        }
    }));

    if let Err(payload) = outcome {
        let kept = parent.keep();
        eprintln!(
            "the first-party proof app failed; kept at {}",
            kept.display()
        );
        resume_unwind(payload);
    }
}
