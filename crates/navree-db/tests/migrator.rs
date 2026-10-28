//! The migrator on navree-db's connections, the same scenarios on SQLite,
//! PostgreSQL with TimescaleDB (`NAVREE_TEST_POSTGRES_URL`, the `postgres`
//! feature) and MySQL (`NAVREE_TEST_MYSQL_URL`, the `mysql` feature):
//! `--pretend` output, status, dump and load round trips, and what a failing
//! migration leaves behind.

use std::sync::atomic::{AtomicU32, Ordering};

use navree_db::{
    Backend, DatabaseModule, Db, DbOptions, Executor, ExecutorExt, MigrateOptions, MigrationSet,
    Migrator, ModuleBuilderDbExt, Pool, RollbackOptions, Statement,
};
use navree_kernel::{App, Module, ModuleBuilder, Navree};
use navree_schema::{Schema, SchemaResult, raw};

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
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("deals").await
        }
    }
}

/// Seeds a company (a data migration).
mod m0003_seed_acme {
    use navree_schema::{Schema, SchemaResult};

    pub struct Migration;

    impl navree_schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .statement("INSERT INTO companies (name) VALUES ('Acme')")
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .statement("DELETE FROM companies WHERE name = 'Acme'")
                .await
        }
    }
}

/// The CRM's three migrations.
struct Crm;

impl Module for Crm {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("crm")
                .migration(navree_schema::migration!(m0001_create_companies))
                .migration(navree_schema::migration!(m0002_create_deals))
                .migration(navree_schema::migration!(m0003_seed_acme)),
        );
    }
}

/// Creates a table, then fails.
struct CreatesThenFails;

impl navree_schema::Migration for CreatesThenFails {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("partial", |t| {
                t.id();
            })
            .await?;
        schema.statement("SELECT * FROM no_such_table").await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("partial").await
    }
}

/// Inserts a row, then fails; asks for a transaction on MySQL too.
struct InsertsThenFails;

impl navree_schema::Migration for InsertsThenFails {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .statement("INSERT INTO companies (name) VALUES ('Partial')")
            .await?;
        schema.statement("SELECT * FROM no_such_table").await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }

    fn within_transaction_on_mysql(&self) -> bool {
        true
    }
}

/// The failing migrations, after the CRM's.
struct Failing {
    data: bool,
}

impl Module for Failing {
    fn register(&self, m: &mut ModuleBuilder) {
        let set = MigrationSet::new("failing").after(["crm"]);
        m.migrations(if self.data {
            set.migration(navree_schema::SchemaMigration::new(
                "m0001_inserts_then_fails",
                InsertsThenFails,
            ))
        } else {
            set.migration(navree_schema::SchemaMigration::new(
                "m0001_creates_then_fails",
                CreatesThenFails,
            ))
        });
    }
}

async fn boot(url: &str, failing: Option<bool>) -> App {
    let app = Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            url: url.to_owned(),
            ..DbOptions::default()
        }))
        .module(Crm);
    let app = match failing {
        Some(data) => app.module(Failing { data }),
        None => app,
    };
    app.boot().await.expect("boots")
}

async fn pool_of(app: &App) -> Pool {
    app.ctx()
        .get::<Db>()
        .await
        .unwrap()
        .pool()
        .unwrap()
        .clone()
}

async fn tables(pool: &Pool) -> Vec<String> {
    let sql = match pool.backend() {
        Backend::Sqlite => {
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        }
        Backend::Postgres => {
            "SELECT tablename::text FROM pg_tables WHERE schemaname = current_schema() ORDER BY tablename"
        }
        Backend::MySql => {
            "SELECT CAST(table_name AS CHAR) FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' ORDER BY table_name"
        }
    };
    pool.fetch_column(&Statement::text(sql)).await.unwrap()
}

async fn companies(pool: &Pool) -> Vec<String> {
    pool.fetch_column(&Statement::text("SELECT name FROM companies ORDER BY name"))
        .await
        .unwrap()
}

