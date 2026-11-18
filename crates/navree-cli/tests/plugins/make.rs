//! Integration tests for `navree make:plugin`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use navree_cli::names::Name;
use navree_cli::plugin::index::Index;
use navree_cli::plugin::make::{MakePluginOptions, make_plugin, plugin_files};
use navree_cli::plugin::manifest::{
    AppFacts, RESERVED_NAMESPACES, default_namespace, read as read_manifest,
};

use crate::support;

fn sample_app_facts() -> AppFacts {
    let index = Index::embedded();
    let mut navree_features: BTreeSet<String> = index.manifest_features.iter().cloned().collect();
    for pkg in &index.packages {
        navree_features.insert(pkg.feature.clone());
    }
    AppFacts {
        first_party: false,
        navree_version: "1.4.0".to_owned(),
        navree_features,
        allowed_features: index.manifest_features.iter().cloned().collect(),
        reserved: RESERVED_NAMESPACES
            .iter()
            .map(|ns| (*ns).to_owned())
            .collect(),
        roles: BTreeSet::new(),
        demo_guest_role: None,
        granted: Vec::new(),
    }
}

fn assert_every_module_names_plugin(src_dir: &std::path::Path) {
    for entry in std::fs::read_dir(src_dir).expect("read plugin src/") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let code = std::fs::read_to_string(&path).expect("read rust source");
        let mut search_from = 0;
        while let Some(offset) = code[search_from..].find("impl Module for") {
            let start = search_from + offset;
            let rest = &code[start..];
            let reg_pos = rest
                .find("fn register(")
                .unwrap_or_else(|| panic!("missing register in {}", path.display()));
            let after_reg = &rest[reg_pos..];
            let open_brace = after_reg
                .find('{')
                .unwrap_or_else(|| panic!("missing register body in {}", path.display()));
            let body = &after_reg[open_brace + 1..];
            let first_stmt = body
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with("//"))
                .unwrap_or("");
            assert_eq!(
                first_stmt,
                "m.plugin(navree::plugin_info!());",
                "every Module implementation in {} must call m.plugin(navree::plugin_info!()) first in register",
                path.display()
            );
            search_from = start + "impl Module for".len();
        }
    }
}

