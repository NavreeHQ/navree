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

use crate::support::TempDb;

/// How many migrations run while the writer holds the lock.
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

struct Lookers;

impl Module for Lookers {
    fn register(&self, m: &mut ModuleBuilder) {
        let mut set = MigrationSet::new("lookers");
        for n in 0..MIGRATIONS {
            set = set.migration(SchemaMigration::new(
                format!("m{n:04}_looker"),
                ReadThenWrite {
                    table: format!("looker_{n}"),
                },
            ));
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

async fn app(temp: &TempDb) -> App {
    Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .module(Lookers)
        .boot()
        .await
        .expect("the app boots")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn migrations_queue_on_the_write_lock_instead_of_failing() {
    let temp = TempDb::new();
    let app = app(&temp).await;
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
    // The writer held the write lock at least once while the migrations ran
    // (how often depends on the machine's load); the migrations above
    // succeeding beside it is what the test proves.
    assert!(
        commits > before,
        "the writer kept writing through the migrations ({before} -> {commits} commits)"
    );
}
