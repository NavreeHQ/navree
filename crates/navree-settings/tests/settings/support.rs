//! Shared fixtures: the settings types of the docs, a `users` table, a
//! portal controller reading `Setting<T>`, and the test app builder.

#![allow(dead_code)]

use std::sync::Arc;

use garde::Validate;
use navree_access::{AccessModule, AccessOptions, AccessService};
use navree_audit::AuditModule;
use navree_cache::{CacheModule, CacheOptions};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_events::{EventBus, EventsModule};
use navree_http::Json;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{controller, injectable, module};
use navree_settings::{ModuleBuilderSettingsExt, Setting, Settings, SettingsModule};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_workspace::{
    NewOrganization, NewWorkspace, OrgMembers, OrgRole, Organization, Organizations, Workspace,
    WorkspaceModule, WorkspaceRole, WorkspaceSelector, Workspaces,
};
use sea_orm::sea_query::{ColumnDef, Query, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

/// What the member portal shows.
#[derive(navree_settings::Settings, Clone, Debug, PartialEq, Serialize, Deserialize, Validate)]
#[settings(key = "portal", scopes(global, organization, workspace, user))]
pub struct PortalSettings {
    #[garde(skip)]
    pub show_leaderboard: bool,
    #[garde(length(max = 40))]
    pub welcome_text: String,
    #[garde(range(min = 1, max = 100))]
    pub page_size: u32,
}

impl Default for PortalSettings {
    fn default() -> Self {
        PortalSettings {
            show_leaderboard: false,
            welcome_text: "Welcome".to_owned(),
            page_size: 25,
        }
    }
}

/// How notification mail is sent.
#[derive(
    navree_settings::Settings, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Validate,
)]
#[settings(
    key = "notifications",
    scopes(global, workspace),
    permission = "notifications.manage"
)]
pub struct NotificationSettings {
    #[garde(skip)]
    pub daily_digest: bool,
    #[garde(length(max = 60))]
    pub sender_name: String,
}

/// A user's view preferences.
#[derive(
    navree_settings::Settings, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Validate,
)]
#[settings(scopes(user))]
pub struct ViewPreferences {
    #[garde(skip)]
    pub compact: bool,
    #[garde(skip)]
    pub theme: String,
}

/// Settings no module registers.
#[derive(
    navree_settings::Settings, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Validate,
)]
#[settings(key = "forgotten")]
pub struct ForgottenSettings {
    #[garde(skip)]
    pub on: bool,
}

/// `users(id, name, email)`, as navree-auth creates it.
struct CreateUsers;

impl MigrationName for CreateUsers {
    fn name(&self) -> &str {
        "m20261027_000001_create_users"
    }
}

#[async_trait]
impl MigrationTrait for CreateUsers {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("users")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("email").string().not_null().unique_key())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("users").to_owned())
            .await
    }
}

/// The portal routes: what `Setting<T>` resolves for the request.
#[injectable]
pub struct PortalController;

#[controller("/portal")]
impl PortalController {
    #[get("/")]
    #[authenticated]
    async fn show(&self, portal: Setting<PortalSettings>) -> Json<Value> {
        Json(json!({
            "welcome": portal.welcome_text,
            "leaderboard": portal.show_leaderboard,
            "source": portal.source("welcome_text"),
            "user": portal.scope().user_id(),
            "workspace": portal.scope().workspace_id(),
            "organization": portal.scope().organization_id(),
        }))
    }

    #[get("/public")]
    #[public]
    async fn public(&self, portal: Setting<PortalSettings>) -> Json<Value> {
        Json(json!({
            "welcome": portal.welcome_text,
            "source": portal.source("welcome_text"),
        }))
    }
}

/// The portal module: registers its settings as an import.
#[module(
    imports = [SettingsModule::for_feature::<PortalSettings>()],
    controllers = [PortalController],
)]
pub struct PortalModule;

/// The notifications module: registers its settings on the builder.
pub struct NotificationsModule;

impl Module for NotificationsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<NotificationSettings>()
            .settings::<ViewPreferences>();
    }
}

/// The `users` table (as migration set `auth`).
pub struct FixturesModule;

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(CreateUsers))
            .global();
    }
}

/// The platform every test app has: database, cache, events, audit,
/// access, workspaces with organizations and the fixtures.
pub fn platform() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.organizations.enabled", true)
        .module(DatabaseModule::from_config())
        .module(CacheModule::for_root(CacheOptions::default()))
        .module(EventsModule::for_root())
        .module(AuditModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(FixturesModule)
        .module(WorkspaceModule::from_config())
}