#[test]
fn make_plugin_writes_the_files_the_writing_page_lists() {
    let temp = tempfile::tempdir().expect("tempdir");

    // 1. Standalone app whose `navree` comes from a relative path.
    let standalone_root = temp.path().join("kesar");
    let app_dir = support::app(
        &standalone_root,
        "\
[package]
name = \"kesar\"
version = \"0.1.0\"
edition = \"2024\"

[dependencies]
navree = { path = \"../navree-checkout/crates/navree\", features = [\"postgres\"] }
",
    );

    let options = MakePluginOptions {
        name: Name::parse("plugin", "announcements").expect("valid name"),
        path: "plugins".to_owned(),
        wrap: None,
    };
    let report = make_plugin(&app_dir, &options).expect("make_plugin succeeds");

    let plugin_dir = app_dir.join("plugins").join("announcements");
    let expected_eight: Vec<PathBuf> = [
        "Cargo.toml",
        "src/lib.rs",
        "src/options.rs",
        "tests/boots.rs",
        "README.md",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        ".github/workflows/ci.yml",
    ]
    .into_iter()
    .map(|rel| {
        rel.split('/')
            .fold(plugin_dir.clone(), |acc, part| acc.join(part))
    })
    .collect();
    assert_eq!(report.created, expected_eight);
    assert!(report.updated.is_empty());
    assert_eq!(
        report.hints,
        vec!["navree plugin:add --path plugins/announcements".to_owned()]
    );
    for file in &expected_eight {
        assert!(file.is_file(), "expected {} to exist", file.display());
    }

    let cargo_text =
        std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("read Cargo.toml");
    let cargo_doc: toml::Value = toml::from_str(&cargo_text).expect("generated Cargo.toml parses");
    assert!(
        cargo_doc.get("workspace").is_none(),
        "scaffolded plugin Cargo.toml must have no [workspace] table"
    );

    let pkg = &cargo_doc["package"];
    assert_eq!(pkg["name"].as_str(), Some("navree-plugin-announcements"));
    assert_eq!(pkg["version"].as_str(), Some("0.1.0"));
    assert_eq!(pkg["edition"].as_str(), Some("2024"));
    assert_eq!(pkg["rust-version"].as_str(), Some("1.95"));
    assert_eq!(
        pkg["description"].as_str(),
        Some("Announcements for Navree apps. Not an official Navree package.")
    );
    assert_eq!(pkg["license"].as_str(), Some("MIT OR Apache-2.0"));
    assert_eq!(
        pkg["keywords"],
        toml::Value::Array(vec![
            toml::Value::String("navree-plugin".to_owned()),
            toml::Value::String("navree".to_owned()),
        ])
    );
    assert_eq!(pkg["readme"].as_str(), Some("README.md"));

    let plugin_table_toml = &pkg["metadata"]["navree"]["plugin"];
    let plugin_table_json =
        serde_json::to_value(plugin_table_toml).expect("plugin table converts to json");
    let manifest = read_manifest(
        "navree-plugin-announcements",
        &plugin_table_json,
        &sample_app_facts(),
    )
    .expect("scaffolded plugin table passes manifest::read");
    assert_eq!(
        manifest.namespace,
        default_namespace("navree-plugin-announcements")
    );
    assert_eq!(manifest.title.as_deref(), Some("Announcements"));
    assert_eq!(
        manifest.module.as_str(),
        "AnnouncementsModule::from_config()"
    );
    assert_eq!(manifest.config, vec!["announcements".to_owned()]);

    let navree_dep = &cargo_doc["dependencies"]["navree"];
    assert_eq!(
        navree_dep["path"].as_str(),
        Some("../../../navree-checkout/crates/navree")
    );
    assert_eq!(navree_dep["default-features"].as_bool(), Some(false));

    let serde_dep = &cargo_doc["dependencies"]["serde"];
    assert_eq!(serde_dep["version"].as_str(), Some("1"));
    assert_eq!(
        serde_dep["features"],
        toml::Value::Array(vec![toml::Value::String("derive".to_owned())])
    );

    let dev_navree = &cargo_doc["dev-dependencies"]["navree"];
    assert_eq!(
        dev_navree["path"].as_str(),
        Some("../../../navree-checkout/crates/navree")
    );
    assert_eq!(dev_navree["default-features"].as_bool(), Some(false));
    assert_eq!(
        dev_navree["features"],
        toml::Value::Array(vec![
            toml::Value::String("testing".to_owned()),
            toml::Value::String("sqlite".to_owned()),
        ])
    );

    let dev_tokio = &cargo_doc["dev-dependencies"]["tokio"];
    assert_eq!(dev_tokio["version"].as_str(), Some("1"));
    assert_eq!(
        dev_tokio["features"],
        toml::Value::Array(vec![
            toml::Value::String("macros".to_owned()),
            toml::Value::String("rt-multi-thread".to_owned()),
        ])
    );

    let lib_rs = std::fs::read_to_string(plugin_dir.join("src/lib.rs")).expect("read src/lib.rs");
    assert!(lib_rs.contains("use navree::prelude::*;"));
    assert!(lib_rs.contains("use navree::plugin::prelude::*;"));
    assert!(lib_rs.contains("pub mod options;"));
    assert!(!lib_rs.contains("pub mod client;"));
    assert!(lib_rs.contains("pub fn from_config() -> AnnouncementsModule"));
    assert!(lib_rs.contains("m.config_section::<"));
    assert!(lib_rs.contains("// navree:wire"));
    assert_every_module_names_plugin(&plugin_dir.join("src"));

    let options_rs =
        std::fs::read_to_string(plugin_dir.join("src/options.rs")).expect("read src/options.rs");
    assert!(options_rs.contains("pub struct AnnouncementsOptions"));
    assert!(options_rs.contains("key = \"announcements\""));
    assert!(options_rs.contains("pub enabled: bool"));

    let boots_rs =
        std::fs::read_to_string(plugin_dir.join("tests/boots.rs")).expect("read tests/boots.rs");
    assert!(boots_rs.contains("async fn the_plugin_boots_and_is_listed()"));
    assert!(boots_rs.contains("TestApp"));
    assert!(boots_rs.contains("AnnouncementsModule::from_config()"));
    assert!(boots_rs.contains("\"navree-plugin-announcements\""));
    assert!(boots_rs.contains("\"announcements\""));
    assert!(boots_rs.contains("app.ctx().plugins()"));

    let ci_yml = std::fs::read_to_string(plugin_dir.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(ci_yml.contains("cargo fmt --check"));
    assert!(ci_yml.contains("cargo clippy"));
    assert!(ci_yml.contains("-D warnings"));
    assert!(ci_yml.contains("cargo test"));

    // 2. Workspace app (`navree = { workspace = true }` resolved from workspace root) + `--wrap reqwest`.
    let ws_root = temp.path().join("mandi-ws");
    let svc_dir = support::workspace_app(
        &ws_root,
        "\
[package]
name = \"svc\"
version.workspace = true
edition.workspace = true

[dependencies]
navree = { workspace = true, features = [\"ui\"] }
",
    );

    let wrapped_options = MakePluginOptions {
        name: Name::parse("plugin", "announcements").expect("valid name"),
        path: "plugins".to_owned(),
        wrap: Some("reqwest".to_owned()),
    };
    let wrapped_report = make_plugin(&svc_dir, &wrapped_options).expect("wrapped make_plugin");
    let wrapped_dir = svc_dir.join("plugins").join("announcements");
    let mut expected_nine = expected_eight
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(&plugin_dir).expect("under plugin_dir");
            wrapped_dir.join(rel)
        })
        .collect::<Vec<_>>();
    expected_nine.push(wrapped_dir.join("src").join("client.rs"));
    assert_eq!(wrapped_report.created, expected_nine);
    assert_eq!(
        wrapped_report.hints,
        vec!["navree plugin:add --path plugins/announcements".to_owned()]
    );

    let wrapped_cargo_text =
        std::fs::read_to_string(wrapped_dir.join("Cargo.toml")).expect("read wrapped Cargo.toml");
    assert!(wrapped_cargo_text.contains("# Set the version of reqwest this plugin wraps."));
    let wrapped_doc: toml::Value =
        toml::from_str(&wrapped_cargo_text).expect("wrapped Cargo.toml parses");
    assert_eq!(
        wrapped_doc["dependencies"]["navree"]["version"].as_str(),
        Some("1.4.0")
    );
    assert_eq!(
        wrapped_doc["dependencies"]["navree"]["default-features"].as_bool(),
        Some(false)
    );
    assert_eq!(wrapped_doc["dependencies"]["reqwest"].as_str(), Some("*"));
    assert_eq!(
        wrapped_doc["dev-dependencies"]["navree"]["version"].as_str(),
        Some("1.4.0")
    );
    assert_eq!(
        wrapped_doc["dev-dependencies"]["navree"]["default-features"].as_bool(),
        Some(false)
    );

    let wrapped_lib =
        std::fs::read_to_string(wrapped_dir.join("src/lib.rs")).expect("read wrapped src/lib.rs");
    assert!(wrapped_lib.contains("pub mod client;"));
    assert!(wrapped_lib.contains("m.import(client::"));
    assert_every_module_names_plugin(&wrapped_dir.join("src"));

    let client_rs =
        std::fs::read_to_string(wrapped_dir.join("src/client.rs")).expect("read src/client.rs");
    assert!(client_rs.contains("reqwest::Client"));
    assert!(client_rs.contains(".plugin(navree::plugin_info!())"));

    // Hyphenated `--wrap` crate converts `-` to `_` in Rust paths while keeping `-` in Cargo.toml.
    let hyphen_files = plugin_files(
        &Name::parse("plugin", "khata").expect("khata"),
        "{ version = \"1.4.0\", default-features = false }",
        Some("async-nats"),
        2026,
    )
    .expect("hyphen wrap renders");
    assert!(
        hyphen_files
            .get("Cargo.toml")
            .expect("Cargo.toml")
            .contains("async-nats = \"*\"")
    );
    assert!(
        hyphen_files
            .get("src/client.rs")
            .expect("src/client.rs")
            .contains("async_nats::Client")
    );
}

#[test]
fn make_plugin_readme_says_it_is_not_official() {
    let temp = tempfile::tempdir().expect("tempdir");
    let options = MakePluginOptions {
        name: Name::parse("plugin", "announcements").expect("valid name"),
        path: "plugins".to_owned(),
        wrap: None,
    };
    make_plugin(temp.path(), &options).expect("make_plugin outside app");

    let plugin_dir = temp.path().join("plugins").join("announcements");
    let readme = std::fs::read_to_string(plugin_dir.join("README.md")).expect("read README.md");
    assert!(
        readme.contains("It is not an official Navree package."),
        "README missing non-official statement:\n{readme}"
    );
    assert!(readme.contains("navree plugin:add --path plugins/announcements"));
    assert!(readme.contains("navree plugin:add navree-plugin-announcements"));

    let cargo_toml: toml::Value = toml::from_str(
        &std::fs::read_to_string(plugin_dir.join("Cargo.toml")).expect("read Cargo.toml"),
    )
    .expect("Cargo.toml parses");
    let description = cargo_toml["package"]["description"]
        .as_str()
        .expect("package.description");
    assert!(
        description.ends_with("Not an official Navree package."),
        "description was: {description}"
    );
}

#[test]
fn make_plugin_licence_names_the_plugins_authors() {
    let files = plugin_files(
        &Name::parse("plugin", "announcements").expect("valid name"),
        "{ version = \"1.4.0\", default-features = false }",
        None,
        2026,
    )
    .expect("plugin_files renders");

    let mit = files.get("LICENSE-MIT").expect("LICENSE-MIT present");
    assert!(
        mit.lines()
            .any(|line| line == "Copyright (c) 2026 the navree-plugin-announcements authors"),
        "unexpected MIT copyright line:\n{mit}"
    );
    assert!(!mit.contains("Amritpal"), "MIT holds framework author name");
    assert!(
        !mit.contains("Navree contributors"),
        "MIT holds Navree contributors"
    );

    let apache = files.get("LICENSE-APACHE").expect("LICENSE-APACHE present");
    assert_eq!(apache, include_str!("../../LICENSE-APACHE"));
}

#[test]
fn make_plugin_registers_through_none_of_the_routes_only_the_app_may_use() {
    let app_only_routes = [
        "http_layer",
        "for_root(",
        "m.multi(",
        "m.value(",
        "m.factory(",
        "on_model",
        "observe",
        "global_scope",
        "helm_staff",
        ".channel(",
        "workspace_scoped",
        "for_policy",
    ];

    for wrap in [None, Some("reqwest")] {
        let files = plugin_files(
            &Name::parse("plugin", "announcements").expect("valid name"),
            "{ version = \"1.4.0\", default-features = false }",
            wrap,
            2026,
        )
        .expect("plugin_files renders");

        for generated in files.iter() {
            for route in app_only_routes {
                assert!(
                    !generated.contents.contains(route),
                    "generated file `{}` (wrap={wrap:?}) holds app-only token `{route}`:\n{}",
                    generated.path,
                    generated.contents
                );
            }
        }
    }
}

#[test]
fn make_plugin_refuses_a_taken_folder_a_bad_wrap_and_a_kept_namespace() {
    let temp = tempfile::tempdir().expect("tempdir");

    // 1. Taken folder.
    let existing = temp.path().join("plugins").join("announcements");
    std::fs::create_dir_all(&existing).expect("create existing dir");
    std::fs::write(existing.join("keep.txt"), "untouched\n").expect("write marker");

    let taken_err = make_plugin(
        temp.path(),
        &MakePluginOptions {
            name: Name::parse("plugin", "announcements").expect("valid name"),
            path: "plugins".to_owned(),
            wrap: None,
        },
    )
    .expect_err("existing folder must be refused");
    assert_eq!(
        format!("{taken_err:#}"),
        "plugins/announcements exists; choose another name or remove the folder"
    );
    assert_eq!(
        std::fs::read_to_string(existing.join("keep.txt")).expect("marker still there"),
        "untouched\n"
    );
    assert!(!existing.join("Cargo.toml").exists());

    // 2. Bad `--wrap` values.
    let sixty_five = "a".repeat(65);
    for bad in [
        "",
        "reqwest/blocking",
        "req west",
        "reqwest;rm",
        sixty_five.as_str(),
    ] {
        let err = make_plugin(
            temp.path(),
            &MakePluginOptions {
                name: Name::parse("plugin", "khata").expect("valid name"),
                path: "plugins".to_owned(),
                wrap: Some(bad.to_owned()),
            },
        )
        .expect_err("bad --wrap must be refused");
        assert_eq!(
            format!("{err:#}"),
            format!("--wrap: `{bad}` is not a crate name")
        );
        assert!(!temp.path().join("plugins").join("khata").exists());
    }

    // 3. Kept namespaces.
    for kept in [
        "billing",
        "workspace",
        "app",
        "views",
        "all",
        "automations",
        "plugins",
    ] {
        let err = make_plugin(
            temp.path(),
            &MakePluginOptions {
                name: Name::parse("plugin", kept).expect("valid snake_case"),
                path: "plugins".to_owned(),
                wrap: None,
            },
        )
        .expect_err("kept namespace must be refused");
        assert_eq!(
            format!("{err:#}"),
            format!(
                "`{kept}` gives the namespace `{kept}`, which the framework keeps for itself; choose another name"
            )
        );
        assert!(!temp.path().join("plugins").join(kept).exists());
    }
}

#[test]
fn make_plugin_refuses_a_name_that_makes_the_package_name_too_long() {
    let temp = tempfile::tempdir().expect("tempdir");

    let fifty = "a".repeat(50);
    let ok_report = make_plugin(
        temp.path(),
        &MakePluginOptions {
            name: Name::parse("plugin", &fifty).expect("50 chars parses"),
            path: "plugins".to_owned(),
            wrap: None,
        },
    )
    .expect("50-character plugin name is accepted");
    assert_eq!(ok_report.created.len(), 8);
    let cargo_toml: toml::Value = toml::from_str(
        &std::fs::read_to_string(temp.path().join("plugins").join(&fifty).join("Cargo.toml"))
            .expect("read 50-char Cargo.toml"),
    )
    .expect("parse 50-char Cargo.toml");
    let package_name = cargo_toml["package"]["name"]
        .as_str()
        .expect("package.name");
    assert_eq!(package_name.len(), 64);

    let fifty_one = "a".repeat(51);
    let err = make_plugin(
        temp.path(),
        &MakePluginOptions {
            name: Name::parse("plugin", &fifty_one).expect("51 chars parses as Name"),
            path: "plugins".to_owned(),
            wrap: None,
        },
    )
    .expect_err("51-character plugin name must be refused");
    assert_eq!(
        format!("{err:#}"),
        format!("`{fifty_one}` is too long: a plugin name is at most 50 characters")
    );
    assert!(!temp.path().join("plugins").join(&fifty_one).exists());
}
