//! The whole CRM API: `crm_api::app` boots, passes `check`, serves the
//! contract's routes, and `db:seed` fills it with the demo data.

use std::collections::BTreeSet;
use std::path::Path;

use crm_core::Stage;
use crm_core::models::{Activity, ActivityScopes, Company, Contact, Deal};
use navree::config::Config;
use navree::contracts::Clock;
use navree::db::Db;
use navree::orm::Model;
use navree::prelude::*;
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use serde_json::{Value, json};

/// Hermetic settings for one test app on the database at `url`.
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

async fn boot() -> (TestApp, TempDirGuard) {
    let (url, db) = temp_sqlite_url();
    (TestApp::from_builder(crm_api::app(sources(url))).await, db)
}

#[tokio::test]
async fn boots_and_check_passes() {
    let (url, _db) = temp_sqlite_url();
    let report = crm_api::app(sources(url.clone()))
        .validate()
        .expect("every import and provider lines up");
    let modules: BTreeSet<&str> = report.modules.iter().map(|m| m.name).collect();
    for module in [
        "CrmCoreModule",
        "UsersModule",
        "CompaniesModule",
        "ContactsModule",
        "DealsModule",
        "ActivitiesModule",
        "DashboardModule",
        "DemoModule",
        "AuthModule",
        "AccessModule",
        "RealtimeModule",
    ] {
        assert!(
            modules.contains(module),
            "{module} is missing from {modules:?}"
        );
    }
    crm_api::app(sources(url))
        .run_with_args(vec!["check".to_owned()])
        .await
        .expect("`check` passes");
}

