//! `LogbookModule` in a `TestApp`: what a request records end to end, with
//! the real emitters (navree-http, navree-db, navree-events, navree-queue,
//! navree-cache), and the module's switches.

use std::sync::Arc;
use std::time::Duration;

use http::{Method, StatusCode};
use navree_cache::{Cache, CacheModule};
use navree_contracts::{Job, JobContext};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_events::{Event, EventBus, EventsModule};
use navree_http::{AppError, Controller, Extension, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_logbook::{EntryKind, Filter, ListQuery, Logbook, LogbookModule, SqliteStore, Term};
use navree_orm::Model as _;
use navree_queue::{Queue, QueueDriver, QueueModule, QueueOptions};
use navree_testing::{TestApp, TestAppBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// The `deals` table: a navree-orm model.
#[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
#[model(table = "deals")]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
}

struct CreateDeals;

impl Migration for CreateDeals {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("deals", |t| {
                if mysql {
                    t.binary("id").length(16).fixed().primary();
                    t.string("title").length(255);
                } else {
                    t.uuid("id").primary();
                    t.raw_column("title", "VARCHAR");
                }
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await
    }
}

/// An app event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealCreated {
    pub id: Uuid,
}

impl Event for DealCreated {
    const NAME: &'static str = "deal.created";
}

/// A queued job.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncDeal {
    pub id: Uuid,
}

impl Job for SyncDeal {
    const NAME: &'static str = "deals.sync";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Ok(())
    }
}

/// Routes under test, all absolute.
pub struct Fixtures;

impl Injectable for Fixtures {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Fixtures) })
    }
}

impl Controller for Fixtures {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "!/echo",
                |Json(body): Json<Value>| async move { Json(json!({ "ok": true, "got": body })) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/metrics", || async { "metrics" }).public(),
            RouteDef::on(Method::GET, "!/boom", || async {
                Err::<(), AppError>(AppError::internal("boom"))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/me",
                |principal: navree_http::CurrentPrincipal| async move {
                    Json(json!({ "user": principal.0.user_id }))
                },
            )
            .authenticated(),
            RouteDef::on(
                Method::POST,
                "!/deals",
                |uow: Uow, Json(body): Json<Value>| async move {
                    let title = body["title"].as_str().unwrap_or("untitled").to_owned();
                    let deal = Deal::create(
                        &uow,
                        Deal {
                            id: Uuid::now_v7(),
                            title,
                        },
                    )
                    .await?;
                    Ok::<_, AppError>((StatusCode::CREATED, Json(json!({ "id": deal.id }))))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/work",
                |Extension(ctx): Extension<AppContext>| async move {
                    let id = Uuid::now_v7();
                    if let Some(bus) = ctx.try_get::<EventBus>().await {
                        bus.emit(DealCreated { id })
                            .await
                            .map_err(AppError::internal)?;
                    }
                    if let Some(queue) = ctx.try_get::<Queue>().await {
                        queue
                            .dispatch(SyncDeal { id })
                            .await
                            .map_err(AppError::internal)?;
                    }
                    if let Some(cache) = ctx.try_get::<Cache>().await {
                        cache
                            .put("deals:count", &1, Duration::from_secs(60))
                            .await
                            .map_err(AppError::internal)?;
                    }
                    if let Some(logbook) = ctx.try_get::<Logbook>().await {
                        logbook.tag(format!("work:{id}"));
                        logbook.dump("the id", &id);
                    }
                    Ok::<_, AppError>(Json(json!({ "id": id })))
                },
            )
            .public(),
        ]
    }
}

/// The fixture routes, migrations and job.
pub struct FixturesModule;

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        use navree_contracts::ModuleBuilderJobsExt;
        m.controller::<Fixtures>()
            .migrations(
                MigrationSet::new("fixtures")
                    .migration(SchemaMigration::new("m0001_create_deals", CreateDeals)),
            )
            .job::<SyncDeal>();
    }
}

/// A test app with Logbook on, in memory.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("logbook.enabled", true)
        .config("logbook.store", "memory")
        .config("http.per_ip_rate_limit", Value::Null)
}

pub async fn app() -> TestApp {
    builder()
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await
}

