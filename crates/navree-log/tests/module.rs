//! `LoggerModule` in a booted app. The subscriber is process-wide: the first
//! boot installs it, later boots reuse it (and apply their levels). Tests that
//! assert on the shared filter hold `SERIAL`.

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_config::{ConfigModule, ConfigSources};
use navree_kernel::{
    BoxError, Factory, KernelError, Module, ModuleBuilder, Navree, ProviderKey, Resolver,
};
use navree_log::{LogControl, LogFormat, LogOptions, LoggerModule};
use serde_json::json;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn options_with_levels(levels: &[(&str, &str)]) -> LogOptions {
    LogOptions {
        levels: levels
            .iter()
            .map(|(target, level)| ((*target).to_owned(), (*level).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        ..LogOptions::default()
    }
}

#[tokio::test]
async fn second_boot_does_not_panic() {
    let _serial = SERIAL.lock().await;
    let first = Navree::app()
        .module(LoggerModule::for_root(options_with_levels(&[(
            "navree_first",
            "warn",
        )])))
        .boot()
        .await
        .expect("first boot");
    let second = Navree::app()
        .module(LoggerModule::for_root(options_with_levels(&[(
            "navree_second",
            "debug",
        )])))
        .boot()
        .await
        .expect("second boot");
    let first_control = first.ctx().get::<LogControl>().await.expect("control");
    let second_control = second.ctx().get::<LogControl>().await.expect("control");
    // One subscriber per process: both apps see the same filter, which now
    // holds the levels of the latest boot.
    assert_eq!(first_control.directives(), "info,navree_second=debug");
    assert_eq!(second_control.directives(), first_control.directives());
    first_control
        .set_level("navree_first", "error")
        .expect("set level through the first app");
    assert_eq!(
        second_control.directives(),
        "info,navree_first=error,navree_second=debug"
    );
    tracing::info!("logged after two boots");
    second.shutdown().await.expect("second shutdown");
    first.shutdown().await.expect("first shutdown");
}

#[tokio::test]
async fn plain_import_reads_the_log_section() {
    // Booting applies `navree_plain=debug` to the shared filter.
    let _serial = SERIAL.lock().await;
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().set(
            "log",
            json!({ "format": "json", "slow_query_ms": 75, "levels": { "navree_plain": "debug" } }),
        )))
        .module(LoggerModule)
        .boot()
        .await
        .expect("boot");
    let options = app.ctx().get::<LogOptions>().await.expect("options");
    assert_eq!(options.format, LogFormat::Json);
    assert_eq!(options.slow_query_ms, 75);
    assert_eq!(options.levels["navree_plain"], "debug");
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn from_config_reads_the_log_section() {
    let _serial = SERIAL.lock().await;
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().env_vars("APP_", [("APP_LOG__SLOW_QUERY_MS", "900")]),
        ))
        .module(LoggerModule::from_config())
        .boot()
        .await
        .expect("boot");
    let options = app.ctx().get::<LogOptions>().await.expect("options");
    assert_eq!(options.slow_query_ms, 900);
    app.shutdown().await.expect("shutdown");
}

#[test]
fn from_config_needs_the_config_module() {
    let errors = Navree::app()
        .module(LoggerModule::from_config())
        .validate()
        .expect_err("Config is missing");
    let message = errors.to_string();
    assert!(message.contains("navree_config"), "{message}");
}

#[test]
fn configuring_the_module_twice_fails() {
    let errors = Navree::app()
        .module(LoggerModule::for_root(LogOptions::default()))
        .module(LoggerModule::from_config())
        .module(ConfigModule)
        .validate()
        .expect_err("configured twice");
    assert!(
        errors
            .0
            .iter()
            .any(|e| matches!(e, KernelError::ConfiguredTwice { .. })),
        "{errors}"
    );
}

#[tokio::test]
async fn invalid_section_fails_the_boot() {
    let errors = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("log", json!({ "level": "loud" })),
        ))
        .module(LoggerModule)
        .boot()
        .await
        .expect_err("invalid level");
    let message = errors.to_string();
    assert!(
        message.contains("log") && message.contains("loud"),
        "{message}"
    );
}

#[tokio::test]
async fn invalid_root_options_fail_the_boot() {
    let _serial = SERIAL.lock().await;
    let errors = Navree::app()
        .module(LoggerModule::for_root(options_with_levels(&[(
            "a b", "debug",
        )])))
        .boot()
        .await
        .expect_err("invalid target");
    assert!(errors.to_string().contains("a b"), "{errors}");
}

/// A provider in a module that does not import `LoggerModule`.
struct NeedsControl(Arc<LogControl>);

struct AdminModule;

impl Module for AdminModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(
            Factory::<NeedsControl>::new(|r: Resolver| async move {
                Ok::<_, BoxError>(Arc::new(NeedsControl(r.get::<LogControl>().await?)))
            })
            .deps(vec![ProviderKey::of::<LogControl>()]),
        )
        .exports::<NeedsControl>();
    }
}

#[tokio::test]
async fn log_control_is_global() {
    let _serial = SERIAL.lock().await;
    let app = Navree::app()
        .module(LoggerModule::for_root(LogOptions::default()))
        .module(AdminModule)
        .boot()
        .await
        .expect("boot");
    let admin = app.ctx().get::<NeedsControl>().await.expect("provider");
    assert!(!admin.0.directives().is_empty());
    app.shutdown().await.expect("shutdown");
}
