//! Shared fixtures: a `users` table for the user directory, a `deals`
//! entity owned by workspaces, an abilities function, event recording and
//! the test app builder.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules, Subject};
use navree_contracts::{Clock, FixedClock, Principal};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_events::{EventBus, EventsModule};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, Mailer, Message, TransportConfig};
use navree_testing::{TestApp, TestAppBuilder, principal};
use navree_view::{ViewModule, ViewOptions};
use navree_workspace::{
    Invitations, Members, ModuleBuilderWorkspaceExt, NewWorkspace, Workspace, WorkspaceModule,
    WorkspaceScoped, Workspaces,
};
use sea_orm::sea_query::{ColumnDef, Query, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::Value;
use tokio::sync::broadcast;
use uuid::Uuid;

/// The global role that passes every check.
pub const SUPER_ADMIN: &str = "super-admin";

/// When every test app's clock starts.
pub fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap()
}

/// `users(id, name, email)`, as navree-auth creates it.
struct CreateUsers;

impl MigrationName for CreateUsers {
    fn name(&self) -> &str {
        "m20261026_000001_create_users"
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

/// `deals(id, workspace_id, title)`: a workspace-owned table.
struct CreateDeals;

impl MigrationName for CreateDeals {
    fn name(&self) -> &str {
        "m20261026_000002_create_deals"
    }
}

#[async_trait]
impl MigrationTrait for CreateDeals {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("deals")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("title").string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("deals").to_owned())
            .await
    }
}

/// The `deals` entity.
pub mod deal {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "deals")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub title: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

impl WorkspaceScoped for deal::Entity {}

/// The fixtures module: the `users` and `deals` tables, deals as a
/// workspace-scoped subject.
pub struct FixturesModule;

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(CreateUsers))
            .migrations(
                MigrationSet::new("deals")
                    .after(["workspace"])
                    .migration(CreateDeals),
            )
            .import(AccessModule::for_feature::<deal::Entity>())
            .workspace_scoped::<deal::Entity>();
    }
}

/// The app's rules: the permissions (global, or the workspace role's inside
/// a workspace), plus `read Report` for anyone inside a workspace.
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.team_id.is_some() {
        rules.can("read", "Report");
    }
}

/// The access options of the test apps.
pub fn access() -> AccessOptions {
    AccessOptions {
        abilities,
        super_admin_role: Some(SUPER_ADMIN.to_owned()),
    }
}

/// A test app: database, events, mail (array transport), views, access and
/// workspaces with `config` on top, a fixed clock and the fixtures.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config(
            "workspace.roles.member.permissions",
            serde_json::json!(["deals.*"]),
        )
        .config(
            "workspace.roles.viewer.permissions",
            serde_json::json!(["deals.read"]),
        )
        .config(
            "workspace.invitations.url",
            "https://app.test/invitations/accept",
        )
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailModule::for_root(MailOptions {
            from: "Navree <hello@navree.test>".into(),
            transport: TransportConfig::Array,
            queue: None,
        }))
        .module(AccessModule::for_root(access()))
        .module(FixturesModule)
        .module(WorkspaceModule::from_config())
}

/// A booted test app with its clock.
pub struct World {
    pub app: TestApp,
    pub clock: Arc<FixedClock>,
}

impl World {
    pub async fn new() -> World {
        World::with(builder()).await
    }

    pub async fn with(builder: TestAppBuilder) -> World {
        let clock = Arc::new(FixedClock::new(start()));
        let app = builder
            .override_value::<dyn Clock>(clock.clone())
            .build()
            .await;
        World { app, clock }
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    pub async fn workspaces(&self) -> Arc<Workspaces> {
        self.app.get::<Workspaces>().await
    }

    pub async fn members(&self) -> Arc<Members> {
        self.app.get::<Members>().await
    }

    pub async fn invitations(&self) -> Arc<Invitations> {
        self.app.get::<Invitations>().await
    }

    /// Inserts a user into `users`.
    pub async fn user(&self, name: &str, email: &str) -> Uuid {
        let id = Uuid::now_v7();
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

    /// Runs `f` in a new unit of work and commits it (rolls back on error).
    pub async fn work<T, F, Fut>(&self, f: F) -> Result<T, navree_workspace::WorkspaceError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: std::future::Future<Output = Result<T, navree_workspace::WorkspaceError>>,
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
        let workspaces = self.workspaces().await;
        let name = name.to_owned();
        self.work(|uow| async move {
            workspaces
                .create(&uow, owner, NewWorkspace::named(name))
                .await
        })
        .await
        .unwrap()
    }

    /// Adds `user` to `workspace` with `role`.
    pub async fn join(&self, workspace: Uuid, user: Uuid, role: &str) {
        let members = self.members().await;
        let role = role.into();
        self.work(|uow| async move { members.add(&uow, workspace, user, role).await })
            .await
            .unwrap();
    }

    /// Every mail sent so far.
    pub async fn mails(&self) -> Vec<Message> {
        self.app
            .get::<Mailer>()
            .await
            .array_transport()
            .expect("array transport")
            .sent()
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

    pub fn as_user(&self, user: Uuid) -> navree_testing::TestClient {
        self.app.acting_as(principal(user))
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

    /// The names of the events received so far, drained.
    pub fn names(&mut self) -> Vec<String> {
        self.drain().into_iter().map(|(name, _)| name).collect()
    }
}

/// A principal for `user`.
pub fn signed_in(user: Uuid) -> Principal {
    principal(user)
}
