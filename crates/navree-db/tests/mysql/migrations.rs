//! Module-owned migrations on MySQL: order, batches, status, rollback,
//! `migrate:fresh`, and what a failed migration leaves behind when DDL
//! commits implicitly.

use navree_db::Executor;
use navree_db::{DatabaseModule, Db, MigrationStatus, Migrator, ModuleBuilderDbExt};
use navree_kernel::BoxFuture;
use navree_kernel::{App, Module, ModuleBuilder, Navree};
use navree_schema::{
    Dialect, NamedMigration, ReferentialAction, Schema, SchemaError, SchemaResult,
};

use crate::fixtures::CreateTable;
use crate::support::{Scratch, count, string, tables};

/// Owns the `users` table.
struct UsersModule;

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "users",
            [],
            vec![CreateTable::boxed("m0001_create_users", "users")],
        ));
    }
}

/// Owns `companies` (foreign key to `users`) and `tags`; runs after
/// `users` and names a `billing` module that is not in the app.
struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "crm",
            ["users", "billing"],
            vec![
                CreateTable::referencing(
                    "m0002_create_companies",
                    "companies",
                    "users",
                    ReferentialAction::Restrict,
                ),
                CreateTable::boxed("m0001_create_tags", "tags"),
            ],
        ));
    }
}

async fn boot(scratch: &Scratch, crm: bool) -> App {
    let mut builder = Navree::app().module(DatabaseModule::for_root(scratch.options()));
    if crm {
        // Registered before `users`, so only `after` puts users first.
        builder = builder.module(CrmModule);
    }
    builder.module(UsersModule).boot().await.expect("app boots")
}

async fn db(app: &App) -> Db {
    (*app.ctx().get::<Db>().await.expect("Db is provided")).clone()
}

#[tokio::test]
async fn migrations_run_in_dependency_order() {
    let Some(scratch) = Scratch::new("migrations_run_in_dependency_order").await else {
        return;
    };
    let app = boot(&scratch, true).await;
    let applied = Migrator::up(app.ctx()).await.expect("migrates");
    assert_eq!(
        applied,
        [
            "users/m0001_create_users",
            "crm/m0001_create_tags",
            "crm/m0002_create_companies",
        ]
    );
    let db = db(&app).await;
    assert_eq!(
        tables(db.pool()).await,
        ["companies", "navree_migrations", "tags", "users"]
    );
    assert_eq!(
        string(
            db.pool(),
            "SELECT CAST(data_type AS CHAR) FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND table_name = 'navree_migrations' \
             AND column_name = 'applied_at'"
        )
        .await,
        "datetime"
    );
}

#[tokio::test]
async fn migrate_is_idempotent_and_status_reports() {
    let Some(scratch) = Scratch::new("migrate_is_idempotent_and_status_reports").await else {
        return;
    };
    let app = boot(&scratch, true).await;
    let pending = Migrator::status(app.ctx()).await.expect("status");
    assert_eq!(pending.len(), 3);
    assert!(pending.iter().all(|s| !s.applied && s.batch.is_none()));

    let before = chrono::Utc::now() - chrono::Duration::seconds(2);
    assert_eq!(Migrator::up(app.ctx()).await.unwrap().len(), 3);
    assert!(Migrator::up(app.ctx()).await.unwrap().is_empty());
    let after = chrono::Utc::now() + chrono::Duration::seconds(2);

    let status = Migrator::status(app.ctx()).await.expect("status");
    assert_eq!(status.len(), 3);
    for entry in &status {
        assert!(entry.applied, "{entry:?}");
        assert_eq!(entry.batch, Some(1));
        let applied_at = entry.applied_at.expect("applied_at reads back");
        assert!(
            before <= applied_at && applied_at <= after,
            "applied_at is the UTC time it ran: {applied_at}"
        );
    }
}

