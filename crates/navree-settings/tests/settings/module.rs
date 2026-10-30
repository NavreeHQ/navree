//! `SettingsModule`: registration, configuration, the store choice, and
//! keeping stored values in step with workspaces and organizations.

use navree_kernel::Module;
use navree_settings::{
    ModuleBuilderSettingsExt, Scope, ScopeKind, ScopeTarget, Settings, SettingsModule,
    SettingsOptions, SettingsRegistry, SettingsStore, SettingsStoreKind,
};
use navree_testing::TestApp;
use navree_workspace::{Organizations, Workspaces};
use serde_json::json;

use crate::support::{PortalSettings, World, builder, platform};

/// Another type that claims `portal`.
mod other {
    /// Another portal.
    #[derive(
        navree_settings::Settings,
        Clone,
        Debug,
        Default,
        PartialEq,
        serde::Serialize,
        serde::Deserialize,
        navree_validation::Validate,
    )]
    #[settings(key = "portal")]
    pub struct PortalSettings {
        #[validate(skip)]
        pub colour: String,
    }
}

/// Registers the clashing type.
struct ClashModule;

impl Module for ClashModule {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        m.settings::<other::PortalSettings>();
    }
}

#[tokio::test]
async fn every_registration_is_in_the_registry() {
    let world = World::new().await;
    let registry = world.app.get::<SettingsRegistry>().await;
    let keys: Vec<&str> = registry
        .all()
        .iter()
        .map(|entry| entry.definition.key)
        .collect();
    // `notification-templates` comes with the `notify` feature, on in the
    // tests.
    assert_eq!(
        keys,
        [
            "notification-templates",
            "notifications",
            "portal",
            "view-preferences"
        ]
    );
    let portal = registry.get("portal").unwrap();
    assert_eq!(
        portal.definition.levels(),
        [
            ScopeKind::Global,
            ScopeKind::Organization,
            ScopeKind::Workspace,
            ScopeKind::User
        ]
    );
    assert!(portal.has_field("welcome_text"));
    assert_eq!(
        serde_json::Value::Object(portal.defaults.clone()),
        json!({"show_leaderboard": false, "welcome_text": "Welcome", "page_size": 25})
    );
    assert!(!registry.contains("forgotten"));
}

#[tokio::test]
async fn two_types_with_one_key_fail_the_boot() {
    let error = match builder().module(ClashModule).try_build().await {
        Ok(_) => panic!("the boot should fail"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("two settings types use the key `portal`")
            && error.contains("support::PortalSettings")
            && error.contains("other::PortalSettings"),
        "{error}"
    );
}

#[tokio::test]
async fn options_come_from_the_settings_section() {
    let world = World::with(
        builder()
            .config("settings.store", "memory")
            .config("settings.cache_ttl_secs", 60),
    )
    .await;
    let options = world.app.get::<SettingsOptions>().await;
    assert_eq!(options.store, SettingsStoreKind::Memory);
    assert_eq!(options.cache_ttl_secs, 60);
    assert!(options.routes);

    // The memory store works and leaves the table empty.
    let settings = world.settings().await;
    world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<PortalSettings>(&uow, Scope::global(), json!({"page_size": 30}))
                    .await
            }
        })
        .await
        .unwrap();
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::global())
            .await
            .unwrap()
            .page_size,
        30
    );
    let store = world.app.get::<dyn SettingsStore>().await;
    assert_eq!(store.list(None, None).await.unwrap().len(), 1);
    assert!(
        navree_settings::DatabaseSettingsStore::new(world.db().await)
            .list(None, None)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn for_root_takes_explicit_options() {
    let world = World::with(platform().module(crate::support::PortalModule).module(
        SettingsModule::for_root(SettingsOptions {
            store: SettingsStoreKind::Memory,
            cache_ttl_secs: 0,
            routes: false,
        }),
    ))
    .await;
    let options = world.app.get::<SettingsOptions>().await;
    assert_eq!(options.store, SettingsStoreKind::Memory);
    assert_eq!(options.cache_ttl(), None);
}

#[tokio::test]
async fn the_database_store_needs_the_database_module() {
    let error = match TestApp::builder()
        .module(SettingsModule::from_config())
        .try_build()
        .await
    {
        Ok(app) => match app.get_settings().await {
            Ok(()) => panic!("resolving Settings should fail"),
            Err(error) => error,
        },
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("settings.store = \"database\" needs DatabaseModule"),
        "{error}"
    );
}

/// Resolves `Settings` from an app, as a string error.
trait GetSettings {
    async fn get_settings(&self) -> Result<(), String>;
}

impl GetSettings for TestApp {
    async fn get_settings(&self) -> Result<(), String> {
        self.ctx()
            .get::<Settings>()
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[tokio::test]
async fn deleted_workspaces_and_organizations_lose_their_values() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(ada, "Globex").await;
    world.attach(holding.id, acme.id).await;
    for (scope, patch) in [
        (
            Scope::organization(holding.id),
            json!({"show_leaderboard": true}),
        ),
        (
            Scope::workspace(acme.id),
            json!({"welcome_text": "Hi Acme"}),
        ),
        (
            Scope::workspace(globex.id),
            json!({"welcome_text": "Hi Globex"}),
        ),
    ] {
        let settings = settings.clone();
        world
            .work(|uow| async move { settings.update::<PortalSettings>(&uow, scope, patch).await })
            .await
            .unwrap();
    }
    // Cached before the deletes.
    assert!(
        settings
            .get::<PortalSettings>(Scope::workspace(acme.id))
            .await
            .unwrap()
            .show_leaderboard
    );

    let workspaces = world.app.get::<Workspaces>().await;
    world
        .work(|uow| async move { workspaces.delete(&uow, globex.id).await })
        .await
        .unwrap();
    assert!(
        settings
            .stored::<PortalSettings>(Scope::workspace(globex.id))
            .await
            .unwrap()
            .is_empty()
    );

    let organizations = world.app.get::<Organizations>().await;
    world
        .work(|uow| async move { organizations.delete(&uow, holding.id).await })
        .await
        .unwrap();
    assert!(
        settings
            .stored::<PortalSettings>(Scope::organization(holding.id))
            .await
            .unwrap()
            .is_empty()
    );
    // Acme was detached: its organization's values no longer apply.
    let portal = settings
        .resolve::<PortalSettings>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert!(!portal.show_leaderboard);
    assert_eq!(portal.welcome_text, "Hi Acme");

    // forget_scope reports the keys it removed.
    assert_eq!(
        settings
            .forget_scope(ScopeTarget::new(ScopeKind::Workspace, acme.id))
            .await
            .unwrap(),
        ["portal"]
    );
}
