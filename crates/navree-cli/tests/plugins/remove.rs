//! Integration tests for `navree plugin:remove`.

use std::path::Path;

use navree_cli::plugin::remove::{self, RemoveOptions};

use crate::support::{self, ScriptedCargo};

const ANNOUNCEMENTS_MANIFEST: &str = "\
title = \"Announcements\"
module = \"AnnouncementsModule::from_config()\"
features = [\"workspace\", \"ui\"]
config = [\"announcements\"]
env = [\"APP_ANNOUNCEMENTS__SIGNING_KEY\"]
grants = { member = [\"announcements.read\"] }
";

fn install_announcements_fixture(root: &Path) -> String {
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"workspace\", \"ui\"] }
navree-plugin-announcements = \"1.0.0\"

[workspace]
";
    support::app(root, cargo_toml);
    let original_app_rs = std::fs::read_to_string(root.join("src/app.rs")).expect("read app.rs");

    let wired_app_rs = original_app_rs.replace(
        "    // navree:modules",
        "        .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n    // navree:modules",
    );
    std::fs::write(root.join("src/app.rs"), wired_app_rs).expect("write wired app.rs");

    std::fs::write(
        root.join("config/announcements.toml"),
        "# [announcements]\n",
    )
    .expect("write config/announcements.toml");

    let workspace_toml = "\
[roles.member]
permissions = [
    \"announcements.read\", # plugin:add navree-plugin-announcements
]
";
    std::fs::write(root.join("config/workspace.toml"), workspace_toml)
        .expect("write config/workspace.toml");

    let mut env_example =
        std::fs::read_to_string(root.join(".env.example")).expect("read .env.example");
    env_example
        .push_str("\n# plugin:add navree-plugin-announcements\nAPP_ANNOUNCEMENTS__SIGNING_KEY=\n");
    std::fs::write(root.join(".env.example"), env_example).expect("write .env.example");

    original_app_rs
}

#[test]
fn plugin_remove_unwires_and_keeps_config_and_data() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let original_app_rs = install_announcements_fixture(root);

    let config_before =
        std::fs::read_to_string(root.join("config/announcements.toml")).expect("read config");
    let workspace_before =
        std::fs::read_to_string(root.join("config/workspace.toml")).expect("read workspace.toml");
    let env_before = std::fs::read_to_string(root.join(".env.example")).expect("read .env.example");

    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    let mut out = Vec::new();
    remove::run(
        root,
        &RemoveOptions {
            package: "navree-plugin-announcements".to_owned(),
        },
        &mut runner,
        &mut out,
    )
    .expect("remove announcements succeeds");

    let printed = String::from_utf8(out).expect("utf8 output");
    let expected = "\
  unwired  .module(navree_plugin_announcements::AnnouncementsModule::from_config()) (src/app.rs)
  removed  navree-plugin-announcements (Cargo.toml)
  kept     config/announcements.toml, the grants in config/workspace.toml, APP_ANNOUNCEMENTS__SIGNING_KEY in .env.example, the features navree: workspace, ui, and the plugin's tables and rows
";
    assert_eq!(printed, expected);
    assert_eq!(
        std::fs::read_to_string(root.join("src/app.rs")).expect("read app.rs after"),
        original_app_rs
    );
    assert_eq!(
        std::fs::read_to_string(root.join("config/announcements.toml")).unwrap(),
        config_before
    );
    assert_eq!(
        std::fs::read_to_string(root.join("config/workspace.toml")).unwrap(),
        workspace_before
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".env.example")).unwrap(),
        env_before
    );

    // First-party `billing`: unwires `src/app.rs`, prints kept feature, records no `cargo remove`.
    let temp_fp = tempfile::tempdir().expect("tempdir");
    let fp_root = temp_fp.path();
    let fp_cargo = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"billing\"] }

[workspace]
";
    support::app(fp_root, fp_cargo);
    let fp_before = std::fs::read_to_string(fp_root.join("src/app.rs")).unwrap();
    let fp_wired = fp_before.replace(
        "    // navree:modules",
        "        .module(navree::billing::BillingModule::from_config())\n    // navree:modules",
    );
    std::fs::write(fp_root.join("src/app.rs"), fp_wired).unwrap();

    let mut fp_runner = ScriptedCargo::new();
    let mut fp_out = Vec::new();
    remove::run(
        fp_root,
        &RemoveOptions {
            package: "billing".to_owned(),
        },
        &mut fp_runner,
        &mut fp_out,
    )
    .expect("remove billing succeeds");

    assert_eq!(
        String::from_utf8(fp_out).unwrap(),
        "\
  unwired  .module(navree::billing::BillingModule::from_config()) (src/app.rs)
  kept     the feature navree: billing (other code may use it)
"
    );
    assert_eq!(
        std::fs::read_to_string(fp_root.join("src/app.rs")).unwrap(),
        fp_before
    );
    assert!(
        !fp_runner
            .ran
            .iter()
            .any(|cmd| cmd.starts_with("cargo remove")),
        "unexpected invocations: {:?}",
        fp_runner.ran
    );
}

#[test]
fn plugin_remove_leaves_a_hand_edited_line_and_says_so() {
    let cases: &[(&str, Option<(&str, &str)>)] = &[
        // Rewrapped across multiple lines by hand.
        (
            "        .module(\n            navree_plugin_announcements::AnnouncementsModule::from_config(),\n        )\n    // navree:modules",
            None,
        ),
        // Given custom options via `for_root(..)`.
        (
            "        .module(navree_plugin_announcements::AnnouncementsModule::for_root(opts))\n    // navree:modules",
            None,
        ),
        // Moved into `src/modules/mod.rs`.
        (
            "    // navree:modules",
            Some((
                "src/modules/mod.rs",
                "pub fn extra(b: AppBuilder) -> AppBuilder { b.module(navree_plugin_announcements::AnnouncementsModule::from_config()) }\n",
            )),
        ),
        // Commented out.
        (
            "        // .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n    // navree:modules",
            None,
        ),
    ];

    for (idx, (replacement, extra_file)) in cases.iter().enumerate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
navree-plugin-announcements = \"1.0.0\"

[workspace]
";
        support::app(root, cargo_toml);
        let base_app = std::fs::read_to_string(root.join("src/app.rs")).unwrap();
        let custom_app = base_app.replace("    // navree:modules", replacement);
        std::fs::write(root.join("src/app.rs"), &custom_app).unwrap();

        if let Some((rel, contents)) = extra_file {
            let full = root.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, contents).unwrap();
        }

        let mut runner = ScriptedCargo::new().package(
            "navree-plugin-announcements",
            "1.0.0",
            "module = \"AnnouncementsModule::from_config()\"",
            "navree_plugin_announcements",
        );
        let mut out = Vec::new();
        remove::run(
            root,
            &RemoveOptions {
                package: "navree-plugin-announcements".to_owned(),
            },
            &mut runner,
            &mut out,
        )
        .unwrap_or_else(|err| panic!("case {idx} failed: {err}"));

        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.starts_with(
                "  next     remove .module(navree_plugin_announcements::AnnouncementsModule::from_config()) from the file that wires it\n"
            ),
            "case {idx} output: {printed:?}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/app.rs")).unwrap(),
            custom_app,
            "case {idx} modified src/app.rs"
        );
        if let Some((rel, contents)) = extra_file {
            assert_eq!(
                std::fs::read_to_string(root.join(rel)).unwrap(),
                *contents,
                "case {idx} modified {rel}"
            );
        }
        assert!(
            runner
                .ran
                .contains(&"cargo remove navree-plugin-announcements".to_owned()),
            "case {idx} invocations: {:?}",
            runner.ran
        );
    }
}