#[tokio::test]
async fn rollback_reverts_the_last_batch() {
    let Some(scratch) = Scratch::new("rollback_reverts_the_last_batch").await else {
        return;
    };
    let first = boot(&scratch, false).await;
    assert_eq!(
        Migrator::up(first.ctx()).await.unwrap(),
        ["users/m0001_create_users"]
    );
    first.shutdown().await.unwrap();

    let app = boot(&scratch, true).await;
    assert_eq!(Migrator::up(app.ctx()).await.unwrap().len(), 2);
    let status = Migrator::status(app.ctx()).await.unwrap();
    assert_eq!(
        status.iter().map(|s| s.batch).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(2)]
    );
    assert_eq!(
        Migrator::rollback(app.ctx()).await.unwrap(),
        ["crm/m0002_create_companies", "crm/m0001_create_tags"]
    );
    let db = db(&app).await;
    assert_eq!(tables(db.pool()).await, ["navree_migrations", "users"]);
    assert_eq!(
        Migrator::rollback(app.ctx()).await.unwrap(),
        ["users/m0001_create_users"]
    );
    assert_eq!(tables(db.pool()).await, ["navree_migrations"]);
    assert!(Migrator::rollback(app.ctx()).await.unwrap().is_empty());
}

/// Rows referencing each other, a stray table and a join table with two
/// cascading parents: everything goes, and every pooled connection checks
/// foreign keys again afterwards.
#[tokio::test]
async fn fresh_drops_every_table_and_reruns() {
    let Some(scratch) = Scratch::new("fresh_drops_every_table_and_reruns").await else {
        return;
    };
    let app = boot(&scratch, true).await;
    Migrator::up(app.ctx()).await.unwrap();
    let db = db(&app).await;
    for sql in [
        "INSERT INTO users (id, title) VALUES (UNHEX('0199A7A0000070008000000000000001'), 'aru')",
        "INSERT INTO companies (id, title, owner_id) VALUES \
         (UNHEX('0199A7A0000070008000000000000002'), 'agra', UNHEX('0199A7A0000070008000000000000001'))",
        "CREATE TABLE stray (id INT PRIMARY KEY)",
        "CREATE TABLE roles (id VARCHAR(32) PRIMARY KEY)",
        "CREATE TABLE permissions (id VARCHAR(32) PRIMARY KEY)",
        "CREATE TABLE role_permissions (\
             role_id VARCHAR(32) NOT NULL, permission_id VARCHAR(32) NOT NULL, \
             PRIMARY KEY (role_id, permission_id), \
             FOREIGN KEY (role_id) REFERENCES roles (id) ON DELETE CASCADE, \
             FOREIGN KEY (permission_id) REFERENCES permissions (id) ON DELETE CASCADE)",
        "INSERT INTO roles (id) VALUES ('admin')",
        "INSERT INTO permissions (id) VALUES ('deals.manage')",
        "INSERT INTO role_permissions (role_id, permission_id) VALUES ('admin', 'deals.manage')",
    ] {
        db.pool()
            .execute_script(sql)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    let applied = Migrator::fresh(app.ctx()).await.expect("fresh");
    assert_eq!(applied.len(), 3);
    assert_eq!(
        tables(db.pool()).await,
        ["companies", "navree_migrations", "tags", "users"]
    );
    assert_eq!(count(db.pool(), "users").await, 0);
    assert!(
        Migrator::status(app.ctx())
            .await
            .unwrap()
            .iter()
            .all(|s| s.batch == Some(1))
    );

    // Hold several connections at once: each checks foreign keys.
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(db.begin().await.unwrap());
    }
    for tx in &held {
        assert_eq!(
            string(tx, "SELECT CAST(@@SESSION.foreign_key_checks AS CHAR)").await,
            "1"
        );
    }
    drop(held);
    let error = db
        .pool()
        .execute_script(
            "INSERT INTO companies (id, title, owner_id) VALUES \
             (UNHEX('0199A7A0000070008000000000000003'), 'orphan', UNHEX('0199A7A0000070008000000000000009'))",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("foreign key"), "{error}");
}

#[tokio::test]
async fn fresh_on_an_empty_database_only_migrates() {
    let Some(scratch) = Scratch::new("fresh_on_an_empty_database_only_migrates").await else {
        return;
    };
    let app = boot(&scratch, false).await;
    assert_eq!(
        Migrator::fresh(app.ctx()).await.expect("fresh"),
        ["users/m0001_create_users"]
    );
}

/// A migration's failure.
fn failure(table: &str, message: &str) -> SchemaError {
    SchemaError::Definition {
        table: table.to_owned(),
        message: message.to_owned(),
    }
}

