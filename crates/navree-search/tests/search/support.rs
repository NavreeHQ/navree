//! Test models (`contacts`, `companies`, `deals`), their migration, and
//! test apps on the database `NAVREE_TEST_DATABASE` names
//! ([`crate::servers`]).

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use navree_orm::{Model, Record};
use navree_queue::QueueModule;
use navree_search::{
    Document, Embeddings, IndexSettings, Search, SearchContext, SearchModule, Searchable,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_workspace::WorkspaceScoped;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::servers::{self, Server};

/// `contacts`: derived, soft-deleted, with filter and sort fields.
#[derive(Model, Searchable, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "contacts", subject = "Contact")]
#[searchable(name = "Contact", soft_delete = "deleted_at")]
pub struct Contact {
    pub id: Uuid,
    #[searchable(boost = 3)]
    pub first_name: String,
    #[searchable(boost = 3)]
    pub last_name: String,
    #[searchable(boost = 2, rename = "email_address")]
    pub email: Option<String>,
    #[searchable]
    pub title: Option<String>,
    #[searchable(filter)]
    pub status: String,
    #[searchable(filter)]
    pub owner_id: Uuid,
    #[searchable(filter, sort)]
    pub score: i64,
    #[searchable(sort)]
    pub created_at: DateTime<Utc>,
    #[searchable(embed)]
    pub notes: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// `companies`: derived and workspace-scoped (the derive finds the
/// `WorkspaceScoped` column).
#[derive(Model, Searchable, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "companies", subject = "Company")]
#[searchable(name = "Company")]
pub struct Company {
    pub id: Uuid,
    pub workspace_id: Uuid,
    #[searchable(boost = 3)]
    pub name: String,
    #[searchable]
    pub domain: Option<String>,
}

impl WorkspaceScoped for Company {}

/// `deals`: a hand-written `Searchable` with the company's name.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "deals", subject = "Deal")]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    pub company_id: Option<Uuid>,
    pub stage: String,
    pub amount_cents: i64,
    pub closes_on: Option<NaiveDate>,
    pub archived: bool,
}

impl Searchable for Deal {
    fn settings() -> IndexSettings {
        IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .text("company_name", 2)
            .filter("stage")
            .filter("amount_cents")
            .sort("amount_cents")
            .sort("closes_on")
            .filter("closes_on")
    }

    fn document<'a>(
        deal: &'a Record<Deal>,
        ctx: &'a SearchContext,
    ) -> BoxFuture<'a, Result<Option<Document>, BoxError>> {
        Box::pin(async move {
            if deal.archived {
                return Ok(None);
            }
            let company = match deal.company_id {
                Some(id) => Company::find(ctx.uow(), id).await?,
                None => None,
            };
            Ok(Some(
                Document::new(deal.id)
                    .field("title", &deal.title)
                    .field("company_name", company.map(|c| c.into_inner().name))
                    .field("stage", &deal.stage)
                    .field("amount_cents", deal.amount_cents)
                    .field("closes_on", deal.closes_on),
            ))
        })
    }
}

/// Creates the three tables, with each database's types for UUIDs,
/// timestamps and dates (MySQL keeps UUIDs as 16 bytes).
struct CreateTables;