#[test]
fn plugin_remove_runs_no_code_of_the_plugin() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    install_announcements_fixture(root);

    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        ANNOUNCEMENTS_MANIFEST,
        "navree_plugin_announcements",
    );
    let mut out = Vec::new();
    remove::run(
        root,
        &RemoveOptions {
            package: "navree-plugin-announcements".to_owned(),
        },
        &mut runner,
        &mut out,
    )
    .expect("remove succeeds");

    for cmd in &runner.ran {
        for forbidden in ["cargo run", "cargo build", "cargo check", "cargo test"] {
            assert!(
                !cmd.starts_with(forbidden),
                "unexpected code-running command: {cmd}"
            );
        }
    }
    assert_eq!(
        runner.ran,
        vec![
            "cargo metadata --format-version 1".to_owned(),
            "cargo remove navree-plugin-announcements".to_owned(),
        ]
    );
}

#[test]
fn plugin_remove_refuses_a_package_the_app_does_not_depend_on() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    install_announcements_fixture(root);
    let before = support::files(root);

    let mut runner = ScriptedCargo::new();
    let mut out = Vec::new();
    let err = remove::run(
        root,
        &RemoveOptions {
            package: "navree-plugin-khata".to_owned(),
        },
        &mut runner,
        &mut out,
    )
    .expect_err("missing dependency must fail");
    assert_eq!(
        err.to_string(),
        "navree-plugin-khata is not a dependency of this app"
    );
    assert!(runner.ran.is_empty());
    assert_eq!(support::files(root), before);

    // A failing `cargo remove` restores `src/app.rs`, `Cargo.toml`, and `Cargo.lock`.
    let mut fail_runner = ScriptedCargo::new()
        .package(
            "navree-plugin-announcements",
            "1.0.0",
            ANNOUNCEMENTS_MANIFEST,
            "navree_plugin_announcements",
        )
        .fail("cargo remove", "error: could not edit Cargo.toml\n");
    let mut fail_out = Vec::new();
    let fail_err = remove::run(
        root,
        &RemoveOptions {
            package: "navree-plugin-announcements".to_owned(),
        },
        &mut fail_runner,
        &mut fail_out,
    )
    .expect_err("failing cargo remove must fail");
    assert_eq!(
        fail_err.to_string(),
        "cargo remove failed: error: could not edit Cargo.toml; nothing was changed"
    );
    assert!(fail_out.is_empty());
    assert_eq!(support::files(root), before);
}

