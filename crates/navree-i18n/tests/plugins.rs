//! Translations a plugin ships: read below the app's own, held to the plugin's
//! namespace, and copied into the app by `i18n:publish`.

use std::fs;
use std::path::Path;

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{EmbeddedLocales, ModuleBuilderLocalesExt};
use navree_i18n::{I18n, I18nModule, args};
use navree_kernel::{Module, ModuleBuilder, Navree, PluginInfo};
use navree_testing::TestApp;
use serde_json::json;

fn announcements() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-announcements",
        env!("CARGO_CRATE_NAME"),
        "announcements",
        "1.0.0",
    )
}

/// The plugin's own code: it names `navree_contracts`, `navree_kernel` and
/// `navree_i18n_macros`, never `navree_i18n`, as a plugin built without the
/// `i18n` feature would.
mod plugin {
    use navree_contracts::ModuleBuilderLocalesExt;
    use navree_kernel::{Module, ModuleBuilder};

    pub struct AnnouncementsModule;

    impl Module for AnnouncementsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(super::announcements())
                .locales(navree_i18n_macros::embed_locales!("tests/fixtures/plugin-locales"));
        }
    }
}

fn write(root: &Path, relative: &str, source: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

#[tokio::test]
async fn plugin_translations_load_after_the_app_and_need_no_i18n_feature() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    write(
        dir.path(),
        "en/main.ftl",
        "announcements-title = Notices from Harpreet Sandhu\n",
    );
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.locales", json!(["en", "hi"]))
        .module(I18nModule::from_config())
        .module(plugin::AnnouncementsModule)
        .build()
        .await;
    let i18n = app.get::<I18n>().await;
    assert_eq!(
        i18n.t("en", "announcements-title", args!()).unwrap(),
        "Notices from Harpreet Sandhu"
    );
    assert_eq!(
        i18n.t("en", "announcements-fee", args!()).unwrap(),
        "Fee for Ludhiana: ₹1,00,000"
    );
    assert_eq!(
        i18n.t("hi", "announcements-title", args!()).unwrap(),
        "सूचनाएँ"
    );
    assert_eq!(
        i18n.t("hi", "announcements-fee", args!()).unwrap(),
        "लुधियाना के लिए शुल्क: ₹1,00,000"
    );
    app.shutdown().await.unwrap();
    // Without I18nModule the files are not read, and the app boots.
    Navree::app()
        .module(plugin::AnnouncementsModule)
        .boot()
        .await
        .expect("boots without I18nModule");
}

#[tokio::test]
async fn a_plugin_message_outside_its_namespace_fails_the_boot() {
    const OUTSIDE: &[(&str, &str, &str)] = &[(
        "en",
        "locales/en/notices.ftl",
        "notice-title = Notices for Meera Sharma\n-brand = Navree\nannouncements-ok = Fine\n",
    )];
    struct Outside;
    impl Module for Outside {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements())
                .locales(EmbeddedLocales::from(OUTSIDE));
        }
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let Err(error) = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .module(I18nModule::from_config())
        .module(Outside)
        .try_build()
        .await
    else {
        panic!("the boot must fail")
    };
    let error = error.to_string();
    assert!(
        error.contains("plugin `navree-plugin-announcements`: message `notice-title` in locales/en/notices.ftl does not start with `announcements-`"),
        "{error}"
    );
    assert!(
        error.contains("plugin `navree-plugin-announcements`: message `-brand` in locales/en/notices.ftl does not start with `-announcements-`"),
        "{error}"
    );
    assert!(!error.contains("announcements-ok"));
    // `i18n:check` loads leniently and lists them.
    let shown = dir.path().to_string_lossy().replace('\\', "/");
    let error = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("i18n.dir", shown.as_str()),
        ))
        .module(I18nModule::from_config())
        .module(Outside)
        .run_with_args(vec!["i18n:check".to_owned()])
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("message `notice-title` in locales/en/notices.ftl"),
        "{error}"
    );
}

