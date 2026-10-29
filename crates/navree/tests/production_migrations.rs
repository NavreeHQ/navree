//! Every crate's production migrations, run by the migrator: up, again (no
//! change), down (`migrate:reset`), fresh and a rollback, on SQLite, on
//! PostgreSQL with TimescaleDB (`NAVREE_TEST_POSTGRES_URL`, the `postgres`
//! feature) and on MySQL (`NAVREE_TEST_MYSQL_URL`, the `mysql` feature).
//!
//! ```text
//! cargo test -p navree --test production_migrations \
//!   --features postgres,mysql,social,oauth,flags,media,billing,search,ai,micro,i18n
//! ```
//!
//! Upgrades: `NAVREE_MIGRATE_ONLY` (database URLs, separated by spaces)
//! only migrates those databases and keeps them, so another version of
//! Navree can migrate a database with this same file;
//! `NAVREE_UPGRADE_DATABASES` checks databases migrated that way: nothing
//! is pending, every row reads back, and they roll back cleanly.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};

use navree::config::{ConfigModule, ConfigSources};
use navree::db::{DatabaseModule, Db, DbOptions, Executor, ExecutorExt, Migrator, Pool, Statement};
use navree::kernel::{App, AppBuilder, Navree};

/// Whether `url` is a MySQL or MariaDB database, where navree-search's
/// default engine (`postgres`: PostgreSQL's or SQLite's full-text search)
/// does not run (its migrations create nothing there).
fn is_mysql(url: &str) -> bool {
    url.starts_with("mysql") || url.starts_with("mariadb")
}

/// Every module with production migrations, as an app adds it (search not
/// on MySQL).
fn modules(app: AppBuilder, url: &str) -> AppBuilder {
    let app = app
        .module(navree::crypto::CryptoModule::from_config())
        .module(navree::view::ViewModule::from_config())
        .module(navree::storage::StorageModule::from_config())
        .module(navree::access::AccessModule::for_root(
            navree::access::AccessOptions::default(),
        ))
        .module(navree::audit::AuditModule::from_config())
        .module(navree::auth::AuthModule::from_config())
        .module(navree::directory::DirectoryModule::from_config())
        .module(navree::domains::DomainsModule::from_config())
        .module(navree::fleet::FleetModule::from_config())
        .module(navree::integrations::IntegrationsModule::from_config())
        .module(navree::lighthouse::LighthouseModule::from_config())
        .module(navree::mail::MailModule::from_config())
        .module(navree::notify::NotifyModule::from_config())
        .module(navree::queue::QueueModule::from_config())
        .module(navree::schedule::ScheduleModule::for_root())
        .module(navree::session::SessionModule::from_config())
        .module(navree::settings::SettingsModule::from_config())
        .module(navree::sheets::SheetsModule::from_config())
        .module(navree::sso::SsoModule::from_config())
        .module(navree::webhooks::WebhooksModule::from_config())
        .module(navree::workflows::WorkflowsModule::from_config())
        .module(navree::workspace::WorkspaceModule::from_config());
    #[cfg(feature = "ai")]
    let app = app.module(navree::ai::AiModule::from_config());
    #[cfg(feature = "billing")]
    let app = app.module(navree::billing::BillingModule::from_config());
    #[cfg(feature = "flags")]
    let app = app.module(navree::flags::FlagsModule::from_config());
    #[cfg(feature = "i18n")]
    let app = app.module(navree::i18n::I18nModule::from_config());
    #[cfg(feature = "media")]
    let app = app.module(navree::media::MediaModule::from_config());
    #[cfg(feature = "micro")]
    let app = app.module(navree::micro::MicroModule::from_config());
    #[cfg(feature = "oauth")]
    let app = app.module(navree::oauth::OAuthServerModule::from_config());
    #[cfg(feature = "search")]
    let app = if is_mysql(url) {
        app
    } else {
        app.module(navree::search::SearchModule::from_config())
    };
    let _ = url;
    #[cfg(feature = "social")]
    let app = app.module(navree::social::SocialModule::from_config());
    app
}

/// The modules whose migration sets the app must run.
fn expected_modules(url: &str) -> BTreeSet<&'static str> {
    let mut modules = BTreeSet::from([
        "access",
        "audit",
        "auth",
        "directory",
        "domains",
        "fleet",
        "integrations",
        "lighthouse",
        "mail",
        "notify",
        "queue",
        "schedule",
        "session",
        "settings",
        "sheets",
        "sso",
        "webhooks",
        "workflows",
        "workspace",
    ]);
    for (on, module) in [
        (cfg!(feature = "ai"), "ai"),
        (cfg!(feature = "billing"), "billing"),
        (cfg!(feature = "flags"), "flags"),
        (cfg!(feature = "i18n"), "i18n"),
        (cfg!(feature = "media"), "media"),
        (cfg!(feature = "micro"), "micro"),
        (cfg!(feature = "oauth"), "oauth"),
        (cfg!(feature = "search") && !is_mysql(url), "search"),
        (cfg!(feature = "social"), "social"),
    ] {
        if on {
            modules.insert(module);
        }
    }
    modules
}

