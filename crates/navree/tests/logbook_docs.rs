//! The examples of `content/docs/tools/logbook.mdx`, written as an app
//! writes them (the facade, the prelude and the macros), compiled and run.

use navree::db::Db;
use navree::logbook::Logbook;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Installation: the module in the app root.
// ---------------------------------------------------------------------------

fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        // ... your other modules
        .module(LogbookModule::from_config())
}

#[tokio::test]
async fn the_installed_app_boots_and_serves_the_timeline() {
    let app = TestApp::from_builder(app(ConfigSources::new()
        .set("logbook.enabled", true)
        .set("logbook.store", "memory")))
    .await;
    app.client()
        .get("/_logbook")
        .send()
        .await
        .assert_status(200);
    app.shutdown().await.unwrap();
}

#[test]
fn leaving_the_module_out_of_release_builds() {
    let sources = ConfigSources::new();
    let mut app = Navree::app().module(ConfigModule::for_root(sources));
    // ...
    if cfg!(debug_assertions) {
        app = app.module(LogbookModule::from_config());
    }
    let _ = app;
}

// ---------------------------------------------------------------------------
// Tags and dumps from a service.
// ---------------------------------------------------------------------------

/// An uploaded file to import.
struct ImportFile {
    id: u32,
    body: String,
}

/// What an import did.
#[derive(Debug)]
struct ImportReport {
    rows: usize,
}

#[derive(Debug, Serialize)]
struct Row {
    name: String,
}

fn parse_rows(file: &ImportFile) -> Result<Vec<Row>, BoxError> {
    Ok(file
        .body
        .lines()
        .map(|line| Row {
            name: line.to_owned(),
        })
        .collect())
}

#[injectable]
struct ContactImport {
    logbook: Arc<Logbook>,
}

impl ContactImport {
    async fn run(&self, file: &ImportFile) -> Result<ImportReport, BoxError> {
        self.logbook.tag(format!("import:{}", file.id));
        let rows = parse_rows(file)?;
        self.logbook.dump("parsed rows", &rows[..rows.len().min(5)]);
        // ...
        Ok(ImportReport { rows: rows.len() })
    }
}

#[injectable]
struct ImportController {
    imports: Arc<ContactImport>,
}

#[controller("/imports", version = "1")]
impl ImportController {
    #[post("/")]
    #[public]
    async fn store(&self) -> Result<Json<Value>> {
        let file = ImportFile {
            id: 42,
            body: "Ada\nGrace\nKatherine\nDorothy\nMary\nAnnie".to_owned(),
        };
        let report = self.imports.run(&file).await.map_err(AppError::internal)?;
        Ok(Json(json!({ "rows": report.rows })))
    }
}

#[module(providers = [ContactImport], controllers = [ImportController])]
struct ContactsModule;

#[tokio::test]
async fn tags_and_dumps_land_on_the_request() {
    let app = TestApp::builder()
        .config("logbook.enabled", true)
        .config("logbook.store", "memory")
        .module(LogbookModule::from_config())
        .module(ContactsModule)
        .build()
        .await;
    app.client()
        .post("/api/v1/imports")
        .send()
        .await
        .assert_status(200);
    let batch = app.get::<Logbook>().await.last_request().await.unwrap();
    assert!(batch.tags().contains(&"import:42".to_owned()));
    let dump = &batch.dumps()[0];
    assert_eq!(dump.label, "parsed rows");
    assert_eq!(dump.value.as_array().unwrap().len(), 5);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn tags_and_dumps_do_nothing_when_logbook_is_off() {
    let app = TestApp::builder()
        .module(LogbookModule::from_config())
        .module(ContactsModule)
        .build()
        .await;
    app.client()
        .post("/api/v1/imports")
        .send()
        .await
        .assert_status(200);
    assert!(app.get::<Logbook>().await.last_request().await.is_none());
}

// ---------------------------------------------------------------------------
// Using Logbook in tests: the deals board loads in three queries.
// ---------------------------------------------------------------------------

#[derive(Model, Clone, Debug, PartialEq, Eq, Serialize)]
#[model(table = "deals")]
struct Deal {
    id: Uuid,
    title: String,
}

struct CreateDeals;

impl Migration for CreateDeals {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("deals", |t| {
                t.uuid("id").primary();
                t.string("title");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await
    }
}

#[injectable]
struct BoardController;

#[controller("/deals", version = "1")]
impl BoardController {
    #[get("/board")]
    #[authenticated]
    async fn board(&self, uow: Uow) -> Result<Json<Value>> {
        let deals = Deal::query().order_by(Deal::TITLE).get(&uow).await?;
        Ok(Json(json!({ "deals": deals })))
    }
}

#[module(controllers = [BoardController], register = Self::wire)]
struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("deals").migration(SchemaMigration::new(
            "m0001_create_deals",
            CreateDeals,
        )));
    }
}

async fn seed_board(app: &TestApp, deals: usize) {
    let db = app.get::<Db>().await;
    for n in 0..deals {
        Deal::create(
            &*db,
            Deal {
                id: Uuid::now_v7(),
                title: format!("Deal {n}"),
            },
        )
        .await
        .unwrap();
    }
}

fn rep_id() -> Uuid {
    Uuid::from_u128(0x0192_f0c1)
}

#[tokio::test]
async fn the_board_loads_in_three_queries() {
    let app = TestApp::builder()
        .temp_database()
        .config("logbook.enabled", true)
        .config("logbook.store", "memory")
        .module(DatabaseModule::from_config())
        .module(LogbookModule::from_config())
        .module(DealsModule)
        .build()
        .await;
    seed_board(&app, 20).await;

    app.acting_as(principal(rep_id()))
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_status(200);

    let batch = app.get::<Logbook>().await.last_request().await.unwrap();
    assert!(batch.queries().len() <= 3, "{:#?}", batch.queries());
    assert!(batch.duplicate_queries().is_empty());
    app.shutdown().await.unwrap();
}