impl Migration for CreateTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let (uuid, timestamp, date, boolean) = match schema.target().await?.dialect() {
            Dialect::Postgres => ("UUID", "TIMESTAMPTZ", "DATE", "BOOLEAN"),
            Dialect::MySql => ("BINARY(16)", "DATETIME(6)", "DATE", "BOOLEAN"),
            Dialect::Sqlite => ("TEXT", "TEXT", "TEXT", "BOOLEAN"),
        };
        for sql in [
            format!(
                "CREATE TABLE contacts (id {uuid} PRIMARY KEY NOT NULL, first_name TEXT NOT NULL, \
                 last_name TEXT NOT NULL, email TEXT NULL, title TEXT NULL, status TEXT NOT NULL, \
                 owner_id {uuid} NOT NULL, score BIGINT NOT NULL, created_at {timestamp} NOT NULL, \
                 notes TEXT NULL, deleted_at {timestamp} NULL)"
            ),
            format!(
                "CREATE TABLE companies (id {uuid} PRIMARY KEY NOT NULL, workspace_id {uuid} NOT NULL, \
                 name TEXT NOT NULL, domain TEXT NULL)"
            ),
            format!(
                "CREATE TABLE deals (id {uuid} PRIMARY KEY NOT NULL, title TEXT NOT NULL, \
                 company_id {uuid} NULL, stage TEXT NOT NULL, amount_cents BIGINT NOT NULL, \
                 closes_on {date} NULL, archived {boolean} NOT NULL DEFAULT FALSE)"
            ),
        ] {
            schema.statement(&sql).await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in ["contacts", "companies", "deals"] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}

/// The test tables and the three searchable models.
pub struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("crm").migration(SchemaMigration::new(
            "m20261026_000001_create_test_tables",
            CreateTables,
        )))
        .import(SearchModule::for_feature::<Contact>())
        .import(SearchModule::for_feature::<Company>())
        .import(SearchModule::for_feature::<Deal>());
    }
}

/// Whether the suite's database runs the `postgres` engine (PostgreSQL
/// and SQLite do; MySQL does not).
pub fn has_database_engine() -> bool {
    Server::current() != Server::MySql
}

/// The engines a test that compares the `memory` engine with the database
/// one runs on.
pub fn engines() -> &'static [&'static str] {
    if has_database_engine() {
        &["memory", "postgres"]
    } else {
        &["memory"]
    }
}

/// The engine of a test of the database engine's SQL: `postgres` where it
/// runs, else `memory` (the query builder is the same).
pub fn database_engine() -> &'static str {
    if has_database_engine() {
        "postgres"
    } else {
        "memory"
    }
}

/// `true` when the suite runs on one of `servers`; else prints why `test`
/// is skipped.
pub fn runs_on(servers: &[Server], test: &str) -> bool {
    let current = Server::current();
    let runs = servers.contains(&current);
    if !runs {
        println!("SKIPPED {test}: not on {current:?}");
    }
    runs
}

/// A test app: a database of its own on the current server, a memory
/// queue, the search module from configuration and the test models.
/// `config` goes on top.
pub fn builder(engine: &str, sync: &str) -> TestAppBuilder {
    servers::database(TestApp::builder())
        .config("queue.driver", "memory")
        .config("search.engine", engine)
        .config("search.sync", sync)
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(SearchModule::from_config())
        .module(CrmModule)
}

/// [`builder`] built.
pub async fn app(engine: &str, sync: &str) -> TestApp {
    builder(engine, sync).build().await
}

pub fn now() -> DateTime<Utc> {
    "2026-10-26T12:00:00Z".parse().unwrap()
}

/// A contact.
pub fn new_contact(first: &str, last: &str) -> Contact {
    Contact {
        id: Uuid::now_v7(),
        first_name: first.to_owned(),
        last_name: last.to_owned(),
        email: Some(format!("{}@example.io", first.to_lowercase())),
        title: None,
        status: "lead".to_owned(),
        owner_id: Uuid::nil(),
        score: 0,
        created_at: now(),
        notes: None,
        deleted_at: None,
    }
}

/// A company of `workspace`.
pub fn new_company(workspace: Uuid, name: &str) -> Company {
    Company {
        id: Uuid::now_v7(),
        workspace_id: workspace,
        name: name.to_owned(),
        domain: None,
    }
}

/// An open deal without a company or a closing date.
pub fn new_deal(title: &str, stage: &str, amount_cents: i64) -> Deal {
    Deal {
        id: Uuid::now_v7(),
        title: title.to_owned(),
        company_id: None,
        stage: stage.to_owned(),
        amount_cents,
        closes_on: None,
        archived: false,
    }
}

