//! The examples of `content/docs/packages/settings.mdx`, written as an app
//! writes them (the facade, the prelude and `#[derive(Settings)]`), compiled
//! and run.
#![cfg(feature = "sqlite")]

use std::sync::Mutex;

use navree::BoxFuture;
use navree::db::Db;
use navree::events::Listener;
use navree::prelude::*;
use navree::settings::{
    Scope, ScopeKind, Settings, SettingsChanged, SettingsError, SettingsFake, Source,
};
use navree::testing::{TestApp, principal};
use navree::validation::Validate;
use navree::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Installation: the module in the app root.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(DatabaseModule::from_config())
        .module(CacheModule::from_config())
        .module(EventsModule::for_root())
        .module(AuditModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AuthModule::from_config())
        .module(WorkspaceModule::from_config())
        .module(PortalModule)
        .module(SettingsModule::from_config()) // section `settings`
}

// ---------------------------------------------------------------------------
// Defining settings.
// ---------------------------------------------------------------------------

/// What the member portal shows.
#[derive(navree::settings::Settings, Clone, Debug, Serialize, Deserialize, Validate)]
#[settings(key = "portal", scopes(global, organization, workspace, user))]
pub struct PortalSettings {
    /// Whether the leaderboard is on the home page.
    #[validate(skip)]
    pub show_leaderboard: bool,
    /// The greeting on the home page.
    #[validate(length(max = 200))]
    pub welcome_text: String,
    /// Rows per page.
    #[validate(range(min = 1, max = 100))]
    pub page_size: u32,
}

impl Default for PortalSettings {
    fn default() -> Self {
        Self {
            show_leaderboard: false,
            welcome_text: "Welcome".to_owned(),
            page_size: 25,
        }
    }
}

/// How notification mail is sent.
#[derive(navree::settings::Settings, Clone, Debug, Default, Serialize, Deserialize, Validate)]
#[settings(key = "notifications", scopes(global, workspace))]
pub struct NotificationSettings {
    /// Whether members get a daily digest.
    #[validate(skip)]
    pub daily_digest: bool,
}

/// The portal module.
#[module(
    imports = [SettingsModule::for_feature::<PortalSettings>()],
    providers = [PortalService],
    controllers = [PortalController],
)]
pub struct PortalModule;

// ---------------------------------------------------------------------------
// Reading settings.
// ---------------------------------------------------------------------------

/// The portal's home.
#[derive(Serialize)]
pub struct PortalHome {
    welcome: String,
    leaderboard: bool,
}

/// The portal routes.
#[injectable]
pub struct PortalController;

#[controller("/portal")]
impl PortalController {
    #[get("/home")]
    #[authenticated]
    async fn home(&self, portal: Setting<PortalSettings>) -> Json<PortalHome> {
        Json(PortalHome {
            welcome: portal.welcome_text.clone(),
            leaderboard: portal.show_leaderboard,
        })
    }
}

/// Reads the portal's settings.
#[injectable]
pub struct PortalService {
    settings: Arc<Settings>,
}

impl PortalService {
    /// The workspace's welcome text, or a greeting naming it.
    pub async fn welcome(&self, workspace: &Workspace) -> Result<String, SettingsError> {
        let portal = self.settings.resolve::<PortalSettings>(workspace).await?;
        if portal.source("welcome_text") == Some(Source::Default) {
            return Ok(format!("Welcome to {}", workspace.name));
        }
        Ok(portal.welcome_text.clone())
    }

    /// Whether the workspace shows the leaderboard.
    pub async fn leaderboard(&self, workspace: Uuid) -> Result<bool, SettingsError> {
        let portal = self
            .settings
            .get::<PortalSettings>(Scope::workspace(workspace))
            .await?;
        Ok(portal.show_leaderboard)
    }

    /// The user's page size.
    pub async fn page_size(&self, user: Uuid) -> Result<u32, SettingsError> {
        Ok(self
            .settings
            .get::<PortalSettings>(Scope::user(user))
            .await?
            .page_size)
    }
}

#[test]
fn scopes_name_their_levels() {
    let (holding, acme, ada) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    assert_eq!(Scope::global().target().kind, ScopeKind::Global);
    assert_eq!(
        Scope::organization(holding).target().kind,
        ScopeKind::Organization
    );
    assert_eq!(Scope::workspace(acme).target().kind, ScopeKind::Workspace);
    assert_eq!(
        Scope::user(ada).in_workspace(acme).target().kind,
        ScopeKind::User
    );
    assert_eq!(
        Scope::workspace(acme)
            .in_organization(holding)
            .organization_id(),
        Some(holding)
    );
}

// ---------------------------------------------------------------------------
// Changing settings: a listener of `settings.changed`.
// ---------------------------------------------------------------------------

