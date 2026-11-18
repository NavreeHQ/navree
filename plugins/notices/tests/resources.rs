//! Publishing `navree-plugin-notices` resources with `plugin:publish notices`,
//! and checking the `plugins` report of what `NoticesModule` adds and ships.

#![allow(missing_docs)]

use std::fs;
use std::path::Path;
use std::process::Command;

use navree::prelude::*;
use navree_plugin_notices::NoticesModule;

const CHILD_ENV: &str = "NOTICES_PUBLISH_TEST_ARGS";

/// Runs `Navree::app()` with `ConfigModule` and `NoticesModule` in the child
/// process's working directory when invoked as the helper subprocess.
fn maybe_run_publish_child() -> bool {
    let Ok(encoded) = std::env::var(CHILD_ENV) else {
        return false;
    };
    let args: Vec<String> = serde_json::from_str(&encoded).expect("encoded args");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime
        .block_on(
            Navree::app()
                .module(ConfigModule::for_root(ConfigSources::new()))
                .module(NoticesModule::from_config())
                .run_with_args(args),
        )
        .expect("plugin:publish succeeded");
    true
}

fn run_in(dir: &Path, args: &[&str]) -> String {
    let encoded = serde_json::to_string(&args).expect("encode args");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .arg("--exact")
        .arg("plugin_publish_copies_the_views_and_locales_and_keeps_the_apps_own")
        .arg("--nocapture")
        .current_dir(dir)
        .env(CHILD_ENV, encoded)
        .output()
        .expect("spawn child test process");
    assert!(
        output.status.success(),
        "command {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

#[test]
fn plugin_publish_copies_the_views_and_locales_and_keeps_the_apps_own() {
    if maybe_run_publish_child() {
        return;
    }

    let temp = tempfile::tempdir().expect("temp app folder");
    let root = temp.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ludhiana-society\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write Cargo.toml");

    // Dry run writes nothing.
    let dry = run_in(root, &["plugin:publish", "notices", "--dry-run"]);
    assert!(
        dry.contains("resources/views/notices/list.html: would be written"),
        "{dry}"
    );
    assert!(
        dry.contains("locales/en/notices/notices.ftl: would be written"),
        "{dry}"
    );
    assert!(!root.join("resources").exists());
    assert!(!root.join("locales").exists());
    assert!(!root.join("config").exists());

    // First publish writes views, locales, and config.
    let first = run_in(root, &["plugin:publish", "notices"]);
    assert!(
        first.contains("resources/views/notices/list.html: written"),
        "{first}"
    );
    assert!(
        first.contains("resources/views/notices/notice.html: written"),
        "{first}"
    );
    assert!(
        first.contains("locales/en/notices/notices.ftl: written"),
        "{first}"
    );
    assert!(
        first.contains("locales/pa-IN/notices/notices.ftl: written"),
        "{first}"
    );

    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let embedded_list = fs::read_to_string(crate_root.join("views/list.html")).unwrap();
    let embedded_notice = fs::read_to_string(crate_root.join("views/notice.html")).unwrap();
    let embedded_en = fs::read_to_string(crate_root.join("locales/en/notices.ftl")).unwrap();
    let embedded_pa = fs::read_to_string(crate_root.join("locales/pa-IN/notices.ftl")).unwrap();

    assert_eq!(
        fs::read_to_string(root.join("resources/views/notices/list.html")).unwrap(),
        embedded_list
    );
    assert_eq!(
        fs::read_to_string(root.join("resources/views/notices/notice.html")).unwrap(),
        embedded_notice
    );
    assert_eq!(
        fs::read_to_string(root.join("locales/en/notices/notices.ftl")).unwrap(),
        embedded_en
    );
    assert_eq!(
        fs::read_to_string(root.join("locales/pa-IN/notices/notices.ftl")).unwrap(),
        embedded_pa
    );

    #[cfg(windows)]
    {
        assert_eq!(
            fs::read_to_string(root.join(r"resources\views\notices\list.html")).unwrap(),
            embedded_list
        );
        assert_eq!(
            fs::read_to_string(root.join(r"resources\views\notices\notice.html")).unwrap(),
            embedded_notice
        );
    }

    // Customise a view and a locale file in the app; a second run keeps both and says so.
    let custom_view = "<section><h2>Model Town Society</h2></section>\n";
    let custom_locale = "notices-heading = Model Town notices\n";
    fs::write(root.join("resources/views/notices/list.html"), custom_view).unwrap();
    fs::write(root.join("locales/en/notices/notices.ftl"), custom_locale).unwrap();

    let second = run_in(root, &["plugin:publish", "notices"]);
    assert!(
        second.contains(
            "resources/views/notices/list.html: exists, kept (use --force to replace it)"
        ),
        "{second}"
    );
    assert!(
        second.contains("locales/en/notices/notices.ftl: exists, kept (use --force to replace it)"),
        "{second}"
    );
    assert_eq!(
        fs::read_to_string(root.join("resources/views/notices/list.html")).unwrap(),
        custom_view
    );
    assert_eq!(
        fs::read_to_string(root.join("locales/en/notices/notices.ftl")).unwrap(),
        custom_locale
    );

    // `--force` replaces the customised files with the plugin's copies.
    let forced = run_in(root, &["plugin:publish", "notices", "--force"]);
    assert!(
        forced.contains("resources/views/notices/list.html: written"),
        "{forced}"
    );
    assert_eq!(
        fs::read_to_string(root.join("resources/views/notices/list.html")).unwrap(),
        embedded_list
    );
    assert_eq!(
        fs::read_to_string(root.join("locales/en/notices/notices.ftl")).unwrap(),
        embedded_en
    );
}

#[test]
fn the_plugins_listing_says_what_it_adds_and_publishes() {
    let report = Navree::app()
        .module(NoticesModule::from_config())
        .plugin_report()
        .expect("valid plugin graph");
    let text = report.to_string();

    let expected = "\
notices  navree-plugin-notices 0.1.0  Notice board
  NoticesModule  navree_plugin_notices::NoticesModule
    commands  notices:expire
    adds      1 AppSheet, 1 ControllerRegistration, 1 EmbeddedLocales, 1 EmbeddedTemplates, 2 JobRegistration, 1 LinkRegistration, 1 MigrationSet, 1 Permissions, 2 Publishable, 1 Schedule, 1 SectionRegistration, 1 ShellNavItem
    publishes 2 views, 2 locales";
    assert_eq!(text, expected);
}