#[test]
fn plugin_remove_refuses_the_framework_itself() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }

[workspace]
";
    support::app(root, cargo_toml);
    let before = support::files(root);

    let mut runner = ScriptedCargo::new();
    let mut out = Vec::new();
    let err = remove::run(
        root,
        &RemoveOptions {
            package: "navree".to_owned(),
        },
        &mut runner,
        &mut out,
    )
    .expect_err("navree itself must be refused");
    assert_eq!(err.to_string(), "`navree` is the framework, not a plugin");
    assert!(runner.ran.is_empty());
    assert_eq!(support::files(root), before);
}

#[test]
fn plugin_remove_refuses_a_dependency_that_declares_no_plugin() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
reqwest = \"0.12.0\"

[workspace]
";
    support::app(root, cargo_toml);
    let before = support::files(root);

    let mut runner = ScriptedCargo::new().package("reqwest", "0.12.0", None, "reqwest");
    let mut out = Vec::new();
    let err = remove::run(
        root,
        &RemoveOptions {
            package: "reqwest".to_owned(),
        },
        &mut runner,
        &mut out,
    )
    .expect_err("dependency without plugin table must be refused");
    assert_eq!(
        err.to_string(),
        "reqwest 0.12.0 declares no plugin ([package.metadata.navree.plugin] is missing); nothing was changed"
    );
    assert_eq!(
        runner.ran,
        vec!["cargo metadata --format-version 1".to_owned()]
    );
    assert_eq!(support::files(root), before);
}

