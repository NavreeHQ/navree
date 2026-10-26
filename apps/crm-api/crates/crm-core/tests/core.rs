//! crm-core against a real app: the platform modules plus `CrmCoreModule`,
//! booted on a temporary SQLite database.

use chrono::{NaiveDate, TimeZone, Utc};
use crm_core::embeds::{CompanyRef, ContactRef, OwnerRef};
use crm_core::entities::{activity, company, contact, deal};
use crm_core::enums::{ActivityKind, CompanySize, ContactStatus, Stage, SubjectType};
use crm_core::{CrmCoreModule, permissions, platform};
use navis::access::{AbilityFactory, AccessService};
use navis::audit::AuditLog;
use navis::auth::{AuthService, User};
use navis::db::{Db, Migrator};
use navis::prelude::*;
use navis::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, EntityTrait};
use serde_json::Value;
use uuid::Uuid;

struct Core {
    app: TestApp,
    _db: TempDirGuard,
}

fn sources(url: String) -> ConfigSources {
    ConfigSources::new()
        .set("app.key", navis::crypto::generate_key())
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
        TestApp::from_builder(platform(Navis::app(), sources(url)).module(CrmCoreModule)).await;
    Core { app, _db: db }
}

async fn user(core: &Core, name: &str, email: &str) -> User {
    core.app
        .get::<AuthService>()
        .await
        .create_user(name, email, "password123")
        .await
        .expect("the user is created")
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
async fn every_crm_table_round_trips_its_columns() {
    let core = boot().await;
    let owner = user(&core, "Priya Shah", "rep1@navis.test").await;
    let db = core.app.get::<Db>().await;
    let conn = db.conn();
    let now = Utc.with_ymd_and_hms(2026, 10, 26, 9, 30, 0).unwrap();

    let acme = company::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set("Acme".into()),
        domain: Set(Some("acme.test".into())),
        industry: Set(Some("Software".into())),
        size: Set(Some(CompanySize::Medium)),
        phone: Set(None),
        website: Set(Some("https://acme.test".into())),
        city: Set(Some("Berlin".into())),
        country: Set(Some("Germany".into())),
        owner_id: Set(owner.id),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
    }
    .insert(conn)
    .await
    .expect("company");
    let ada = contact::ActiveModel {
        id: Set(Uuid::now_v7()),
        first_name: Set("Ada".into()),
        last_name: Set("Lovelace".into()),
        email: Set(Some("ada@acme.test".into())),
        phone: Set(None),
        title: Set(Some("CTO".into())),
        status: Set(ContactStatus::Prospect),
        company_id: Set(Some(acme.id)),
        owner_id: Set(owner.id),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
    }
    .insert(conn)
    .await
    .expect("contact");
    let renewal = deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set("Renewal".into()),
        amount_cents: Set(1_250_000),
        currency: Set("USD".into()),
        stage: Set(Stage::Negotiation),
        position: Set(0),
        expected_close_date: Set(NaiveDate::from_ymd_opt(2026, 12, 1)),
        closed_at: Set(None),
        lost_reason: Set(None),
        company_id: Set(acme.id),
        contact_id: Set(Some(ada.id)),
        owner_id: Set(owner.id),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(conn)
    .await
    .expect("deal");
    let call = activity::ActiveModel {
        id: Set(Uuid::now_v7()),
        kind: Set(ActivityKind::Task),
        subject: Set("Send the quote".into()),
        body: Set(Some("Include the SLA".into())),
        due_at: Set(Some(now)),
        completed_at: Set(None),
        subject_type: Set(SubjectType::Deal),
        subject_id: Set(renewal.id),
        owner_id: Set(owner.id),
        assignee_id: Set(Some(owner.id)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(conn)
    .await
    .expect("activity");

    let found = company::Entity::find_by_id(acme.id)
        .one(conn)
        .await
        .unwrap();
    assert_eq!(found.as_ref(), Some(&acme));
    let found = contact::Entity::find_by_id(ada.id)
        .one(conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (found.status, found.full_name()),
        (ContactStatus::Prospect, "Ada Lovelace".to_owned())
    );
    let found = deal::Entity::find_by_id(renewal.id)
        .one(conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((found.stage, found.probability()), (Stage::Negotiation, 75));
    assert_eq!(found, renewal);
    let found = activity::Entity::find_by_id(call.id)
        .one(conn)
        .await
        .unwrap();
    assert_eq!(found, Some(call));

    // Enum columns hold the contract strings, which ability conditions see.
    let record = navis::access::record_json(&renewal);
    assert_eq!(record["stage"], "negotiation");
    assert_eq!(record["owner_id"], owner.id.to_string());
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
async fn embeds_load_names_by_id() {
    let core = boot().await;
    let priya = user(&core, "Priya Shah", "rep1@navis.test").await;
    let tom = user(&core, "Tom Becker", "rep2@navis.test").await;
    let db = core.app.get::<Db>().await;
    let conn = db.conn();
    let now = Utc::now();
    let acme = company::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set("Acme".into()),
        owner_id: Set(priya.id),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await
    .expect("company");
    let ada = contact::ActiveModel {
        id: Set(Uuid::now_v7()),
        first_name: Set("Ada".into()),
        last_name: Set("Lovelace".into()),
        status: Set(ContactStatus::Lead),
        owner_id: Set(tom.id),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await
    .expect("contact");

    let unknown = Uuid::now_v7();
    let owners = OwnerRef::load(conn, [priya.id, tom.id, priya.id, unknown])
        .await
        .expect("owners");
    assert_eq!(owners.len(), 2);
    assert_eq!(
        owners[&tom.id],
        OwnerRef {
            id: tom.id,
            name: "Tom Becker".into()
        }
    );
    let companies = CompanyRef::load(conn, [acme.id]).await.expect("companies");
    assert_eq!(
        companies[&acme.id],
        CompanyRef {
            id: acme.id,
            name: "Acme".into()
        }
    );
    let contacts = ContactRef::load(conn, [ada.id]).await.expect("contacts");
    assert_eq!(
        contacts[&ada.id],
        ContactRef {
            id: ada.id,
            full_name: "Ada Lovelace".into()
        }
    );
    assert!(OwnerRef::load(conn, []).await.expect("none").is_empty());
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn platform_and_core_pass_check() {
    let (url, _db) = temp_sqlite_url();
    platform(Navis::app(), sources(url))
        .module(CrmCoreModule)
        .run_with_args(vec!["check".to_owned()])
        .await
        .expect("`check` passes");
}

#[tokio::test]
async fn the_platform_provides_the_system_clock() {
    let core = boot().await;
    let clock = core.app.get::<dyn navis::contracts::Clock>().await;
    let drift = (clock.now() - Utc::now()).num_seconds().abs();
    assert!(drift < 5, "the system clock is {drift}s off");
    core.app.shutdown().await.expect("shutdown");
}

/// `migrate:fresh --seed` on a migrated, seeded CRM database: every table
/// (navis-access's `role_permissions` with its two cascading parents
/// included) is dropped and recreated, and the roles are seeded again.
#[tokio::test]
async fn migrate_fresh_runs_on_a_migrated_database() {
    let (url, _db) = temp_sqlite_url();
    let app =
        TestApp::from_builder(platform(Navis::app(), sources(url)).module(CrmCoreModule)).await;
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
