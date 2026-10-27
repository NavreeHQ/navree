//! The examples of `content/docs/workspaces/demo-workspaces.mdx`, written
//! as an app writes them (the facade, the prelude and the macros),
//! compiled and run.
#![cfg(all(feature = "workspace", feature = "testing"))]

use chrono::{DateTime, Utc};
use navree::BoxFuture;
use navree::db::Db;
use navree::schedule::{ScheduleModule, Scheduler};
use navree::testing::{TempDirGuard, TestApp, TestAppBuilder, principal};
use navree::workspace::{
    DEMO_HEADER, DemoMode, DemoSeed, DemoSeeder, DemoWorkspaces, NewWorkspace, WorkspaceKind,
};
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{DbErr, EntityTrait};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// The CRM module: a workspace-owned `companies` table and its demo seeder.
// ---------------------------------------------------------------------------

pub mod company {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "companies")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub name: String,
        pub stage: String,
        pub last_contacted_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl Subject for company::Entity {
    const SUBJECT: &'static str = "Company";
    const PERMISSION_PREFIX: &'static str = "companies";
}

impl WorkspaceScoped for company::Entity {}

pub type Company = company::Entity;

struct CreateCompanies;

impl MigrationName for CreateCompanies {
    fn name(&self) -> &str {
        "m20261027_000001_create_companies"
    }
}

#[async_trait]
impl MigrationTrait for CreateCompanies {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("companies")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("stage").string().not_null())
                    .col(
                        ColumnDef::new("last_contacted_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("companies").to_owned())
            .await
    }
}

/// One company of `fixtures/demo/crm/companies.json`.
#[derive(Deserialize)]
struct CompanyFixture {
    name: String,
    stage: String,
    last_contacted_at: DateTime<Utc>,
}

/// Fills a demo workspace with the CRM's sample companies.
#[injectable]
pub struct CrmDemo;

impl DemoSeeder for CrmDemo {
    fn name(&self) -> &'static str {
        "crm"
    }

    // Emptied (the demo's rows only) before a reset seeds again.
    fn tables(&self) -> &'static [&'static str] {
        &["companies"]
    }

    fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let companies: Vec<CompanyFixture> = demo.fixture("crm/companies")?;
            for fixture in companies {
                WorkspaceRepo::<Company>::insert(
                    demo.uow(),
                    demo.id(),
                    company::ActiveModel {
                        id: Set(Uuid::now_v7()),
                        workspace_id: Set(demo.id()),
                        name: Set(fixture.name),
                        stage: Set(fixture.stage),
                        last_contacted_at: Set(fixture.last_contacted_at),
                    },
                )
                .await?;
            }
            Ok(())
        })
    }
}

#[injectable]
pub struct CompaniesController;

#[controller("/companies", version = "1")]
#[guard(InWorkspace::guard())]
impl CompaniesController {
    #[get("/")]
    #[permissions("companies.read")]
    async fn index(&self, ws: CurrentWorkspace, uow: Uow) -> Result<Json<Vec<company::Model>>> {
        let select = WorkspaceRepo::<Company>::select(ws.id());
        Ok(Json(select.all(&*uow.tx().await?).await?))
    }

    #[post("/")]
    #[permissions("companies.create")]
    async fn store(&self, ws: CurrentWorkspace, uow: Uow) -> Result<Json<company::Model>> {
        let created = WorkspaceRepo::<Company>::insert(
            &uow,
            ws.id(),
            company::ActiveModel {
                id: Set(Uuid::now_v7()),
                workspace_id: Set(ws.id()),
                name: Set("Prospect's own".to_owned()),
                stage: Set("lead".to_owned()),
                last_contacted_at: Set(Utc::now()),
            },
        )
        .await?;
        Ok(Json(created))
    }
}

#[module(
    imports = [AccessModule::for_feature::<Company>()],
    controllers = [CompaniesController],
    register = Self::wire,
)]
pub struct CrmModule;

impl CrmModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("companies")
                .after(["workspace"])
                .migration(CreateCompanies),
        )
        .workspace_scoped::<Company>()
        .demo_seeder::<CrmDemo>();
    }
}

// ---------------------------------------------------------------------------
// A seeder that clears its own way.
// ---------------------------------------------------------------------------

#[injectable]
pub struct PipelineDemo;

impl DemoSeeder for PipelineDemo {
    fn name(&self) -> &'static str {
        "pipeline"
    }

    fn seed<'a>(&'a self, _demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }

    fn clear<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            // WorkspaceRepo's column (`WorkspaceScoped::WORKSPACE_COLUMN`).
            demo.clear::<Company>().await?;
            Ok(())
        })
    }
}

