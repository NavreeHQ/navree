//! The migrator on a real PostgreSQL server (with TimescaleDB when it has
//! it): schema and SeaORM migrations, pretending, rollbacks, refresh, fresh,
//! and a schema dump loaded into an empty database.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a URL of a throwaway server whose user may
//! create databases, e.g.
//! `postgres://postgres:secret@127.0.0.1:55432/postgres`. Each test creates
//! its own database and drops it at the end.
#![cfg(feature = "postgres")]

use std::sync::atomic::{AtomicU32, Ordering};

use navree_db::{
    DatabaseModule, Db, DbOptions, FreshOptions, MigrateOptions, MigrationSet, Migrator,
    ModuleBuilderDbExt, RollbackOptions,
};
use navree_kernel::{App, Module, ModuleBuilder, Navree};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ConnectionTrait, Database, DbErr, Statement};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own on the test server.
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new() -> Option<Scratch> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!(
            "navree_migrations_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let conn = Database::connect(&admin).await.unwrap();
        conn.execute_unprepared(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .await
            .unwrap();
        conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        conn.close().await.unwrap();
        let (base, _) = admin.rsplit_once('/').unwrap();
        let url = format!("{base}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        conn.execute_unprepared(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
        conn.close().await.unwrap();
    }
}

mod m0001_create_companies {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("companies", |t| {
                    t.uuid_primary();
                    t.string("name").unique();
                    t.timestamps_tz();
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("companies").await
        }
    }
}

mod m0002_create_events {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create_hypertable("events", "occurred_at", |t| {
                    t.uuid_primary();
                    t.foreign_uuid("company_id")
                        .constrained()
                        .cascade_on_delete();
                    t.string("kind");
                    t.timestamp_tz("occurred_at");
                    let events = t.hypertable("occurred_at");
                    events.chunk_interval("1 day").retain_for("90 days");
                    events.compress().segment_by(["company_id"]).after("7 days");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("events").await
        }
    }
}

/// The tags table, written with SeaORM.
struct CreateTags;

impl MigrationName for CreateTags {
    fn name(&self) -> &str {
        "m0003_create_tags"
    }
}

#[async_trait]
impl MigrationTrait for CreateTags {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("tags")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("label").string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("tags").to_owned())
            .await
    }
}

struct Crm;

impl Module for Crm {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("crm")
                .migration(navree_schema::migration!(m0001_create_companies))
                .migration(navree_schema::migration!(m0002_create_events))
                .migration(CreateTags),
        );
    }
}

async fn boot(url: &str) -> App {
    Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            url: url.to_owned(),
            ..DbOptions::default()
        }))
        .module(Crm)
        .boot()
        .await
        .expect("the app boots")
}

async fn tables(app: &App) -> Vec<String> {
    let db = app.ctx().get::<Db>().await.unwrap();
    db.conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT tablename::text AS name FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect()
}

async fn hypertables(app: &App) -> Vec<String> {
    let db = app.ctx().get::<Db>().await.unwrap();
    let found = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT hypertable_name::text AS name FROM timescaledb_information.hypertables",
        ))
        .await;
    match found {
        Ok(rows) => rows
            .iter()
            .map(|row| row.try_get::<String>("", "name").unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[tokio::test]
async fn the_migrator_on_postgres() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = boot(&scratch.url).await;

    let pretended = Migrator::run(
        app.ctx(),
        &MigrateOptions {
            pretend: true,
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(pretended.migrations.len(), 3);
    assert!(
        pretended.migrations[2].statements[0].starts_with("CREATE TABLE \"tags\""),
        "{:?}",
        pretended.migrations[2].statements
    );
    assert!(tables(&app).await.is_empty(), "pretending ran nothing");

    let report = Migrator::run(app.ctx(), &MigrateOptions::default())
        .await
        .unwrap();
    assert_eq!(report.migrations.len(), 3);
    assert_eq!(
        tables(&app).await,
        ["companies", "events", "navree_migrations", "tags"]
    );
    let timescale = !hypertables(&app).await.is_empty();
    if timescale {
        assert_eq!(hypertables(&app).await, ["events"]);
    } else {
        assert!(
            report.migrations[1]
                .notices
                .iter()
                .any(|n| n.contains("is a regular table")),
            "{:?}",
            report.migrations[1].notices
        );
    }

    let reverted = Migrator::rollback_with(
        app.ctx(),
        &RollbackOptions {
            step: Some(1),
            ..RollbackOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(reverted.labels(), ["crm/m0003_create_tags"]);
    let refreshed = Migrator::refresh(app.ctx(), None).await.unwrap();
    assert_eq!(refreshed.reverted.migrations.len(), 2);
    assert_eq!(refreshed.migrated.migrations.len(), 3);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("postgres-schema.sql");
    Migrator::dump(app.ctx(), Some(&path)).await.unwrap();
    let dump = std::fs::read_to_string(&path).unwrap();
    assert!(dump.contains("INSERT INTO \"navree_migrations\""), "{dump}");
    if timescale {
        assert!(
            dump.contains("SELECT create_hypertable('public.events'"),
            "{dump}"
        );
    }

    // A second database starts from the dump.
    let copy = Scratch::new().await.unwrap();
    let copied = boot(&copy.url).await;
    let report = Migrator::run(
        copied.ctx(),
        &MigrateOptions {
            schema_path: Some(path),
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(report.loaded_dump.is_some());
    assert!(
        report.migrations.is_empty(),
        "the dump records every migration"
    );
    assert_eq!(tables(&copied).await, tables(&app).await);
    assert_eq!(hypertables(&copied).await, hypertables(&app).await);
    copied.shutdown().await.unwrap();
    copy.drop().await;

    let fresh = Migrator::fresh_with(
        app.ctx(),
        &FreshOptions {
            drop_views: true,
            drop_types: true,
            ..FreshOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(fresh.migrations.len(), 3);
    assert!(
        Migrator::status(app.ctx())
            .await
            .unwrap()
            .iter()
            .all(|s| s.batch == Some(1))
    );
    app.shutdown().await.unwrap();
    scratch.drop().await;
}
