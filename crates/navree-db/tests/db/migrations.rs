//! Module-owned migrations: order, batches, rollback, fresh, status, commands.

use navree_db::{DatabaseModule, Db, MigrationStatus, Migrator, ModuleBuilderDbExt};
use navree_kernel::{App, Module, ModuleBuilder, Navree};
use sea_orm::ConnectionTrait;

use crate::fixtures::{CreateTable, FailAfterCreating, count, tables};
use crate::support::TempDb;

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

/// Owns `companies` (foreign key to `users`) and `tags`; must run after
/// `users`, and names a `billing` module that is not in the app.
struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "crm",
            ["users", "billing"],
            vec![
                CreateTable::referencing("m0002_create_companies", "companies", "users"),
                CreateTable::boxed("m0001_create_tags", "tags"),
            ],
        ));
    }
}

async fn boot(temp: &TempDb, crm: bool) -> App {
    let mut builder = Navree::app().module(DatabaseModule::for_root(temp.options()));
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
    let temp = TempDb::new();
    let app = boot(&temp, true).await;
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
        tables(&db).await,
        ["companies", "navree_migrations", "tags", "users"]
    );
}

#[tokio::test]
async fn migrate_is_idempotent_and_status_reports() {
    let temp = TempDb::new();
    let app = boot(&temp, true).await;

    let pending = Migrator::status(app.ctx()).await.expect("status");
    assert_eq!(pending.len(), 3);
    assert!(pending.iter().all(|s| !s.applied && s.batch.is_none()));
    assert_eq!(pending[0].module, "users");
    assert_eq!(pending[0].name, "m0001_create_users");

    assert_eq!(Migrator::up(app.ctx()).await.unwrap().len(), 3);
    assert!(
        Migrator::up(app.ctx()).await.unwrap().is_empty(),
        "nothing left to run"
    );

    let status = Migrator::status(app.ctx()).await.expect("status");
    assert_eq!(status.len(), 3);
    for entry in &status {
        assert!(entry.applied, "{entry:?}");
        assert_eq!(entry.batch, Some(1));
        assert!(entry.applied_at.is_some());
    }
}

#[tokio::test]
async fn rollback_last_batch() {
    let temp = TempDb::new();
    let first = boot(&temp, false).await;
    assert_eq!(
        Migrator::up(first.ctx()).await.unwrap(),
        ["users/m0001_create_users"]
    );
    first.shutdown().await.unwrap();

    let app = boot(&temp, true).await;
    assert_eq!(Migrator::up(app.ctx()).await.unwrap().len(), 2);
    let status = Migrator::status(app.ctx()).await.unwrap();
    assert_eq!(
        status.iter().map(|s| s.batch).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(2)]
    );

    // The last batch, newest first.
    assert_eq!(
        Migrator::rollback(app.ctx()).await.unwrap(),
        ["crm/m0002_create_companies", "crm/m0001_create_tags"]
    );
    let db = db(&app).await;
    assert_eq!(tables(&db).await, ["navree_migrations", "users"]);

    assert_eq!(
        Migrator::rollback(app.ctx()).await.unwrap(),
        ["users/m0001_create_users"]
    );
    assert_eq!(tables(&db).await, ["navree_migrations"]);
    assert!(Migrator::rollback(app.ctx()).await.unwrap().is_empty());
    assert!(
        Migrator::status(app.ctx())
            .await
            .unwrap()
            .iter()
            .all(|s| !s.applied)
    );
}

#[tokio::test]
async fn fresh_drops_and_reruns() {
    let temp = TempDb::new();
    let app = boot(&temp, true).await;
    Migrator::up(app.ctx()).await.unwrap();
    let db = db(&app).await;
    db.conn()
        .execute_unprepared(
            "INSERT INTO users (id, title) VALUES ('0199a7a0-0000-7000-8000-000000000001', 'ann');
             INSERT INTO companies (id, title, owner_id) VALUES ('0199a7a0-0000-7000-8000-000000000002', 'acme', '0199a7a0-0000-7000-8000-000000000001');
             CREATE TABLE stray (id INTEGER PRIMARY KEY);",
        )
        .await
        .unwrap();

    let applied = Migrator::fresh(app.ctx()).await.expect("fresh");
    assert_eq!(applied.len(), 3);
    assert_eq!(
        tables(&db).await,
        ["companies", "navree_migrations", "tags", "users"]
    );
    assert_eq!(count(&db, "users").await, 0);
    assert_eq!(count(&db, "companies").await, 0);
    assert!(
        Migrator::status(app.ctx())
            .await
            .unwrap()
            .iter()
            .all(|s| s.batch == Some(1))
    );
}