#[module(register = Self::wire)]
pub struct PipelineModule;

impl PipelineModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.demo_seeder::<PipelineDemo>();
    }
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

const COMPANIES: &str = r#"[
  { "name": "Northwind Logistics", "stage": "customer", "last_contacted_at": "{{ now - 2d }}" },
  { "name": "Halcyon Health", "stage": "proposal", "last_contacted_at": "{{ now - 5h }}" },
  { "name": "Brightwater Analytics", "stage": "lead", "last_contacted_at": "{{ now }}" }
]"#;

/// `fixtures/demo/crm/companies.json` in a temporary directory.
fn fixtures() -> TempDirGuard {
    let dir = TempDirGuard::new();
    std::fs::create_dir_all(dir.path().join("crm")).unwrap();
    std::fs::write(dir.path().join("crm").join("companies.json"), COMPANIES).unwrap();
    dir
}

fn builder(fixtures: &TempDirGuard) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .config("workspace.users_table", "")
        .config(
            "workspace.roles.member.permissions",
            json!(["companies.*"]),
        )
        .config(
            "workspace.demo.fixtures",
            fixtures.path().display().to_string(),
        )
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(CrmModule)
}

#[tokio::test]
async fn the_create_command_makes_a_read_only_demo() {
    let dir = fixtures();
    let app = builder(&dir).build().await;
    let ada = Uuid::now_v7();
    app.run_command(
        "workspace:demo-create",
        &["Acme demo", "--owner", &ada.to_string(), "--slug", "demo"],
    )
    .await
    .unwrap();

    let client = app.acting_as(principal(ada));
    let listed = client
        .get("/api/v1/companies")
        .header("X-Workspace", "demo")
        .send()
        .await
        .assert_status(200);
    assert_eq!(listed.header(DEMO_HEADER), Some("read_only"));
    assert_eq!(listed.json_value().as_array().unwrap().len(), 3);
    client
        .post("/api/v1/companies")
        .header("X-Workspace", "demo")
        .send()
        .await
        .assert_error(403, "Demo workspaces are read-only.");
    client
        .get("/api/workspaces")
        .send()
        .await
        .assert_json_path("data.0.kind", json!("demo"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_reset_demo_takes_writes_until_the_nightly_reset() {
    let dir = fixtures();
    let app = builder(&dir)
        .config("workspace.demo.mode", "reset")
        .config("workspace.demo.reset_at", "03:00")
        .module(ScheduleModule::for_root())
        .build()
        .await;
    let ada = Uuid::now_v7();
    let demos = app.get::<DemoWorkspaces>().await;
    assert_eq!(demos.mode(), DemoMode::Reset);
    let uow = app.get::<Db>().await.uow(Some(ada), None).await;
    let demo = demos
        .create(&uow, ada, NewWorkspace::named("Acme demo"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(demo.kind(), WorkspaceKind::Demo);

    let client = app.acting_as(principal(ada));
    let written = client
        .post("/api/v1/companies")
        .header("X-Workspace", "acme-demo")
        .send()
        .await
        .assert_status(200);
    assert_eq!(written.header(DEMO_HEADER), Some("reset"));
    let count = || async {
        WorkspaceRepo::<Company>::select(demo.id)
            .all(app.get::<Db>().await.conn())
            .await
            .unwrap()
            .len()
    };
    assert_eq!(count().await, 4);

    // The scheduler's tick at 03:00 UTC runs `workspace:demo-reset`. The
    // app's clock is the system's here, so run the command itself.
    let scheduler = app.get::<Scheduler>().await;
    let tasks = scheduler.list(Utc::now());
    assert!(tasks.iter().any(|task| task.name == "workspace:demo-reset"));
    app.run_command("workspace:demo-reset", &[]).await.unwrap();
    assert_eq!(count().await, 3);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_seeder_can_clear_its_own_way() {
    let dir = fixtures();
    let app = builder(&dir)
        .config("workspace.demo.mode", "reset")
        .module(PipelineModule)
        .build()
        .await;
    let ada = Uuid::now_v7();
    let demos = app.get::<DemoWorkspaces>().await;
    assert_eq!(demos.seeders().await.unwrap(), ["crm", "pipeline"]);
    let uow = app.get::<Db>().await.uow(None, None).await;
    let demo = demos
        .create(&uow, ada, NewWorkspace::named("Demo"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let reset = demos.reset(demo.id).await.unwrap();
    assert_eq!(reset.seeders, ["crm", "pipeline"]);
    let companies = company::Entity::find()
        .all(app.get::<Db>().await.conn())
        .await
        .unwrap();
    assert_eq!(companies.len(), 3);
    app.shutdown().await.unwrap();
}
