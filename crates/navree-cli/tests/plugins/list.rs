//! Integration tests for `navree plugin:list` and `navree check`'s plugin and
//! library-name warnings.

use navree_cli::check::{CheckOptions, run_check};
use navree_cli::plugin::list::{self, ListOptions};
use serde_json::{Value, json};

use crate::support::{self, ScriptedCargo};

#[test]
fn plugin_list_says_which_plugins_are_wired() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"billing\", \"helm\"] }
navree-plugin-announcements = \"1.0.0\"

[workspace]
";
    support::app(root, cargo_toml);

    let app_rs = std::fs::read_to_string(root.join("src/app.rs")).unwrap();
    std::fs::write(
        root.join("src/app.rs"),
        app_rs.replace(
            "    // navree:modules",
            "        .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n    // navree:modules",
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("src/platform.rs"),
        "use navree::billing::BillingModule;\n\npub fn add(b: navree::kernel::AppBuilder) -> navree::kernel::AppBuilder {\n    b.module(BillingModule::from_config())\n}\n",
    )
    .unwrap();

    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        "module = \"AnnouncementsModule::from_config()\"",
        "navree_plugin_announcements",
    );
    let mut out = Vec::new();
    list::run(root, &ListOptions { json: false }, &mut runner, &mut out)
        .expect("plugin:list succeeds");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\
navree-plugin-announcements  1.0.0  wired      src/app.rs
billing                      1.4.0  wired      src/platform.rs
helm                         1.4.0  not wired  run: navree plugin:add helm
"
    );

    let mut json_out = Vec::new();
    list::run(
        root,
        &ListOptions { json: true },
        &mut runner,
        &mut json_out,
    )
    .expect("plugin:list --json succeeds");
    let parsed: Value =
        serde_json::from_slice(&json_out).expect("plugin:list --json outputs valid JSON");
    assert_eq!(
        parsed,
        json!([
            {
                "package": "navree-plugin-announcements",
                "version": "1.0.0",
                "first_party": false,
                "module": "navree_plugin_announcements::AnnouncementsModule::from_config()",
                "wired": true,
                "file": "src/app.rs"
            },
            {
                "package": "billing",
                "version": "1.4.0",
                "first_party": true,
                "module": "navree::billing::BillingModule::from_config()",
                "wired": true,
                "file": "src/platform.rs"
            },
            {
                "package": "helm",
                "version": "1.4.0",
                "first_party": true,
                "module": "navree::helm::HelmModule::from_config()",
                "wired": false,
                "file": null
            }
        ])
    );

    // An app with no plugins prints the empty-state sentence.
    let empty_temp = tempfile::tempdir().expect("tempdir");
    let empty_root = empty_temp.path();
    support::app(
        empty_root,
        "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }\n\n[workspace]\n",
    );
    let mut empty_runner = ScriptedCargo::new();
    let mut empty_out = Vec::new();
    list::run(
        empty_root,
        &ListOptions { json: false },
        &mut empty_runner,
        &mut empty_out,
    )
    .expect("empty plugin:list succeeds");
    assert_eq!(
        String::from_utf8(empty_out).unwrap(),
        "No plugins: no dependency of this app declares one.\n"
    );
}

#[test]
fn plugin_list_does_not_count_another_crates_module_of_the_same_name_as_wired() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"billing\"] }
navree-plugin-khata = \"1.0.0\"
navree-plugin-notices = \"1.0.0\"

[workspace]
";
    support::app(root, cargo_toml);

    let app_rs = std::fs::read_to_string(root.join("src/app.rs")).unwrap();
    std::fs::write(
        root.join("src/app.rs"),
        app_rs.replace(
            "    // navree:modules",
            "        .module(navree_plugin_khata::ClientModule::<reqwest::Client,KhataOptions>::new())\n        .module(other_billing::BillingModule::from_config())\n    // navree:modules",
        ),
    )
    .unwrap();

    let mut runner = ScriptedCargo::new()
        .package(
            "navree-plugin-khata",
            "1.0.0",
            "module = \"ClientModule::<reqwest::Client,KhataOptions>::new()\"",
            "navree_plugin_khata",
        )
        .package(
            "navree-plugin-notices",
            "1.0.0",
            "module = \"ClientModule::<reqwest::Client,NoticesOptions>::new()\"",
            "navree_plugin_notices",
        );

    let mut out = Vec::new();
    list::run(root, &ListOptions { json: false }, &mut runner, &mut out)
        .expect("plugin:list succeeds");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\
