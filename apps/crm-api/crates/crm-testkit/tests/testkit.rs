//! The test kit itself: a booted CRM with the demo accounts, real JWT
//! logins per role, the fixed clock, the queue and the realtime channels.

use std::time::Duration;

use chrono::{TimeZone, Utc};
use crm_testkit::{Role, TestCrm};
use navree::contracts::Clock;
use navree::mail::{Mailer, TemplateMail};
use navree::prelude::*;
use serde_json::{Value, json};

/// A feature module the way the CRM's are written: a controller with an
/// access-checked route.
mod ping {
    use navree::prelude::*;

    #[injectable]
    pub struct PingController;

    #[controller("/ping", version = "1")]
    impl PingController {
        #[get("/")]
        #[can(Read, crm_core::Deal)]
        async fn ping(&self, CurrentUser(user): CurrentUser) -> Json<String> {
            Json(user.email.clone())
        }
    }

    #[module(imports = [crm_core::CrmCoreModule], controllers = [PingController])]
    pub struct PingModule;
}

#[tokio::test]
async fn login_as_signs_in_each_demo_account_over_http() {
    let crm = TestCrm::builder().build().await;
    for (role, email) in [
        (Role::Admin, "admin@navree.test"),
        (Role::Manager, "manager@navree.test"),
        (Role::Rep1, "rep1@navree.test"),
        (Role::Rep2, "rep2@navree.test"),
    ] {
        let client = crm.login_as(role).await;
        assert_eq!(client.id(), crm.user_id(role));
        assert_eq!(role.email(), email);
        client
            .get("/api/auth/me")
            .send()
            .await
            .assert_status(200)
            .assert_json_path("email", json!(email))
            .assert_json_path("id", json!(crm.user_id(role)));
    }
    crm.client()
        .get("/api/auth/me")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    crm.shutdown().await;
}

#[tokio::test]
async fn feature_modules_join_the_app() {
    let crm = TestCrm::builder().module(ping::PingModule).build().await;
    let rep = crm.login_as(Role::Rep2).await;
    let body = rep
        .get("/api/v1/ping")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body, json!("rep2@navree.test"));
    crm.client()
        .get("/api/v1/ping")
        .send()
        .await
        .assert_status(401);
    crm.shutdown().await;
}

#[tokio::test]
async fn each_role_gets_its_abilities() {
    let crm = TestCrm::builder().build().await;
    let rules = |body: Value| body.as_array().cloned().expect("a rule array");

    let admin = crm.login_as(Role::Admin).await;
    let body = admin
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        rules(body),
        [json!({ "action": "manage", "subject": "all", "reason": "super admin" })]
    );

    let manager = crm.login_as(Role::Manager).await;
    let body = manager
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let manager_rules = rules(body);
    for granted in [
        json!({ "action": "manage", "subject": "Company" }),
        json!({ "action": "manage", "subject": "Deal" }),
        json!({ "action": "export", "subject": "Contact" }),
        json!({ "action": "read", "subject": "User" }),
    ] {
        assert!(
            manager_rules.contains(&granted),
            "{granted} missing from {manager_rules:#?}"
        );
    }

    let rep = crm.login_as(Role::Rep1).await;
    let body = rep
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let rep_rules = rules(body);
    assert!(
        !rep_rules.iter().any(|rule| rule["action"] == "manage"),
        "{rep_rules:#?}"
    );
    assert!(rep_rules.contains(&json!({
        "action": "update",
        "subject": "Deal",
        "conditions": { "owner_id": crm.user_id(Role::Rep1) },
    })));
    assert_eq!(
        rep_rules.last().map(|rule| rule["inverted"].clone()),
        Some(json!(true)),
        "won/lost deals rule is last: {rep_rules:#?}"
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn the_clock_is_fixed_and_shared_with_the_app() {
    let start = Utc.with_ymd_and_hms(2026, 3, 2, 8, 0, 0).unwrap();
    let crm = TestCrm::builder().now(start).build().await;
    let clock = crm.get::<dyn Clock>().await;
    assert_eq!(clock.now(), start);
    crm.clock().advance(Duration::from_secs(3600));
    assert_eq!(clock.now(), start + chrono::Duration::hours(1));
    assert_eq!(crm.clock().now(), clock.now());

    let default = TestCrm::builder().build().await;
    assert_eq!(default.clock().now(), TestCrm::start());
    crm.shutdown().await;
    default.shutdown().await;
}

/// Registers the mail template the queue test sends.
struct HelloMailModule;

impl Module for HelloMailModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/hello.txt", "Hello {{ name }}")]);
    }
}

#[tokio::test]
async fn run_queue_runs_queued_jobs() {
    let crm = TestCrm::builder().module(HelloMailModule).build().await;
    let mailer = crm.get::<Mailer>().await;
    mailer
        .queue(
            &TemplateMail::new("rep1@navree.test", "Hello", "mail/hello")
                .context(json!({ "name": "Priya" })),
        )
        .await
        .expect("queued");
    let sent = || mailer.array_transport().expect("array transport").sent();
    assert!(sent().is_empty(), "queued, not sent yet");
    let stats = crm.run_queue().await;
    assert_eq!((stats.succeeded, stats.failed), (1, 0), "{stats:?}");
    assert_eq!(sent()[0].text.as_deref(), Some("Hello Priya"));
    assert_eq!(crm.run_queue().await.succeeded, 0);
    crm.shutdown().await;
}

#[tokio::test]
async fn private_channels_authorize_by_ability_and_owner() {
    let crm = TestCrm::builder().build().await;
    let rep = crm.login_as(Role::Rep1).await;
    let auth = |channel: String| {
        rep.post("/broadcasting/auth")
            .json(&json!({ "socket_id": "123.456", "channel_name": channel }))
    };
    auth(crm_core::DEALS_CHANNEL.to_owned())
        .send()
        .await
        .assert_status(200);
    auth(format!("private-users.{}", crm.user_id(Role::Rep1)))
        .send()
        .await
        .assert_status(200);
    auth(format!("private-users.{}", crm.user_id(Role::Rep2)))
        .send()
        .await
        .assert_status(403);
    crm.shutdown().await;
}

#[tokio::test]
async fn config_overrides_reach_the_app() {
    let crm = TestCrm::builder()
        .config("api.title", "Testing title")
        .build()
        .await;
    crm.client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("info.title", json!("Testing title"));
    crm.shutdown().await;
}
