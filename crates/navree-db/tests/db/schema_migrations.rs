//! Migrations written with the schema builder: by batch and step, rolled
//! back, reset and refreshed, pretended, dumped and loaded, and through
//! the commands.

use std::path::Path;

use navree_config::{ConfigModule, ConfigSources};
use navree_db::{
    DatabaseModule, Db, FreshOptions, MigrateOptions, MigrationSet, Migrator, ModuleBuilderDbExt,
    RollbackOptions,
};
use navree_db::{Executor, ExecutorExt, Statement};
use navree_kernel::{App, AppBuilder, Module, ModuleBuilder, Navree};
use serde_json::json;

use crate::fixtures::{CreateTable, count, tables};
use crate::support::TempDb;

mod m0001_create_companies {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("companies", |t| {
                    t.id();
                    t.string("name").unique();
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("companies").await
        }
    }
}

mod m0002_create_deals {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("deals", |t| {
                    t.id();
                    t.foreign_id("company_id").constrained().cascade_on_delete();
                    t.string("title");
                    t.timestamps();
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("deals").await
        }
    }
}

/// Changes a column of `companies`, which `deals` references: SQLite
/// rebuilds the table.
mod m0004_widen_company_names {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .table("companies", |t| {
                    t.text("name").change();
                    t.string("domain").nullable();
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .table("companies", |t| {
                    t.string("name").change();
                    t.drop_column("domain");
                })
                .await
        }
    }
}

/// Leaves a deal pointing at a company that does not exist.
mod m0005_orphan_deal {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .statement("INSERT INTO deals (company_id, title) VALUES (999, 'orphan')")
                .await
        }

        async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
            Ok(())
        }
    }
}

/// A time-series table: a hypertable on TimescaleDB, an indexed table here.
mod m0001_create_events {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create_hypertable("events", "occurred_at", |t| {
                    t.uuid_primary();
                    t.string("kind");
                    t.timestamp_tz("occurred_at");
                    t.hypertable("occurred_at").retain_for("90 days");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("events").await
        }
    }
}

/// `crm`: companies and deals (module migrations) and tags (a migration of
/// its own type, `CreateTable`), plus
/// `extra` later migrations.
struct Crm {
    extra: Vec<&'static str>,
}

impl Module for Crm {
    fn register(&self, m: &mut ModuleBuilder) {
        let mut set = MigrationSet::new("crm")
            .migration(navree_schema::migration!(m0001_create_companies))
            .migration(navree_schema::migration!(m0002_create_deals))
            .migration(CreateTable {
                name: "m0003_create_tags",
                table: "tags",
                references: None,
            });
        for extra in &self.extra {
            set = match *extra {
                "m0004" => set.migration(navree_schema::migration!(m0004_widen_company_names)),
                "m0005" => set.migration(navree_schema::migration!(m0005_orphan_deal)),
                other => panic!("unknown extra migration {other}"),
            };
        }
        m.migrations(set);
    }
}

struct Events;

impl Module for Events {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("events").migration(navree_schema::migration!(m0001_create_events)),
        );
    }
}

