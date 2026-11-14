//! `for_root(options)` replaces a module's section; only `from_config()` reads
//! it. A section in the configuration for a module that was given its options
//! in code is reported, but not a section nested in it that is registered on
//! its own and read either way.

use navree_config::{ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
use navree_kernel::{ConfiguredModule, Module, ModuleBuilder, Navree};
use serde::Deserialize;

/// The shelf's settings.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "shelf")]
#[serde(default)]
struct ShelfOptions {
    /// The colour of the shelf.
    color: String,
}

/// The shelf's lights: a section nested in `shelf`, read whichever way the
/// shelf is configured.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "shelf.lights")]
#[serde(default)]
struct LightOptions {
    /// Whether the lights are on.
    on: bool,
}

struct ShelfModule;

impl ShelfModule {
    fn for_root(options: ShelfOptions) -> impl Module {
        ConfiguredModule::<ShelfModule, _>::for_root(options, register)
    }

    fn from_config() -> impl Module {
        ConfiguredModule::<ShelfModule, ShelfOptions>::from_config(register)
    }
}

impl Module for ShelfModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<ShelfOptions>) {
    match options {
        Some(options) => m.value(options),
        None => m.config::<ShelfOptions>(),
    };
    m.config::<LightOptions>();
}

async fn boots(sources: ConfigSources, module: impl Module) -> Result<(), String> {
    match Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(module)
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

fn strict() -> ConfigSources {
    ConfigSources::new()
        .set("app.strict_config", true)
        .set("shelf.color", "red")
}

#[tokio::test]
async fn a_module_that_reads_its_section_is_not_reported() {
    assert_eq!(boots(strict(), ShelfModule::from_config()).await, Ok(()));
    assert_eq!(boots(strict(), ShelfModule).await, Ok(()));
}

#[tokio::test]
async fn a_section_for_a_module_given_its_options_in_code_is_reported() {
    let error = boots(strict(), ShelfModule::for_root(ShelfOptions::default()))
        .await
        .expect_err("the section is ignored");
    assert!(
        error.contains("`shelf` is set in the configuration"),
        "{error}"
    );
    assert!(error.contains("ShelfModule"), "{error}");
    assert!(error.contains("for_root"), "{error}");
}

#[tokio::test]
async fn without_a_section_for_root_is_fine() {
    let sources = ConfigSources::new().set("app.strict_config", true);
    assert_eq!(
        boots(sources, ShelfModule::for_root(ShelfOptions::default())).await,
        Ok(())
    );
}

#[tokio::test]
async fn a_nested_section_read_either_way_is_not_reported() {
    let sources = ConfigSources::new()
        .set("app.strict_config", true)
        .set("shelf.lights.on", true);
    assert_eq!(
        boots(sources, ShelfModule::for_root(ShelfOptions::default())).await,
        Ok(())
    );
}

#[tokio::test]
async fn a_key_of_its_own_beside_a_nested_section_is_still_reported() {
    let sources = strict().set("shelf.lights.on", true);
    let error = boots(sources, ShelfModule::for_root(ShelfOptions::default()))
        .await
        .expect_err("shelf.color is ignored");
    assert!(
        error.contains("`shelf` is set in the configuration"),
        "{error}"
    );
}

#[tokio::test]
async fn without_strict_config_the_report_is_only_logged() {
    let sources = ConfigSources::new().set("shelf.color", "red");
    assert_eq!(
        boots(sources, ShelfModule::for_root(ShelfOptions::default())).await,
        Ok(())
    );
}