/// A cache of rendered portal pages, keyed by level.
#[injectable]
pub struct PortalCache {
    #[inject(default)]
    forgotten: Mutex<Vec<(ScopeKind, Option<Uuid>)>>,
}

impl PortalCache {
    async fn forget(&self, scope_type: ScopeKind, scope_id: Option<Uuid>) {
        self.forgotten.lock().unwrap().push((scope_type, scope_id));
    }
}

impl Listener<SettingsChanged> for PortalCache {
    fn handle<'a>(
        &'a self,
        event: &'a SettingsChanged,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if event.key == "portal" {
                self.forget(event.scope_type, event.scope_id).await;
            }
            Ok(())
        })
    }
}

/// Registers the listener.
pub struct PortalCacheModule;

impl Module for PortalCacheModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<PortalCache>()
            .exports::<PortalCache>()
            .on::<SettingsChanged, PortalCache>()
            .global();
    }
}

#[tokio::test]
async fn writes_resolve_inherit_and_announce() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(PortalModule)
        .module(PortalCacheModule)
        .module(SettingsModule::from_config())
        .build()
        .await;
    let settings = app.get::<Settings>().await;
    let db = app.get::<Db>().await;
    let acme = Uuid::now_v7();

    let uow = db.uow(None, None).await;
    settings
        .update::<PortalSettings>(&uow, Scope::global(), json!({"page_size": 50}))
        .await
        .unwrap();
    settings
        .update::<PortalSettings>(
            &uow,
            Scope::workspace(acme),
            json!({"welcome_text": "Welcome to Acme"}),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let portal = settings
        .resolve::<PortalSettings>(Scope::workspace(acme))
        .await
        .unwrap();
    assert_eq!(portal.page_size, 50);
    assert_eq!(portal.source("page_size"), Some(Source::Global));
    assert_eq!(portal.source("welcome_text"), Some(Source::Workspace));
    assert_eq!(portal.overridden, ["welcome_text"]);

    let uow = db.uow(None, None).await;
    settings
        .inherit::<PortalSettings>(&uow, Scope::workspace(acme), "welcome_text")
        .await
        .unwrap();
    settings
        .reset::<PortalSettings>(&uow, Scope::workspace(acme))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::workspace(acme))
            .await
            .unwrap()
            .welcome_text,
        "Welcome"
    );

    let error = {
        let uow = db.uow(None, None).await;
        let error = settings
            .update::<PortalSettings>(&uow, Scope::global(), json!({"page_size": 0}))
            .await
            .unwrap_err();
        uow.rollback().await.unwrap();
        error
    };
    let body = error.to_app_error().unwrap().body();
    assert_eq!(
        body["errors"]["page_size"],
        json!(["page_size must not be less than 1"])
    );

    let cache = app.get::<PortalCache>().await;
    assert_eq!(
        *cache.forgotten.lock().unwrap(),
        [
            (ScopeKind::Global, None),
            (ScopeKind::Workspace, Some(acme)),
            (ScopeKind::Workspace, Some(acme)),
        ]
    );

    let service = app.get::<PortalService>().await;
    assert_eq!(service.page_size(Uuid::now_v7()).await.unwrap(), 50);
}

// ---------------------------------------------------------------------------
// Testing.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_home_page_shows_the_workspace_welcome() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(PortalModule)
        .module(SettingsModule::from_config())
        .build()
        .await;
    let settings = app.get::<Settings>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    settings
        .update::<PortalSettings>(&uow, Scope::global(), json!({"welcome_text": "Hello"}))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    app.acting_as(principal(Uuid::now_v7()))
        .get("/api/portal/home")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("welcome", json!("Hello"));
}

#[tokio::test]
async fn the_leaderboard_follows_the_organization() {
    let (acme, holding) = (Uuid::now_v7(), Uuid::now_v7());
    let fake = SettingsFake::new().attach(acme, holding);
    fake.set::<PortalSettings>(
        Scope::organization(holding),
        json!({"show_leaderboard": true}),
    )
    .await
    .unwrap();

    let service = PortalService {
        settings: Arc::new(fake.settings()),
    };
    assert!(service.leaderboard(acme).await.unwrap());
}

#[tokio::test]
async fn the_fake_records_what_the_app_writes() {
    let fake = SettingsFake::new().register::<PortalSettings>();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(PortalModule)
        .module(SettingsModule::from_config())
        .override_value(Arc::new(fake.settings()))
        .build()
        .await;
    let ada = Uuid::now_v7();
    app.acting_as(principal(ada))
        .patch("/api/settings/portal?scope=user")
        .json(&json!({"page_size": 10}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("sources.page_size", json!("user"));
    fake.assert_changed::<PortalSettings>(Scope::user(ada));
    fake.assert_not_changed::<NotificationSettings>();
}
