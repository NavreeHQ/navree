//! In-process certificates (`domains.tls = "acme"`) with the fake CA:
//! issuing for verified domains, sealed keys, renewals, failures, expiry
//! reports, the challenge answers, the app's own hosts and reloading.

use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use navree_domains::{DomainsError, TlsStatus, models};
use navree_lighthouse::LighthouseFake;
use navree_orm::Model as _;
use navree_testing::TestAppBuilder;
use serde_json::json;
use uuid::Uuid;

use crate::support::World;

const DAY: Duration = Duration::from_secs(86_400);

fn acme(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("domains.tls", "acme")
        .config("domains.acme.terms_agreed", true)
        .config("domains.acme.contact", json!(["ops@example.com"]))
}

#[tokio::test]
async fn verified_domains_get_certificates() {
    let world = World::with(acme).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    // Verified, waiting for its certificate.
    assert_eq!(domain.tls, TlsStatus::Pending);
    let expires = world.now() + TimeDelta::days(90);
    world.fakes.ca.expire_at(expires);
    let mut events = world.events(&["domains.certificate_issued"]).await;
    let domains = world.domains().await;

    let issued = domains.issue_pending().await.unwrap();
    assert_eq!(issued, ["crm.acme-corp.test"]);
    assert_eq!(world.fakes.ca.orders(), ["crm.acme-corp.test"]);
    let domain = domains.get(domain.id).await.unwrap();
    assert_eq!(domain.tls, TlsStatus::Active);
    assert_eq!(
        domain.certificate_expires_at.map(|at| at.date_naive()),
        Some(expires.date_naive())
    );
    assert!(domains.certificates().contains("crm.acme-corp.test"));
    // The key is sealed with the app key.
    let stored = models::Certificate::query()
        .where_(models::Certificate::HOST.eq("crm.acme-corp.test"))
        .first(&*world.db().await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.domain_id, Some(domain.id));
    assert!(stored.chain_pem.contains("BEGIN CERTIFICATE"));
    assert!(!stored.private_key.contains("PRIVATE KEY"));
    assert!(!format!("{stored:?}").contains(&stored.private_key));
    assert!(serde_json::to_value(&*stored).unwrap().get("private_key").is_none());
    let issued = events.drain();
    assert_eq!(issued[0].1["host"], json!("crm.acme-corp.test"));
    assert_eq!(issued[0].1["renewal"], json!(false));
    // The challenge answer was withdrawn once the order finished.
    let challenges = world.fakes.ca.challenges();
    assert_eq!(challenges.len(), 1);
    assert_eq!(
        domains.challenge_answer(&challenges[0].1).await.unwrap(),
        None
    );
    // Nothing more to issue.
    assert!(domains.issue_pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn only_verified_domains_and_own_hosts_get_certificates() {
    let world = World::with(acme).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    world.add(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    assert!(matches!(
        domains.issue_certificate("crm.acme-corp.test").await,
        Err(DomainsError::NotVerified(_))
    ));
    assert!(matches!(
        domains.issue_certificate("unknown.test").await,
        Err(DomainsError::DomainNotFound)
    ));
    assert!(world.fakes.ca.orders().is_empty());

    let on_demand = World::new().await;
    let workspace = on_demand.workspace(Uuid::now_v7(), "Acme").await;
    on_demand.verified(workspace, "crm.acme-corp.test").await;
    assert!(matches!(
        on_demand
            .domains()
            .await
            .issue_certificate("crm.acme-corp.test")
            .await,
        Err(DomainsError::Config(_))
    ));
}

#[tokio::test]
async fn a_refused_order_is_recorded_on_the_domain() {
    let world = World::with(acme).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    world.fakes.ca.fail_next("too many certificates already issued");
    let domains = world.domains().await;
    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.failed.len(), 1);
    assert!(run.failed[0].1.contains("too many certificates"));
    let domain = domains.get(domain.id).await.unwrap();
    assert_eq!(domain.tls, TlsStatus::Failed);
    assert!(domain.tls_error.unwrap().contains("too many certificates"));
    // Retried once it is due again (check_hours, 24 by default).
    assert!(domains.check_certificates(None).await.unwrap().failed.is_empty());
    world.advance(DAY);
    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.issued, ["crm.acme-corp.test"]);
    assert_eq!(
        domains.get(domain.id).await.unwrap().tls,
        TlsStatus::Active
    );
}

#[tokio::test]
async fn certificates_are_renewed_before_they_expire_and_reported_when_they_are_not() {
    let lighthouse = LighthouseFake::new();
    let reporter = lighthouse.lighthouse();
    let world = World::with(|builder| acme(builder).override_value(Arc::new(reporter))).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    world.fakes.ca.expire_at(world.now() + TimeDelta::days(45));
    assert_eq!(
        domains.check_certificates(None).await.unwrap().issued,
        ["crm.acme-corp.test"]
    );
    // 45 days left: nothing to do (renewals start 30 days before).
    world.advance(DAY);
    let run = domains.check_certificates(None).await.unwrap();
    assert!(run.renewed.is_empty() && run.expiring.is_empty(), "{run:?}");
    // 30 days before: renewed.
    world.advance(14 * DAY);
    world.fakes.ca.expire_at(world.now() + TimeDelta::days(90));
    let mut events = world
        .events(&["domains.certificate_issued", "domains.certificate_expiring"])
        .await;
    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.renewed, ["crm.acme-corp.test"]);
    let names = events.drain();
    assert_eq!(names[0].1["renewal"], json!(true));
    lighthouse.assert_nothing_reported();

    // Renewals that keep failing: reported once it is within 14 days.
    world.advance(80 * DAY);
    world.fakes.ca.fail_next("the CA is down");
    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.failed.len(), 1);
    assert_eq!(run.expiring.len(), 1);
    assert_eq!(run.expiring[0].0, "crm.acme-corp.test");
    // The fake CA's certificates end at midnight.
    let days_left = (run.expiring[0].1 - world.now()).num_days();
    assert!((9..=10).contains(&days_left), "{days_left}");
    let expiring = events.drain();
    assert_eq!(expiring.len(), 1);
    assert_eq!(expiring[0].0, "domains.certificate_expiring");
    assert_eq!(expiring[0].1["domain_id"], json!(domain.id));
    assert_eq!(expiring[0].1["days_left"], json!(days_left));
    let reports = lighthouse.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].message,
        format!("The certificate for crm.acme-corp.test expires in {days_left} days")
    );
    assert_eq!(reports[0].tags["domain"], "crm.acme-corp.test");
}