/// Inserts `rows` through the model API in one unit of work and commits.
pub async fn insert_contacts(db: &Db, rows: Vec<Contact>) -> Vec<Contact> {
    insert(db, rows).await
}

/// Inserts `rows` of any model in one unit of work and commits.
pub async fn insert<M: Model>(db: &Db, rows: Vec<M>) -> Vec<M> {
    let uow = db.uow(None, None).await;
    let mut out = Vec::new();
    for row in rows {
        out.push(M::create(&uow, row).await.unwrap().into_inner());
    }
    uow.commit().await.unwrap();
    out
}

/// Reads `model` again and saves `change` to it in a unit of work of its
/// own (the change events reach the observers).
pub async fn update<M: Model>(db: &Db, model: &M, change: impl FnOnce(&mut M) + Send) -> M {
    let key = model.key();
    in_uow(db, async move |uow| {
        let mut record = M::find(uow, key).await.unwrap().expect("the record exists");
        record.update(uow, change).await.unwrap();
        record.into_inner()
    })
    .await
}

/// Deletes `model` in a unit of work of its own.
pub async fn delete<M: Model>(db: &Db, model: &M) {
    let key = model.key();
    in_uow(db, async move |uow| {
        let mut record = M::find(uow, key).await.unwrap().expect("the record exists");
        assert!(record.delete(uow).await.unwrap());
    })
    .await;
}

/// The stored `Contact` `id`.
pub async fn reload(db: &Db, id: Uuid) -> Contact {
    Contact::find(db, id).await.unwrap().unwrap().into_inner()
}

/// Runs `f` in a new unit of work and commits it.
pub async fn in_uow<T>(db: &Db, f: impl AsyncFnOnce(&Uow) -> T) -> T {
    let uow = db.uow(None, None).await;
    let out = f(&uow).await;
    uow.commit().await.unwrap();
    out
}

/// Waits for inline syncs.
pub async fn settle(app: &TestApp) {
    app.get::<Search>().await.flush_pending().await.unwrap();
}

/// The `data` ids of a page, as strings.
pub fn ids<T: serde::Serialize>(page: &navree_db::Page<T>) -> Vec<String> {
    page.data
        .iter()
        .map(|item| {
            let value = serde_json::to_value(item).unwrap();
            match &value["id"] {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            }
        })
        .collect()
}

/// Embeddings for tests: a 3-dimensional vector counting the words
/// "onboarding", "billing" and "renewal" (plus a constant, so no vector
/// is zero).
#[derive(Default)]
pub struct WordEmbeddings {
    pub calls: std::sync::atomic::AtomicUsize,
}

impl Embeddings for WordEmbeddings {
    fn embed<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<Vec<f32>>, BoxError>> {
        Box::pin(async move {
            self.calls
                .fetch_add(texts.len(), std::sync::atomic::Ordering::SeqCst);
            Ok(texts
                .iter()
                .map(|text| {
                    let text = text.to_lowercase();
                    let count = |w: &str| text.matches(w).count() as f32;
                    vec![
                        count("onboarding") + 0.01,
                        count("billing") + 0.01,
                        count("renewal") + 0.01,
                    ]
                })
                .collect())
        })
    }
}

/// An app with vector search on the memory store and [`WordEmbeddings`].
pub async fn vector_app() -> (TestApp, Arc<WordEmbeddings>) {
    let embeddings = Arc::new(WordEmbeddings::default());
    let app = builder("memory", "inline")
        .config("search.vectors.store", "memory")
        .config("search.vectors.dimensions", 3)
        .override_value(embeddings.clone() as Arc<dyn Embeddings>)
        .build()
        .await;
    (app, embeddings)
}

/// The server URL from `var`, or `None` after printing the skip notice
/// (`NAVREE_TEST_REQUIRE_SERVICES=1` makes a missing service a failure).
pub fn service(var: &str, test: &str) -> Option<String> {
    match std::env::var(var) {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test} needs {var}"
            );
            println!("SKIPPED {test}: set {var}");
            None
        }
    }
}