fn crm_app(temp: &TempDb, extra: &[&'static str]) -> AppBuilder {
    Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .module(Crm {
            extra: extra.to_vec(),
        })
}

async fn boot(builder: AppBuilder) -> App {
    builder.boot().await.expect("the app boots")
}

async fn db(app: &App) -> Db {
    (*app.ctx().get::<Db>().await.expect("Db is provided")).clone()
}

async fn batches(db: &Db) -> Vec<(String, i64)> {
    db.pool()
        .fetch_all(&Statement::text(
            "SELECT module || '/' || name AS migration, batch FROM navree_migrations ORDER BY batch, module, name",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (
                row.get::<String>("migration").unwrap(),
                row.get::<i64>("batch").unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_migrations_of_a_set_run_in_name_order() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    let report = Migrator::run(app.ctx(), &MigrateOptions::default())
        .await
        .unwrap();
    assert_eq!(
        report.labels(),
        [
            "crm/m0001_create_companies",
            "crm/m0002_create_deals",
            "crm/m0003_create_tags"
        ]
    );
    assert_eq!(report.migrations[0].batch, 1);
    assert_eq!(
        report.migrations[0].statements,
        [
            "CREATE TABLE \"companies\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \"name\" VARCHAR NOT NULL)",
            "CREATE UNIQUE INDEX \"companies_name_unique\" ON \"companies\" (\"name\")"
        ],
        "a schema migration's statements are recorded"
    );
    let db = db(&app).await;
    assert_eq!(
        tables(&db).await,
        ["companies", "deals", "navree_migrations", "tags"]
    );
    assert!(Migrator::up(app.ctx()).await.unwrap().is_empty());
    let status = Migrator::status(app.ctx()).await.unwrap();
    assert!(status.iter().all(|s| s.applied && s.batch == Some(1)));
}

#[tokio::test]
async fn steps_batches_and_rollbacks() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    let db = db(&app).await;
    Migrator::run(
        app.ctx(),
        &MigrateOptions {
            step: true,
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        batches(&db).await,
        [
            ("crm/m0001_create_companies".to_owned(), 1),
            ("crm/m0002_create_deals".to_owned(), 2),
            ("crm/m0003_create_tags".to_owned(), 3)
        ],
        "--step gives each migration its own batch"
    );

    let reverted = Migrator::rollback_with(
        app.ctx(),
        &RollbackOptions {
            step: Some(2),
            ..RollbackOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        reverted.labels(),
        ["crm/m0003_create_tags", "crm/m0002_create_deals"],
        "the last two migrations, newest first"
    );
    assert_eq!(tables(&db).await, ["companies", "navree_migrations"]);

    Migrator::up(app.ctx()).await.unwrap();
    assert_eq!(
        batches(&db).await,
        [
            ("crm/m0001_create_companies".to_owned(), 1),
            ("crm/m0002_create_deals".to_owned(), 2),
            ("crm/m0003_create_tags".to_owned(), 2)
        ]
    );
    let reverted = Migrator::rollback_with(
        app.ctx(),
        &RollbackOptions {
            batch: Some(1),
            ..RollbackOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(reverted.labels(), ["crm/m0001_create_companies"]);
    assert!(
        Migrator::rollback_with(
            app.ctx(),
            &RollbackOptions {
                batch: Some(7),
                ..RollbackOptions::default()
            },
        )
        .await
        .unwrap()
        .migrations
        .is_empty(),
        "a batch that does not exist reverts nothing"
    );

    let reset = Migrator::reset(app.ctx(), false).await.unwrap();
    assert_eq!(
        reset.labels(),
        ["crm/m0003_create_tags", "crm/m0002_create_deals"],
        "everything still applied, newest first"
    );
    assert_eq!(tables(&db).await, ["navree_migrations"]);
    assert!(batches(&db).await.is_empty());
}

#[tokio::test]
async fn refresh_rolls_back_then_migrates() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    let db = db(&app).await;
    Migrator::up(app.ctx()).await.unwrap();
    db.pool()
        .execute_script("INSERT INTO companies (name) VALUES ('Acme')")
        .await
        .unwrap();
    let refreshed = Migrator::refresh(app.ctx(), None).await.unwrap();
    assert_eq!(refreshed.reverted.migrations.len(), 3);
    assert_eq!(refreshed.migrated.migrations.len(), 3);
    assert_eq!(
        count(&db, "companies").await,
        0,
        "the tables were recreated"
    );

    Migrator::rollback(app.ctx()).await.unwrap();
    Migrator::run(
        app.ctx(),
        &MigrateOptions {
            step: true,
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    let refreshed = Migrator::refresh(app.ctx(), Some(1)).await.unwrap();
    assert_eq!(refreshed.reverted.labels(), ["crm/m0003_create_tags"]);
    assert_eq!(refreshed.migrated.labels(), ["crm/m0003_create_tags"]);
}

#[tokio::test]
async fn pretend_records_the_sql_and_changes_nothing() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    let db = db(&app).await;
    let report = Migrator::run(
        app.ctx(),
        &MigrateOptions {
            pretend: true,
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(report.migrations.len(), 3);
    assert_eq!(
        report.migrations[1].statements[0],
        "CREATE TABLE \"deals\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \"company_id\" INTEGER NOT NULL, \"title\" VARCHAR NOT NULL, \"created_at\" DATETIME, \"updated_at\" DATETIME, FOREIGN KEY (\"company_id\") REFERENCES \"companies\" (\"id\") ON DELETE CASCADE)"
    );
    let tags = &report.migrations[2].statements;
    assert_eq!(tags.len(), 2, "the table and its unique index: {tags:?}");
    assert!(
        tags[0].starts_with("CREATE TABLE \"tags\""),
        "every migration's statements are recorded: {tags:?}"
    );
    assert!(
        tables(&db).await.is_empty(),
        "nothing ran, not even the migrations table"
    );

    Migrator::up(app.ctx()).await.unwrap();
    let report = Migrator::rollback_with(
        app.ctx(),
        &RollbackOptions {
            pretend: true,
            ..RollbackOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        report
            .migrations
            .iter()
            .map(|m| m.statements.clone())
            .collect::<Vec<_>>(),
        [
            vec!["DROP TABLE \"tags\"".to_owned()],
            vec!["DROP TABLE IF EXISTS \"deals\"".to_owned()],
            vec!["DROP TABLE IF EXISTS \"companies\"".to_owned()],
        ]
    );
    assert_eq!(tables(&db).await.len(), 4, "still there");
    assert_eq!(batches(&db).await.len(), 3, "still recorded");
}

#[tokio::test]
async fn sqlite_rebuilds_keep_the_rows_that_reference_the_table() {
    let temp = TempDb::new();
    let first = boot(crm_app(&temp, &[])).await;
    Migrator::up(first.ctx()).await.unwrap();
    let db = db(&first).await;
    db.pool()
        .execute_script(
            "INSERT INTO companies (name) VALUES ('Acme'); \
             INSERT INTO deals (company_id, title) VALUES (1, 'Big one')",
        )
        .await
        .unwrap();
    first.shutdown().await.unwrap();

    let app = boot(crm_app(&temp, &["m0004"])).await;
    let report = Migrator::run(app.ctx(), &MigrateOptions::default())
        .await
        .unwrap();
    assert_eq!(report.labels(), ["crm/m0004_widen_company_names"]);
    let db = self::db(&app).await;
    assert_eq!(
        count(&db, "deals").await,
        1,
        "rebuilding companies did not cascade into deals"
    );
    for _ in 0..3 {
        let on: i32 = db
            .pool()
            .fetch_scalar(&Statement::text("PRAGMA foreign_keys"))
            .await
            .unwrap();
        assert_eq!(on, 1, "the pool still enforces foreign keys");
    }
    Migrator::rollback(app.ctx()).await.unwrap();
    assert_eq!(count(&db, "deals").await, 1);
}

#[tokio::test]
async fn a_migration_leaving_dangling_references_rolls_back() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &["m0005"])).await;
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("crm/m0005_orphan_deal"), "{error}");
    assert!(
        error.contains(
            "leaves 1 row whose foreign key points nowhere: `deals` row 1 references `companies`"
        ),
        "{error}"
    );
    let db = db(&app).await;
    assert_eq!(count(&db, "deals").await, 0, "rolled back");
    let status = Migrator::status(app.ctx()).await.unwrap();
    assert!(!status.last().unwrap().applied);
}

#[tokio::test]
async fn notices_and_options_reach_migrations() {
    let temp = TempDb::new();
    let app = boot(
        Navree::app()
            .module(DatabaseModule::for_root(temp.options()))
            .module(Events),
    )
    .await;
    let report = Migrator::run(app.ctx(), &MigrateOptions::default())
        .await
        .unwrap();
    assert_eq!(
        report.migrations[0].notices,
        [
            "the primary key of `events` is (id, occurred_at), not (id): a hypertable's unique keys must include its time column",
            "SQLite does not support TimescaleDB: `events` is a regular table with an index on `occurred_at`; its retention (90 days) policy is skipped"
        ]
    );

    // The `schema` section configures the builder inside migrations.
    let temp = TempDb::new();
    let app = boot(
        Navree::app()
            .module(ConfigModule::for_root(
                ConfigSources::new().set("schema.hypertable_keys", json!("reject")),
            ))
            .module(DatabaseModule::for_root(temp.options()))
            .module(Events),
    )
    .await;
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    assert!(
        error.contains("its primary key (id) lacks occurred_at"),
        "{error}"
    );
}

#[tokio::test]
async fn a_schema_dump_loads_into_an_empty_database() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    Migrator::up(app.ctx()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema").join("sqlite-schema.sql");
    let written = Migrator::dump(app.ctx(), Some(&path)).await.unwrap();
    assert_eq!(written, path);
    let dump = std::fs::read_to_string(&path).unwrap();
    assert!(dump.contains("CREATE TABLE \"companies\""), "{dump}");
    assert!(
        dump.contains("INSERT INTO \"navree_migrations\" (\"module\", \"name\", \"batch\", \"applied_at\") VALUES ('crm', 'm0001_create_companies', 1, '"),
        "{dump}"
    );

    // A new database: the dump is loaded, then only what it does not record runs.
    let fresh = TempDb::new();
    let app = boot(crm_app(&fresh, &["m0004"])).await;
    let report = Migrator::run(
        app.ctx(),
        &MigrateOptions {
            schema_path: Some(path.clone()),
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    let loaded = report.loaded_dump.as_ref().expect("the dump was loaded");
    assert_eq!(loaded.path, path);
    assert_eq!(report.labels(), ["crm/m0004_widen_company_names"]);
    let db = db(&app).await;
    assert_eq!(
        batches(&db).await.last().unwrap(),
        &("crm/m0004_widen_company_names".to_owned(), 2)
    );

    // A database that has migrations is never loaded into.
    let report = Migrator::run(
        app.ctx(),
        &MigrateOptions {
            schema_path: Some(path),
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(report.loaded_dump.is_none() && report.migrations.is_empty());
}

#[tokio::test]
async fn fresh_drops_views_on_request() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    Migrator::up(app.ctx()).await.unwrap();
    let db = db(&app).await;
    db.pool()
        .execute_script("CREATE VIEW company_names AS SELECT name FROM companies")
        .await
        .unwrap();
    let report = Migrator::fresh_with(
        app.ctx(),
        &FreshOptions {
            drop_views: true,
            ..FreshOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(report.migrations.len(), 3);
    let views = db
        .pool()
        .fetch_all(&Statement::text(
            "SELECT name FROM sqlite_master WHERE type = 'view'",
        ))
        .await
        .unwrap();
    assert!(views.is_empty());
}

async fn run(app: &App, command: &str, args: &[&str]) -> Result<(), String> {
    app.run_command(command, args.iter().map(|a| (*a).to_owned()).collect())
        .await
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn the_commands() {
    let temp = TempDb::new();
    let app = boot(crm_app(&temp, &[])).await;
    let db = db(&app).await;
    run(&app, "migrate:install", &[]).await.unwrap();
    assert_eq!(tables(&db).await, ["navree_migrations"]);
    run(&app, "migrate", &["--pretend"]).await.unwrap();
    assert_eq!(tables(&db).await, ["navree_migrations"]);
    run(&app, "migrate", &["--step"]).await.unwrap();
    assert_eq!(batches(&db).await.len(), 3);
    run(&app, "migrate:status", &["--pending"]).await.unwrap();
    run(&app, "migrate:rollback", &["--step", "1"])
        .await
        .unwrap();
    assert!(!tables(&db).await.contains(&"tags".to_owned()));
    run(&app, "migrate:rollback", &["--batch=2"]).await.unwrap();
    assert!(!tables(&db).await.contains(&"deals".to_owned()));
    run(&app, "migrate:refresh", &[]).await.unwrap();
    assert_eq!(batches(&db).await.len(), 3);
    run(&app, "migrate:reset", &["--pretend"]).await.unwrap();
    assert_eq!(batches(&db).await.len(), 3, "pretending reverts nothing");
    run(&app, "migrate:reset", &[]).await.unwrap();
    assert!(batches(&db).await.is_empty());
    run(&app, "migrate", &[]).await.unwrap();
    assert_eq!(
        run(&app, "migrate:fresh", &["--drop-views", "--drop-types"])
            .await
            .unwrap_err(),
        "--drop-types drops PostgreSQL types; this database is SQLite"
    );
    assert_eq!(
        batches(&db).await.len(),
        3,
        "checked before dropping anything"
    );
    run(&app, "migrate:fresh", &["--drop-views"]).await.unwrap();
    assert_eq!(batches(&db).await.len(), 3);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dump.sql");
    run(&app, "schema:dump", &["--path", path.to_str().unwrap()])
        .await
        .unwrap();
    assert!(Path::new(&path).is_file());

    assert_eq!(
        run(&app, "migrate:rollback", &["--steps", "2"])
            .await
            .unwrap_err(),
        "unknown option `--steps` for migrate:rollback; it takes --step <n>, --batch <n>, --pretend, --force"
    );
    assert_eq!(
        run(&app, "migrate:rollback", &["--step", "two"])
            .await
            .unwrap_err(),
        "`--step` takes a positive whole number, not `two`"
    );
    assert_eq!(
        run(&app, "migrate:rollback", &["--step", "1", "--batch", "1"])
            .await
            .unwrap_err(),
        "migrate:rollback takes --step or --batch, not both"
    );
}

#[tokio::test]
async fn destructive_commands_need_force_in_production() {
    let temp = TempDb::new();
    let app = boot(
        Navree::app()
            .module(ConfigModule::for_root(
                ConfigSources::new().set("env", json!("production")),
            ))
            .module(DatabaseModule::for_root(temp.options()))
            .module(Crm { extra: Vec::new() }),
    )
    .await;
    run(&app, "migrate", &[]).await.unwrap();
    for command in [
        "migrate:fresh",
        "migrate:reset",
        "migrate:refresh",
        "migrate:rollback",
    ] {
        let error = run(&app, command, &[]).await.unwrap_err();
        assert!(
            error.starts_with(&format!(
                "{command} would change or drop tables and this app runs in production (`env = \"production\"`)"
            )),
            "{error}"
        );
        assert!(error.ends_with("pass --force to run it anyway"), "{error}");
    }
    run(&app, "migrate:rollback", &["--force"]).await.unwrap();
    let db = db(&app).await;
    assert!(batches(&db).await.is_empty());
}