#[tokio::test]
async fn challenge_answers_are_served_while_an_order_is_open() {
    let world = World::with(acme).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    world.verified(workspace, "crm.acme-corp.test").await;
    world.fakes.ca.keep_challenges();
    world
        .domains()
        .await
        .issue_certificate("crm.acme-corp.test")
        .await
        .unwrap();
    let (_, token, answer) = world.fakes.ca.challenges().remove(0);
    let response = world
        .app
        .client()
        .get(&format!("/.well-known/acme-challenge/{token}"))
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.text(), answer);
    assert_eq!(response.header("content-type"), Some("text/plain"));
    world
        .app
        .client()
        .get("/.well-known/acme-challenge/unknown")
        .send()
        .await
        .assert_status(404);
    // Answers expire after an hour.
    world.advance(Duration::from_secs(3601));
    world
        .app
        .client()
        .get(&format!("/.well-known/acme-challenge/{token}"))
        .send()
        .await
        .assert_status(404);
    // No ask endpoint with ACME.
    world
        .app
        .client()
        .get("/_domains/allow?domain=crm.acme-corp.test")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_apps_own_hosts_get_certificates_too() {
    let world = World::with(|builder| {
        acme(builder).config("domains.acme.hosts", json!(["app.example.com"]))
    })
    .await;
    let domains = world.domains().await;
    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.issued, ["app.example.com"]);
    assert!(domains.certificates().contains("app.example.com"));
    let stored = models::Certificate::query()
        .where_(models::Certificate::HOST.eq("app.example.com"))
        .first(&*world.db().await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.domain_id, None);
    // Not due again until it is close to expiry.
    assert!(domains.check_certificates(None).await.unwrap().issued.is_empty());
    assert_eq!(world.fakes.ca.orders(), ["app.example.com"]);
}

#[tokio::test]
async fn stored_certificates_are_loaded_by_every_server() {
    let world = World::with(acme).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    domains.issue_certificate("crm.acme-corp.test").await.unwrap();
    // Another server: nothing in memory, everything in the database.
    domains.certificates().remove("crm.acme-corp.test");
    assert!(domains.certificates().hosts().is_empty());
    assert_eq!(domains.reload_certificates().await.unwrap(), 1);
    assert_eq!(domains.certificates().hosts(), ["crm.acme-corp.test"]);
    // A removed domain's certificate goes with it.
    domains.remove(domain.id).await.unwrap();
    assert!(domains.certificates().hosts().is_empty());
    assert_eq!(domains.reload_certificates().await.unwrap(), 0);
}

#[tokio::test]
async fn acme_needs_an_app_key_to_store_certificates() {
    let world = World::with(|builder| acme(builder).config("app.key", "")).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    world.verified(workspace, "crm.acme-corp.test").await;
    let error = world
        .domains()
        .await
        .issue_certificate("crm.acme-corp.test")
        .await
        .unwrap_err();
    assert!(
        matches!(error, DomainsError::Config(_) | DomainsError::Crypto(_)),
        "{error}"
    );
    // Nothing was ordered.
    assert!(world.fakes.ca.orders().is_empty());
}