#[tokio::test]
async fn i18n_publish_writes_a_plugins_files() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.locales", json!(["en", "hi"]))
        .module(I18nModule::from_config())
        .module(plugin::AnnouncementsModule)
        .build()
        .await;
    let en = dir.path().join("en/announcements/announcements.ftl");
    let hi = dir.path().join("hi/announcements/announcements.ftl");
    app.run_command("i18n:publish", &["announcements"])
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(&en).unwrap(),
        include_str!("fixtures/plugin-locales/en/announcements.ftl")
    );
    assert_eq!(
        fs::read_to_string(&hi).unwrap(),
        include_str!("fixtures/plugin-locales/hi/announcements.ftl")
    );
    assert!(!dir.path().join("pa").exists(), "pa is not a locale of the app");

    // An existing file is kept without --force and replaced with it; the plugin's name works too.
    fs::write(&en, "announcements-title = Harpreet Sandhu's notices\n").unwrap();
    app.run_command("i18n:publish", &["navree-plugin-announcements"])
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(&en).unwrap(),
        "announcements-title = Harpreet Sandhu's notices\n"
    );
    app.run_command("i18n:publish", &["announcements", "--force"])
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(&en).unwrap(),
        include_str!("fixtures/plugin-locales/en/announcements.ftl")
    );

    app.run_command("i18n:publish", &["validation"])
        .await
        .unwrap();
    assert!(dir.path().join("en/validation.ftl").exists());
    assert_eq!(
        app.run_command("i18n:publish", &["notices"])
            .await
            .unwrap_err()
            .to_string(),
        "i18n:publish: nothing to publish called `notices` (available: validation, announcements)"
    );
    assert_eq!(
        app.run_command("i18n:publish", &[])
            .await
            .unwrap_err()
            .to_string(),
        "usage: i18n:publish validation|<plugin> [--force]"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn i18n_publish_takes_the_plugins_locale_of_the_same_language() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.locales", json!(["en", "pa-IN"]))
        .module(I18nModule::from_config())
        .module(plugin::AnnouncementsModule)
        .build()
        .await;
    app.run_command("i18n:publish", &["announcements"])
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("pa-IN/announcements/announcements.ftl")).unwrap(),
        include_str!("fixtures/plugin-locales/pa/announcements.ftl")
    );
    assert!(!dir.path().join("hi").exists(), "hi is not a locale of the app");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn i18n_publish_refuses_a_destination_folder_that_is_a_link() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let elsewhere = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    fs::create_dir_all(dir.path().join("en")).unwrap();
    link_folder(elsewhere.path(), &dir.path().join("en/announcements"));
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.locales", json!(["en", "hi"]))
        .module(I18nModule::from_config())
        .module(plugin::AnnouncementsModule)
        .build()
        .await;
    assert_eq!(
        app.run_command("i18n:publish", &["announcements"])
            .await
            .unwrap_err()
            .to_string(),
        "i18n:publish: en/announcements is a symbolic link or inside one; nothing was written"
    );
    assert!(
        fs::read_dir(elsewhere.path()).unwrap().next().is_none(),
        "nothing through the link"
    );
    assert!(!dir.path().join("hi").exists(), "nothing at all");
    app.shutdown().await.unwrap();
}

/// Makes `link` a symbolic link to the folder `target`.
#[cfg(unix)]
fn link_folder(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Makes `link` a junction to the folder `target` with `mklink /J`, which needs no
/// administrator rights, and checks it was made.
#[cfg(windows)]
fn link_folder(target: &Path, link: &Path) {
    // `mklink` reads a `/` as the start of an option: pass `\` only.
    let backslashed = |path: &Path| path.to_string_lossy().replace('/', "\\");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(backslashed(link))
        .arg(backslashed(target))
        .output()
        .expect("cmd runs");
    assert!(
        output.status.success(),
        "mklink /J failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
