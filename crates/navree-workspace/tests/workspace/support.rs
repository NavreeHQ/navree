//! Shared fixtures: a `users` table for the user directory, a `deals`
//! entity owned by workspaces, an abilities function, event recording and
//! the test app builder.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_contracts::{Clock, FixedClock, Principal};
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use navree_db::{
    DatabaseModule, Db, Executor, MigrationSet, ModuleBuilderDbExt, Statement, Uow, Value,
};
use navree_events::{EventBus, EventsModule};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, Mailer, Message, TransportConfig};
use navree_testing::{TestApp, TestAppBuilder, principal};
use navree_view::{ViewModule, ViewOptions};
use navree_workspace::{
    AccessRequests, Invitations, Members, ModuleBuilderWorkspaceExt, NewOrganization, NewPartition,
    NewWorkspace, OrgMembers, OrgRole, Organization, Organizations, Partition, PartitionScoped,
    PartitionSelector, Partitions, Workspace, WorkspaceModule, WorkspaceScoped, WorkspaceSelector,
    Workspaces,
};
use tokio::sync::broadcast;
use uuid::Uuid;

/// The global role that passes every check.
pub const SUPER_ADMIN: &str = "super-admin";

/// When every test app's clock starts.
pub fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap()
}

/// Creates table `.0` with the columns `.1` adds (given the database's
/// dialect); `down` drops it.
pub struct Create(pub &'static str, pub fn(&mut Blueprint, Dialect));

impl Migration for Create {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        let columns = self.1;
        schema.create(self.0, |t| columns(t, dialect)).await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(self.0).await
    }
}

/// A UUID column: `BINARY(16)` on MySQL (how navree-db writes UUIDs
/// there), `uuid` elsewhere.
pub fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// `users(id, name, email)`, as navree-auth creates it.
fn users(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    t.string("name");
    t.string("email").unique();
}

/// `deals(id, workspace_id, title)`: a workspace-owned table.
fn deals(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    uuid(t, "workspace_id", dialect);
    t.string("title");
}

/// `contacts(id, workspace_id, partition_id, name)`: a table owned by
/// workspaces and sliced by partitions.
fn contacts(t: &mut Blueprint, dialect: Dialect) {
    uuid(t, "id", dialect).primary();
    uuid(t, "workspace_id", dialect);
    uuid(t, "partition_id", dialect).nullable();
    t.string("name");
}

/// The `contacts` model: the access subject `Contact`.
pub mod contact {
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "contacts", subject = "Contact", permission_prefix = "contacts")]
    pub struct Model {
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub partition_id: Option<Uuid>,
        pub name: String,
    }
}

/// A contact.
pub type Contact = contact::Model;

impl WorkspaceScoped for Contact {}

impl PartitionScoped for Contact {}

/// The `deals` model: the access subject `Deal`.
pub mod deal {
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
    pub struct Model {
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub title: String,
    }
}

/// A deal.
pub type Deal = deal::Model;

impl WorkspaceScoped for Deal {}

/// A deal of `workspace` titled `title`, with a new id.
pub fn new_deal(workspace: Uuid, title: &str) -> Deal {
    Deal {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        title: title.to_owned(),
    }
}

/// The fixtures module: the `users` and `deals` tables, deals as a
/// workspace-scoped subject.
pub struct FixturesModule;

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(SchemaMigration::new(
            "m20261026_000001_create_users",
            Create("users", users),
        )))
        .migrations(
            MigrationSet::new("deals")
                .after(["workspace"])
                .migration(SchemaMigration::new(
                    "m20261026_000002_create_deals",
                    Create("deals", deals),
                ))
                .migration(SchemaMigration::new(
                    "m20261026_000003_create_contacts",
                    Create("contacts", contacts),
                )),
        )
        .import(AccessModule::for_feature::<Deal>())
        .import(AccessModule::for_feature::<Contact>())
        .workspace_scoped::<Deal>();
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
    crate::database::database(TestApp::builder())
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
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
            ..MailOptions::default()
        }))
        .module(AccessModule::for_root(access()))
        .module(FixturesModule)
        .module(WorkspaceModule::from_config())
}

/// [`builder`] with organizations and partitions on, and an app-defined
/// organization role `partner` that gives the workspace role `viewer`.
pub fn org_builder() -> TestAppBuilder {
    builder()
        .config("workspace.organizations.enabled", true)
        .config("workspace.partitions.enabled", true)
        .config(
            "workspace.organizations.roles.partner.permissions",
            serde_json::json!(["organization.read", "reports.read"]),
        )
        .config(
            "workspace.organizations.roles.partner.workspace_role",
            "viewer",
        )
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

    pub async fn organizations(&self) -> Arc<Organizations> {
        self.app.get::<Organizations>().await
    }

    pub async fn org_members(&self) -> Arc<OrgMembers> {
        self.app.get::<OrgMembers>().await
    }

    pub async fn access_requests(&self) -> Arc<AccessRequests> {
        self.app.get::<AccessRequests>().await
    }

    pub async fn partitions(&self) -> Arc<Partitions> {
        self.app.get::<Partitions>().await
    }

    /// Creates organization `name` owned by `owner`.
    pub async fn organization(&self, owner: Uuid, name: &str) -> Organization {
        let organizations = self.organizations().await;
        let name = name.to_owned();
        self.work(|uow| async move {
            organizations
                .create(&uow, owner, NewOrganization::named(name))
                .await
        })
        .await
        .unwrap()
    }

    /// Attaches `workspace` to `org`.
    pub async fn attach(&self, org: Uuid, workspace: Uuid) {
        let organizations = self.organizations().await;
        self.work(|uow| async move { organizations.attach(&uow, org, workspace).await })
            .await
            .unwrap();
    }

    /// Gives `user` organization role `role` in `org`, reaching `workspaces`.
    pub async fn grant(&self, org: Uuid, user: Uuid, role: &str, workspaces: WorkspaceSelector) {
        let members = self.org_members().await;
        let role = OrgRole::parse(role);
        self.work(|uow| async move { members.grant(&uow, org, user, role, workspaces).await })
            .await
            .unwrap();
    }

    /// Creates partition `name` in `workspace`.
    pub async fn partition(&self, workspace: Uuid, name: &str) -> Partition {
        let partitions = self.partitions().await;
        let name = name.to_owned();
        self.work(|uow| async move {
            partitions
                .create(&uow, workspace, NewPartition::named(name))
                .await
        })
        .await
        .unwrap()
    }

    /// Limits `user` in `workspace` to `partitions`.
    pub async fn limit_partitions(
        &self,
        workspace: Uuid,
        user: Uuid,
        partitions: PartitionSelector,
    ) {
        let service = self.partitions().await;
        self.work(|uow| async move { service.grant(&uow, workspace, user, partitions).await })
            .await
            .unwrap();
    }

    /// Inserts a user into `users`.
    pub async fn user(&self, name: &str, email: &str) -> Uuid {
        let id = Uuid::now_v7();
        self.db()
            .await
            .execute(&Statement::portable(
                "INSERT INTO users (id, name, email) VALUES (?, ?, ?)",
                [Value::from(id), Value::from(name), Value::from(email)],
            ))
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
    receivers: Vec<(String, broadcast::Receiver<serde_json::Value>)>,
}

impl Events {
    /// Every event received so far, as (name, payload), drained.
    pub fn drain(&mut self) -> Vec<(String, serde_json::Value)> {
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