/// The app on the database at `url`.
async fn app(url: &str) -> App {
    let sources = ConfigSources::new()
        .set("database.url", url)
        .set("database.max_connections", 4)
        .set("app.key", navree::crypto::generate_key())
        .set("billing.provider", "fake")
        .set("billing.routes", false)
        .set("micro.transport", "memory")
        .set(
            "social.redirect_base",
            "https://app.example.com/auth/callback",
        )
        .set("ai.fake", true)
        .set("search.sync", "inline")
        // Lighthouse records nothing while its tables come and go (a flush
        // racing a down migration would add a chunk to a hypertable being
        // dropped).
        .set("lighthouse.enabled", false);
    modules(
        Navree::app()
            .module(ConfigModule::for_root(sources))
            .module(DatabaseModule::from_config()),
        url,
    )
    .boot()
    .await
    .unwrap_or_else(|error| panic!("the app boots on {url}: {error}"))
}

/// The tables of the database (views and the database's own excluded).
async fn tables(pool: &Pool) -> Vec<String> {
    let sql = match pool.backend() {
        navree::db::Backend::Sqlite => {
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        }
        navree::db::Backend::Postgres => {
            "SELECT tablename::text FROM pg_tables WHERE schemaname = current_schema() ORDER BY tablename"
        }
        navree::db::Backend::MySql => {
            "SELECT CAST(table_name AS CHAR) FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' ORDER BY table_name"
        }
    };
    pool.fetch_column::<String>(&Statement::text(sql))
        .await
        .expect("lists the tables")
}

async fn pool_of(app: &App) -> Pool {
    let db = app.ctx().get::<Db>().await.expect("a Db");
    db.pool().expect("navree-db's pool").clone()
}

/// What one database showed.
#[derive(Debug)]
#[allow(dead_code)]
struct Outcome {
    modules: usize,
    migrations: usize,
}

/// Up, again, down, fresh and a rollback on the database at `url`.
async fn every_migration(url: &str) -> Outcome {
    let app = app(url).await;
    let ctx = app.ctx();

    let applied = Migrator::up(ctx).await.expect("migrates");
    let status = Migrator::status(ctx).await.expect("status");
    assert!(!applied.is_empty());
    assert_eq!(
        applied.len(),
        status.len(),
        "every registered migration ran"
    );
    assert!(status.iter().all(|s| s.applied && s.batch == Some(1)));
    let modules: BTreeSet<&str> = status.iter().map(|s| s.module.as_str()).collect();
    for module in expected_modules(url) {
        assert!(modules.contains(module), "the `{module}` migrations ran");
    }
    let pool = pool_of(&app).await;
    let created = tables(&pool).await;
    assert!(created.len() > modules.len(), "{created:?}");

    assert!(
        Migrator::up(ctx).await.expect("migrates again").is_empty(),
        "nothing is pending after a run"
    );

    let reverted = Migrator::reset(ctx, false).await.expect("resets");
    assert_eq!(
        reverted.migrations.len(),
        applied.len(),
        "every migration rolled back"
    );
    assert!(
        Migrator::status(ctx)
            .await
            .unwrap()
            .iter()
            .all(|s| !s.applied)
    );
    assert_eq!(
        tables(&pool).await,
        ["navree_migrations"],
        "the down migrations drop every table the up migrations created"
    );

    let fresh = Migrator::fresh(ctx).await.expect("fresh");
    assert_eq!(fresh.len(), applied.len());
    assert_eq!(tables(&pool).await, created, "fresh builds the same tables");

    let last = Migrator::rollback(ctx).await.expect("rolls back");
    assert_eq!(last.len(), applied.len(), "fresh is one batch");
    assert_eq!(tables(&pool).await, ["navree_migrations"]);
    Migrator::up(ctx).await.expect("migrates once more");

    Outcome {
        modules: modules.len(),
        migrations: applied.len(),
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own on the server of `variable`, dropped by
/// [`Scratch::drop`].
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
            "navree_production_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let admin = Pool::connect(&DbOptions {
            url: admin_url.clone(),
            max_connections: 1,
            ..DbOptions::default()
        })
        .await
        .expect("the admin URL connects");
        admin
            .execute_script(&format!("CREATE DATABASE {name}"))
            .await
            .expect("the test user may create databases");
        let url = with_database(&admin_url, &name);
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let force = if self.admin.backend() == navree::db::Backend::Postgres {
            " WITH (FORCE)"
        } else {
            ""
        };
        self.admin
            .execute_script(&format!("DROP DATABASE IF EXISTS {}{force}", self.name))
            .await
            .expect("drops the database");
        self.admin.close().await;
    }
}

