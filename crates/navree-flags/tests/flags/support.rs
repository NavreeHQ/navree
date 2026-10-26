//! Shared fixtures: the flags of the docs, a `users` table, a deals
//! controller behind flags, a plan resolver and the test app builder.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::Path;
use navree_access::{AccessModule, AccessOptions, AccessService};
use navree_audit::AuditModule;
use navree_cache::{CacheModule, CacheOptions};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_events::{EventBus, EventsModule};
use navree_flags::{CurrentFlags, Flags, FlagsModule, ModuleBuilderFlagsExt, PlanResolver};
use navree_http::{AppError, Json};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use navree_macros::{controller, flag, injectable, module};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_workspace::{NewWorkspace, Workspace, WorkspaceModule, Workspaces};
use sea_orm::sea_query::{ColumnDef, Query, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

/// The redesigned deal pipeline board with drag-and-drop between stages.
#[flag]
pub struct NewPipeline;

/// AI summaries on the deal page.
#[flag(name = "ai-summaries", default = false, expose)]
pub struct AiSummaries;

/// The CSV export of deals; on unless someone turns it off.
#[flag(default = true)]
pub struct DealExport;

/// A flag no module registers.
#[flag]
pub struct Forgotten;

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

/// The plans of workspaces, set by tests.
#[injectable]
pub struct TestPlans {
    #[inject(default)]
    plans: Mutex<HashMap<Uuid, String>>,
    #[inject(default)]
    lookups: Mutex<u32>,
}

impl TestPlans {
    pub fn set(&self, workspace: Uuid, plan: &str) {
        self.plans
            .lock()
            .unwrap()
            .insert(workspace, plan.to_owned());
    }

    pub fn lookups(&self) -> u32 {
        *self.lookups.lock().unwrap()
    }
}

impl PlanResolver for TestPlans {
    fn plan(&self, workspace: Uuid) -> BoxFuture<'_, Result<Option<String>, BoxError>> {
        *self.lookups.lock().unwrap() += 1;
        let plan = self.plans.lock().unwrap().get(&workspace).cloned();
        Box::pin(async move { Ok(plan) })
    }
}

/// The deals routes behind flags.
#[injectable]
pub struct DealsController;

#[controller("/deals", version = "1")]
impl DealsController {
    #[get("/board")]
    #[authenticated]
    async fn board(&self, flags: CurrentFlags) -> Result<Json<Value>, AppError> {
        let v2 = flags.active::<NewPipeline>().await?;
        // The second check is memoized.
        let again = flags.active::<NewPipeline>().await?;
        assert_eq!(v2, again);
        Ok(Json(json!({
            "board": if v2 { "v2" } else { "v1" },
            "user": flags.scope().user_id(),
            "workspace": flags.scope().workspace_id(),
        })))
    }

    #[get("/forecast")]
    #[authenticated]
    #[requires_flag(NewPipeline)]
    async fn forecast(&self) -> Json<Value> {
        Json(json!({"currency": "USD"}))
    }

    #[post("/forecast/{id}")]
    #[authenticated]
    #[requires_flag(NewPipeline)]
    async fn rerun(&self, Path(id): Path<Uuid>, Json(body): Json<Value>) -> Json<Value> {
        Json(json!({"id": id, "body": body}))
    }

    #[get("/summary")]
    #[authenticated]
    #[requires_flag(AiSummaries, status = 403)]
    async fn summary(&self) -> Json<Value> {
        Json(json!({"summary": "..."}))
    }

    #[get("/managed-forecast")]
    #[permissions("deals.manage")]
    #[requires_flag(NewPipeline)]
    async fn managed_forecast(&self) -> Json<Value> {
        Json(json!({"managed": true}))
    }

    #[get("/export")]
    #[authenticated]
    #[requires_flag(DealExport)]
    async fn export(&self) -> &'static str {
        "id,title"
    }
}

/// The deals module: its flags and routes.
#[module(
    imports = [
        FlagsModule::for_feature::<NewPipeline>(),
        FlagsModule::for_feature::<AiSummaries>(),
        FlagsModule::for_feature::<DealExport>(),
    ],
    controllers = [DealsController],
)]
pub struct DealsModule;

/// The `users` table (as migration set `auth`) and the plan resolver.
pub struct FixturesModule;

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(CreateUsers))
            .plan_resolver::<TestPlans>()
            .exports::<TestPlans>()
            .global();
    }
}

/// The platform every test app has: database, cache, events, audit,
/// access, workspaces and the fixtures.
pub fn platform() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule::from_config())
        .module(CacheModule::for_root(CacheOptions::default()))
        .module(EventsModule::for_root())
        .module(AuditModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(FixturesModule)
        .module(WorkspaceModule::from_config())
}

/// The platform plus `FlagsModule::from_config()` and the deals module.
pub fn builder() -> TestAppBuilder {
    platform()
        .module(DealsModule)
        .module(FlagsModule::from_config())
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

    pub async fn flags(&self) -> Arc<Flags> {
        self.app.get::<Flags>().await
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    pub async fn plans(&self) -> Arc<TestPlans> {
        self.app.get::<TestPlans>().await
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
