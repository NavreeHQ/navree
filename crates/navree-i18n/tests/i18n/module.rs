//! Booting `I18nModule`: configuration, embedded files, boot errors and
//! reloading.

use std::fs;
use std::time::Duration;

use navree_config::ConfigSources;
use navree_i18n::{I18n, I18nError, I18nModule, I18nOptions, Missing, Source, args, embed_locales};
use navree_testing::TestApp;
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, FIXTURES};

#[tokio::test]
async fn boots_from_config_and_translates() {
    let app = support::builder().build().await;
    let i18n = app.get::<I18n>().await;
    assert_eq!(i18n.locales(), ["en", "fr", "de", "pt-BR"]);
    assert_eq!(i18n.options().missing, Missing::Error);
    assert_eq!(
        i18n.t("fr", "deal-created", args!(count = 3)).unwrap(),
        "3 affaires créées"
    );
    assert_eq!(
        i18n.t("pt-BR", "deal-created", args!(count = 2)).unwrap(),
        "2 negócios criados"
    );
    assert!(matches!(
        i18n.t("fr", "nope", args!()),
        Err(I18nError::MissingMessage { .. })
    ));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn lists_come_from_comma_separated_environment_variables() {
    let app = TestApp::builder()
        .sources(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_I18N__LOCALES", "en,fr,de"),
                ("APP_I18N__DEFAULT_LOCALE", "fr"),
                ("APP_I18N__NEGOTIATION", "query,header"),
                ("APP_I18N__DIR", FIXTURES),
            ],
        ))
        .module(I18nModule::from_config())
        .build()
        .await;
    let i18n = app.get::<I18n>().await;
    assert_eq!(i18n.locales(), ["en", "fr", "de"]);
    assert_eq!(i18n.default_locale(), "fr");
    assert_eq!(i18n.options().negotiation, [Source::Query, Source::Header]);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn embedded_files_need_no_directory() {
    let app = TestApp::builder()
        .module(I18nModule::for_root(I18nOptions {
            files: embed_locales!("tests/fixtures/locales"),
            locales: vec!["en".into(), "fr".into(), "de".into()],
            dir: "does/not/exist".into(),
            ..I18nOptions::default()
        }))
        .build()
        .await;
    let i18n = app.get::<I18n>().await;
    assert_eq!(
        i18n.t("de", "deal-created", args!(count = 1)).unwrap(),
        "Ein Geschäft angelegt"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_syntax_error_fails_the_boot_naming_the_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("en")).unwrap();
    fs::write(dir.path().join("en/main.ftl"), "ok = Fine\n\nbroken = { $count\n").unwrap();
    let error = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .module(I18nModule::from_config())
        .try_build()
        .await
        .err()
        .expect("the boot fails");
    let message = error.to_string();
    assert!(message.contains("/en/main.ftl:3: syntax error"), "{message}");
}

#[tokio::test]
async fn invalid_settings_fail_the_boot_naming_the_key() {
    let error = TestApp::builder()
        .config("i18n.locales", json!(["en", "fr"]))
        .config("i18n.default_locale", "de")
        .module(I18nModule::from_config())
        .try_build()
        .await
        .err()
        .expect("the boot fails");
    assert!(
        error.to_string().contains("i18n.default_locale `de` must be one of i18n.locales"),
        "{error}"
    );
    let error = TestApp::builder()
        .module(I18nModule::for_root(I18nOptions {
            locales: vec!["en".into(), "not a tag!".into()],
            ..I18nOptions::default()
        }))
        .try_build()
        .await
        .err()
        .expect("the boot fails");
    assert!(error.to_string().contains("`not a tag!`"), "{error}");
}

#[tokio::test]
async fn saved_locales_need_a_database() {
    let app = support::builder().build().await;
    let i18n = app.get::<I18n>().await;
    // Without a database nothing is saved: the default locale.
    assert_eq!(i18n.user_locale(Uuid::now_v7()).await.unwrap().tag(), "en");
    assert_eq!(i18n.workspace_locale(Uuid::now_v7()).await.unwrap().tag(), "en");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn reload_picks_up_changed_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("en")).unwrap();
    let file = dir.path().join("en/main.ftl");
    fs::write(&file, "greeting = Hello\n").unwrap();
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.reload", true)
        .module(I18nModule::from_config())
        .build()
        .await;
    let i18n = app.get::<I18n>().await;
    assert_eq!(i18n.t("en", "greeting", args!()).unwrap(), "Hello");

    // A broken file keeps the previous messages.
    fs::write(&file, "greeting = { $broken\n").unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(i18n.t("en", "greeting", args!()).unwrap(), "Hello");

    fs::write(&file, "greeting = Hello again\n").unwrap();
    let mut text = String::new();
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        text = i18n.t("en", "greeting", args!()).unwrap();
        if text == "Hello again" {
            break;
        }
    }
    assert_eq!(text, "Hello again");
    app.shutdown().await.unwrap();
}