/// Creates `table`, then fails; outside a transaction, as MySQL runs
/// migrations by default.
struct FailAfterCreating {
    name: &'static str,
    table: &'static str,
}

impl NamedMigration for FailAfterCreating {
    fn name(&self) -> &str {
        self.name
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move {
            schema
                .statement(&format!("CREATE TABLE {} (id INT PRIMARY KEY)", self.table))
                .await?;
            Err(failure(self.table, "boom"))
        })
    }

    fn down<'a>(&'a self, _schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async { Ok(()) })
    }

    fn transactional(&self, dialect: Dialect) -> bool {
        dialect != Dialect::MySql
    }
}

/// Inserts a row into `users`, then fails, inside its transaction.
struct FailAfterInserting;

impl NamedMigration for FailAfterInserting {
    fn name(&self) -> &str {
        "m0002_backfill"
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move {
            schema
                .statement(
                    "INSERT INTO users (id, title) VALUES (UNHEX('0199A7A0000070008000000000000001'), 'aru')",
                )
                .await?;
            Err(failure("users", "backfill failed"))
        })
    }

    fn down<'a>(&'a self, _schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async { Ok(()) })
    }

    /// A data migration asks for a transaction, on MySQL too.
    fn transactional(&self, _dialect: Dialect) -> bool {
        true
    }
}

struct Failing;

impl Module for Failing {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "failing",
            [],
            vec![
                CreateTable::boxed("m1_good", "good"),
                Box::new(FailAfterCreating {
                    name: "m2_bad",
                    table: "partial",
                }),
            ],
        ));
    }
}

async fn status_of(app: &App) -> Vec<MigrationStatus> {
    Migrator::status(app.ctx()).await.unwrap()
}

/// DDL commits implicitly on MySQL: a failed migration keeps what it
/// created, and is not recorded.
#[tokio::test]
async fn a_failed_migration_keeps_its_ddl_and_is_not_recorded() {
    let Some(scratch) = Scratch::new("a_failed_migration_keeps_its_ddl_and_is_not_recorded").await
    else {
        return;
    };
    let app = Navree::app()
        .module(DatabaseModule::for_root(scratch.options()))
        .module(Failing)
        .boot()
        .await
        .unwrap();
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("failing/m2_bad"), "{error}");
    assert!(error.contains("boom"), "{error}");
    let db = db(&app).await;
    assert_eq!(
        tables(db.pool()).await,
        ["good", "navree_migrations", "partial"]
    );
    let status = status_of(&app).await;
    assert!(status[0].applied);
    assert!(!status[1].applied);
}

struct Backfill;

impl Module for Backfill {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "users",
            [],
            vec![
                CreateTable::boxed("m0001_create_users", "users"),
                Box::new(FailAfterInserting),
            ],
        ));
    }
}

/// A data migration that asks for a transaction rolls back its rows.
#[tokio::test]
async fn a_transactional_data_migration_rolls_back_its_rows() {
    let Some(scratch) = Scratch::new("a_transactional_data_migration_rolls_back_its_rows").await
    else {
        return;
    };
    let app = Navree::app()
        .module(DatabaseModule::for_root(scratch.options()))
        .module(Backfill)
        .boot()
        .await
        .unwrap();
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("users/m0002_backfill"), "{error}");
    let db = db(&app).await;
    assert_eq!(count(db.pool(), "users").await, 0, "the insert rolled back");
    let status = status_of(&app).await;
    assert!(status[0].applied);
    assert!(!status[1].applied);
}

#[tokio::test]
async fn migrate_commands_run_through_the_app() {
    let Some(scratch) = Scratch::new("migrate_commands_run_through_the_app").await else {
        return;
    };
    let app = boot(&scratch, true).await;
    let db = db(&app).await;
    app.run_command("migrate", vec![]).await.expect("migrate");
    assert_eq!(tables(db.pool()).await.len(), 4);
    app.run_command("migrate:status", vec![])
        .await
        .expect("status");
    app.run_command("migrate:rollback", vec![])
        .await
        .expect("rollback");
    assert_eq!(tables(db.pool()).await, ["navree_migrations"]);
    app.run_command("migrate:fresh", vec![])
        .await
        .expect("fresh");
    assert_eq!(tables(db.pool()).await.len(), 4);
}