navree-plugin-khata    1.0.0  wired      src/app.rs
navree-plugin-notices  1.0.0  not wired  run: navree plugin:add navree-plugin-notices
billing                1.4.0  not wired  run: navree plugin:add billing
"
    );

    // `navree check` warns about `navree-plugin-notices` only (neither `khata` nor first-party `billing`).
    let mut check_out = Vec::new();
    let ok = run_check(root, &CheckOptions::default(), &mut runner, &mut check_out)
        .expect("run_check succeeds");
    assert!(ok);
    let check_printed = String::from_utf8(check_out).unwrap();
    assert!(
        check_printed.starts_with(
            "warning  navree-plugin-notices declares a plugin, but nothing under src/ wires it: run navree plugin:add navree-plugin-notices, or remove the dependency\n"
        ),
        "unexpected check output: {check_printed:?}"
    );
    assert!(!check_printed.contains("navree-plugin-khata"));
    assert!(!check_printed.contains("billing"));

    // Once a file names `navree::billing` alongside `BillingModule`, `billing` counts as wired.
    std::fs::write(
        root.join("src/platform.rs"),
        "pub fn billing(b: navree::kernel::AppBuilder) -> navree::kernel::AppBuilder {\n    b.module(navree::billing::BillingModule::from_config())\n}\n",
    )
    .unwrap();
    let mut out_after = Vec::new();
    list::run(
        root,
        &ListOptions { json: false },
        &mut runner,
        &mut out_after,
    )
    .expect("plugin:list after wiring billing");
    assert!(
        String::from_utf8(out_after)
            .unwrap()
            .contains("billing                1.4.0  wired      src/platform.rs\n")
    );
}

#[test]
fn check_warns_about_an_installed_plugin_that_is_not_wired() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\", \"helm\"] }
navree-plugin-announcements = \"1.0.0\"

[workspace]
";
    support::app(root, cargo_toml);

    let mut runner = ScriptedCargo::new().package(
        "navree-plugin-announcements",
        "1.0.0",
        "module = \"AnnouncementsModule::from_config()\"",
        "navree_plugin_announcements",
    );
    let mut out = Vec::new();
    let ok = run_check(root, &CheckOptions::default(), &mut runner, &mut out)
        .expect("run_check succeeds");
    assert!(ok);
    let printed = String::from_utf8(out).unwrap();
    let warning_line = "warning  navree-plugin-announcements declares a plugin, but nothing under src/ wires it: run navree plugin:add navree-plugin-announcements, or remove the dependency\n";
    assert!(
        printed.starts_with(warning_line),
        "unexpected output: {printed:?}"
    );
    assert!(!printed.contains("helm"));
    assert_eq!(runner.ran[0], "cargo metadata --format-version 1");
    assert_eq!(runner.ran[1], "cargo run --quiet -- check");

    // Neither `--graph` nor `--json` invokes `cargo metadata`.
    let mut graph_runner = ScriptedCargo::new();
    let mut graph_out = Vec::new();
    assert!(
        run_check(
            root,
            &CheckOptions {
                graph: true,
                ..CheckOptions::default()
            },
            &mut graph_runner,
            &mut graph_out,
        )
        .unwrap()
    );
    assert!(
        !graph_runner
            .ran
            .iter()
            .any(|cmd| cmd.starts_with("cargo metadata"))
    );

    let mut json_runner = ScriptedCargo::new();
    let mut json_out = Vec::new();
    assert!(
        run_check(
            root,
            &CheckOptions {
                json: true,
                ..CheckOptions::default()
            },
            &mut json_runner,
            &mut json_out,
        )
        .unwrap()
    );
    assert!(
        !json_runner
            .ran
            .iter()
            .any(|cmd| cmd.starts_with("cargo metadata"))
    );
}

