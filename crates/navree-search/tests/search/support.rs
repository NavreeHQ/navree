//! Test entities (`contacts`, `companies`, `deals`), their migrations, and
//! test apps.

#![allow(dead_code)]

use std::sync::Arc;

use navree_access::Subject;
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Repo, Uow};
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use navree_queue::QueueModule;
use navree_search::{
    Document, Embeddings, IndexSettings, Search, SearchContext, SearchModule, Searchable,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_workspace::WorkspaceScoped;
use sea_orm::{ActiveValue::Set, ConnectionTrait, DbErr, EntityTrait};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::Value;
use uuid::Uuid;

/// `contacts`: derived, soft-deleted, with filter and sort fields.
pub mod contact {
    use navree_search::Searchable;
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Searchable)]
    #[sea_orm(table_name = "contacts")]
    #[searchable(name = "Contact", soft_delete = "deleted_at")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub created_at: DateTimeUtc,
        #[searchable(embed)]
        pub notes: Option<String>,
        pub deleted_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `companies`: derived and workspace-scoped (the derive finds the
/// `WorkspaceScoped` column).
pub mod company {
    use navree_search::Searchable;
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Searchable)]
    #[sea_orm(table_name = "companies")]
    #[searchable(name = "Company")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        #[searchable(boost = 3)]
        pub name: String,
        #[searchable]
        pub domain: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for company::Entity {}

/// `deals`: a hand-written `Searchable` with the company's name.
pub mod deal {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "deals")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        pub company_id: Option<Uuid>,
        pub stage: String,
        pub amount_cents: i64,
        pub closes_on: Option<Date>,
        pub archived: bool,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub type Contact = contact::Entity;
pub type Company = company::Entity;
pub type Deal = deal::Entity;

impl Subject for contact::Entity {
    const SUBJECT: &'static str = "Contact";
    const PERMISSION_PREFIX: &'static str = "contacts";
}

impl Subject for company::Entity {
    const SUBJECT: &'static str = "Company";
    const PERMISSION_PREFIX: &'static str = "companies";
}

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

impl Searchable for deal::Entity {
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
        deal: &'a deal::Model,
        ctx: &'a SearchContext,
    ) -> BoxFuture<'a, Result<Option<Document>, BoxError>> {
        Box::pin(async move {
            if deal.archived {
                return Ok(None);
            }
            let company = match deal.company_id {
                Some(id) => company::Entity::find_by_id(id).one(ctx.conn()).await?,
                None => None,
            };
            Ok(Some(
                Document::new(deal.id)
                    .field("title", &deal.title)
                    .field("company_name", company.map(|c| c.name))
                    .field("stage", &deal.stage)
                    .field("amount_cents", deal.amount_cents)
                    .field("closes_on", deal.closes_on),
            ))
        })
    }
}

/// Creates the three tables.
struct CreateTables;

impl MigrationName for CreateTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_test_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        // SQLite stores uuids, timestamps and dates as text; PostgreSQL has
        // types for them.
        let (uuid, timestamp, date) = match manager.get_database_backend() {
            sea_orm::DbBackend::Postgres => ("UUID", "TIMESTAMPTZ", "DATE"),
            _ => ("TEXT", "TEXT", "TEXT"),
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
                 closes_on {date} NULL, archived BOOLEAN NOT NULL DEFAULT FALSE)"
            ),
        ] {
            conn.execute_unprepared(&sql).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        for table in ["contacts", "companies", "deals"] {
            conn.execute_unprepared(&format!("DROP TABLE {table}"))
                .await?;
        }
        Ok(())
    }
}

/// The test tables and the three searchable entities.
pub struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("crm").migration(CreateTables))
            .import(SearchModule::for_feature::<Contact>())
            .import(SearchModule::for_feature::<Company>())
            .import(SearchModule::for_feature::<Deal>());
    }
}

/// A test app: a temporary SQLite database, a memory queue, the search
/// module from configuration and the test entities. `config` goes on top.
pub fn builder(engine: &str, sync: &str) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
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

pub fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-10-26T12:00:00Z".parse().unwrap()
}

/// A contact row.
pub fn new_contact(first: &str, last: &str) -> contact::ActiveModel {
    contact::ActiveModel {
        id: Set(Uuid::now_v7()),
        first_name: Set(first.to_owned()),
        last_name: Set(last.to_owned()),
        email: Set(Some(format!("{}@example.io", first.to_lowercase()))),
        title: Set(None),
        status: Set("lead".to_owned()),
        owner_id: Set(Uuid::nil()),
        score: Set(0),
        created_at: Set(now()),
        notes: Set(None),
        deleted_at: Set(None),
    }
}

/// Inserts `rows` through `Repo` in one unit of work and commits.
pub async fn insert_contacts(db: &Db, rows: Vec<contact::ActiveModel>) -> Vec<contact::Model> {
    let uow = db.uow(None, None).await;
    let mut out = Vec::new();
    for row in rows {
        out.push(Repo::<Contact>::insert(&uow, row).await.unwrap());
    }
    uow.commit().await.unwrap();
    out
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
