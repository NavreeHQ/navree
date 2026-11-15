//! Drivers a module registers under a name, with made-up slots: the
//! settings a driver reads, and `config:check`, `config:schema` and the
//! boot agreeing on the names that resolve.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_config::__private::{
    build_driver, check_registrations, contribute_slot, register_driver, selector_error,
};
use navree_config::check::CheckOptions;
use navree_config::{
    ConfigModule, ConfigRegistry, ConfigSection, ConfigSources, DriverName, DriverSlot,
    ModuleBuilderConfigExt, RegisteredDriver, SectionRegistration,
};
use navree_kernel::{
    AppBuilder, AppContext, CommandSpec, ConfiguredModule, Factory, Module, ModuleBuilder, Navree,
    ProviderKey, Resolver,
};
use serde::{Deserialize, Deserializer};
use serde_json::json;

/// The selector `vault.kind` of a made-up `vault` section.
const SLOT: DriverSlot = DriverSlot::new("vault.kind", &["memory"], "m.vault_kind", "vault kind");

/// The boxes of the vault (`[vault.boxes.{name}]`), each naming its driver,
/// or named after it.
const BOX_SLOT: DriverSlot = DriverSlot::new(
    "vault.boxes.*.driver",
    &["memory"],
    "m.vault_box",
    "vault box",
)
.defaults_to_table_name();

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
                &r,
                &SLOT,
                "vault.kind",
                name,
                settings_key.to_owned(),
                None,
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

/// Which kind of vault keeps the boxes: built in, or a registered one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum VaultKind {
    #[default]
    Memory,
    Registered(DriverName),
}

/// `memory`, else a driver name, else the error that lists the names and
/// never repeats the value: how a driver kind reads its selector.
impl<'de> Deserialize<'de> for VaultKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if name == "memory" {
            return Ok(VaultKind::Memory);
        }
        DriverName::new(&name)
            .map(VaultKind::Registered)
            .map_err(|_| serde::de::Error::custom(selector_error("kind", &SLOT)))
    }
}

/// A made-up section with a driver selector and a table of boxes.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "vault")]
#[serde(default)]
struct VaultOptions {
    /// Where the vault keeps its boxes.
    #[config(driver = "memory", register = "m.vault_kind")]
    kind: VaultKind,
    /// Prepended to every box name.
    prefix: String,
    /// The boxes, by name.
    boxes: BTreeMap<String, BoxConfig>,
}

/// One box of the vault.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct BoxConfig {
    /// Its driver; the box's own name when not set.
    #[allow(dead_code)]
    driver: Option<String>,
}

/// The vault an app booted: what its registered kind said, when one is
/// selected.
struct Vault(Option<Arc<String>>);

/// Reads the `vault` section and builds the selected vault kind, as a
/// driver kind's module of the framework does.
struct VaultModule;

impl VaultModule {
    /// The module given its options in code.
    fn for_root(options: VaultOptions) -> impl Module {
        ConfiguredModule::<VaultModule, _>::for_root(options, register_vault)
    }
}

impl Module for VaultModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_vault(m, None);
    }
}

fn register_vault(m: &mut ModuleBuilder, options: Option<VaultOptions>) {
    contribute_slot(m, SLOT);
    contribute_slot(m, BOX_SLOT);
    match options {
        Some(options) => {
            m.value(options);
        }
        None => {
            m.config::<VaultOptions>();
        }
    }
    m.factory(
        Factory::<Vault>::new(|r: Resolver| async move {
            let options = r.get::<VaultOptions>().await?;
            check_registrations(&r.app(), &SLOT, VaultOptions::schema())?;
            check_registrations(&r.app(), &BOX_SLOT, None)?;
            let built = match options.kind {
                VaultKind::Memory => None,
                VaultKind::Registered(name) => Some(
                    build_driver::<String>(
                        &r,
                        &SLOT,
                        "vault.kind",
                        name,
                        format!("vault.{name}"),
                        None,
                        None,
                    )
                    .await?,
                ),
            };
            Ok(Arc::new(Vault(built)))
        })
        .deps(vec![ProviderKey::of::<VaultOptions>()]),
    );
}

/// Registers the vault kind `chest` a second time.
struct SecondChestModule;

impl Module for SecondChestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_chest(m, &SLOT, "chest");
    }
}

/// Registers a vault kind under the built-in name `memory`.
struct MemoryChestModule;

impl Module for MemoryChestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_chest(m, &SLOT, "memory");
    }
}

/// Registers the vault box `chest`.
struct BoxChestModule;

impl Module for BoxChestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_chest(m, &BOX_SLOT, "chest");
    }
}

/// A module that registers one command.
struct CommandModule(CommandSpec);

impl Module for CommandModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(self.0.clone());
    }
}

