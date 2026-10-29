//! Migrations on a SQLite file other connections write to: a migration
//! reads before it writes (`has_table`, a column listing), and a deferred
//! `BEGIN` that upgrades to a write later fails with `database is locked` at
//! once, without waiting for `busy_timeout`, when another connection holds
//! the write lock then. Migrations begin `IMMEDIATE` instead, so they queue
//! on the lock like any other writer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use navree_db::{
    DatabaseModule, Db, Executor, MigrateOptions, MigrationSet, Migrator, ModuleBuilderDbExt as _,
    Statement, TxOptions,
};
use navree_kernel::{App, Module, ModuleBuilder, Navree};
use navree_schema::{Schema, SchemaMigration, SchemaResult};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::support::TempDb;

/// How many migrations of each kind run while the writer holds the lock.
const MIGRATIONS: usize = 12;

/// Looks before it writes, as most schema-builder migrations do (the
/// webhooks and fleet hypertable migrations list foreign keys and check
/// for hypertables first).
struct ReadThenWrite {
    table: String,
}

impl navree_schema::Migration for ReadThenWrite {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        if !schema.has_table(&self.table).await? {
            schema
                .statement(&format!(
                    "CREATE TABLE \"{}\" (\"id\" INTEGER PRIMARY KEY)",
                    self.table
                ))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(&self.table).await
    }
}

/// The same on SeaORM's schema manager.
struct SeaReadThenWrite {
    name: String,
    table: String,
}

impl MigrationName for SeaReadThenWrite {
    fn name(&self) -> &str {
        &self.name
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for SeaReadThenWrite {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_table(&self.table).await? {
            manager
                .create_table(
                    Table::create()
                        .table(self.table.clone())
                        .col(ColumnDef::new("id").integer().not_null().primary_key())
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(self.table.clone()).to_owned())
            .await
    }
}

struct Lookers {
    sea: bool,
}

impl Module for Lookers {
    fn register(&self, m: &mut ModuleBuilder) {
        let mut set = MigrationSet::new("lookers");
        for n in 0..MIGRATIONS {
            set = if self.sea {
                set.migration(SeaReadThenWrite {
                    name: format!("m{n:04}_sea_looker"),
                    table: format!("sea_looker_{n}"),
                })
            } else {
                set.migration(SchemaMigration::new(
                    format!("m{n:04}_looker"),
                    ReadThenWrite {
                        table: format!("looker_{n}"),
                    },
                ))
            };
        }
        m.migrations(set);
    }
}

/// Another connection of the app writing all the time: takes the write lock
/// (`BEGIN IMMEDIATE`), holds it for a moment, and lets go for a moment.
struct Writer {
    stop: Arc<AtomicBool>,
    commits: Arc<AtomicU32>,
    task: tokio::task::JoinHandle<()>,
}

impl Writer {
    async fn start(db: &Db) -> Writer {
        db.execute(&Statement::text(
            "CREATE TABLE noise (id INTEGER PRIMARY KEY, n INTEGER)",
        ))
        .await
        .expect("creates the table the writer fills");
        let stop = Arc::new(AtomicBool::new(false));
        let commits = Arc::new(AtomicU32::new(0));
        let task = tokio::spawn({
            let (db, stop, commits) = (db.clone(), stop.clone(), commits.clone());
            async move {
                while !stop.load(Ordering::SeqCst) {
                    let tx = db
                        .begin_with(TxOptions::new().immediate())
                        .await
                        .expect("the writer takes the write lock");
                    tx.execute(&Statement::text("INSERT INTO noise (n) VALUES (1)"))
                        .await
                        .expect("writes");
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    tx.commit().await.expect("commits");
                    commits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(15)).await;
                }
            }
        });
        // Migrations start with the writer running.
        while commits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Writer {
            stop,
            commits,
            task,
        }
    }

    /// Stops, and says how many times it committed.
    async fn stop(self) -> u32 {
        self.stop.store(true, Ordering::SeqCst);
        self.task.await.expect("the writer ends");
        self.commits.load(Ordering::SeqCst)
    }
}

async fn app(temp: &TempDb, sea: bool) -> App {
    Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .module(Lookers { sea })
        .boot()
        .await
        .expect("the app boots")
}

async fn migrate_beside_a_writer(sea: bool) {
    let temp = TempDb::new();
    let app = app(&temp, sea).await;
    let db = (*app.ctx().get::<Db>().await.expect("Db")).clone();
    let writer = Writer::start(&db).await;
    let before = writer.commits.load(Ordering::SeqCst);

    let up = Migrator::run(app.ctx(), &MigrateOptions::default()).await;
    let reset = Migrator::reset(app.ctx(), false).await;
    let commits = writer.stop().await;

    let report =
        up.expect("migrations wait for the writer instead of failing with `database is locked`");
    assert_eq!(report.migrations.len(), MIGRATIONS);
    assert_eq!(
        reset.expect("and so do their rollbacks").migrations.len(),
        MIGRATIONS
    );
    assert!(
        commits > before + 2,
        "the writer kept writing through the migrations ({before} -> {commits} commits)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_migrations_queue_on_the_write_lock_instead_of_failing() {
    migrate_beside_a_writer(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seaorm_migrations_queue_on_the_write_lock_instead_of_failing() {
    migrate_beside_a_writer(true).await;
}
