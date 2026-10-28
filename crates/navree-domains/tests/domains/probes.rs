//! On-demand TLS: the proxy's certificates read over TLS, their expiry
//! recorded and reported.

use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use navree_domains::TlsStatus;
use navree_lighthouse::LighthouseFake;
use serde_json::json;
use uuid::Uuid;

use crate::support::World;

const DAY: Duration = Duration::from_secs(86_400);

#[tokio::test]
async fn certificates_of_verified_domains_are_probed_daily() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    world.add(workspace, "pending.acme-corp.test").await;
    let expires = world.now() + TimeDelta::days(60);
    world.fakes.probe.expires("crm.acme-corp.test", expires);
    let domains = world.domains().await;

    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.probed, [("crm.acme-corp.test".to_owned(), expires)]);
    assert!(run.expiring.is_empty());
    // Pending domains are not probed.
    assert_eq!(world.fakes.probe.probed(), ["crm.acme-corp.test"]);
    let domain = domains.get(domain.id).await.unwrap();
    assert_eq!(domain.certificate_expires_at, Some(expires));
    assert_eq!(domain.tls, TlsStatus::Active);
    // Due again a day later.
    assert!(
        domains
            .check_certificates(None)
            .await
            .unwrap()
            .probed
            .is_empty()
    );
    world.advance(DAY);
    assert_eq!(
        domains.check_certificates(None).await.unwrap().probed.len(),
        1
    );
    // A host given by name is probed now.
    assert_eq!(
        domains
            .check_certificates(Some("crm.acme-corp.test"))
            .await
            .unwrap()
            .probed
            .len(),
        1
    );
}

#[tokio::test]
async fn certificates_close_to_expiry_are_reported() {
    let lighthouse = LighthouseFake::new();
    let reporter = lighthouse.lighthouse();
    let world = World::with(|builder| builder.override_value(Arc::new(reporter))).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    world
        .fakes
        .probe
        .expires("crm.acme-corp.test", world.now() + TimeDelta::days(5));
    let mut events = world.events(&["domains.certificate_expiring"]).await;
    let run = world
        .domains()
        .await
        .check_certificates(None)
        .await
        .unwrap();
    assert_eq!(run.expiring.len(), 1);
    let expiring = events.drain();
    assert_eq!(expiring[0].1["host"], json!("crm.acme-corp.test"));
    assert_eq!(expiring[0].1["domain_id"], json!(domain.id));
    assert_eq!(expiring[0].1["days_left"], json!(5));
    let reports = lighthouse.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].fingerprint.as_deref(),
        Some(
            &[
                "domains".to_owned(),
                "certificate_expiring".to_owned(),
                "crm.acme-corp.test".to_owned()
            ][..]
        )
    );
    assert_eq!(
        serde_json::to_value(reports[0].level).unwrap(),
        json!("warning")
    );
}

#[tokio::test]
async fn a_failed_probe_is_recorded() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    world.fakes.probe.fail(
        "crm.acme-corp.test",
        "the TLS handshake failed: unrecognised name",
    );
    let domains = world.domains().await;
    let run = domains.check_certificates(None).await.unwrap();
    assert_eq!(run.failed.len(), 1);
    let domain = domains.get(domain.id).await.unwrap();
    assert_eq!(domain.tls, TlsStatus::Failed);
    assert!(domain.tls_error.unwrap().contains("unrecognised name"));
}

#[tokio::test]
async fn probes_can_be_turned_off() {
    let world = World::with(|builder| builder.config("domains.certificates.probe", false)).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    world.verified(workspace, "crm.acme-corp.test").await;
    let run = world
        .domains()
        .await
        .check_certificates(None)
        .await
        .unwrap();
    assert!(run.probed.is_empty() && run.failed.is_empty());
    assert!(world.fakes.probe.probed().is_empty());
}
