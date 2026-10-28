//! DNS verification: the TXT record, the routing check, the schedule,
//! giving up, resolver failures and rechecks of verified domains.

use std::time::Duration;

use navree_domains::{DomainStatus, DomainsError, TlsStatus};
use serde_json::json;
use uuid::Uuid;

use crate::support::{ADDRESS, PREFIX, TARGET, World};

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

#[tokio::test]
async fn a_domain_is_verified_once_its_records_appear() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    let mut events = world.events(&["domains.verified"]).await;

    let checked = domains.verify(domain.id).await.unwrap();
    assert_eq!(checked.status, DomainStatus::Pending);
    assert_eq!(checked.points_to_app, Some(false));
    let token = &domain.dns_records[0].value;
    assert_eq!(
        checked.last_error.as_deref(),
        Some(
            format!(
                "TXT record {PREFIX}.crm.acme-corp.test with the value {token} not found; \
                 crm.acme-corp.test does not point at {TARGET}, {ADDRESS}"
            )
            .as_str()
        )
    );
    assert_eq!(checked.last_checked_at, Some(world.now()));
    assert_eq!(
        checked.next_check_at,
        Some(world.now() + chrono::TimeDelta::minutes(5))
    );

    // The TXT record alone is not enough while routing is required.
    world
        .dns()
        .set_txt(&format!("{PREFIX}.crm.acme-corp.test"), token);
    let checked = domains.verify(domain.id).await.unwrap();
    assert_eq!(checked.status, DomainStatus::Pending);
    assert!(checked.last_error.unwrap().contains("does not point at"));

    world.dns().set_cname("crm.acme-corp.test", TARGET);
    let verified = domains.verify(domain.id).await.unwrap();
    assert_eq!(verified.status, DomainStatus::Verified);
    assert_eq!(verified.points_to_app, Some(true));
    assert_eq!(verified.verified_at, Some(world.now()));
    assert_eq!(verified.last_error, None);
    // On-demand TLS: the proxy gets the certificate on the first visit.
    assert_eq!(verified.tls, TlsStatus::Active);
    // Checked again a day later for its routing.
    assert_eq!(
        verified.next_check_at,
        Some(world.now() + chrono::TimeDelta::hours(24))
    );
    let names = events.drain();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].1["domain"]["status"], json!("verified"));
    let resolved = domains
        .resolve("CRM.acme-corp.test:443")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.workspace_id, workspace);
    assert_eq!(resolved.id, domain.id);
    assert_eq!(resolved.purpose, "app");
}

#[tokio::test]
async fn routing_is_optional_when_configured_so() {
    let world =
        World::with(|builder| builder.config("domains.verification.require_routing", false)).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    world
        .dns()
        .set_txt(&domain.dns_records[0].name, &domain.dns_records[0].value);
    let verified = world.domains().await.verify(domain.id).await.unwrap();
    assert_eq!(verified.status, DomainStatus::Verified);
    assert_eq!(verified.points_to_app, Some(false));
    assert!(verified.last_error.unwrap().contains("does not point at"));
}

#[tokio::test]
async fn without_a_target_only_the_txt_record_is_checked() {
    let world = World::with(|builder| {
        builder
            .config("domains.target", serde_json::Value::Null)
            .config("domains.addresses", json!([]))
    })
    .await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    assert_eq!(domain.dns_records.len(), 1);
    world.publish(&domain);
    let verified = world.domains().await.verify(domain.id).await.unwrap();
    assert_eq!(verified.status, DomainStatus::Verified);
    assert_eq!(verified.points_to_app, None);
}

#[tokio::test]
async fn the_scheduler_checks_due_domains_until_it_gives_up() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    let mut events = world.events(&["domains.verification_failed"]).await;

    let checked = domains.verify_due().await.unwrap();
    assert_eq!(checked.len(), 1);
    // Not due again for five minutes.
    assert!(domains.verify_due().await.unwrap().is_empty());
    world.advance(4 * MINUTE);
    assert!(domains.verify_due().await.unwrap().is_empty());
    world.advance(MINUTE);
    assert_eq!(domains.verify_due().await.unwrap().len(), 1);

    // 72 hours after it was added, it fails.
    world.advance(72 * HOUR);
    let checked = domains.verify_due().await.unwrap();
    assert_eq!(checked[0].status, DomainStatus::Failed);
    assert_eq!(checked[0].next_check_at, None);
    let failed = events.drain();
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0].1["reason"]
            .as_str()
            .unwrap()
            .contains("TXT record")
    );
    world.advance(HOUR);
    assert!(domains.verify_due().await.unwrap().is_empty());

    // Verifying by hand still works once the records are there.
    world.publish(&domain);
    let verified = domains.verify(domain.id).await.unwrap();
    assert_eq!(verified.status, DomainStatus::Verified);
}

#[tokio::test]
async fn a_resolver_failure_is_recorded_and_returned() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    world
        .dns()
        .fail(&format!("{PREFIX}.crm.acme-corp.test"), "SERVFAIL");
    let domains = world.domains().await;
    let error = domains.verify(domain.id).await.unwrap_err();
    assert!(matches!(error, DomainsError::Dns { .. }), "{error}");
    let stored = domains.get(domain.id).await.unwrap();
    assert_eq!(stored.status, DomainStatus::Pending);
    assert!(stored.last_error.unwrap().contains("SERVFAIL"));
    assert_eq!(
        stored.next_check_at,
        Some(world.now() + chrono::TimeDelta::minutes(5))
    );
    // The scheduler logs a failure and goes on with the others.
    let other = world.add(workspace, "go.acme-corp.test").await;
    world.publish(&other);
    world.advance(5 * MINUTE);
    let checked: Vec<(String, DomainStatus)> = domains
        .verify_due()
        .await
        .unwrap()
        .into_iter()
        .map(|domain| (domain.host, domain.status))
        .collect();
    assert_eq!(
        checked,
        [("go.acme-corp.test".to_owned(), DomainStatus::Verified)]
    );
    assert!(
        domains
            .get(domain.id)
            .await
            .unwrap()
            .last_error
            .unwrap()
            .contains("SERVFAIL")
    );
}

#[tokio::test]
async fn verified_domains_are_rechecked_for_their_routing() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    domains.make_primary(domain.id).await.unwrap();
    world.dns().remove("crm.acme-corp.test");
    world.advance(23 * HOUR);
    assert!(domains.verify_due().await.unwrap().is_empty());
    world.advance(HOUR);
    let checked = domains.verify_due().await.unwrap();
    assert_eq!(checked.len(), 1);
    // Still verified (the workspace keeps its domain), but reported.
    assert_eq!(checked[0].status, DomainStatus::Verified);
    assert_eq!(checked[0].points_to_app, Some(false));
    assert!(checked[0].last_error.is_some());
    assert_eq!(checked[0].id, domain.id);
    // A check writes its own columns only.
    assert!(domains.get(domain.id).await.unwrap().primary);
}

#[tokio::test]
async fn rechecks_can_be_turned_off() {
    let world =
        World::with(|builder| builder.config("domains.verification.recheck_hours", 0)).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    assert_eq!(domain.next_check_at, None);
    world.advance(1000 * HOUR);
    assert!(world.domains().await.verify_due().await.unwrap().is_empty());
}
