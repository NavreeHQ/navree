//! `GET /api/audit`: filtered, paginated, newest first, permission `audit.read`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navis_audit::AuditModule;
use navis_contracts::{AuditEntry, AuditKind, Clock, FixedClock};
use navis_db::DatabaseModule;
use navis_testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{self, audit_log};

/// An app whose clock moves one minute per entry, with `admin` granted
/// `audit.read`.
async fn seeded() -> (TestApp, Uuid, Uuid, Uuid) {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
    ));
    let (builder, fixtures) = support::builder();
    let app = builder
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let admin = Uuid::now_v7();
    let alice = Uuid::now_v7();
    let bob = Uuid::now_v7();
    fixtures.authorizer.grant(admin, "audit.read");

    let log = audit_log(&app).await;
    let entries = [
        AuditEntry::new(AuditKind::Data, "deal.created")
            .actor(alice)
            .subject("Deal", "d1"),
        AuditEntry::new(AuditKind::Data, "deal.updated")
            .actor(alice)
            .subject("Deal", "d1"),
        AuditEntry::new(AuditKind::Data, "company.updated")
            .actor(bob)
            .subject("Company", "c1"),
        AuditEntry::new(AuditKind::Auth, "auth.login").actor(bob),
        AuditEntry::new(AuditKind::Access, "access.role_assigned")
            .actor(admin)
            .subject("User", "u1"),
        AuditEntry::new(AuditKind::Security, "access.denied")
            .actor(alice)
            .subject("Deal", "d2"),
        AuditEntry::new(AuditKind::Data, "deal.updated")
            .actor(bob)
            .subject("Deal", "d2"),
    ];
    for entry in entries {
        log.record(entry).await.unwrap();
        clock.advance(Duration::from_secs(60));
    }
    log.flush().await;
    (app, admin, alice, bob)
}

fn actions(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|item| item["action"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn list_endpoint_filters_and_paginates() {
    let (app, admin, alice, _bob) = seeded().await;
    let client = app.acting_as(principal(admin));

    let all = client.get("/api/audit").send().await.assert_status(200);
    let body = all.json_value();
    assert_eq!(
        actions(&body),
        [
            "deal.updated",
            "access.denied",
            "access.role_assigned",
            "auth.login",
            "company.updated",
            "deal.updated",
            "deal.created"
        ],
        "newest first"
    );
    assert_eq!(
        body["meta"],
        json!({"current_page": 1, "per_page": 15, "total": 7, "last_page": 1})
    );
    let first = &body["data"][6];
    assert_eq!(first["kind"], "data");
    assert_eq!(first["actor_id"], json!(alice));
    assert_eq!(first["subject_type"], "Deal");
    assert_eq!(first["subject_id"], "d1");
    assert_eq!(first["created_at"], "2026-10-01T09:00:00Z");
    for key in ["id", "old_values", "new_values", "request_id", "metadata"] {
        assert!(first.get(key).is_some(), "missing `{key}` in {first}");
    }

    let page = client
        .get("/api/audit")
        .query(&[("per_page", "3"), ("page", "2")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        actions(&page),
        ["auth.login", "company.updated", "deal.updated"]
    );
    assert_eq!(
        page["meta"],
        json!({"current_page": 2, "per_page": 3, "total": 7, "last_page": 3})
    );

    let by_actor = client
        .get("/api/audit")
        .query(&[("actor_id", alice.to_string())])
        .send()
        .await
        .json_value();
    assert_eq!(
        actions(&by_actor),
        ["access.denied", "deal.updated", "deal.created"]
    );

    let by_record = client
        .get("/api/audit")
        .query(&[("subject_type", "Deal"), ("subject_id", "d2")])
        .send()
        .await
        .json_value();
    assert_eq!(actions(&by_record), ["deal.updated", "access.denied"]);

    let exact = client
        .get("/api/audit")
        .query(&[("action", "deal.updated")])
        .send()
        .await
        .json_value();
    assert_eq!(actions(&exact), ["deal.updated", "deal.updated"]);

    let verb = client
        .get("/api/audit")
        .query(&[("action", "updated")])
        .send()
        .await
        .json_value();
    assert_eq!(
        actions(&verb),
        ["deal.updated", "company.updated", "deal.updated"],
        "a bare verb matches every `<subject>.<verb>`"
    );

    let underscore = client
        .get("/api/audit")
        .query(&[("action", "role_assigned")])
        .send()
        .await
        .json_value();
    assert_eq!(actions(&underscore), ["access.role_assigned"]);

    let by_kind = client
        .get("/api/audit")
        .query(&[("kind", "security")])
        .send()
        .await
        .json_value();
    assert_eq!(actions(&by_kind), ["access.denied"]);
}

#[tokio::test]
async fn list_requires_the_audit_read_permission() {
    let (app, _admin, alice, _bob) = seeded().await;
    app.client()
        .get("/api/audit")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    app.acting_as(principal(alice))
        .get("/api/audit")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn malformed_filters_are_400() {
    let (app, admin, _alice, _bob) = seeded().await;
    let response = app
        .acting_as(principal(admin))
        .get("/api/audit")
        .query(&[("actor_id", "nope"), ("kind", "other")])
        .send()
        .await
        .assert_validation_error("actor_id")
        .assert_validation_error("kind");
    assert_eq!(
        response.json_value()["message"],
        serde_json::json!([
            "actor_id must be a UUID",
            "kind must be one of the following values: data, auth, access, security, custom",
        ])
    );
}

#[tokio::test]
async fn routes_can_be_turned_off_for_apps_without_an_authorizer() {
    // No RouteAuthorizer (no FixturesModule): the app still boots.
    let app = TestApp::builder()
        .temp_database()
        .config("audit.routes", false)
        .module(DatabaseModule)
        .module(AuditModule)
        .build()
        .await;
    app.acting_as(principal(Uuid::now_v7()))
        .get("/api/audit")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn action_filter_treats_wildcards_literally() {
    let (builder, fixtures) = support::builder();
    let app = builder.build().await;
    let admin = Uuid::now_v7();
    fixtures.authorizer.grant(admin, "audit.read");
    let log = audit_log(&app).await;
    for action in ["user.roleXassigned", "user.role_assigned", "user.100%"] {
        log.record(AuditEntry::new(AuditKind::Access, action))
            .await
            .unwrap();
    }
    log.flush().await;
    let client = app.acting_as(principal(admin));

    for (filter, expected) in [
        ("role_assigned", vec!["user.role_assigned"]),
        ("100%", vec!["user.100%"]),
        ("%", vec![]),
    ] {
        let body = client
            .get("/api/audit")
            .query(&[("action", filter)])
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(actions(&body), expected, "filter `{filter}`");
    }
}

#[tokio::test]
async fn route_is_documented_for_openapi() {
    let app = support::app().await;
    let routes = navis_http::route_table(app.ctx()).await.unwrap();
    let route = routes
        .iter()
        .find(|r| r.full_path == "/api/audit")
        .expect("GET /api/audit is mounted");
    assert_eq!(route.method, "GET");
    assert_eq!(route.access.permissions, ["audit.read"]);
    assert_eq!(route.name.as_deref(), Some("audit.index"));
    let api = route.api.as_ref().expect("api metadata");
    assert_eq!(api.tags, ["Audit"]);
    let (name, _schema) = (api.response.expect("response schema"))();
    assert_eq!(name, "AuditPage");
}