/// The platform plus `SettingsModule::from_config()` and the feature
/// modules.
pub fn builder() -> TestAppBuilder {
    platform()
        .module(PortalModule)
        .module(NotificationsModule)
        .module(SettingsModule::from_config())
}

/// A booted test app.
pub struct World {
    pub app: TestApp,
}

impl World {
    pub async fn new() -> World {
        World::with(builder()).await
    }

    pub async fn with(builder: TestAppBuilder) -> World {
        World {
            app: builder.build().await,
        }
    }

    pub async fn settings(&self) -> Arc<Settings> {
        self.app.get::<Settings>().await
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    /// Inserts a user into `users`.
    pub async fn user(&self, name: &str) -> Uuid {
        let id = Uuid::now_v7();
        let email = format!("{}-{id}@navree.test", name.to_lowercase());
        self.db()
            .await
            .conn()
            .execute(
                Query::insert()
                    .into_table("users")
                    .columns(["id", "name", "email"])
                    .values_panic([id.into(), name.into(), email.into()]),
            )
            .await
            .unwrap();
        id
    }

    /// Runs `f` in a new unit of work, committed when it returns `Ok`.
    pub async fn work<T, E, F, Fut>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let uow = self.db().await.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await.unwrap();
                Ok(value)
            }
            Err(error) => {
                uow.rollback().await.unwrap();
                Err(error)
            }
        }
    }

    /// Creates workspace `name` owned by `owner`.
    pub async fn workspace(&self, owner: Uuid, name: &str) -> Workspace {
        let workspaces = self.app.get::<Workspaces>().await;
        let name = name.to_owned();
        self.work(|uow| async move {
            workspaces
                .create(&uow, owner, NewWorkspace::named(name))
                .await
        })
        .await
        .unwrap()
    }

    /// Creates organization `name` owned by `owner`.
    pub async fn organization(&self, owner: Uuid, name: &str) -> Organization {
        let organizations = self.app.get::<Organizations>().await;
        let name = name.to_owned();
        self.work(|uow| async move {
            organizations
                .create(&uow, owner, NewOrganization::named(name))
                .await
        })
        .await
        .unwrap()
    }

    /// Attaches `workspace` to `organization`.
    pub async fn attach(&self, organization: Uuid, workspace: Uuid) -> Workspace {
        let organizations = self.app.get::<Organizations>().await;
        self.work(|uow| async move { organizations.attach(&uow, organization, workspace).await })
            .await
            .unwrap()
    }

    /// Adds `user` to `workspace` with `role`.
    pub async fn member(&self, workspace: Uuid, user: Uuid, role: WorkspaceRole) {
        let members = self.app.get::<navree_workspace::Members>().await;
        self.work(|uow| async move { members.add(&uow, workspace, user, role).await })
            .await
            .unwrap();
    }

    /// Adds `user` to `organization` with `role`, reaching every workspace.
    pub async fn org_member(&self, organization: Uuid, user: Uuid, role: OrgRole) {
        let members = self.app.get::<OrgMembers>().await;
        self.work(|uow| async move {
            members
                .add(&uow, organization, user, role, WorkspaceSelector::All)
                .await
        })
        .await
        .unwrap();
    }

    /// Gives `user` permission `permission` directly.
    pub async fn grant(&self, user: Uuid, permission: &str) {
        let access = self.app.get::<AccessService>().await;
        let permission = permission.to_owned();
        self.work(|uow| async move { access.grant_to_user(&uow, user, &permission).await })
            .await
            .unwrap();
    }

    pub fn as_user(&self, user: Uuid) -> TestClient {
        self.app.acting_as(principal(user))
    }

    /// Records the JSON payloads of events `names`.
    pub async fn events(&self, names: &[&str]) -> Events {
        let bus = self.app.get::<EventBus>().await;
        Events {
            receivers: names
                .iter()
                .map(|name| ((*name).to_owned(), bus.subscribe_json(name)))
                .collect(),
        }
    }
}

/// JSON event receivers.
pub struct Events {
    receivers: Vec<(String, broadcast::Receiver<Value>)>,
}

impl Events {
    /// Every event received so far, as (name, payload), drained.
    pub fn drain(&mut self) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        for (name, receiver) in &mut self.receivers {
            while let Ok(payload) = receiver.try_recv() {
                out.push((name.clone(), payload));
            }
        }
        out
    }
}
