//! Security events reach the audit table, not
//! only stdout: throttled logins, rate limiting, and access denials raised
//! outside the route authorizer (the owner rule, private
//! channels a user may not join).

use crm_core::seed::DEMO_PASSWORD;
use crm_testkit::{Role, TestCrm};
use navree::audit::{AuditFilter, AuditLog, AuditRecord};
use navree::db::PageQuery;
use serde_json::json;

async fn crm() -> TestCrm {
    TestCrm::builder().app(crm_api::app).build().await
}

/// The audit entries with `action`, newest first (after the background
/// writer caught up).
async fn entries(crm: &TestCrm, action: &str) -> Vec<AuditRecord> {
    let log = crm.get::<AuditLog>().await;
    log.flush().await;
    log.list(
        &AuditFilter {
            action: Some(action.to_owned()),
            ..AuditFilter::default()
        },
        PageQuery::new(1, 100),
    )
    .await
    .expect("the audit log lists")
    .data
}

#[tokio::test]
async fn throttled_logins_are_audited() {
    let crm = crm().await;
    let login = |password: &'static str| {
        crm.client()
            .post("/api/auth/login")
            .json(&json!({ "email": Role::Rep1.email(), "password": password }))
            .send()
    };
    for _ in 0..5 {
        login("wrong password").await.assert_status(401);
    }
    login("wrong password").await.assert_status(429);
    login(DEMO_PASSWORD).await.assert_status(429);

    let throttled = entries(&crm, "auth.throttled").await;
    assert_eq!(throttled.len(), 1, "{throttled:?}");
    assert_eq!(throttled[0].kind, "security");
    assert_eq!(throttled[0].metadata["flow"], "login");
    assert_eq!(throttled[0].metadata["email"], Role::Rep1.email());
    assert!(throttled[0].ip.is_some());
    crm.shutdown().await;
}

#[tokio::test]
async fn denials_outside_the_route_authorizer_are_audited() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = rep
        .post("/api/v1/companies")
        .json(&json!({ "name": "Agra" }))
        .send()
        .await
        .assert_status(201)
        .json_value()["id"]
        .clone();

    // A rep may not hand a new record to someone else.
    rep.post("/api/v1/deals")
        .json(&json!({
            "title": "Renewal",
            "company_id": company,
            "amount_cents": 100,
            "owner_id": crm.user_id(Role::Rep2),
        }))
        .send()
        .await
        .assert_status(403);
    // Another user's private channel.
    rep.post("/broadcasting/auth")
        .json(&json!({
            "socket_id": "123.456",
            "channel_name": format!("private-users.{}", crm.user_id(Role::Rep2)),
        }))
        .send()
        .await
        .assert_status(403);

    let denied = entries(&crm, "access.denied").await;
    let routes: Vec<&str> = denied
        .iter()
        .map(|entry| entry.metadata["route"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        routes,
        ["/broadcasting/auth", "/api/v1/deals"],
        "{denied:?}"
    );
    for entry in &denied {
        assert_eq!(entry.kind, "security");
        assert_eq!(entry.actor_id, Some(rep.id()));
        assert_eq!(entry.metadata["check"], "handler");
        assert!(entry.request_id.is_some());
    }
    crm.shutdown().await;
}
