//! Drivers a module registers under a name, with made-up slots: the
//! settings a driver reads, and `config:check`, `config:schema` and the
//! boot agreeing on the names that resolve.

use std::sync::Arc;

use navree_config::__private::{build_driver, register_driver};
use navree_config::{ConfigModule, ConfigSources, DriverName, DriverSlot};
use navree_kernel::{Factory, Module, ModuleBuilder, Navree, Resolver};
use serde::Deserialize;
use serde_json::json;

/// The selector `vault.kind` of a made-up `vault` section.
const SLOT: DriverSlot = DriverSlot::new("vault.kind", &["memory"], "m.vault_kind", "vault kind");

/// What the `chest` vault kind reads from its settings.
#[derive(Default, Deserialize)]
#[serde(default)]
struct ChestSettings {
    capacity: u32,
    city: String,
}

/// Registers `name` for `slot`: a driver that says what it read, from
/// where, and for which instance.
fn register_chest(m: &mut ModuleBuilder, slot: &DriverSlot, name: &'static str) {
    let settings = (!slot.key().contains('*')).then_some("vault");
    register_driver::<String, _, _>(m, slot, settings, name, move |ctx| async move {
        assert_eq!(ctx.name().as_str(), name);
        let read = ctx.settings::<ChestSettings>()?;
        Ok(Arc::new(format!(
            "{} {} {} {:?}",
            read.capacity,
            read.city,
            ctx.settings_key(),
            ctx.instance()
        )))
    });
}

/// Registers the vault kind `chest`.
struct ChestModule;

impl Module for ChestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_chest(m, &SLOT, "chest");
    }
}

/// What the `chest` driver said, built for the settings at a key.
struct ChestReport(Arc<String>);

/// Builds the `chest` vault kind with its settings at `.0`.
struct ReportModule(&'static str);

impl Module for ReportModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let settings_key = self.0;
        m.factory(Factory::<ChestReport>::new(move |r: Resolver| async move {
            let name = DriverName::new("chest")?;
            let built = build_driver::<String>(
                &r.app(),
                &SLOT,
                "vault.kind",
                name,
                settings_key.to_owned(),
                None,
            )
            .await?;
            Ok(Arc::new(ChestReport(built)))
        }));
    }
}

/// What the `chest` driver of an app with `sources` (no `ConfigModule`
/// without them) says for the settings at `settings_key`; the boot errors
/// when it does not boot.
async fn chest_report(
    sources: Option<ConfigSources>,
    settings_key: &'static str,
) -> Result<String, String> {
    let mut app = Navree::app();
    if let Some(sources) = sources {
        app = app.module(ConfigModule::for_root(sources));
    }
    let app = app
        .module(ChestModule)
        .module(ReportModule(settings_key))
        .boot()
        .await
        .map_err(|errors| errors.to_string())?;
    let report = app
        .ctx()
        .get::<ChestReport>()
        .await
        .map_err(|error| error.to_string())?;
    Ok(report.0.as_str().to_owned())
}

#[tokio::test]
async fn driver_settings_come_from_their_key_and_name_it_on_error() {
    let sources = || {
        ConfigSources::new().set(
            "vault",
            json!({
                "chest": { "capacity": 500, "city": "Ludhiana" },
                "broken": { "capacity": "lots-of-₹" },
            }),
        )
    };
    assert_eq!(
        chest_report(Some(sources()), "vault.chest").await.unwrap(),
        "500 Ludhiana vault.chest None"
    );

    let error = chest_report(Some(sources()), "vault.broken")
        .await
        .unwrap_err();
    assert!(
        error.contains("vault.broken") && error.contains("capacity"),
        "{error}"
    );
    assert!(!error.contains("lots-of-₹"), "{error}");

    assert_eq!(
        chest_report(Some(sources()), "vault.missing")
            .await
            .unwrap(),
        "0  vault.missing None"
    );

    let error = chest_report(None, "vault.chest").await.unwrap_err();
    assert!(
        error.contains(
            "vault.chest: the app has no ConfigModule, so driver `chest` cannot read its settings"
        ),
        "{error}"
    );
}