#[tokio::test]
async fn requests_are_recorded_with_headers_and_bodies() {
    let app = app().await;
    app.client()
        .post("/echo?draft=1&api_token=abc")
        .header("x-api-token", "secret")
        .bearer("not-stored")
        .json(&json!({ "name": "Ada", "password": "hunter2" }))
        .send()
        .await
        .assert_status(200);

    let logbook = app.get::<Logbook>().await;
    let batch = logbook.last_request().await.expect("the request");
    let request = batch.request().unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/echo");
    assert_eq!(request.route.as_deref(), Some("/echo"));
    assert_eq!(request.status, 200);
    let sent = request.request.as_ref().expect("captured request");
    assert_eq!(
        sent.header("authorization"),
        None,
        "credentials are never stored"
    );
    assert_eq!(sent.header("x-api-token"), Some("[redacted]"));
    assert_eq!(
        sent.query.as_deref(),
        Some("draft=1&api_token=%5Bredacted%5D")
    );
    let body = sent.body.as_ref().unwrap();
    let body: Value = serde_json::from_str(body.text.as_deref().unwrap()).unwrap();
    assert_eq!(body, json!({ "name": "Ada", "password": "[redacted]" }));
    let answered = request.response.as_ref().expect("captured response");
    let reply = answered.body.as_ref().unwrap().text.as_deref().unwrap();
    assert!(reply.contains("\"ok\":true"), "{reply}");
    assert_eq!(answered.header("content-type"), Some("application/json"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn bodies_are_cut_at_the_configured_size() {
    let app = builder()
        .config("logbook.capture.max_body_kb", 1)
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    let long = "x".repeat(4000);
    app.client()
        .post("/echo")
        .json(&json!({ "text": long }))
        .send()
        .await
        .assert_status(200);
    let batch = app.get::<Logbook>().await.last_request().await.unwrap();
    let body = batch
        .request()
        .unwrap()
        .request
        .clone()
        .unwrap()
        .body
        .unwrap();
    assert!(body.truncated);
    assert_eq!(body.text.unwrap().len(), 1024);
    assert_eq!(body.size, Some(4000 + r#"{"text":""}"#.len() as u64));
}

#[tokio::test]
async fn ignored_paths_and_logbook_itself_are_not_recorded() {
    let app = app().await;
    app.client().get("/metrics").send().await.assert_status(200);
    app.client()
        .get("/health/live")
        .send()
        .await
        .assert_status(404);
    app.client()
        .get("/_logbook")
        .send()
        .await
        .assert_status(200);
    app.client()
        .get("/_logbook/api/entries")
        .send()
        .await
        .assert_status(200);
    app.client()
        .post("/echo")
        .json(&json!({}))
        .send()
        .await
        .assert_status(200);
    let logbook = app.get::<Logbook>().await;
    let requests = logbook
        .entries(&ListQuery::new(
            Filter::of([Term::Type(EntryKind::Request)]),
            50,
        ))
        .await
        .unwrap();
    let paths: Vec<String> = requests.iter().map(|e| e.title()).collect();
    assert_eq!(paths, ["POST /echo"]);
}

#[tokio::test]
async fn failed_requests_record_the_exception() {
    let app = app().await;
    app.client().get("/boom").send().await.assert_status(500);
    let batch = app.get::<Logbook>().await.last_request().await.unwrap();
    assert!(batch.head().unwrap().failed);
    assert_eq!(batch.exceptions().len(), 1);
}

#[tokio::test]
async fn test_apps_turn_logbook_off() {
    let app = TestApp::builder()
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    let logbook = app.get::<Logbook>().await;
    assert!(!logbook.is_enabled());
    app.client()
        .get("/_logbook")
        .send()
        .await
        .assert_status(404);
    app.client()
        .post("/echo")
        .json(&json!({}))
        .send()
        .await
        .assert_status(200);
    assert!(logbook.last_request().await.is_none());
}

#[tokio::test]
async fn queries_and_data_changes_are_recorded_and_tagged() {
    let app = crate::servers::database(builder())
        .module(DatabaseModule::from_config())
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    let created = app
        .client()
        .post("/deals")
        .json(&json!({ "title": "Acme renewal" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id = created["id"].as_str().unwrap().to_owned();
    let logbook = app.get::<Logbook>().await;
    let batch = logbook.last_request().await.unwrap();
    assert!(!batch.queries().is_empty(), "{:#?}", batch.entries());
    assert!(batch.queries().iter().any(|q| q.sql.contains("INSERT")));
    let models = batch.models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].entity, "deals");
    assert_eq!(models[0].op, "created");
    assert_eq!(models[0].new.as_ref().unwrap()["title"], "Acme renewal");
    // `tag:deal:<prefix>` finds the request that touched the deal.
    let found = logbook
        .entries(&ListQuery::new(
            Filter::parse(&format!("tag:deal:{}", &id[..8])).unwrap(),
            10,
        ))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind(), EntryKind::Request);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn events_jobs_cache_tags_and_dumps_are_recorded_from_their_modules() {
    let app = crate::servers::database(builder())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(CacheModule::from_config())
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    app.client().post("/work").send().await.assert_status(200);
    let logbook = app.get::<Logbook>().await;
    let request = logbook.last_request().await.unwrap();
    assert_eq!(request.events()[0].name, "deal.created");
    assert_eq!(request.cache_calls()[0].operation, "write");
    assert_eq!(request.dumps()[0].label, "the id");
    assert!(request.tags().iter().any(|t| t.starts_with("work:")));

    app.get::<Queue>().await.run_pending().await.unwrap();
    let job = logbook.last_job().await.expect("the job ran");
    assert_eq!(job.jobs()[0].job_type, "deals.sync");
    assert_eq!(
        job.head().unwrap().trace_id,
        request.head().unwrap().trace_id,
        "the job continues the request's trace"
    );
    let in_trace = logbook
        .entries(&ListQuery::new(
            Filter::of([
                Term::Trace(request.head().unwrap().trace_id.clone().unwrap()),
                Term::Type(EntryKind::Job),
            ]),
            10,
        ))
        .await
        .unwrap();
    assert_eq!(in_trace.len(), 1);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn pruning_keeps_the_newest_entries() {
    let app = builder()
        .config("logbook.max_entries", 3)
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    for _ in 0..5 {
        app.client()
            .post("/echo")
            .json(&json!({}))
            .send()
            .await
            .assert_status(200);
    }
    let logbook = app.get::<Logbook>().await;
    assert!(logbook.count().await.unwrap() >= 5);
    logbook.prune().await.unwrap();
    assert!(logbook.count().await.unwrap() <= 3);
    assert!(logbook.last_request().await.is_some());
}

#[tokio::test]
async fn the_store_is_pruned_at_boot() {
    let dir = navree_testing::TempDirGuard::new();
    let file = dir.path().join("logbook.db");
    {
        let store = SqliteStore::open(&file).await.unwrap();
        let old = chrono::Utc::now() - chrono::Duration::hours(48);
        navree_logbook::LogbookStore::insert(
            &store,
            vec![
                crate::support::entry_at(old, None, crate::support::dump("yesterday's")),
                crate::support::entry_at(chrono::Utc::now(), None, crate::support::dump("today's")),
            ],
        )
        .await
        .unwrap();
    }
    let app = TestApp::builder()
        .config("logbook.enabled", true)
        .config("logbook.file", file.to_string_lossy().to_string())
        .module(LogbookModule::from_config())
        .build()
        .await;
    let logbook = app.get::<Logbook>().await;
    let kept = logbook
        .entries(&ListQuery::new(Filter::all(), 10))
        .await
        .unwrap();
    let titles: Vec<String> = kept.iter().map(|e| e.title()).collect();
    assert_eq!(titles, ["today's"]);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn logbook_clear_deletes_every_entry() {
    let app = app().await;
    app.client()
        .post("/echo")
        .json(&json!({}))
        .send()
        .await
        .assert_status(200);
    let logbook = app.get::<Logbook>().await;
    assert!(logbook.count().await.unwrap() > 0);
    app.run_command("logbook:clear", &[]).await.unwrap();
    assert_eq!(logbook.count().await.unwrap(), 0);
    assert!(app.run_command("logbook:clear", &["now"]).await.is_err());
}

#[tokio::test]
async fn the_file_store_writes_its_own_sqlite_file() {
    let dir = navree_testing::TempDirGuard::new();
    let file = dir.path().join("storage").join("logbook.db");
    let app = TestApp::builder()
        .config("logbook.enabled", true)
        .config("logbook.file", file.to_string_lossy().to_string())
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    app.client()
        .post("/echo")
        .json(&json!({ "kept": true }))
        .send()
        .await
        .assert_status(200);
    let logbook = app.get::<Logbook>().await;
    assert!(logbook.last_request().await.is_some());
    app.shutdown().await.unwrap();
    assert!(file.exists());
    let reopened = SqliteStore::open(&file).await.unwrap();
    let kept = navree_logbook::LogbookStore::list(
        &reopened,
        &ListQuery::new(Filter::parse("type:request").unwrap(), 10),
    )
    .await
    .unwrap();
    assert_eq!(kept.len(), 1, "entries survive a restart");
}

#[tokio::test]
async fn bad_options_fail_the_boot_naming_the_key() {
    let error = builder()
        .config("logbook.path", "logbook")
        .module(LogbookModule::from_config())
        .try_build()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("logbook.path"), "{error}");
}