/// What `config:check` prints for an app built by `add`, without booting it.
async fn check_text(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    let out = Arc::new(Mutex::new(String::new()));
    let sink = out.clone();
    let probe = CommandModule(
        CommandSpec::new("probe:check", "config:check's report", move |ctx, _| {
            let sink = sink.clone();
            async move {
                let report =
                    navree_config::__private::check_report(&ctx, &CheckOptions::default()).await?;
                *sink.lock().unwrap() = report.render();
                Ok(())
            }
        })
        .offline(),
    );
    add(Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(probe))
    .run_with_args(vec!["probe:check".to_owned()])
    .await
    .unwrap();
    out.lock().unwrap().clone()
}

/// The boot errors of an app built by `add` over `ConfigModule::for_root(sources)`.
async fn boot(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    match add(Navree::app().module(ConfigModule::for_root(sources)))
        .boot()
        .await
    {
        Ok(_) => panic!("the app booted"),
        Err(errors) => errors.to_string(),
    }
}

/// The configuration with `vault` set to `value`.
fn vault(value: serde_json::Value) -> ConfigSources {
    ConfigSources::new().set("vault", value)
}

#[tokio::test]
async fn config_check_fails_on_an_unregistered_driver_as_the_boot_does() {
    // A registered kind boots with its settings, and the check passes: the
    // `chest` table is claimed by its registration.
    let sources = || vault(json!({"kind": "chest", "chest": {"capacity": 5}}));
    let app = Navree::app()
        .module(ConfigModule::for_root(sources()))
        .module(VaultModule)
        .module(ChestModule)
        .boot()
        .await
        .unwrap();
    let booted = app.ctx().get::<Vault>().await.unwrap();
    assert_eq!(
        booted.0.as_deref().map(String::as_str),
        Some("5  vault.chest None")
    );
    let text = check_text(sources(), |app| app.module(VaultModule).module(ChestModule)).await;
    assert!(text.contains("config:check: ok ("), "{text}");

    // A name no module registers: the key and the names known, never the
    // value.
    let unregistered = "vault.kind does not name a built-in vault kind or one a module registers with m.vault_kind (known: memory, chest)";
    let sources = || vault(json!({"kind": "tijori"}));
    let add = |app: AppBuilder| app.module(VaultModule).module(ChestModule);
    let text = boot(sources(), add).await;
    assert!(text.contains(unregistered), "{text}");
    assert!(!text.contains("tijori"), "{text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {unregistered}")), "{text}");
    assert!(!text.contains("tijori"), "{text}");

    // One name registered twice, although it is not the selected one.
    let twice = "vault kind `chest` is registered by both `ChestModule` and `SecondChestModule`; register it once";
    let sources = || vault(json!({"kind": "memory"}));
    let add = |app: AppBuilder| {
        app.module(VaultModule)
            .module(ChestModule)
            .module(SecondChestModule)
    };
    let text = boot(sources(), add).await;
    assert!(text.contains(twice), "{text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {twice}")), "{text}");

    // A registration under a built-in name.
    let built_in = "m.vault_kind(\"memory\") in `MemoryChestModule`: `memory` is a built-in vault kind; register yours under another name";
    let add = |app: AppBuilder| app.module(VaultModule).module(MemoryChestModule);
    let text = boot(sources(), add).await;
    assert!(text.contains(built_in), "{text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {built_in}")), "{text}");

    // A value that is not a driver name is the section's read error only.
    let selector = selector_error("kind", &SLOT);
    let sources = || vault(json!({"kind": "Not Valid"}));
    let add = |app: AppBuilder| app.module(VaultModule).module(ChestModule);
    let text = boot(sources(), add).await;
    assert!(text.contains(&selector), "{text}");
    assert!(!text.contains("or one a module registers"), "{text}");
    let text = check_text(sources(), add).await;
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("error    "))
            .count(),
        1,
        "{text}"
    );
    assert!(text.contains(&selector), "{text}");
    assert!(!text.contains("or one a module registers"), "{text}");

    // A module given its options in code does not read the selector.
    let text = check_text(vault(json!({"kind": "tijori"})), |app| {
        app.module(VaultModule::for_root(VaultOptions::default()))
            .module(ChestModule)
    })
    .await;
    assert!(!text.contains("or one a module registers"), "{text}");

    // A table of tables: each table names its driver, or is named after it.
    let sources = vault(json!({
        "boxes": {
            "chest": { "size": 3 },
            "almari": { "driver": "tijori" },
        },
    }));
    let text = check_text(sources, |app| {
        app.module(VaultModule).module(BoxChestModule)
    })
    .await;
    assert!(
        text.contains("error    vault.boxes.almari.driver does not name a built-in vault box or one a module registers with m.vault_box (known: memory, chest)"),
        "{text}"
    );
    assert!(!text.contains("vault.boxes.chest"), "{text}");
}

