//! `ConfigModule` and typed sections registered with `.config::<T>()`.

use std::sync::Arc;

use keel_config::{Config, ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
use keel_kernel::{BootErrors, Keel, KernelError, Module, ModuleBuilder};
use serde::Deserialize;
use tempfile::TempDir;

#[derive(Debug, Deserialize)]
struct BillingConfig {
    currency: String,
    #[serde(default)]
    plan: Option<String>,
}

impl ConfigSection for BillingConfig {
    const KEY: &'static str = "billing";

    fn validate(&self) -> Result<(), String> {
        if self.currency.len() == 3 {
            Ok(())
        } else {
            Err("currency must be a 3-letter ISO code".to_owned())
        }
    }
}

/// A section whose every field has a default.
#[derive(Debug, Deserialize)]
struct FeatureFlags {
    #[serde(default)]
    beta: bool,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    25
}

impl ConfigSection for FeatureFlags {
    const KEY: &'static str = "features.flags";
}

/// Uses the billing section; does not import `ConfigModule` (it is global).
struct BillingModule;

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<BillingConfig>();
    }
}

struct FeaturesModule;

impl Module for FeaturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<FeatureFlags>();
    }
}

fn toml(dir: &TempDir, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join("app.toml");
    std::fs::write(&path, contents).expect("write fixture");
    path
}

async fn boot_errors(sources: ConfigSources, module: impl Module) -> BootErrors {
    Keel::app()
        .module(ConfigModule::for_root(sources))
        .module(module)
        .boot()
        .await
        .expect_err("boot must fail")
}

#[tokio::test]
async fn set_overrides_everything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = toml(&dir, "[billing]\ncurrency = \"USD\"\nplan = \"pro\"\n");
    let sources = ConfigSources::new()
        .set("billing.currency", "EUR")
        .file(&file)
        .env_vars("APP_", [("APP_BILLING__CURRENCY", "GBP")]);

    let app = Keel::app()
        .module(ConfigModule::for_root(sources))
        .module(BillingModule)
        .boot()
        .await
        .expect("boot");

    let billing = app.ctx().get::<BillingConfig>().await.expect("billing");
    assert_eq!(billing.currency, "EUR");
    assert_eq!(billing.plan.as_deref(), Some("pro"));
    let config = app.ctx().get::<Config>().await.expect("config");
    assert_eq!(
        config.get_string("billing.currency").as_deref(),
        Some("EUR")
    );
}

#[tokio::test]
async fn config_section_validation_failure_fails_boot_naming_section() {
    let errors = boot_errors(
        ConfigSources::new().set("billing.currency", "EURO"),
        BillingModule,
    )
    .await;
    assert!(
        matches!(errors.0.as_slice(), [KernelError::Construction { provider, .. }] if provider.contains("BillingConfig")),
        "{errors:?}"
    );
    let text = errors.to_string();
    assert!(text.contains("`billing`"), "{text}");
    assert!(
        text.contains("currency must be a 3-letter ISO code"),
        "{text}"
    );
}

#[tokio::test]
async fn missing_section_is_deserialized_from_an_empty_table() {
    let app = Keel::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(FeaturesModule)
        .boot()
        .await
        .expect("boot");
    let flags = app.ctx().get::<FeatureFlags>().await.expect("flags");
    assert!(!flags.beta);
    assert_eq!(flags.limit, 25);

    // A required field still has to be configured.
    let text = boot_errors(ConfigSources::new(), BillingModule)
        .await
        .to_string();
    assert!(text.contains("`billing`"), "{text}");
    assert!(text.contains("currency"), "{text}");
}

#[tokio::test]
async fn nested_section_keys_are_dotted_paths() {
    let app = Keel::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().env_vars("APP_", [("APP_FEATURES__FLAGS__BETA", "true")]),
        ))
        .module(FeaturesModule)
        .boot()
        .await
        .expect("boot");
    assert!(app.ctx().get::<FeatureFlags>().await.expect("flags").beta);
}

#[tokio::test]
async fn config_is_a_global_singleton() {
    // Neither module imports ConfigModule: its export is global.
    let app = Keel::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("billing.currency", "EUR"),
        ))
        .module(BillingModule)
        .module(FeaturesModule)
        .boot()
        .await
        .expect("boot");
    let a = app.ctx().get::<Config>().await.expect("config");
    let b = app.ctx().get::<Config>().await.expect("config");
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn sections_without_config_module_fail_validation_naming_config() {
    let errors = Keel::app()
        .module(BillingModule)
        .validate()
        .expect_err("Config is missing");
    let text = errors.to_string();
    assert!(text.contains(std::any::type_name::<Config>()), "{text}");
    assert!(text.contains("BillingModule"), "{text}");
}

#[tokio::test]
async fn config_load_errors_fail_boot_naming_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = toml(&dir, "[billing\ncurrency = \"EUR\"\n");
    let text = boot_errors(ConfigSources::new().file(&file), BillingModule)
        .await
        .to_string();
    assert!(text.contains("app.toml"), "{text}");
    assert!(text.contains("line 1"), "{text}");
}

#[tokio::test]
async fn plain_config_module_loads_the_default_sources() {
    // The test's working directory (the crate root) has no `.env` or
    // `config/app.toml`, so only `APP_` environment variables apply.
    let app = Keel::app()
        .module(ConfigModule)
        .module(FeaturesModule)
        .boot()
        .await
        .expect("boot");
    assert!(app.ctx().get::<Config>().await.is_ok());
}
