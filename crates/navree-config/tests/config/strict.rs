//! `app.strict_config`: the boot refuses keys that nothing reads and unsafe
//! settings of a production-like environment; without it they are only
//! logged.

use navree_config::{ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
use navree_kernel::{Module, ModuleBuilder, Navree};
use serde::Deserialize;

/// A section with one setting.
#[derive(Debug, Default, Deserialize, ConfigSection)]
#[config(key = "queue")]
#[serde(default)]
struct Queue {
    /// How many jobs run at once.
    workers: u32,
}

struct QueueModule;

impl Module for QueueModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Queue>();
    }
}

const KEY: &str = "base64:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// Boots an app with `sources`; the error is the boot's message.
async fn boots(sources: ConfigSources) -> Result<(), String> {
    match Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(QueueModule)
        .boot()
        .await
    {
        Ok(app) => {
            app.shutdown().await.expect("shutdown");
            Ok(())
        }
        Err(errors) => Err(errors.to_string()),
    }
}

#[tokio::test]
async fn an_unknown_key_is_only_logged_by_default() {
    let sources = ConfigSources::new().set("queue.worker", 3);
    assert_eq!(boots(sources).await, Ok(()));
}

#[tokio::test]
async fn strict_config_refuses_an_unknown_key() {
    let sources = ConfigSources::new()
        .set("app.strict_config", true)
        .set("queue.worker", 3);
    let error = boots(sources).await.expect_err("refused");
    assert!(error.contains("app.strict_config"), "{error}");
    assert!(error.contains("queue.worker"), "{error}");
    assert!(error.contains("Did you mean `queue.workers`?"), "{error}");
}

#[tokio::test]
async fn strict_config_passes_a_clean_configuration() {
    let sources = ConfigSources::new()
        .set("app.strict_config", true)
        .set("queue.workers", 3);
    assert_eq!(boots(sources).await, Ok(()));
}

#[tokio::test]
async fn strict_config_refuses_a_production_app_without_a_key() {
    for environment in ["production", "staging"] {
        let sources = ConfigSources::new()
            .set("app.strict_config", true)
            .set("app.env", environment);
        let error = boots(sources).await.expect_err("refused");
        assert!(error.contains("app.key"), "{environment}: {error}");
    }
}

#[tokio::test]
async fn strict_config_refuses_sqlite_and_a_mail_log_in_production() {
    let sources = ConfigSources::new()
        .set("app.strict_config", true)
        .set("app.env", "production")
        .set("app.key", KEY)
        .set("database.url", "sqlite://app.db?mode=rwc")
        .set("mail.transport", "log");
    let error = boots(sources).await.expect_err("refused");
    assert!(error.contains("database.url"), "{error}");
    assert!(error.contains("mail.transport"), "{error}");
    assert!(!error.contains("app.key"), "{error}");
    assert!(!error.contains(KEY), "{error}");
}

#[tokio::test]
async fn a_production_app_with_a_key_boots_in_strict_mode() {
    let sources = ConfigSources::new()
        .set("app.strict_config", true)
        .set("app.env", "production")
        .set("app.key", KEY);
    assert_eq!(boots(sources).await, Ok(()));
}

#[tokio::test]
async fn production_without_strict_config_boots_and_logs() {
    let sources = ConfigSources::new().set("app.env", "production");
    assert_eq!(boots(sources).await, Ok(()));
}