#[test]
fn plugin_remove_names_the_page_sources_and_the_folder_it_leaves() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"ui\"] }
navree-plugin-geo = { path = \"plugins/geo\" }

[workspace]
";
    support::app(root, cargo_toml);

    let geo_dir = root.join("plugins/geo");
    std::fs::create_dir_all(geo_dir.join("src")).unwrap();
    let geo_manifest = "\
module = \"GeoModule::from_config()\"
features = [\"ui\"]
config = [\"geo\"]
policy = { img_src = [\"https://tile.openstreetmap.org\"] }
";
    std::fs::write(
        geo_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"navree-plugin-geo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.navree.plugin]\n{geo_manifest}"
        ),
    )
    .unwrap();
    std::fs::write(geo_dir.join("src/lib.rs"), "// geo plugin\n").unwrap();

    let app_rs = std::fs::read_to_string(root.join("src/app.rs")).unwrap();
    std::fs::write(
        root.join("src/app.rs"),
        app_rs.replace(
            "    // navree:modules",
            "        .module(navree_plugin_geo::GeoModule::from_config())\n    // navree:modules",
        ),
    )
    .unwrap();

    std::fs::write(root.join("config/geo.toml"), "# [geo]\n").unwrap();

    let ui_policy_toml = "\
# Sources every page may load beyond Navree UI's own policy: /docs/ui/page-policy/
[allow.geo]
# plugin:add navree-plugin-geo asks for these. Uncomment a line to allow it on every page.
# img_src = [\"https://tile.openstreetmap.org\"]
";
    std::fs::write(root.join("config/ui.policy.toml"), ui_policy_toml).unwrap();

    let mut runner = ScriptedCargo::new().path_package(
        "plugins/geo",
        "navree-plugin-geo",
        "0.1.0",
        geo_manifest,
    );
    let mut out = Vec::new();
    remove::run(
        root,
        &RemoveOptions {
            package: "navree-plugin-geo".to_owned(),
        },
        &mut runner,
        &mut out,
    )
    .expect("remove geo succeeds");

    let printed = String::from_utf8(out).unwrap();
    assert_eq!(
        printed,
        "\
  unwired  .module(navree_plugin_geo::GeoModule::from_config()) (src/app.rs)
  removed  navree-plugin-geo (Cargo.toml)
  kept     config/geo.toml, the page sources in config/ui.policy.toml, the features navree: ui, plugins/geo/, and the plugin's tables and rows
"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("config/ui.policy.toml")).unwrap(),
        ui_policy_toml
    );
    assert!(geo_dir.join("Cargo.toml").is_file());
}

#[test]
fn plugin_remove_succeeds_with_unmarked_owner_grants_and_process_env_roles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[workspace]

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"workspace\"] }
navree-plugin-khata = \"1.0.0\"
";
    let root = support::app(tmp.path(), cargo_toml);
    let app_rs = std::fs::read_to_string(root.join("src/app.rs")).unwrap();
    std::fs::write(
        root.join("src/app.rs"),
        app_rs.replace(
            "    // navree:modules",
            "        .module(navree_plugin_khata::KhataModule::from_config())\n    // navree:modules",
        ),
    )
    .unwrap();
    // Owner added an unmarked `khata.escalate` permission in `config/workspace.toml`, and `support`
    // is defined only in `process_env` (or even deleted).
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::write(
        root.join("config/workspace.toml"),
        "[roles.admin]\npermissions = [\"khata.escalate\"]\n",
    )
    .unwrap();

    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-khata",
        "1.0.0",
        "module = \"KhataModule::from_config()\"\ngrants = { support = [\"khata.reply\"] }",
        "navree_plugin_khata",
    );
    let mut out = Vec::new();
    remove::run_with_env(
        &root,
        &RemoveOptions {
            package: "navree-plugin-khata".to_owned(),
        },
        &[(
            "APP_WORKSPACE__ROLES__SUPPORT__PERMISSIONS".to_owned(),
            "tickets.reply".to_owned(),
        )],
        &mut runner,
        &mut out,
    )
    .expect("remove succeeds despite unmarked grant and custom role");

    let printed = String::from_utf8(out).unwrap();
    assert!(
        printed.starts_with(
            "  unwired  .module(navree_plugin_khata::KhataModule::from_config()) (src/app.rs)\n"
        ),
        "{printed}"
    );
}