/// `--pretend`, status, rollbacks, and a dump loaded into `copy_url`.
async fn pretend_status_and_dumps(url: &str, copy_url: &str) {
    let app = boot(url, None).await;
    let ctx = app.ctx();
    let pool = pool_of(&app).await;
    let backend = pool.backend();

    let before = Migrator::status(ctx).await.unwrap();
    assert_eq!(
        before
            .iter()
            .map(|s| (s.module.as_str(), s.name.as_str(), s.applied))
            .collect::<Vec<_>>(),
        [
            ("crm", "m0001_create_companies", false),
            ("crm", "m0002_create_deals", false),
            ("crm", "m0003_seed_acme", false),
        ]
    );

    let pretended = Migrator::run(
        ctx,
        &MigrateOptions {
            pretend: true,
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    let statements: Vec<Vec<String>> = pretended
        .migrations
        .iter()
        .map(|m| m.statements.clone())
        .collect();
    let companies_sql = match backend {
        Backend::Postgres => {
            "CREATE TABLE \"companies\" (\"id\" BIGINT NOT NULL GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, \"name\" VARCHAR(255) NOT NULL)"
        }
        Backend::MySql => {
            "CREATE TABLE `companies` (`id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY, `name` VARCHAR(255) NOT NULL) DEFAULT CHARACTER SET utf8mb4"
        }
        Backend::Sqlite => {
            "CREATE TABLE \"companies\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \"name\" VARCHAR NOT NULL)"
        }
    };
    assert_eq!(statements[0][0], companies_sql, "{statements:?}");
    assert_eq!(
        statements[2],
        ["INSERT INTO companies (name) VALUES ('Acme')"]
    );
    assert_eq!(
        tables(&pool).await,
        ["navree_migrations"],
        "pretending runs nothing (status created the migrations table)"
    );

    let report = Migrator::run(ctx, &MigrateOptions::default()).await.unwrap();
    assert_eq!(report.migrations.len(), 3);
    assert_eq!(report.migrations[0].statements[0], companies_sql);
    assert_eq!(companies(&pool).await, ["Acme"]);
    let after = Migrator::status(ctx).await.unwrap();
    assert!(
        after
            .iter()
            .all(|s| s.applied && s.batch == Some(1) && s.applied_at.is_some()),
        "{after:?}"
    );
    let applied_at = after[0].applied_at.unwrap();
    assert!((chrono::Utc::now() - applied_at).num_minutes().abs() < 5, "{applied_at}");

    // Pretending to roll back prints the downs and changes nothing.
    let pretended = Migrator::rollback_with(
        ctx,
        &RollbackOptions {
            pretend: true,
            ..RollbackOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(pretended.labels().len(), 3);
    assert_eq!(
        pretended.migrations[0].statements,
        ["DELETE FROM companies WHERE name = 'Acme'"]
    );
    assert_eq!(companies(&pool).await, ["Acme"]);

    // A dump loads into an empty database, which then has nothing pending.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema.sql");
    Migrator::dump(ctx, Some(&path)).await.unwrap();
    let dump = std::fs::read_to_string(&path).unwrap();
    assert!(dump.contains("navree_migrations"), "{dump}");

    let copy = boot(copy_url, None).await;
    let loaded = Migrator::run(
        copy.ctx(),
        &MigrateOptions {
            schema_path: Some(path.clone()),
            ..MigrateOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(loaded.loaded_dump.is_some());
    assert!(loaded.migrations.is_empty(), "the dump records every migration");
    let copied = Migrator::status(copy.ctx()).await.unwrap();
    assert_eq!(
        copied
            .iter()
            .map(|s| (s.name.clone(), s.batch))
            .collect::<Vec<_>>(),
        after.iter().map(|s| (s.name.clone(), s.batch)).collect::<Vec<_>>()
    );
    let copy_pool = pool_of(&copy).await;
    assert_eq!(tables(&copy_pool).await, tables(&pool).await);
    let again = dir.path().join("again.sql");
    Migrator::dump(copy.ctx(), Some(&again)).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&again).unwrap(),
        dump,
        "a loaded dump dumps the same"
    );
    copy.shutdown().await.unwrap();

    // Down the whole way, then up again.
    let reverted = Migrator::reset(ctx, false).await.unwrap();
    assert_eq!(
        reverted.labels(),
        [
            "crm/m0003_seed_acme",
            "crm/m0002_create_deals",
            "crm/m0001_create_companies"
        ]
    );
    assert_eq!(tables(&pool).await, ["navree_migrations"]);
    Migrator::up(ctx).await.unwrap();
    app.shutdown().await.unwrap();
}

/// A migration that creates a table and fails leaves neither the table nor
/// its row where DDL is transactional; on MySQL the table stays (DDL commits
/// at once) and the row is not written. A data migration that asks for a
/// transaction rolls its rows back everywhere.
async fn failures(url: &str) {
    let app = boot(url, Some(false)).await;
    let ctx = app.ctx();
    let pool = pool_of(&app).await;
    let mysql = pool.backend() == Backend::MySql;
    let error = Migrator::up(ctx).await.unwrap_err().to_string();
    assert!(
        error.contains("failing/m0001_creates_then_fails") && error.contains("no_such_table"),
        "{error}"
    );
    let left = tables(&pool).await;
    assert_eq!(
        left.contains(&"partial".to_owned()),
        mysql,
        "the table stays only on MySQL: {left:?}"
    );
    let status = Migrator::status(ctx).await.unwrap();
    assert_eq!(
        status.iter().filter(|s| s.applied).count(),
        3,
        "the CRM's ran; the failing one is not recorded"
    );
    let recorded: i64 = pool
        .fetch_scalar(&Statement::text(
            "SELECT COUNT(*) FROM navree_migrations WHERE module = 'failing'",
        ))
        .await
        .unwrap();
    assert_eq!(recorded, 0);
    if mysql {
        pool.execute_script("DROP TABLE partial").await.unwrap();
    }
    app.shutdown().await.unwrap();

    let app = boot(url, Some(true)).await;
    let pool = pool_of(&app).await;
    let error = Migrator::up(app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("failing/m0001_inserts_then_fails"), "{error}");
    assert_eq!(
        companies(&pool).await,
        ["Acme"],
        "the inserted row rolled back with the migration"
    );
    app.shutdown().await.unwrap();
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own on the server of `variable`.
struct Scratch {
    admin: Pool,
    name: String,
    url: String,
}

impl Scratch {
    async fn new(variable: &str) -> Option<Scratch> {
        let Ok(admin_url) = std::env::var(variable) else {
            eprintln!("{variable} is not set: skipping");
            return None;
        };
        let name = format!(
            "navree_migrator_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let admin = Pool::connect(&DbOptions {
            url: admin_url.clone(),
            max_connections: 1,
            ..DbOptions::default()
        })
        .await
        .unwrap();
        admin
            .execute_script(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        let (base, query) = match admin_url.split_once('?') {
            Some((base, query)) => (base.to_owned(), format!("?{query}")),
            None => (admin_url.clone(), String::new()),
        };
        let host_end = base.rfind('/').unwrap_or(base.len());
        let url = format!("{}/{name}{query}", &base[..host_end]);
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let force = if self.admin.backend() == Backend::Postgres {
            " WITH (FORCE)"
        } else {
            ""
        };
        self.admin
            .execute_script(&format!("DROP DATABASE IF EXISTS {}{force}", self.name))
            .await
            .unwrap();
        self.admin.close().await;
    }
}

fn sqlite_url(dir: &tempfile::TempDir, file: &str) -> String {
    format!("sqlite://{}?mode=rwc", dir.path().join(file).display())
}

#[tokio::test]
async fn pretend_status_and_dumps_on_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    pretend_status_and_dumps(&sqlite_url(&dir, "a.db"), &sqlite_url(&dir, "b.db")).await;
}

#[tokio::test]
async fn failures_on_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    failures(&sqlite_url(&dir, "a.db")).await;
}

async fn on_server(variable: &str) {
    let (Some(a), Some(b), Some(c)) = (
        Scratch::new(variable).await,
        Scratch::new(variable).await,
        Scratch::new(variable).await,
    ) else {
        return;
    };
    pretend_status_and_dumps(&a.url, &b.url).await;
    failures(&c.url).await;
    for scratch in [a, b, c] {
        scratch.drop().await;
    }
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn on_postgres() {
    on_server("NAVREE_TEST_POSTGRES_URL").await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn on_mysql() {
    on_server("NAVREE_TEST_MYSQL_URL").await;
}

/// The schema builder runs on each of navree-db's connections: the pool,
/// a transaction (rolled back with it) and the `Db`.
#[tokio::test]
async fn schemas_run_on_every_connection() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&DbOptions {
        url: sqlite_url(&dir, "a.db"),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let schema = Schema::new(&db);
    schema
        .create("notes", |t| {
            t.id();
            t.string("body").default(raw("'x'"));
        })
        .await
        .unwrap();
    assert!(Schema::new(db.pool().unwrap()).has_table("notes").await.unwrap());

    let tx = db.begin().await.unwrap();
    Schema::new(&tx).drop("notes").await.unwrap();
    assert!(!Schema::new(&tx).has_table("notes").await.unwrap());
    tx.rollback().await.unwrap();
    assert!(Schema::new(&db).has_table("notes").await.unwrap());

    let error = Schema::new(&db).drop("missing").await.unwrap_err();
    let navree_schema::SchemaError::Database { source, .. } = &error else {
        panic!("{error:?}");
    };
    assert!(
        source.downcast_ref::<navree_db::DbError>().is_some(),
        "the driver's error is navree-db's: {source:?}"
    );
}
