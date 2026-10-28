//! Moving a verified domain to another workspace: only through a fresh TXT
//! proof of the workspace that takes it (`Domains::transfer`,
//! `domains:transfer`), and finding the claims on a host.

use std::time::Duration;

use navree_domains::{DomainStatus, DomainsError, NewDomain, models};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support::World;

const HOST: &str = "crm.acme-corp.test";

/// How many rows `custom_domains` has for `host`, in any state.
async fn rows(world: &World, host: &str) -> usize {
    models::Domain::query()
        .where_(models::Domain::HOST.eq(host))
        .get(&*world.db().await)
        .await
        .unwrap()
        .into_models()
        .len()
}

#[tokio::test]
async fn find_prefers_the_verified_domain_and_claims_lists_them_all() {
    let world = World::new().await;
    let squatter = world.workspace(Uuid::now_v7(), "Squatter").await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let claim = domains.add(squatter, NewDomain::new(HOST)).await.unwrap();
    let mine = domains.add(owner, NewDomain::new(HOST)).await.unwrap();
    assert_eq!(domains.claims(HOST).await.unwrap().len(), 2);
    world.publish(&mine);
    domains.verify(mine.id).await.unwrap();
    assert_eq!(domains.find(HOST).await.unwrap().unwrap().id, mine.id);
    let all = domains.claims("CRM.acme-corp.test").await.unwrap();
    assert_eq!(all[0].id, mine.id);
    assert_eq!(all[1].id, claim.id);
    assert!(domains.claims("not a domain").await.unwrap().is_empty());
}

// -- transfer ----------------------------------------------------------------

#[tokio::test]
async fn a_verified_domain_moves_only_through_a_fresh_proof() {
    let world = World::new().await;
    let old = world.workspace(Uuid::now_v7(), "Old").await;
    let new = world.workspace(Uuid::now_v7(), "New").await;
    let domains = world.domains().await;
    let mut events = world
        .events(&["domains.verified", "domains.claimed_elsewhere"])
        .await;
    let held = world.verified(old, HOST).await;
    events.drain();

    // No proof yet: the claim is pending with a token of its own, and the
    // old workspace keeps the domain. The old token in the DNS proves nothing.
    let claim = domains.transfer(HOST, new).await.unwrap();
    assert_eq!(claim.status, DomainStatus::Pending);
    assert_eq!(claim.workspace_id, new);
    assert_ne!(claim.dns_records[0].value, held.dns_records[0].value);
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        old
    );
    let again = domains.transfer(HOST, new).await.unwrap();
    assert_eq!(again.id, claim.id);
    assert_eq!(again.dns_records[0].value, claim.dns_records[0].value);
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        old
    );
    assert!(events.drain().is_empty());

    // The new owner publishes the fresh token: the domain moves.
    world.publish(&claim);
    let moved = domains.transfer(HOST, new).await.unwrap();
    assert_eq!(moved.status, DomainStatus::Verified);
    assert_eq!(moved.id, claim.id);
    let resolved = domains.resolve(HOST).await.unwrap().unwrap();
    assert_eq!(resolved.workspace_id, new);
    assert_eq!(resolved.id, claim.id);
    assert!(domains.list(old).await.unwrap().is_empty());
    assert_eq!(rows(&world, HOST).await, 1);

    // The old workspace hears about it.
    let seen = events.drain();
    let names: Vec<&str> = seen.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["domains.verified", "domains.claimed_elsewhere"]);
    assert_eq!(seen[1].1["domain"]["id"], json!(held.id));
    assert_eq!(seen[1].1["domain"]["status"], json!("verified"));
    assert_eq!(seen[1].1["claimed_by"], json!(new));
}

#[tokio::test]
async fn a_transfer_completes_on_the_schedule_once_the_proof_appears() {
    let world = World::new().await;
    let old = world.workspace(Uuid::now_v7(), "Old").await;
    let new = world.workspace(Uuid::now_v7(), "New").await;
    let domains = world.domains().await;
    world.verified(old, HOST).await;
    let claim = domains.transfer(HOST, new).await.unwrap();
    world.advance(Duration::from_secs(5 * 60));
    domains.verify_due().await.unwrap();
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        old
    );
    world.publish(&claim);
    world.advance(Duration::from_secs(5 * 60));
    domains.verify_due().await.unwrap();
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        new
    );
}