#[test]
fn plugin_list_runs_no_code_of_the_plugin() {
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

    for json_flag in [false, true] {
        let mut runner = ScriptedCargo::new().package(
            "navree-plugin-announcements",
            "1.0.0",
            "module = \"AnnouncementsModule::from_config()\"",
            "navree_plugin_announcements",
        );
        let mut out = Vec::new();
        list::run(
            root,
            &ListOptions { json: json_flag },
            &mut runner,
            &mut out,
        )
        .expect("plugin:list succeeds");
        assert_eq!(
            runner.ran,
            vec!["cargo metadata --format-version 1".to_owned()]
        );
    }
}

#[test]
fn check_reports_a_package_whose_library_is_named_like_one_of_navrees_crates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
freetype-sys = \"0.20.0\"

[workspace]
";
    support::app(root, cargo_toml);

    let mut runner = ScriptedCargo::new()
        .package("freetype-sys", "0.20.0", None, "freetype")
        .transitive_package("ludhiana-ledger", "0.1.0", "navree_billing")
        .transitive_package("amritsar-core", "0.2.0", "navree");

    let mut out = Vec::new();
    let ok = run_check(root, &CheckOptions::default(), &mut runner, &mut out)
        .expect("run_check succeeds");
    assert!(ok);
    let printed = String::from_utf8(out).unwrap();
    assert!(
        printed.starts_with(
            "warning  amritsar-core 0.2.0 names its library `navree` (not `amritsar_core`), like a `navree` crate: remove or replace the dependency\nwarning  ludhiana-ledger 0.1.0 names its library `navree_billing` (not `ludhiana_ledger`), like a `navree` crate: remove or replace the dependency\n"
        ),
        "unexpected check output: {printed:?}"
    );
    assert!(!printed.contains("freetype"));
    assert_eq!(runner.ran[0], "cargo metadata --format-version 1");
    assert_eq!(runner.ran[1], "cargo run --quiet -- check");

    // Plain mode only: `--json` and `--graph` emit no warning lines.
    let mut json_out = Vec::new();
    assert!(
        run_check(
            root,
            &CheckOptions {
                json: true,
                ..CheckOptions::default()
            },
            &mut runner,
            &mut json_out,
        )
        .unwrap()
    );
    assert!(json_out.is_empty());
}

#[test]
fn plugin_list_marks_a_package_whose_library_is_named_like_one_of_navrees_crates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let cargo_toml = "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
jalandhar-notes = \"0.3.0\"

[workspace]
";
    support::app(root, cargo_toml);

    let mut runner = ScriptedCargo::new()
        .package(
            "jalandhar-notes",
            "0.3.0",
            "module = \"NotesModule::from_config()\"",
            "navree_social",
        )
        .transitive_package("ludhiana-ledger", "0.1.0", "navree_billing");

    let mut out = Vec::new();
    list::run(root, &ListOptions { json: false }, &mut runner, &mut out)
        .expect("plugin:list succeeds");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\
jalandhar-notes  0.3.0  warning    library is `navree_social` (not `jalandhar_notes`)
ludhiana-ledger  0.1.0  warning    library is `navree_billing` (not `ludhiana_ledger`)
"
    );

    let mut json_out = Vec::new();
    list::run(
        root,
        &ListOptions { json: true },
        &mut runner,
        &mut json_out,
    )
    .expect("plugin:list --json succeeds");
    let parsed: Value = serde_json::from_slice(&json_out).expect("valid JSON");
    assert_eq!(
        parsed,
        json!([
            {
                "package": "jalandhar-notes",
                "version": "0.3.0",
                "first_party": false,
                "module": null,
                "wired": false,
                "file": null,
                "library": "navree_social",
                "warning": "library is `navree_social` (not `jalandhar_notes`)"
            },
            {
                "package": "ludhiana-ledger",
                "version": "0.1.0",
                "first_party": false,
                "module": null,
                "wired": false,
                "file": null,
                "library": "navree_billing",
                "warning": "library is `navree_billing` (not `ludhiana_ledger`)"
            }
        ])
    );
}
