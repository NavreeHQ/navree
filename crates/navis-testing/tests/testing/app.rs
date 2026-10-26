//! `TestApp`: building, configuration, temporary databases, overrides,
//! resolving providers and shutting down.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navis_config::{Config, ConfigModule, ConfigSources};
use navis_contracts::{Clock, FixedClock as ContractsFixedClock};
use navis_http::HttpModule;
use navis_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Lifecycle, Module, ModuleBuilder,
    Navis, Resolver,
};
use navis_testing::{FixedClock, TestApp, temp_sqlite_url};

use crate::support::DemoModule;

/// The file path inside a `sqlite://<path>?mode=rwc` URL.
fn sqlite_path(url: &str) -> PathBuf {
    let path = url
        .strip_prefix("sqlite://")
        .and_then(|rest| rest.strip_suffix("?mode=rwc"))
        .unwrap_or_else(|| panic!("not a sqlite://...?mode=rwc URL: {url}"));
    PathBuf::from(path)
}

async fn database_url(app: &TestApp) -> String {
    app.get::<Config>()
        .await
        .get_string("database.url")
        .expect("database.url is set")
}

#[tokio::test]
async fn temp_database_is_isolated_per_app() {
    let first = TestApp::builder().temp_database().build().await;
    let second = TestApp::builder().temp_database().build().await;

    let (url_a, url_b) = (database_url(&first).await, database_url(&second).await);
    assert_ne!(url_a, url_b);
    let (dir_a, dir_b) = (
        sqlite_path(&url_a).parent().unwrap().to_owned(),
        sqlite_path(&url_b).parent().unwrap().to_owned(),
    );
    assert_ne!(dir_a, dir_b, "each app has its own directory");
    assert!(dir_a.is_dir() && dir_b.is_dir());
    assert_eq!(
        first
            .get::<Config>()
            .await
            .get_bool("database.migrate_on_boot"),
        Some(true)
    );

    drop(first);
    assert!(!dir_a.exists(), "the directory goes with the app");
    assert!(dir_b.is_dir(), "the other app's database stays");
}

#[test]
fn temp_sqlite_url_points_into_a_fresh_directory_removed_on_drop() {
    let (url, guard) = temp_sqlite_url();
    let path = sqlite_path(&url);
    assert_eq!(path.parent(), Some(guard.path()));
    assert!(guard.path().is_dir());
    assert!(!path.exists(), "SQLite creates the file (mode=rwc)");

    std::fs::write(&path, b"x").unwrap();
    let dir = guard.path().to_owned();
    drop(guard);
    assert!(!dir.exists());
}

#[tokio::test]
async fn config_values_reach_the_app() {
    let app = TestApp::builder()
        .config("billing.currency", "EUR")
        .config("billing", serde_json::json!({ "trial_days": 14 }))
        .build()
        .await;
    let config = app.get::<Config>().await;
    assert_eq!(
        config.get_string("billing.currency").as_deref(),
        Some("EUR")
    );
    assert_eq!(
        config.get_string("billing.trial_days").as_deref(),
        Some("14"),
        "objects merge"
    );
}

#[tokio::test]
async fn override_value_replaces_a_provider() {
    struct ClockModule;
    impl Module for ClockModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value_arc::<dyn Clock>(Arc::new(navis_contracts::SystemClock))
                .exports::<dyn Clock>();
        }
    }

    let start = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let clock = Arc::new(FixedClock::new(start));
    let app = TestApp::builder()
        .module(ClockModule)
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;

    let resolved = app.get::<dyn Clock>().await;
    assert_eq!(resolved.now(), start);
    clock.advance(std::time::Duration::from_secs(60));
    assert_eq!(resolved.now(), start + chrono::TimeDelta::seconds(60));
}

#[test]
fn fixed_clock_is_the_contracts_clock() {
    fn same<T>(_: T, _: T) {}
    let now = chrono::Utc::now();
    same(FixedClock::new(now), ContractsFixedClock::new(now));
}

#[tokio::test]
#[should_panic(expected = "navis_config::config::Config")]
async fn get_names_the_missing_provider() {
    let app = TestApp::from_builder(Navis::app().module(EmptyModule)).await;
    let _ = app.get::<Config>().await;
}

struct EmptyModule;

impl Module for EmptyModule {
    fn register(&self, _m: &mut ModuleBuilder) {}
}

