//! `i18n:check`, `i18n:missing` and `i18n:publish`.

use std::fs;
use std::path::Path;

use navree_config::{ConfigModule, ConfigSources};
use navree_i18n::I18nModule;
use navree_kernel::Navree;
use navree_testing::TestApp;
use serde_json::json;

use crate::support::{self, FIXTURES};

#[tokio::test]
async fn check_lists_the_missing_messages_and_fails() {
    let app = support::builder().build().await;
    let error = app
        .run_command("i18n:check", &[])
        .await
        .unwrap_err()
        .to_string();
    let lines: Vec<&str> = error.lines().collect();
    assert!(
        lines.contains(&"tests/fixtures/locales/fr  login: missing (defined in tests/fixtures/locales/en/main.ftl:5)"),
        "{error}"
    );
    assert!(
        lines.contains(&"tests/fixtures/locales/de  pipeline-empty: missing (defined in tests/fixtures/locales/en/deals.ftl:14)"),
        "{error}"
    );
    // pt-BR is regional: a summary, not problems.
    assert!(
        lines.contains(
            &"tests/fixtures/locales/pt-BR  11 of 12 messages missing (falls back to en)"
        ),
        "{error}"
    );
    assert_eq!(lines.last(), Some(&"12 problem(s) in 4 locale(s)"));
    app.shutdown().await.unwrap();
}

fn write(root: &Path, relative: &str, source: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

#[tokio::test]
async fn check_passes_when_every_locale_is_complete() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "en/main.ftl", "hello = Hello\n");
    write(dir.path(), "fr/main.ftl", "hello = Bonjour\n");
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.locales", json!(["en", "fr"]))
        .module(I18nModule::from_config())
        .build()
        .await;
    app.run_command("i18n:check", &[]).await.unwrap();
    assert!(app.run_command("i18n:check", &["--what"]).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn check_reports_syntax_errors_instead_of_failing_the_boot() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "en/main.ftl",
        "ok = Fine\n\nbroken = { $count\n",
    );
    write(dir.path(), "en/more.ftl", "ok = Again\n");
    let shown = dir.path().to_string_lossy().replace('\\', "/");
    let error = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("i18n.dir", shown.as_str()),
        ))
        .module(I18nModule::from_config())
        .run_with_args(vec!["i18n:check".to_owned()])
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!("{shown}/en/main.ftl:3: syntax error")),
        "{error}"
    );
    assert!(
        error.contains(&format!(
            "{shown}/en/more.ftl:1: `ok` is already defined in {shown}/en/main.ftl:1"
        )),
        "{error}"
    );
    assert!(error.ends_with("2 problem(s) in 1 locale(s)"), "{error}");
}

#[tokio::test]
async fn missing_lists_ids_for_a_translator() {
    let app = support::builder().build().await;
    let error = app.run_command("i18n:missing", &["ja"]).await.unwrap_err();
    assert!(error.to_string().contains("not supported"), "{error}");
    assert!(app.run_command("i18n:missing", &[]).await.is_err());
    app.run_command("i18n:missing", &["de"]).await.unwrap();
    let i18n = app.get::<navree_i18n::I18n>().await;
    assert_eq!(
        i18n.missing("de").unwrap(),
        [
            "deal-amount",
            "deal-closes",
            "deal-closes-long",
            "deal-stage-negotiation",
            "deal-total",
            "login",
            "mail-footer",
            "pipeline-empty",
            "welcome",
        ]
    );
    assert!(i18n.missing("en").unwrap().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn publish_copies_the_shipped_validation_messages() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "fr/validation.ftl",
        "field-email = adresse e-mail\n",
    );
    let app = TestApp::builder()
        .config("i18n.dir", dir.path().to_string_lossy().as_ref())
        .config("i18n.locales", json!(["en", "fr", "pt", "ja"]))
        .module(I18nModule::from_config())
        .build()
        .await;
    app.run_command("i18n:publish", &["validation"])
        .await
        .unwrap();
    let english = fs::read_to_string(dir.path().join("en/validation.ftl")).unwrap();
    assert!(english.contains("validation-required = { $attribute } should not be empty"));
    let portuguese = fs::read_to_string(dir.path().join("pt/validation.ftl")).unwrap();
    assert!(portuguese.contains("não deve estar vazio"));
    assert!(!dir.path().join("ja").exists());
    // An existing file is kept unless --force.
    assert_eq!(
        fs::read_to_string(dir.path().join("fr/validation.ftl")).unwrap(),
        "field-email = adresse e-mail\n"
    );
    app.run_command("i18n:publish", &["validation", "--force"])
        .await
        .unwrap();
    assert!(
        fs::read_to_string(dir.path().join("fr/validation.ftl"))
            .unwrap()
            .contains("ne doit pas être vide")
    );
    assert!(app.run_command("i18n:publish", &["mail"]).await.is_err());
    assert!(app.run_command("i18n:publish", &[]).await.is_err());
    app.shutdown().await.unwrap();
}

#[test]
fn fixtures_exist() {
    assert!(Path::new(FIXTURES).join("en/deals.ftl").exists());
}
