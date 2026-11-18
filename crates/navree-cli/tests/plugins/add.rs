//! Integration tests for `navree plugin:add`.

use std::fs;
use std::path::{Path, PathBuf};

use navree_cli::plugin::add::{self, AddOptions};
use navree_cli::plugin::index::Index;
use navree_cli::plugin::manifest::{self, AppFacts};

use crate::support::{self, ScriptedCargo};

const APP_CARGO_TOML: &str = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[workspace]

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
";

const MEMBER_CARGO_TOML: &str = "\
[package]
name = \"svc\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { workspace = true }
";

const ANNOUNCEMENTS_MANIFEST: &str = "\
title = \"Announcements\"
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\", \"ui\"]
config = [\"announcements\"]
env = [\"APP_ANNOUNCEMENTS__SIGNING_KEY\"]
grants = { member = [\"announcements.read\"] }
";

const ANNOUNCEMENTS_PLUGINS_JSON: &str = r#"{
  "navree": "1.4.0",
  "plugins": [
    {
      "name": "navree-plugin-announcements",
      "namespace": "announcements",
      "version": "1.0.0",
      "title": "Announcements",
      "description": "Notices for every member of a workspace, by Simran Kaur's team in Ludhiana.",
      "homepage": null,
      "repository": "https://code.ludhiana.example/navree-plugin-announcements",
      "license": "MIT OR Apache-2.0",
      "first_party": false,
      "modules": [
        {
          "name": "AnnouncementsModule",
          "type": "navree_plugin_announcements::AnnouncementsModule",
          "commands": ["announcements:prune"],
          "contributions": {
            "navree_config::registry::SectionRegistration": 1,
            "navree_contracts::permissions::Permissions": 1,
            "navree_http::controller::ControllerRegistration": 1,
            "navree_kernel::plugin::Publishable": 2,
            "navree_schema::migration::MigrationSet": 1
          },
          "publishable": {"views": 3, "locales": 2}
        }
      ]
    }
  ]
}"#;

fn write_shell(root: &Path, body: &str) {
    let ui_dir = root.join("src/ui");
    fs::create_dir_all(&ui_dir).expect("create src/ui");
    fs::write(ui_dir.join("shell.rs"), body).expect("write src/ui/shell.rs");
}

#[test]
fn plugin_add_writes_the_lines_the_installing_page_shows() {
    // 1. Community crate without `--publish-config`, shell without `.contributed()`.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = support::app(tmp.path(), APP_CARGO_TOML);
    write_shell(&root, "pub fn shell() -> Shell { Shell::new(\"Kesar\") }\n");
    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    let mut out = Vec::new();
    add::run(
        &root,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            ..AddOptions::default()
        },
        &mut runner,
        &mut out,
    )
    .expect("plugin:add succeeds");

    let expected = "\
  added    navree-plugin-announcements 1.0.0 (Cargo.toml)
  feature  navree: workspace, ui (Cargo.toml)
  wired    .module(navree_plugin_announcements::AnnouncementsModule::from_config()) (src/app.rs)
  env      APP_ANNOUNCEMENTS__SIGNING_KEY (.env.example)
  next     navree config:publish --plugin announcements
  grant    member: announcements.read (config/workspace.toml)
  next     navree migrate
  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs
  next     navree plugins, to see what navree-plugin-announcements registers (it builds and runs the app)
Installed navree-plugin-announcements 1.0.0. It is not an official Navree package: read its code before you deploy it.
";
    assert_eq!(String::from_utf8(out).unwrap(), expected);

    let installing_page = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/installing.mdx"),
    )
    .expect("read docs/plugins/installing.mdx")
    .replace("\r\n", "\n");
    let (_, after_heading) = installing_page
        .split_once("\n## plugin:add\n")
        .expect("## plugin:add heading in docs/plugins/installing.mdx");
    let section = after_heading
        .split("\n## ")
        .next()
        .expect("section under ## plugin:add");
    let (_, after_fence) = section
        .split_once("\n```text\n")
        .expect("first text block under ## plugin:add");
    let (first_text_block, _) = after_fence
        .split_once("\n```\n")
        .expect("closing fence of first text block");
    assert_eq!(format!("{first_text_block}\n"), expected);

    let written_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(written_toml.contains("navree-plugin-announcements = \"1.0.0\""));
    assert!(written_toml.contains("features = [\"postgres\", \"workspace\", \"ui\"]"));

    // 2. With `--publish-config` and `plugins --json` output.
    let tmp_pub = tempfile::tempdir().expect("tempdir");
    let root_pub = support::app(tmp_pub.path(), APP_CARGO_TOML);
    write_shell(
        &root_pub,
        "pub fn shell() -> Shell { Shell::new(\"Kesar\") }\n",
    );
    let mut runner_pub = ScriptedCargo::new()
        .package(
            "navree-plugin-announcements",
            "1.0.0",
            ANNOUNCEMENTS_MANIFEST,
            "navree_plugin_announcements",
        )
        .plugins_json(ANNOUNCEMENTS_PLUGINS_JSON);
    let mut out_pub = Vec::new();
    add::run(
        &root_pub,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            publish_config: true,
            ..AddOptions::default()
        },
        &mut runner_pub,
        &mut out_pub,
    )
    .expect("plugin:add --publish-config succeeds");

    let expected_pub = "\
  run      cargo run --quiet -- config:publish --plugin announcements (builds and runs the app)
  added    navree-plugin-announcements 1.0.0 (Cargo.toml)
  feature  navree: workspace, ui (Cargo.toml)
  wired    .module(navree_plugin_announcements::AnnouncementsModule::from_config()) (src/app.rs)
  env      APP_ANNOUNCEMENTS__SIGNING_KEY (.env.example)
  wrote    config/announcements.toml
  grant    member: announcements.read (config/workspace.toml)
  next     navree migrate
  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs

announcements  navree-plugin-announcements 1.0.0  Announcements
  AnnouncementsModule  navree_plugin_announcements::AnnouncementsModule
    commands  announcements:prune
    adds      1 ControllerRegistration, 1 MigrationSet, 1 Permissions, 2 Publishable, 1 SectionRegistration
    publishes 3 views, 2 locales
Installed navree-plugin-announcements 1.0.0. It is not an official Navree package: read its code before you deploy it.
";
    assert_eq!(String::from_utf8(out_pub).unwrap(), expected_pub);

    // 3. With shell calling `for_request(`.
    let tmp_for = tempfile::tempdir().expect("tempdir");
    let root_for = support::app(tmp_for.path(), APP_CARGO_TOML);
    write_shell(
        &root_for,
        "pub fn page() { Shell::new(\"Kesar\").for_request(&headers); }\n",
    );
    let mut runner_for = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    let mut out_for = Vec::new();
    add::run(
        &root_for,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_for,
        &mut out_for,
    )
    .expect("plugin:add with for_request shell succeeds");
    let text_for = String::from_utf8(out_for).unwrap();
    assert!(text_for.contains(
        "  next     show plugin links in the sidebar: add .contributed() to Shell::new(..) in src/ui/shell.rs, and build Desk's page from the AppShell extractor (.context(..), .user_menu(..)) instead of Shell::for_request; see /docs/ui/app-shell/\n"
    ));

    // 4. First-party `billing`.
    let tmp_bill = tempfile::tempdir().expect("tempdir");
    let root_bill = support::app(tmp_bill.path(), APP_CARGO_TOML);
    let billing_json = r#"{
  "navree": "1.4.0",
  "plugins": [
    {
      "name": "navree-billing",
      "namespace": "billing",
      "version": "1.4.0",
      "title": null,
      "modules": [
        {
          "name": "BillingModule",
          "type": "navree_billing::BillingModule",
          "commands": [],
          "contributions": {
            "navree_config::registry::SectionRegistration": 1
          },
          "publishable": {}
        }
      ]
    }
  ]
}"#;
    let mut runner_bill = ScriptedCargo::new().plugins_json(billing_json);
    let mut out_bill = Vec::new();
    add::run(
        &root_bill,
        &AddOptions {
            package: Some("billing".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_bill,
        &mut out_bill,
    )
    .expect("plugin:add billing succeeds");
    let expected_bill = "\
  run      cargo run --quiet -- config:publish --plugin billing (builds and runs the app)
  feature  navree: billing (Cargo.toml)
  wired    .module(navree::billing::BillingModule::from_config()) (src/app.rs)
  wrote    config/billing.toml
  next     navree migrate

billing  navree-billing 1.4.0
  BillingModule  navree_billing::BillingModule
    adds      1 SectionRegistration
Installed billing.
";
    assert_eq!(String::from_utf8(out_bill).unwrap(), expected_bill);

    // 5. First-party `browser` (dev-dependency feature).
    let tmp_br = tempfile::tempdir().expect("tempdir");
    let root_br = support::app(tmp_br.path(), APP_CARGO_TOML);
    let mut runner_br = ScriptedCargo::new();
    let mut out_br = Vec::new();
    add::run(
        &root_br,
        &AddOptions {
            package: Some("browser".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_br,
        &mut out_br,
    )
    .expect("plugin:add browser succeeds");
    assert_eq!(
        String::from_utf8(out_br).unwrap(),
        "  feature  navree: browser ([dev-dependencies] in Cargo.toml)\nInstalled browser.\n"
    );
}

#[test]
fn a_refused_manifest_leaves_cargo_toml_and_the_lock_as_they_were() {
    let cases: &[(&str, Option<&str>, bool, &str)] = &[
        (
            "bad-module",
            Some(r#"module = "X::new(); std::process::exit(1)""#),
            false,
            "navree-plugin-khata 1.0.0: [package.metadata.navree.plugin] `module` must be a path to the module, with an optional `::<Type, ..>` and an optional `()` at the end, and nothing else (no spaces); nothing was changed",
        ),
        (
            "unknown-key",
            Some("module = \"KhataModule::from_config()\"\nscript = \"build.sh\""),
            false,
            "navree-plugin-khata 1.0.0: [package.metadata.navree.plugin] unknown key `script` (known: namespace, title, module, features, config, env, grants, policy; a newer plugin may need a newer navree); nothing was changed",
        ),
        (
            "missing-table",
            None,
            false,
            "navree-plugin-khata 1.0.0 declares no plugin ([package.metadata.navree.plugin] is missing); nothing was changed",
        ),
        (
            "cargo-add-fails",
            Some(r#"module = "KhataModule::from_config()""#),
            true,
            "cargo add failed: error: registry unreachable; nothing was changed",
        ),
    ];

    for &(label, table_toml, fail_add, expected_err) in cases {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = support::app(tmp.path(), APP_CARGO_TOML);
        let before = support::files(&root);
        assert!(!before.contains_key("Cargo.lock"));

        let mut runner = ScriptedCargo::new().package(
            "navree-plugin-khata",
            "1.0.0",
            table_toml,
            "navree_plugin_khata",
        );
        if fail_add {
            runner = runner.fail("cargo add", "error: registry unreachable\n");
        }

        let err = add::run(
            &root,
            &AddOptions {
                package: Some("navree-plugin-khata".to_owned()),
                ..AddOptions::default()
            },
            &mut runner,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), expected_err, "case {label}");
        assert_eq!(support::files(&root), before, "files changed in {label}");
    }
}

#[test]
fn a_refused_manifest_leaves_the_workspaces_lock_as_it_was() {
    let cases: &[(&str, Option<&str>, bool)] = &[
        (
            "bad-module",
            Some(r#"module = "X::new(); std::process::exit(1)""#),
            false,
        ),
        (
            "unknown-key",
            Some("module = \"KhataModule::from_config()\"\nscript = \"build.sh\""),
            false,
        ),
        ("missing-table", None, false),
        (
            "cargo-add-fails",
            Some(r#"module = "KhataModule::from_config()""#),
            true,
        ),
    ];

    for &(label, table_toml, fail_add) in cases {
        let tmp = tempfile::tempdir().expect("tempdir");
        let member = support::workspace_app(tmp.path(), MEMBER_CARGO_TOML);
        let before = support::files(tmp.path());

        let mut runner = ScriptedCargo::new().package(
            "navree-plugin-khata",
            "1.0.0",
            table_toml,
            "navree_plugin_khata",
        );
        if fail_add {
            runner = runner.fail("cargo add", "error: registry unreachable\n");
        }

        let result = add::run(
            &member,
            &AddOptions {
                package: Some("navree-plugin-khata".to_owned()),
                ..AddOptions::default()
            },
            &mut runner,
            &mut Vec::new(),
        );
        assert!(result.is_err(), "expected error in {label}");
        assert_eq!(
            support::files(tmp.path()),
            before,
            "workspace files changed in {label}"
        );
        assert!(
            !member.join("Cargo.lock").exists(),
            "member lock created in {label}"
        );
    }
}

#[test]
fn plugin_add_in_a_workspace_app_writes_where_the_app_reads() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let member = support::workspace_app(tmp.path(), MEMBER_CARGO_TOML);
    let mut runner = ScriptedCargo::new()
        .package(
            "navree-plugin-announcements",
            "1.0.0",
            ANNOUNCEMENTS_MANIFEST,
            "navree_plugin_announcements",
        )
        .plugins_json(ANNOUNCEMENTS_PLUGINS_JSON);

    add::run(
        &member,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            publish_config: true,
            ..AddOptions::default()
        },
        &mut runner,
        &mut Vec::new(),
    )
    .expect("workspace install succeeds");

    assert!(tmp.path().join("config/workspace.toml").is_file());
    assert!(tmp.path().join("config/announcements.toml").is_file());
    let root_env = fs::read_to_string(tmp.path().join(".env.example")).unwrap();
    assert!(root_env.contains("APP_ANNOUNCEMENTS__SIGNING_KEY="));
    assert!(!member.join("config").exists());

    let shown_ws_root = std::path::absolute(tmp.path()).unwrap();
    let publish_idx = runner
        .ran
        .iter()
        .position(|cmd| cmd.starts_with("cargo run --quiet -- config:publish"))
        .expect("config:publish recorded");
    let plugins_idx = runner
        .ran
        .iter()
        .position(|cmd| cmd.starts_with("cargo run --quiet -- plugins --json"))
        .expect("plugins --json recorded");
    assert_eq!(runner.ran_in[publish_idx], shown_ws_root);
    assert_eq!(runner.ran_in[plugins_idx], shown_ws_root);

    // An unmarked grant under `announcements.` in the workspace root's `config/workspace.toml` refuses install.
    let tmp_conflict = tempfile::tempdir().expect("tempdir");
    let member_conflict = support::workspace_app(tmp_conflict.path(), MEMBER_CARGO_TOML);
    fs::write(
        tmp_conflict.path().join("config/workspace.toml"),
        "[roles.member]\npermissions = [\"announcements.existing\"]\n",
    )
    .unwrap();
    let mut runner_conflict = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    let err = add::run(
        &member_conflict,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_conflict,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "navree-plugin-announcements 1.0.0: [package.metadata.navree.plugin] `grants`: the app already grants permissions under `announcements.` (in config/workspace.toml); a plugin suggests grants only in a namespace of its own; nothing was changed"
    );
}

#[test]
fn plugin_add_refuses_a_path_outside_the_app() {
    let outer = tempfile::tempdir().expect("outer");
    let app_dir = outer.path().join("app");
    support::app(&app_dir, APP_CARGO_TOML);
    let other = tempfile::tempdir().expect("other");
    fs::write(
        other.path().join("Cargo.toml"),
        "[package]\nname = \"navree-plugin-khata\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let shown_root = std::path::absolute(&app_dir).unwrap();
    for bad in [PathBuf::from("../khata"), other.path().to_path_buf()] {
        let mut runner = ScriptedCargo::new();
        let joined = if bad.is_absolute() {
            bad.clone()
        } else {
            app_dir.join(&bad)
        };
        let shown_dir = std::path::absolute(&joined).unwrap();
        let err = add::run(
            &app_dir,
            &AddOptions {
                path: Some(bad),
                ..AddOptions::default()
            },
            &mut runner,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                shown_dir.display(),
                shown_root.display()
            )
        );
        assert!(runner.ran.is_empty());
    }
}

#[test]
fn plugin_add_runs_no_code_of_a_community_crate_unless_asked() {
    // 1. Default: no `cargo run`, `cargo build`, `cargo check` or `cargo test`.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = support::app(tmp.path(), APP_CARGO_TOML);
    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    add::run(
        &root,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            ..AddOptions::default()
        },
        &mut runner,
        &mut Vec::new(),
    )
    .unwrap();
    for cmd in &runner.ran {
        for forbidden in ["cargo run", "cargo build", "cargo check", "cargo test"] {
            assert!(!cmd.starts_with(forbidden), "unexpected invocation: {cmd}");
        }
    }

    // 2. With `--publish-config`: runs `config:publish --plugin` then `plugins --json`, `run` line first.
    let tmp_pub = tempfile::tempdir().expect("tempdir");
    let root_pub = support::app(tmp_pub.path(), APP_CARGO_TOML);
    let mut runner_pub = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    let mut out_pub = Vec::new();
    add::run(
        &root_pub,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            publish_config: true,
            ..AddOptions::default()
        },
        &mut runner_pub,
        &mut out_pub,
    )
    .unwrap();
    let run_cmds: Vec<&str> = runner_pub
        .ran
        .iter()
        .map(String::as_str)
        .filter(|c| c.starts_with("cargo run"))
        .collect();
    assert_eq!(
        run_cmds,
        [
            "cargo run --quiet -- config:publish --plugin announcements",
            "cargo run --quiet -- plugins --json",
        ]
    );
    let text_pub = String::from_utf8(out_pub).unwrap();
    assert!(text_pub.starts_with(
        "  run      cargo run --quiet -- config:publish --plugin announcements (builds and runs the app)\n"
    ));

    // 3. With `--publish-config` AND `--no-wire`: neither runs.
    let tmp_nw = tempfile::tempdir().expect("tempdir");
    let root_nw = support::app(tmp_nw.path(), APP_CARGO_TOML);
    let mut runner_nw = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    add::run(
        &root_nw,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            publish_config: true,
            no_wire: true,
            ..AddOptions::default()
        },
        &mut runner_nw,
        &mut Vec::new(),
    )
    .unwrap();
    assert!(
        !runner_nw.ran.iter().any(|c| c.starts_with("cargo run")),
        "unexpected cargo run with --no-wire: {:?}",
        runner_nw.ran
    );
}

#[test]
fn plugin_add_reads_the_manifest_of_the_package_it_added_and_no_other() {
    // 1. Registry install ignores a path package of the same name in metadata.
    let tmp_reg = tempfile::tempdir().expect("tempdir");
    let root_reg = support::app(tmp_reg.path(), APP_CARGO_TOML);
    let mut runner_reg = ScriptedCargo::new()
        .package(
            "navree-plugin-khata",
            "1.2.0",
            r#"module = "RegistryKhataModule::from_config()""#,
            "navree_plugin_khata",
        )
        .path_package(
            "plugins/khata",
            "navree-plugin-khata",
            "0.1.0",
            r#"module = "LocalKhataModule::from_config()""#,
        );
    add::run(
        &root_reg,
        &AddOptions {
            package: Some("navree-plugin-khata".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_reg,
        &mut Vec::new(),
    )
    .unwrap();
    let app_rs_reg = fs::read_to_string(root_reg.join("src/app.rs")).unwrap();
    assert!(app_rs_reg.contains("RegistryKhataModule::from_config()"));
    assert!(!app_rs_reg.contains("LocalKhataModule"));

    // 2. `--path` install ignores a registry package of the same name in metadata.
    let tmp_path = tempfile::tempdir().expect("tempdir");
    let root_path = support::app(tmp_path.path(), APP_CARGO_TOML);
    let local_dir = root_path.join("plugins/khata");
    fs::create_dir_all(&local_dir).unwrap();
    fs::write(
        local_dir.join("Cargo.toml"),
        "[package]\nname = \"navree-plugin-khata\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let mut runner_path = ScriptedCargo::new()
        .package(
            "navree-plugin-khata",
            "1.2.0",
            r#"module = "RegistryKhataModule::from_config()""#,
            "navree_plugin_khata",
        )
        .path_package(
            "plugins/khata",
            "navree-plugin-khata",
            "0.1.0",
            r#"module = "LocalKhataModule::from_config()""#,
        );
    add::run(
        &root_path,
        &AddOptions {
            path: Some(PathBuf::from("plugins/khata")),
            ..AddOptions::default()
        },
        &mut runner_path,
        &mut Vec::new(),
    )
    .unwrap();
    let app_rs_path = fs::read_to_string(root_path.join("src/app.rs")).unwrap();
    assert!(app_rs_path.contains("LocalKhataModule::from_config()"));
    assert!(!app_rs_path.contains("RegistryKhataModule"));

    // 3. Underscore spelling `navree_plugin_khata` adds and prints `navree-plugin-khata`.
    let tmp_u = tempfile::tempdir().expect("tempdir");
    let root_u = support::app(tmp_u.path(), APP_CARGO_TOML);
    let mut runner_u = ScriptedCargo::new().package(
        "navree-plugin-khata",
        "1.0.0",
        r#"module = "KhataModule::from_config()""#,
        "navree_plugin_khata",
    );
    let mut out_u = Vec::new();
    add::run(
        &root_u,
        &AddOptions {
            package: Some("navree_plugin_khata".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_u,
        &mut out_u,
    )
    .unwrap();
    let text_u = String::from_utf8(out_u).unwrap();
    assert!(text_u.contains("  added    navree-plugin-khata 1.0.0 (Cargo.toml)\n"));

    // 4. `navree_billing` installs first-party `billing` even when another package exists in metadata.
    let tmp_b = tempfile::tempdir().expect("tempdir");
    let root_b = support::app(tmp_b.path(), APP_CARGO_TOML);
    let mut runner_b = ScriptedCargo::new().package(
        "navree-billing",
        "9.9.9",
        r#"module = "FakeModule""#,
        "navree_billing",
    );
    let mut out_b = Vec::new();
    add::run(
        &root_b,
        &AddOptions {
            package: Some("navree_billing".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_b,
        &mut out_b,
    )
    .unwrap();
    let text_b = String::from_utf8(out_b).unwrap();
    assert!(text_b.contains("Installed billing.\n"));
}

#[test]
fn a_failure_after_the_first_write_puts_every_file_back() {
    // 1. Third write (`config/workspace.toml`) fails because a folder sits in its place.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = support::app(tmp.path(), APP_CARGO_TOML);
    fs::create_dir_all(root.join("config/workspace.toml")).unwrap();
    let before = support::files(&root);

    let manifest = "\
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\"]
grants = { member = [\"announcements.read\"] }
";
    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        manifest,
        "navree_plugin_announcements",
    );
    let err = add::run(
        &root,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            ..AddOptions::default()
        },
        &mut runner,
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.starts_with("cannot write config/workspace.toml: ")
            && err.ends_with("; nothing was changed"),
        "unexpected error: {err}"
    );
    assert_eq!(support::files(&root), before);
    assert!(root.join("config/workspace.toml").is_dir());

    // 2. Step 10 (`config:publish --plugin`) fails: every planned file, `Cargo.toml` and `Cargo.lock` put back.
    let tmp_run = tempfile::tempdir().expect("tempdir");
    let root_run = support::app(tmp_run.path(), APP_CARGO_TOML);
    let before_run = support::files(&root_run);

    let mut runner_run = ScriptedCargo::new()
        .package(
            "navree-plugin-announcements",
            "1.0.0",
            ANNOUNCEMENTS_MANIFEST,
            "navree_plugin_announcements",
        )
        .fail(
            "cargo run --quiet -- config:publish",
            "error: app failed to compile\n",
        );
    let err_run = add::run(
        &root_run,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            publish_config: true,
            ..AddOptions::default()
        },
        &mut runner_run,
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        err_run,
        "navree-plugin-announcements: config:publish --plugin announcements failed (its output is above); nothing was changed"
    );
    assert_eq!(support::files(&root_run), before_run);
}

#[test]
fn plugin_add_stops_when_a_file_changed_while_cargo_ran() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = support::app(tmp.path(), APP_CARGO_TOML);
    let original_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let app_rs_path = root.join("src/app.rs");
    let edited_app_rs = "// owner edited while cargo ran\npub fn app() {}\n";
    let app_rs_for_hook = app_rs_path.clone();

    let mut runner = ScriptedCargo::new()
        .package(
            "navree-plugin-announcements",
            "1.0.0",
            ANNOUNCEMENTS_MANIFEST,
            "navree_plugin_announcements",
        )
        .during("cargo add", move || {
            fs::write(&app_rs_for_hook, edited_app_rs).unwrap();
        });

    let err = add::run(
        &root,
        &AddOptions {
            package: Some("navree-plugin-announcements".to_owned()),
            ..AddOptions::default()
        },
        &mut runner,
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();

    assert_eq!(
        err,
        "src/app.rs changed while plugin:add ran; nothing was changed"
    );
    assert_eq!(fs::read_to_string(&app_rs_path).unwrap(), edited_app_rs);
    assert_eq!(
        fs::read_to_string(root.join("Cargo.toml")).unwrap(),
        original_toml
    );
    assert!(!root.join("Cargo.lock").exists());
}

#[test]
fn plugin_add_runs_cargo_without_colour_and_shows_its_last_error_line() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = support::app(tmp.path(), APP_CARGO_TOML);
    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-khata",
        "1.0.0",
        r#"module = "KhataModule::from_config()""#,
        "navree_plugin_khata",
    );
    add::run(
        &root,
        &AddOptions {
            package: Some("navree-plugin-khata".to_owned()),
            ..AddOptions::default()
        },
        &mut runner,
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(runner.ran.len(), 2);
    for env in &runner.envs {
        assert!(
            env.contains(&("CARGO_TERM_COLOR".to_owned(), "never".to_owned())),
            "missing CARGO_TERM_COLOR=never in {env:?}"
        );
    }

    let tmp_fail = tempfile::tempdir().expect("tempdir");
    let root_fail = support::app(tmp_fail.path(), APP_CARGO_TOML);
    let mut runner_fail = ScriptedCargo::new().fail(
        "cargo add",
        "warning: x\nerror: could not find `navree-plugin-khata` in registry\n",
    );
    let err = add::run(
        &root_fail,
        &AddOptions {
            package: Some("navree-plugin-khata".to_owned()),
            ..AddOptions::default()
        },
        &mut runner_fail,
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        err,
        "cargo add failed: error: could not find `navree-plugin-khata` in registry; nothing was changed"
    );
}

#[test]
fn plugin_add_takes_a_windows_path_inside_the_app_and_writes_it_with_slashes() {
    let mut raw_paths = vec!["plugins/khata"];
    #[cfg(windows)]
    raw_paths.push(r"plugins\khata");

    for raw in raw_paths {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = support::app(tmp.path(), APP_CARGO_TOML);
        let plugin_dir = root.join("plugins/khata");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("Cargo.toml"),
            "\
[package]
name = \"navree-plugin-khata\"
version = \"0.1.0\"
edition = \"2024\"

[package.metadata.navree.plugin]
module = \"KhataModule::from_config()\"
",
        )
        .unwrap();

        let mut runner = ScriptedCargo::new();
        add::run(
            &root,
            &AddOptions {
                path: Some(PathBuf::from(raw)),
                ..AddOptions::default()
            },
            &mut runner,
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(runner.ran[0], "cargo add --path plugins/khata");
    }
}

#[test]
fn the_geo_example_plugins_manifest_passes_the_grammar() {
    let geo_toml_raw = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/geo/Cargo.toml"),
    )
    .expect("read plugins/geo/Cargo.toml");
    let geo_toml: toml::Table = geo_toml_raw.parse().expect("parse plugins/geo/Cargo.toml");
    let pkg_name = geo_toml["package"]["name"].as_str().expect("package.name");
    let table_toml = geo_toml
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("navree"))
        .and_then(|n| n.get("plugin"))
        .expect("[package.metadata.navree.plugin] in plugins/geo/Cargo.toml");
    let table_json = serde_json::to_value(table_toml).expect("plugin table as json");

    let index = Index::embedded();
    let facts = AppFacts {
        first_party: false,
        navree_version: "1.4.0".to_owned(),
        navree_features: index.manifest_features.iter().cloned().collect(),
        allowed_features: index.manifest_features.iter().cloned().collect(),
        reserved: index.reserved.iter().cloned().collect(),
        ..AppFacts::default()
    };

    let parsed = manifest::read(pkg_name, &table_json, &facts)
        .expect("plugins/geo/Cargo.toml manifest passes grammar");
    assert_eq!(parsed.namespace, "geo");
    assert_eq!(parsed.title.as_deref(), Some("Maps and places"));
    assert_eq!(parsed.module.as_str(), "GeoModule");
    assert_eq!(parsed.features, ["ui"]);
    assert_eq!(parsed.config, ["geo"]);
    assert_eq!(
        parsed.policy.sources,
        [(
            "img_src".to_owned(),
            "https://tile.openstreetmap.org".to_owned()
        )]
    );
    assert!(!parsed.policy.wasm);
}