#[test]
fn config_schema_lists_built_in_and_registered_names() {
    let registry = || ConfigRegistry::new(&[SectionRegistration::of::<VaultOptions>()]);
    let chest = RegisteredDriver::new("vault.kind", DriverName::new("chest").unwrap());
    let document = registry()
        .with_drivers(&[SLOT, BOX_SLOT], &[chest])
        .schema_document();
    assert_eq!(
        document["properties"]["vault"]["properties"]["kind"],
        json!({
            "type": "string",
            "enum": ["memory", "chest"],
            "x-navree-register": "m.vault_kind",
            "description": "Where the vault keeps its boxes.",
        })
    );
    let document = registry().schema_document();
    assert_eq!(
        document["properties"]["vault"]["properties"]["kind"],
        json!({
            "type": "string",
            "anyOf": [{"enum": ["memory"]}, {"pattern": "^[a-z][a-z0-9_]{0,63}$"}],
            "x-navree-register": "m.vault_kind",
            "description": "Where the vault keeps its boxes.",
        })
    );
}

#[tokio::test]
async fn the_boot_does_not_report_the_tables_of_registered_drivers() {
    let sources = vault(json!({"boxes": {"chest": {"size": 3}}})).set("app.strict_config", true);
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(VaultModule)
        .module(BoxChestModule)
        .boot()
        .await
        .unwrap();
}

/// Registers the vault kind `darpan`, whose factory asks the app for the
/// vault it is being built for.
struct DarpanModule;

impl Module for DarpanModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_driver::<String, _, _>(m, &SLOT, Some("vault"), "darpan", |ctx| async move {
            ctx.app().get::<Vault>().await?;
            Ok(Arc::new("darpan".to_owned()))
        });
    }
}

/// The app the vault kind `pehredar` kept from its factory.
type Kept = Arc<Mutex<Option<AppContext>>>;

/// Registers the vault kind `pehredar`, whose factory keeps the app to ask
/// it for things once it runs.
struct PehredarModule(Kept);

impl Module for PehredarModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let kept = self.0.clone();
        register_driver::<String, _, _>(m, &SLOT, Some("vault"), "pehredar", move |ctx| {
            *kept.lock().unwrap() = Some(ctx.app().clone());
            async move { Ok(Arc::new("pehredar".to_owned())) }
        });
    }
}

#[tokio::test]
async fn a_factory_that_asks_for_the_service_it_is_built_for_fails_the_boot_naming_the_cycle() {
    let booting = boot(vault(json!({"kind": "darpan"})), |app| {
        app.module(VaultModule).module(DarpanModule)
    });
    let text = tokio::time::timeout(Duration::from_secs(10), booting)
        .await
        .expect("the boot fails instead of waiting");
    assert!(
        text.contains("vault kind `darpan` (registered with m.vault_kind in `DarpanModule`) failed to start: provider dependency cycle: "),
        "{text}"
    );
    assert!(text.contains("Vault -> "), "{text}");

    // A factory that keeps the app asks it once the app runs, and gets it.
    let kept = Kept::default();
    let app = Navree::app()
        .module(ConfigModule::for_root(vault(json!({"kind": "pehredar"}))))
        .module(VaultModule)
        .module(PehredarModule(kept.clone()))
        .boot()
        .await
        .unwrap();
    let handle = kept.lock().unwrap().clone().expect("the factory ran");
    let booted = tokio::time::timeout(Duration::from_secs(10), handle.get::<Vault>())
        .await
        .expect("a kept handle does not wait")
        .unwrap();
    assert_eq!(booted.0.as_deref().map(String::as_str), Some("pehredar"));
    drop(app);
}

/// A made-up section that sits below `vault` and is read by a module of its own
/// (`[vault.shelf]`), as `notify.push` sits below `notify`.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "vault.shelf")]
#[serde(default)]
struct ShelfOptions {
    /// How many boxes a shelf holds.
    #[allow(dead_code)]
    boxes: u32,
}

/// Registers the section `vault.shelf`.
struct ShelfModule;

impl Module for ShelfModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config_section::<ShelfOptions>();
    }
}

/// Registers the vault kind `shelf`, the name of the section `vault.shelf`.
struct ShelfChestModule;

impl Module for ShelfChestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register_chest(m, &SLOT, "shelf");
    }
}

#[tokio::test]
async fn a_driver_cannot_take_the_name_of_a_section_below_its_own() {
    let setting = "m.vault_kind(\"shelf\") in `ShelfChestModule`: `vault.shelf` is a setting of the vault section; register the vault kind under another name";
    let sources = || vault(json!({"kind": "memory"}));
    let add = |app: AppBuilder| {
        app.module(VaultModule)
            .module(ShelfModule)
            .module(ShelfChestModule)
    };
    let text = boot(sources(), add).await;
    assert!(text.contains(setting), "{text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {setting}")), "{text}");

    // Without the section below it, the name is free.
    let add = |app: AppBuilder| app.module(VaultModule).module(ShelfChestModule);
    let text = check_text(sources(), add).await;
    assert!(text.contains("config:check: ok ("), "{text}");
}