/// `url` with its database replaced by `database`, keeping the query.
fn with_database(url: &str, database: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, format!("?{query}")),
        None => (url, String::new()),
    };
    let after_scheme = base.find("://").map_or(0, |at| at + 3);
    let host_end = base[after_scheme..]
        .find('/')
        .map_or(base.len(), |at| after_scheme + at);
    format!("{}/{database}{query}", &base[..host_end])
}

#[tokio::test]
async fn every_migration_on_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("app.db").display());
    let outcome = every_migration(&url).await;
    eprintln!("SQLite: {outcome:?}");
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn every_migration_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL").await else {
        return;
    };
    let outcome = every_migration(&scratch.url).await;
    eprintln!("PostgreSQL: {outcome:?}");
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn every_migration_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL").await else {
        return;
    };
    let outcome = every_migration(&scratch.url).await;
    eprintln!("MySQL: {outcome:?}");
    scratch.drop().await;
}

/// The URLs of an environment variable, separated by spaces.
fn urls(variable: &str) -> Vec<String> {
    std::env::var(variable)
        .map(|v| v.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Creates the database of server URL `url` afresh, through the admin URL
/// of its server's test variable.
async fn recreate(url: &str) {
    let variable = if url.starts_with("postgres") {
        "NAVREE_TEST_POSTGRES_URL"
    } else {
        "NAVREE_TEST_MYSQL_URL"
    };
    let admin_url = std::env::var(variable).expect("the server's admin URL");
    let name = url
        .split('?')
        .next()
        .and_then(|base| base.rsplit('/').next())
        .expect("a database name")
        .to_owned();
    let admin = Pool::connect(&DbOptions {
        url: admin_url,
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .expect("the admin URL connects");
    let force = if url.starts_with("postgres") {
        " WITH (FORCE)"
    } else {
        ""
    };
    admin
        .execute_script(&format!("DROP DATABASE IF EXISTS {name}{force}"))
        .await
        .expect("drops the database");
    admin
        .execute_script(&format!("CREATE DATABASE {name}"))
        .await
        .expect("creates the database");
    admin.close().await;
}

/// Migrates the databases of `NAVREE_MIGRATE_ONLY` (server databases
/// created afresh) and keeps them.
#[tokio::test]
async fn migrate_only() {
    for url in urls("NAVREE_MIGRATE_ONLY") {
        if !url.starts_with("sqlite") {
            recreate(&url).await;
        }
        let app = app(&url).await;
        let applied = Migrator::up(app.ctx()).await.expect("migrates");
        eprintln!("{url}: {} migrations applied", applied.len());
    }
}

/// Checks the databases of `NAVREE_UPGRADE_DATABASES`, migrated by another
/// version: nothing pending, the recorded rows read back, and they roll
/// back cleanly.
#[tokio::test]
async fn upgrade() {
    for url in urls("NAVREE_UPGRADE_DATABASES") {
        let app = app(&url).await;
        let ctx = app.ctx();
        let status = Migrator::status(ctx).await.expect("status");
        let pending: Vec<String> = status
            .iter()
            .filter(|s| !s.applied)
            .map(|s| format!("{}/{}", s.module, s.name))
            .collect();
        assert!(
            pending.is_empty(),
            "{url}: pending after the upgrade: {pending:?}"
        );
        assert!(
            status
                .iter()
                .all(|s| s.batch.is_some() && s.applied_at.is_some()),
            "{url}: every recorded row reads back"
        );
        let report = Migrator::run(ctx, &navree::db::MigrateOptions::default())
            .await
            .expect("migrates");
        assert!(report.migrations.is_empty(), "{url}: nothing runs again");
        let reverted = Migrator::reset(ctx, false).await.expect("rolls back");
        assert_eq!(reverted.migrations.len(), status.len());
        let pool = pool_of(&app).await;
        assert_eq!(tables(&pool).await, ["navree_migrations"], "{url}");
        Migrator::up(ctx).await.expect("migrates again");
        eprintln!(
            "{url}: {} migrations recorded by the previous migrator, none pending, all rolled back",
            status.len()
        );
    }
}
