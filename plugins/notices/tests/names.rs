//! Checks that every name `navree-plugin-notices` registers sits under the
//! `notices` namespace, that `src/` touches none of the app-only registration
//! seams, and that `[package.metadata.navree.plugin]` matches `[dependencies]`.

#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use navree::access::Subject as _;
use navree::config::ConfigSection as _;
use navree::contracts::Job as _;
use navree::orm::Model as _;
use navree::prelude::*;
use navree::testing::TestApp;
use navree::ui::UiModule;
use navree::view::{ViewModule, Views};
use navree_plugin_notices::{
    BOARD_SHEET, ExpireNotices, NOTICES_MIGRATIONS, NoticesModule, NoticesOptions, PUBLISH, READ,
    notice,
};

#[tokio::test]
async fn every_name_the_plugin_registers_is_under_its_namespace() {
    assert_eq!(NoticesOptions::KEY, "notices");
    assert_eq!(NOTICES_MIGRATIONS, "notices");
    assert!(READ.name.starts_with("notices."));
    assert!(PUBLISH.name.starts_with("notices."));
    assert_eq!(notice::Model::SUBJECT, "notices.Notice");
    assert_eq!(notice::Model::PERMISSION_PREFIX, "notices");
    assert!(notice::Model::meta().table().starts_with("notices_"));
    assert!(ExpireNotices::NAME.starts_with("notices."));
    assert!(
        BOARD_SHEET
            .url()
            .starts_with("/_navree/ui/app/notices.board-")
            && BOARD_SHEET.url().ends_with(".css"),
        "unexpected sheet url: {}",
        BOARD_SHEET.url()
    );

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for locale in ["en", "pa-IN"] {
        let source =
            fs::read_to_string(root.join("locales").join(locale).join("notices.ftl")).unwrap();
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((id, _)) = trimmed.split_once('=') {
                let id = id.trim();
                assert!(
                    id.starts_with("notices-"),
                    "message id `{id}` in {locale} is outside `notices-`"
                );
            }
        }
    }

    let manifest_text = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&manifest_text).unwrap();
    let plugin_meta = &manifest["package"]["metadata"]["navree"]["plugin"];
    for section in plugin_meta["config"].as_array().unwrap() {
        let key = section.as_str().unwrap();
        assert!(key == "notices" || key.starts_with("notices."));
    }
    for var in plugin_meta["env"].as_array().unwrap() {
        let name = var.as_str().unwrap();
        assert!(
            name.starts_with("APP_NOTICES__"),
            "env `{name}` does not start with `APP_NOTICES__`"
        );
    }

    // Booting as a community plugin runs the framework's namespace checks on
    // permissions, subjects, templates, locales, commands, and sheets.
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(navree::orm::OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AccessModule::for_feature::<notice::Model>())
        .module(NoticesModule::from_config())
        .build()
        .await;

    let views = app.get::<Views>().await;
    assert!(views.exists("notices/list.html"));
    assert!(views.exists("notices/notice.html"));

    let plugin = &app.ctx().plugins().plugins[0];
    assert!(!plugin.first_party);
    assert_eq!(plugin.info.namespace, "notices");
    assert_eq!(plugin.modules[0].commands, ["notices:expire"]);

    app.shutdown().await.unwrap();
}

/// Strips line (`//`) and block (`/* ... */`) comments while keeping string
/// literals intact enough so comment words are not mistaken for calls.
fn without_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut block_depth = 0usize;
    while i < bytes.len() {
        if block_depth > 0 {
            if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                block_depth += 1;
                i += 2;
            } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                block_depth -= 1;
                i += 2;
            } else {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            block_depth = 1;
            i += 2;
            continue;
        }
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[test]
fn it_registers_through_none_of_the_routes_only_the_app_may_use() {
    const APP_ONLY: &[&str] = &[
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

    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0usize;
    for entry in fs::read_dir(&src_dir).expect("src directory") {
        let entry = entry.expect("src entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        checked += 1;
        let raw = fs::read_to_string(&path).expect("source file");
        let code = without_comments(&raw);
        for forbidden in APP_ONLY {
            assert!(
                !code.contains(forbidden),
                "{} contains app-only registration `{forbidden}`",
                path.display()
            );
        }
    }
    assert!(checked >= 7, "expected all src/*.rs files to be checked");
}

#[test]
fn the_manifest_features_are_the_crates_own() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(path).expect("Cargo.toml")).expect("valid TOML");

    let set_of = |items: &toml::Value| -> BTreeSet<String> {
        items
            .as_array()
            .expect("feature array")
            .iter()
            .map(|item| item.as_str().expect("feature string").to_owned())
            .collect()
    };

    let meta_features = set_of(&manifest["package"]["metadata"]["navree"]["plugin"]["features"]);
    let dep_features = set_of(&manifest["dependencies"]["navree"]["features"]);
    let dev_features = set_of(&manifest["dev-dependencies"]["navree"]["features"]);

    assert_eq!(meta_features, dep_features);

    let mut expected_dev = dep_features;
    for extra in ["i18n", "testing", "sqlite"] {
        assert!(
            expected_dev.insert(extra.to_owned()),
            "`{extra}` should be a dev-only feature"
        );
    }
    assert_eq!(dev_features, expected_dev);
}