#[tokio::test]
async fn serves_the_contract_routes() {
    let (app, _db) = boot().await;
    let doc = app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("info.title", json!("Navree CRM API"))
        .json_value();
    for (path, method) in [
        ("/api/auth/login", "post"),
        ("/api/auth/refresh", "post"),
        ("/api/auth/logout", "post"),
        ("/api/auth/me", "get"),
        ("/api/me/abilities", "get"),
        ("/api/v1/me", "get"),
        ("/api/v1/users", "get"),
        ("/api/v1/users", "post"),
        ("/api/v1/users/{id}", "patch"),
        ("/api/v1/companies", "get"),
        ("/api/v1/companies", "post"),
        ("/api/v1/companies/{id}", "get"),
        ("/api/v1/companies/{id}", "patch"),
        ("/api/v1/companies/{id}", "delete"),
        ("/api/v1/contacts", "get"),
        ("/api/v1/contacts", "post"),
        ("/api/v1/contacts/{id}", "get"),
        ("/api/v1/contacts/{id}", "patch"),
        ("/api/v1/contacts/{id}", "delete"),
        ("/api/v1/contacts/export", "get"),
        ("/api/v1/deals", "get"),
        ("/api/v1/deals", "post"),
        ("/api/v1/deals/board", "get"),
        ("/api/v1/deals/{id}", "get"),
        ("/api/v1/deals/{id}", "patch"),
        ("/api/v1/deals/{id}", "delete"),
        ("/api/v1/deals/{id}/move", "post"),
        ("/api/v1/activities", "get"),
        ("/api/v1/activities", "post"),
        ("/api/v1/activities/{id}", "patch"),
        ("/api/v1/activities/{id}/complete", "post"),
        ("/api/v1/activities/{id}", "delete"),
        ("/api/v1/dashboard", "get"),
        ("/api/notifications", "get"),
        ("/api/notifications/unread-count", "get"),
        ("/api/notifications/{id}/read", "post"),
        ("/api/notifications/read-all", "post"),
        ("/api/audit", "get"),
    ] {
        assert!(
            doc["paths"][path][method].is_object(),
            "{method} {path} is missing from {:#}",
            doc["paths"]
        );
    }

    // Every CRM route documents its JSON body and success response (the
    // CSV export's body is not JSON; 204s have none), tag and errors.
    let mut undocumented = Vec::new();
    for (path, operations) in doc["paths"].as_object().expect("paths") {
        if !path.starts_with("/api/v1/") {
            continue;
        }
        for (method, operation) in operations.as_object().expect("operations") {
            let responses = operation["responses"].as_object().expect("responses");
            let (status, success) = responses
                .iter()
                .find(|(status, _)| status.starts_with('2'))
                .expect("a success response");
            let json = &success["content"]["application/json"]["schema"];
            if status != "204" && path != "/api/v1/contacts/export" && json.is_null() {
                undocumented.push(format!("{method} {path}: no {status} schema"));
            }
            let has_body =
                matches!(method.as_str(), "post" | "patch") && !path.ends_with("/complete");
            if has_body
                && operation["requestBody"]["content"]["application/json"]["schema"].is_null()
            {
                undocumented.push(format!("{method} {path}: no request schema"));
            }
            if operation["tags"].as_array().is_none_or(Vec::is_empty) {
                undocumented.push(format!("{method} {path}: no tag"));
            }
            if !responses.contains_key("401") {
                undocumented.push(format!("{method} {path}: no 401"));
            }
        }
    }
    assert!(undocumented.is_empty(), "{undocumented:#?}");
    // Public sign-up is off: accounts are created by admins.
    app.client()
        .post("/api/auth/register")
        .json(
            &json!({ "name": "Esha", "email": "esha@navree.test", "password": "a long password" }),
        )
        .send()
        .await
        .assert_status(404);
    app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn seeder_creates_demo_accounts() {
    let (app, _db) = boot().await;
    app.run_command("db:seed", &[]).await.expect("db:seed");

    for (email, role) in [
        ("admin@navree.test", "admin"),
        ("manager@navree.test", "manager"),
        ("rep1@navree.test", "rep"),
        ("rep2@navree.test", "rep"),
    ] {
        app.client()
            .post("/api/auth/login")
            .json(&json!({ "email": email, "password": "password123" }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("user.email", json!(email))
            .assert_json_path("user.roles", json!([role]));
    }

    let db = app.get::<Db>().await;
    let db = &*db;
    let counts = (
        Company::query().with_trashed().count(db).await.unwrap(),
        Contact::query().with_trashed().count(db).await.unwrap(),
        Deal::query().count(db).await.unwrap(),
        Activity::query().count(db).await.unwrap(),
    );
    assert_eq!(counts, (20, 50, 30, 60));
    assert!(
        Company::query()
            .only_trashed()
            .doesnt_exist(db)
            .await
            .unwrap(),
        "no demo company is deleted"
    );

    let deals = Deal::query().get(db).await.unwrap();
    let stages: BTreeSet<Stage> = deals.iter().map(|d| d.stage).collect();
    assert_eq!(stages.len(), Stage::ALL.len(), "every stage has deals");
    for stage in Stage::ALL {
        let mut positions: Vec<i32> = deals
            .iter()
            .filter(|d| d.stage == *stage)
            .map(|d| d.position)
            .collect();
        positions.sort_unstable();
        let expected: Vec<i32> = (0..positions.len() as i32).collect();
        assert_eq!(
            positions, expected,
            "{stage} positions are 0-based and dense"
        );
    }
    for deal in &deals {
        assert_eq!(deal.closed_at.is_some(), deal.stage.is_closed(), "{deal:?}");
        assert_eq!(
            deal.lost_reason.is_some(),
            deal.stage == Stage::Lost,
            "{deal:?}"
        );
    }

    // Tasks fall in every bucket of the task list.
    let now = app.get::<dyn Clock>().await.now();
    let today = now.date_naive();
    let tasks = Activity::query().open_tasks().get(db).await.unwrap();
    let due = |a: &Activity| a.due_at.expect("tasks are due").date_naive();
    assert!(tasks.iter().any(|t| due(t) < today), "an overdue task");
    assert!(tasks.iter().any(|t| due(t) == today), "a task due today");
    assert!(tasks.iter().any(|t| due(t) > today), "an upcoming task");

    // Seeding again adds nothing.
    app.run_command("db:seed", &[])
        .await
        .expect("db:seed again");
    assert_eq!(Company::query().with_trashed().count(db).await.unwrap(), 20);
    app.shutdown().await.expect("shutdown");
}

#[test]
fn dev_config_points_at_the_local_web_app() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = Config::load(&ConfigSources::new().file(dir.join("config/app.toml")))
        .expect("config/app.toml parses");
    for (key, value) in [
        ("database.url", "sqlite://storage/crm.db?mode=rwc"),
        ("http.addr", "127.0.0.1:8080"),
        ("app.url", "http://localhost:3000"),
        (
            "auth.password_reset.url",
            "http://localhost:3000/reset-password",
        ),
        ("realtime.key", "navree-crm-key"),
        ("mail.transport", "log"),
    ] {
        assert_eq!(config.get_string(key).as_deref(), Some(value), "{key}");
    }
    let origins: Vec<String> = config
        .section("http.cors.allowed_origins")
        .expect("origins");
    assert_eq!(origins, ["http://localhost:3000"]);
    let realtime: Vec<String> = config.section("realtime.allowed_origins").expect("origins");
    assert_eq!(realtime, ["http://localhost:3000"]);
    assert!(dir.join(".env.example").is_file());
}