/// Records whether its `on_shutdown` hook ran.
#[derive(Default)]
struct ShutdownProbe {
    stopped: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
struct Stopped(Arc<AtomicBool>);

impl Injectable for ShutdownProbe {
    fn dependencies() -> Vec<navis_kernel::ProviderKey> {
        vec![navis_kernel::ProviderKey::of::<Stopped>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(ShutdownProbe {
                stopped: r.get::<Stopped>().await?.0.clone(),
            })
        })
    }
}

impl Lifecycle for ShutdownProbe {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        self.stopped.store(true, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

struct ProbeModule(Stopped);

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.0.clone())
            .provider::<ShutdownProbe>()
            .lifecycle::<ShutdownProbe>();
    }
}

#[tokio::test]
async fn shutdown_runs_the_shutdown_hooks() {
    let stopped = Stopped::default();
    let app = TestApp::builder()
        .module(ProbeModule(stopped.clone()))
        .build()
        .await;
    let token = app.ctx().shutdown_token();
    app.shutdown().await.expect("clean shutdown");
    assert!(stopped.0.load(Ordering::SeqCst));
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn try_build_returns_the_boot_error() {
    struct NeedsConfig;
    impl Module for NeedsConfig {
        fn register(&self, m: &mut ModuleBuilder) {
            m.import(ConfigModule::for_root(ConfigSources::new()));
        }
    }
    let error = TestApp::builder()
        .module(NeedsConfig)
        .try_build()
        .await
        .expect_err("ConfigModule is configured twice");
    assert!(error.to_string().contains("ConfigModule"), "{error}");
}

#[tokio::test]
async fn an_app_without_http_still_boots() {
    let app =
        TestApp::from_builder(Navis::app().module(ConfigModule::for_root(ConfigSources::new())))
            .await;
    assert!(
        app.ctx()
            .try_get::<navis_http::HttpServer>()
            .await
            .is_none()
    );
}

#[tokio::test]
#[should_panic(expected = "needs HttpModule")]
async fn client_needs_http() {
    let app =
        TestApp::from_builder(Navis::app().module(ConfigModule::for_root(ConfigSources::new())))
            .await;
    let _ = app.client();
}

#[tokio::test]
async fn builder_apps_include_http_and_config() {
    let app = TestApp::builder().module(DemoModule).build().await;
    assert!(app.ctx().try_get::<Config>().await.is_some());
    assert!(
        app.ctx()
            .try_get::<navis_http::HttpServer>()
            .await
            .is_some()
    );
    let _ = HttpModule;
}

#[tokio::test]
async fn sources_replace_the_base_configuration_and_config_stays_on_top() {
    let app = TestApp::builder()
        .config("app.name", "from-set")
        .sources(ConfigSources::new().env_vars(
            "APP_",
            [("APP_APP__NAME", "from-env"), ("APP_APP__ENV", "testing")],
        ))
        .build()
        .await;
    let config = app.get::<Config>().await;
    assert_eq!(config.get_string("app.name").as_deref(), Some("from-set"));
    assert_eq!(config.get_string("app.env").as_deref(), Some("testing"));
}

struct CommandModule(Stopped);

impl Module for CommandModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let flag = self.0.clone();
        m.command(navis_kernel::CommandSpec::new(
            "probe:flip",
            "Sets the flag when called with `on`",
            move |_ctx: AppContext, args: Vec<String>| {
                let flag = flag.clone();
                async move {
                    if args != ["on"] {
                        return Err(format!("unexpected arguments {args:?}").into());
                    }
                    flag.0.store(true, Ordering::SeqCst);
                    Ok(())
                }
            },
        ));
    }
}

#[tokio::test]
async fn run_command_runs_a_registered_command() {
    let flag = Stopped::default();
    let app = TestApp::builder()
        .module(CommandModule(flag.clone()))
        .build()
        .await;

    let error = app
        .run_command("probe:flip", &["off"])
        .await
        .expect_err("the command rejects `off`");
    assert!(
        error.to_string().contains("unexpected arguments"),
        "{error}"
    );

    app.run_command("probe:flip", &["on"]).await.unwrap();
    assert!(flag.0.load(Ordering::SeqCst));

    let unknown = app.run_command("nope", &[]).await.unwrap_err();
    assert!(unknown.to_string().contains("nope"), "{unknown}");
}

#[tokio::test]
async fn the_per_ip_rate_limit_can_be_switched_off_for_request_heavy_tests() {
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .build()
        .await;
    let options = app.get::<navis_http::HttpOptions>().await;
    assert_eq!(options.per_ip_rate_limit, None);

    let default = TestApp::builder().build().await;
    assert_eq!(
        default
            .get::<navis_http::HttpOptions>()
            .await
            .per_ip_rate_limit,
        Some(600)
    );
}