/// A join table whose two parents both cascade deletes (navree-access's
/// `role_permissions`): SQLite's implicit `DELETE` before `DROP TABLE` on
/// the second parent cascades into the join table, whose other parent is
/// already gone ("no such table: main.roles"), unless foreign keys are off.
#[tokio::test]
async fn fresh_drops_join_tables_with_two_cascading_parents() {
    let temp = TempDb::new();
    let app = boot(&temp, true).await;
    Migrator::up(app.ctx()).await.unwrap();
    let db = db(&app).await;
    db.conn()
        .execute_unprepared(
            "CREATE TABLE roles (id TEXT PRIMARY KEY);
             CREATE TABLE permissions (id TEXT PRIMARY KEY);
             CREATE TABLE role_permissions (
                 role_id TEXT NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
                 permission_id TEXT NOT NULL REFERENCES permissions (id) ON DELETE CASCADE,
                 PRIMARY KEY (role_id, permission_id)
             );
             INSERT INTO roles (id) VALUES ('admin');
             INSERT INTO permissions (id) VALUES ('deals.manage');
             INSERT INTO role_permissions (role_id, permission_id) VALUES ('admin', 'deals.manage');
             INSERT INTO users (id, title) VALUES ('0199a7a0-0000-7000-8000-000000000001', 'ann');
             INSERT INTO companies (id, title, owner_id) VALUES ('0199a7a0-0000-7000-8000-000000000002', 'acme', '0199a7a0-0000-7000-8000-000000000001');",
        )
        .await
        .unwrap();

    Migrator::fresh(app.ctx())
        .await
        .expect("fresh drops every table");
    assert_eq!(
        tables(&db).await,
        ["companies", "navree_migrations", "tags", "users"]
    );
    assert_eq!(count(&db, "users").await, 0);

    // Every pooled connection still enforces foreign keys afterwards.
    for _ in 0..4 {
        let row = db
            .conn()
            .query_one_raw(sea_orm::Statement::from_string(
                db.backend(),
                "PRAGMA foreign_keys",
            ))
            .await
            .unwrap()
            .expect("a row");
        assert_eq!(row.try_get_by_index::<i32>(0).unwrap(), 1);
    }
}

/// `a` after `b` and `b` after `a`.
struct CycleA;
struct CycleB;

impl Module for CycleA {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "a",
            ["b"],
            vec![CreateTable::boxed("m1", "a_table")],
        ));
    }
}

impl Module for CycleB {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(crate::fixtures::set(
            "b",
            ["a"],
            vec![CreateTable::boxed("m1", "b_table")],
        ));
    }
}

#[tokio::test]
async fn migration_order_cycle_is_an_error() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .module(CycleA)
        .module(CycleB)
        .boot()
        .await
        .unwrap();
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("cycle"), "{error}");
    assert!(error.contains("`a`") && error.contains("`b`"), "{error}");
    let db = db(&app).await;
    assert!(!tables(&db).await.contains(&"a_table".to_owned()));
}

/// One good migration, then one that creates a table and fails.
struct Failing {
    transactional: bool,
}

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
                    transactional: self.transactional,
                }),
            ],
        ));
    }
}

async fn run_failing(transactional: bool) -> (Vec<String>, String, Vec<MigrationStatus>) {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .module(Failing { transactional })
        .boot()
        .await
        .unwrap();
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    let db = db(&app).await;
    (
        tables(&db).await,
        error,
        Migrator::status(app.ctx()).await.unwrap(),
    )
}

#[tokio::test]
async fn failed_migration_rolls_back_and_is_not_recorded() {
    let (tables, error, status) = run_failing(true).await;
    assert!(error.contains("failing/m2_bad"), "{error}");
    assert!(error.contains("boom"), "{error}");
    assert_eq!(
        tables,
        ["good", "navree_migrations"],
        "partial table rolled back"
    );
    assert!(status[0].applied);
    assert!(!status[1].applied);
}

#[tokio::test]
async fn migration_can_opt_out_of_the_transaction() {
    let (tables, _, status) = run_failing(false).await;
    assert_eq!(tables, ["good", "navree_migrations", "partial"]);
    assert!(!status[1].applied);
}

#[tokio::test]
async fn migrate_commands_run_through_the_app() {
    let temp = TempDb::new();
    let app = boot(&temp, true).await;
    let db = db(&app).await;
    app.run_command("migrate", vec![]).await.expect("migrate");
    assert_eq!(tables(&db).await.len(), 4);
    app.run_command("migrate:status", vec![])
        .await
        .expect("status");
    app.run_command("migrate:status", vec!["--json".to_owned()])
        .await
        .expect("status as JSON");
    // What `--json` prints: the statuses, serialized.
    let json = serde_json::to_value(Migrator::status(app.ctx()).await.unwrap()).unwrap();
    assert_eq!(json[0]["applied"], true);
    assert!(json[0]["module"].is_string() && json[0]["name"].is_string());
    assert!(json[0]["batch"].is_i64());
    app.run_command("migrate:rollback", vec![])
        .await
        .expect("rollback");
    assert_eq!(tables(&db).await, ["navree_migrations"]);
    app.run_command("migrate:fresh", vec![])
        .await
        .expect("fresh");
    assert_eq!(tables(&db).await.len(), 4);
}

#[tokio::test]
async fn migrate_on_boot_runs_pending_migrations() {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(UsersModule)
        .boot()
        .await
        .unwrap();
    let db = db(&app).await;
    assert_eq!(tables(&db).await, ["navree_migrations", "users"]);
}