#[tokio::test]
async fn an_ordinary_claim_never_takes_a_verified_domain() {
    // A claim added before the owner verified (or by a race) that finds its
    // proof after another workspace holds the host does not move it.
    let world = World::new().await;
    let first = world.workspace(Uuid::now_v7(), "First").await;
    let second = world.workspace(Uuid::now_v7(), "Second").await;
    let domains = world.domains().await;
    let claim = domains.add(first, NewDomain::new(HOST)).await.unwrap();
    let other = domains.add(second, NewDomain::new(HOST)).await.unwrap();
    world.publish(&claim);
    domains.verify(claim.id).await.unwrap();
    world.publish(&other);
    let late = domains.verify(other.id).await.unwrap();
    assert_eq!(late.status, DomainStatus::Failed);
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        first
    );
}

#[tokio::test]
async fn transfer_refuses_what_it_cannot_move() {
    let world = World::new().await;
    let old = world.workspace(Uuid::now_v7(), "Old").await;
    let new = world.workspace(Uuid::now_v7(), "New").await;
    let domains = world.domains().await;
    // Nobody holds it: add it instead.
    assert!(matches!(
        domains.transfer(HOST, new).await,
        Err(DomainsError::DomainNotFound)
    ));
    world.add(old, HOST).await;
    assert!(matches!(
        domains.transfer(HOST, new).await,
        Err(DomainsError::DomainNotFound)
    ));
    // The holder asking for its own domain changes nothing.
    let held = world.verified(old, "go.acme-corp.test").await;
    let same = domains.transfer("go.acme-corp.test", old).await.unwrap();
    assert_eq!(same.id, held.id);
    assert!(matches!(
        domains.transfer("not a domain", new).await,
        Err(DomainsError::InvalidDomain(_))
    ));
    assert!(matches!(
        domains.transfer("app.example.com", new).await,
        Err(DomainsError::NotAllowed(_))
    ));
}

#[tokio::test]
async fn transfer_keeps_the_per_workspace_limit() {
    let world = World::with(|builder| builder.config("domains.max_per_workspace", 1)).await;
    let old = world.workspace(Uuid::now_v7(), "Old").await;
    let new = world.workspace(Uuid::now_v7(), "New").await;
    let domains = world.domains().await;
    world.verified(old, HOST).await;
    world.add(new, "go.acme-corp.test").await;
    assert!(matches!(
        domains.transfer(HOST, new).await,
        Err(DomainsError::LimitReached(1))
    ));
}

#[tokio::test]
async fn a_moved_domain_gets_a_certificate_of_its_own() {
    let world = World::with(|builder| {
        builder
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
    })
    .await;
    let old = world.workspace(Uuid::now_v7(), "Old").await;
    let new = world.workspace(Uuid::now_v7(), "New").await;
    let domains = world.domains().await;
    let held = world.verified(old, HOST).await;
    domains.issue_pending().await.unwrap();
    assert!(domains.certificates().contains(HOST));
    let claim = domains.transfer(HOST, new).await.unwrap();
    world.publish(&claim);
    let moved = domains.transfer(HOST, new).await.unwrap();
    assert_eq!(moved.status, DomainStatus::Verified);
    // The old certificate went with the old domain; the new one is ordered.
    assert!(!domains.certificates().contains(HOST));
    assert!(
        models::Certificate::query()
            .where_(models::Certificate::HOST.eq(HOST))
            .first(&*world.db().await)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(domains.issue_pending().await.unwrap(), [HOST]);
    let stored = models::Certificate::query()
        .where_(models::Certificate::HOST.eq(HOST))
        .first(&*world.db().await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.domain_id, Some(moved.id));
    assert_ne!(stored.domain_id, Some(held.id));
}

#[tokio::test]
async fn the_transfer_command_asks_for_the_proof_then_moves_the_domain() {
    let world = World::new().await;
    let old = world.workspace(Uuid::now_v7(), "Old").await;
    let new = world.workspace(Uuid::now_v7(), "New").await;
    let domains = world.domains().await;
    world.verified(old, HOST).await;
    let to = new.to_string();
    world
        .app
        .run_command("domains:transfer", &[HOST, "--to", to.as_str()])
        .await
        .unwrap();
    let claim = domains
        .claims(HOST)
        .await
        .unwrap()
        .into_iter()
        .find(|domain| domain.workspace_id == new)
        .unwrap();
    assert_eq!(claim.status, DomainStatus::Pending);
    world.publish(&claim);
    world
        .app
        .run_command("domains:transfer", &[HOST, "--to", to.as_str()])
        .await
        .unwrap();
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        new
    );
    // Both arguments are needed.
    assert!(
        world
            .app
            .run_command("domains:transfer", &[HOST])
            .await
            .is_err()
    );
    assert!(
        world
            .app
            .run_command("domains:transfer", &["--to", to.as_str()])
            .await
            .is_err()
    );
    assert!(
        world
            .app
            .run_command("domains:transfer", &[HOST, "--to", "nobody"])
            .await
            .is_err()
    );
}
