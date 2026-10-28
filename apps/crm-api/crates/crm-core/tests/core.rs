//! crm-core against a real app: the platform modules plus `CrmCoreModule`,
//! booted on a temporary SQLite database. The models themselves are tested
//! in `models.rs`.

use chrono::Utc;
use crm_core::{CrmCoreModule, permissions, platform};
use navree::access::{AbilityFactory, AccessService};
use navree::audit::AuditLog;
use navree::db::Migrator;
use navree::prelude::*;
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use serde_json::Value;

struct Core {
    app: TestApp,
    _db: TempDirGuard,
}

fn sources(url: String) -> ConfigSources {
    ConfigSources::new()
        .set("app.key", navree::crypto::generate_key())
        .set("database.url", url)
        .set("database.migrate_on_boot", true)
        .set("http.per_ip_rate_limit", Value::Null)
        .set("mail.transport", "array")
        .set("hashing.memory_kib", 1024)
        .set("hashing.iterations", 1)
        .set("observe.metrics", false)
        .set("log.level", "warn")
}

async fn boot() -> Core {
    let (url, db) = temp_sqlite_url();
    let app =
        TestApp::from_builder(platform(Navree::app(), sources(url)).module(CrmCoreModule)).await;
    Core { app, _db: db }
}

#[tokio::test]
async fn crm_migrations_run_after_auth_and_access() {
    let core = boot().await;
    let status = Migrator::status(core.app.ctx()).await.expect("status");
    let position = |module: &str| {
        status
            .iter()
            .position(|m| m.module == module)
            .unwrap_or_else(|| panic!("no `{module}` migrations in {status:?}"))
    };
    assert!(position("crm") > position("auth"));
    assert!(position("crm") > position("access"));
    assert!(
        status
            .iter()
            .filter(|m| m.module == "crm")
            .all(|m| m.applied)
    );
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn crm_subjects_map_their_permission_prefixes() {
    let core = boot().await;
    let factory = core.app.get::<AbilityFactory>().await;
    let prefixes = factory.permission_subjects();
    for (prefix, subject) in [
        ("companies", "Company"),
        ("contacts", "Contact"),
        ("deals", "Deal"),
        ("activities", "Activity"),
        ("users", "User"),
    ] {
        assert_eq!(
            prefixes.get(prefix).map(String::as_str),
            Some(subject),
            "{prefixes:?}"
        );
    }
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn roles_seeder_creates_the_roles_once() {
    let core = boot().await;
    core.app
        .run_command("db:seed", &["crm-roles"])
        .await
        .expect("seeded");
    core.app
        .run_command("db:seed", &["crm-roles"])
        .await
        .expect("seeding again is a no-op");
    let access = core.app.get::<AccessService>().await;
    let roles: Vec<(String, Vec<String>)> = access
        .roles()
        .await
        .expect("roles")
        .into_iter()
        .map(|role| (role.name, role.permissions))
        .collect();
    let mut manager = permissions::MANAGER.map(str::to_owned).to_vec();
    manager.sort();
    assert_eq!(
        roles,
        [
            ("admin".to_owned(), vec![]),
            ("manager".to_owned(), manager),
            ("rep".to_owned(), vec![]),
        ]
    );
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn platform_and_core_pass_check() {
    let (url, _db) = temp_sqlite_url();
    platform(Navree::app(), sources(url))
        .module(CrmCoreModule)
        .run_with_args(vec!["check".to_owned()])
        .await
        .expect("`check` passes");
}

#[tokio::test]
async fn the_platform_provides_the_system_clock() {
    let core = boot().await;
    let clock = core.app.get::<dyn navree::contracts::Clock>().await;
    let drift = (clock.now() - Utc::now()).num_seconds().abs();
    assert!(drift < 5, "the system clock is {drift}s off");
    core.app.shutdown().await.expect("shutdown");
}

/// `migrate:fresh --seed` on a migrated, seeded CRM database: every table
/// (navree-access's `role_permissions` with its two cascading parents
/// included) is dropped and recreated, and the roles are seeded again.
#[tokio::test]
async fn migrate_fresh_runs_on_a_migrated_database() {
    let (url, _db) = temp_sqlite_url();
    let app =
        TestApp::from_builder(platform(Navree::app(), sources(url)).module(CrmCoreModule)).await;
    app.run_command("db:seed", &[]).await.expect("db:seed");
    // The seed's audit entries are written in the background: let them
    // land before the tables go.
    app.get::<AuditLog>().await.flush().await;
    app.run_command("migrate:fresh", &["--seed"])
        .await
        .expect("migrate:fresh drops and recreates every table");
    let roles = app
        .get::<AccessService>()
        .await
        .roles()
        .await
        .expect("roles");
    assert_eq!(roles.len(), 3, "{roles:?}");
    app.shutdown().await.expect("shutdown");
}

/// `migrate:reset` takes the CRM's tables away (every migration reverted,
/// newest first) and `migrate` puts them back: the schema-builder
/// migrations run both ways.
#[tokio::test]
async fn crm_migrations_revert_and_run_again() {
    let core = boot().await;
    let crm_tables = |tables: Vec<String>| -> Vec<String> {
        let mut found: Vec<String> = tables
            .into_iter()
            .filter(|table| {
                ["companies", "contacts", "deals", "activities"].contains(&table.as_str())
            })
            .collect();
        found.sort();
        found
    };
    let db = core.app.get::<navree::db::Db>().await;
    let listing = || async {
        navree::schema::Schema::new(&db)
            .table_listing()
            .await
            .expect("tables")
    };
    assert_eq!(
        crm_tables(listing().await),
        ["activities", "companies", "contacts", "deals"]
    );
    core.app
        .run_command("migrate:reset", &[])
        .await
        .expect("reverted");
    assert!(crm_tables(listing().await).is_empty());
    core.app
        .run_command("migrate", &[])
        .await
        .expect("migrated again");
    assert_eq!(
        crm_tables(listing().await),
        ["activities", "companies", "contacts", "deals"]
    );
    core.app.shutdown().await.expect("shutdown");
}
