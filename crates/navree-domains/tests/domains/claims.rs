//! Proof of control: several workspaces may claim one host, each with its
//! own verification token; the claim whose TXT record is found wins
//! atomically and closes the others; failed claims never block anyone and
//! are cleaned up; a verified domain moves to another workspace only
//! through a fresh TXT proof.

use std::time::Duration;

use navree_domains::{DomainStatus, DomainsError, NewDomain, models};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support::{PREFIX, World};

const HOST: &str = "crm.acme-corp.test";
const HOUR: Duration = Duration::from_secs(3600);

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
async fn a_squatters_pending_claim_does_not_block_the_owner() {
    let world = World::new().await;
    let squatter = world.workspace(Uuid::now_v7(), "Squatter").await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;

    // The squatter types the owner's domain first.
    let claim = domains.add(squatter, NewDomain::new(HOST)).await.unwrap();
    assert_eq!(claim.status, DomainStatus::Pending);

    // The owner is not refused: a claim of their own, with their own token.
    let mine = domains.add(owner, NewDomain::new(HOST)).await.unwrap();
    assert_eq!(mine.status, DomainStatus::Pending);
    assert_ne!(mine.id, claim.id);
    assert_ne!(mine.dns_records[0].value, claim.dns_records[0].value);
    assert_eq!(rows(&world, HOST).await, 2);

    // Neither claim names a workspace, and the ask endpoint says no.
    assert_eq!(domains.resolve(HOST).await.unwrap(), None);
    assert!(!domains.allows(HOST).await.unwrap());
    world
        .app
        .client()
        .get(&format!("/_domains/allow?domain={HOST}"))
        .send()
        .await
        .assert_error_code(404, "domain_not_found");
}

#[tokio::test]
async fn the_claim_that_proves_control_wins_and_the_others_are_closed() {
    let world = World::new().await;
    let squatter = world.workspace(Uuid::now_v7(), "Squatter").await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let mut events = world
        .events(&["domains.verified", "domains.claimed_elsewhere"])
        .await;
    let claim = domains.add(squatter, NewDomain::new(HOST)).await.unwrap();
    let mine = domains.add(owner, NewDomain::new(HOST)).await.unwrap();

    // Only the owner controls the DNS: only their token is published.
    world.publish(&mine);
    let squat_check = domains.verify(claim.id).await.unwrap();
    assert_eq!(squat_check.status, DomainStatus::Pending);
    let won = domains.verify(mine.id).await.unwrap();
    assert_eq!(won.status, DomainStatus::Verified);

    // The host names the owner's workspace, and the ask endpoint says yes.
    let resolved = domains.resolve(HOST).await.unwrap().unwrap();
    assert_eq!(resolved.workspace_id, owner);
    assert_eq!(resolved.id, mine.id);
    world
        .app
        .client()
        .get(&format!("/_domains/allow?domain={HOST}"))
        .send()
        .await
        .assert_status(200);

    // The squatter's claim is closed, with a reason, and an event.
    let closed = domains.get(claim.id).await.unwrap();
    assert_eq!(closed.status, DomainStatus::Failed);
    assert_eq!(closed.next_check_at, None);
    assert!(
        closed
            .last_error
            .as_deref()
            .unwrap()
            .contains("another workspace")
    );
    let seen = events.drain();
    let names: Vec<&str> = seen.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["domains.verified", "domains.claimed_elsewhere"]);
    let (_, payload) = &seen[1];
    assert_eq!(payload["domain"]["id"], json!(claim.id));
    assert_eq!(payload["domain"]["workspace_id"], json!(squatter));
    assert_eq!(payload["claimed_by"], json!(owner));

    // A closed claim stays closed, even when the squatter's token shows up.
    world.publish(&claim);
    let again = domains.verify(claim.id).await.unwrap();
    assert_eq!(again.status, DomainStatus::Failed);
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        owner
    );
    // Nothing more is announced for a claim that was closed already.
    assert!(events.names().is_empty());
}

#[tokio::test]
async fn each_claim_is_proved_by_its_own_token() {
    let world = World::new().await;
    let squatter = world.workspace(Uuid::now_v7(), "Squatter").await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let claim = domains.add(squatter, NewDomain::new(HOST)).await.unwrap();
    let mine = domains.add(owner, NewDomain::new(HOST)).await.unwrap();
    // The squatter's token in the owner's DNS proves nothing for the owner.
    let name = format!("{PREFIX}.{HOST}");
    world.dns().set_txt(&name, &claim.dns_records[0].value);
    world.dns().set_cname(HOST, crate::support::TARGET);
    assert_eq!(
        domains.verify(mine.id).await.unwrap().status,
        DomainStatus::Pending
    );
    // Whoever's token is in the DNS is the one that verifies.
    assert_eq!(
        domains.verify(claim.id).await.unwrap().status,
        DomainStatus::Verified
    );
    assert_eq!(
        domains.get(mine.id).await.unwrap().status,
        DomainStatus::Failed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workspaces_proving_control_at_once_leave_exactly_one_winner() {
    let world = World::new().await;
    let domains = world.domains().await;
    let mut events = world
        .events(&["domains.verified", "domains.claimed_elsewhere"])
        .await;
    for round in 0..6 {
        let host = format!("crm{round}.acme-corp.test");
        let mut claims = Vec::new();
        for n in 0..4 {
            let workspace = world.workspace(Uuid::now_v7(), &format!("W{round}{n}")).await;
            let claim = world.add(workspace, &host).await;
            // Everyone's token is in the DNS: all four prove control now.
            world.publish(&claim);
            claims.push(claim);
        }
        let mut tasks = Vec::new();
        for claim in &claims {
            let domains = domains.clone();
            let id = claim.id;
            tasks.push(tokio::spawn(async move { domains.verify(id).await }));
        }
        for task in tasks {
            // Losing is an answer, not an error.
            task.await.unwrap().unwrap();
        }
        let mut winners = Vec::new();
        for claim in &claims {
            let now = domains.get(claim.id).await.unwrap();
            match now.status {
                DomainStatus::Verified => winners.push(now),
                other => assert_eq!(other, DomainStatus::Failed, "{host}"),
            }
        }
        assert_eq!(winners.len(), 1, "{host}: {winners:?}");
        let resolved = domains.resolve(&host).await.unwrap().unwrap();
        assert_eq!(resolved.id, winners[0].id);
        let verified = models::Domain::query()
            .where_(models::Domain::HOST.eq(host.clone()))
            .where_(models::Domain::STATUS.eq("verified"))
            .count(&*world.db().await)
            .await
            .unwrap();
        assert_eq!(verified, 1, "{host}");
    }
    // One `verified` and three `claimed_elsewhere` per round: no claim is
    // closed twice.
    let seen = events.names();
    assert_eq!(
        seen.iter().filter(|name| *name == "domains.verified").count(),
        6
    );
    assert_eq!(
        seen.iter()
            .filter(|name| *name == "domains.claimed_elsewhere")
            .count(),
        18
    );
}

#[tokio::test]
async fn a_verified_host_is_refused_to_other_workspaces() {
    let world = World::new().await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let other = world.workspace(Uuid::now_v7(), "Other").await;
    world.verified(owner, HOST).await;
    let refused = world
        .domains()
        .await
        .add(other, NewDomain::new(HOST))
        .await
        .unwrap_err();
    assert!(matches!(refused, DomainsError::Taken(host) if host == HOST));
    // And a workspace can't claim a host twice.
    let twice = world
        .domains()
        .await
        .add(owner, NewDomain::new(HOST))
        .await
        .unwrap_err();
    assert!(matches!(twice, DomainsError::Taken(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_workspace_adding_a_host_twice_at_once_gets_domain_taken() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let domains = domains.clone();
        tasks.push(tokio::spawn(async move {
            domains.add(workspace, NewDomain::new(HOST)).await
        }));
    }
    let mut added = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => added += 1,
            Err(error) => assert!(matches!(error, DomainsError::Taken(_)), "{error}"),
        }
    }
    assert_eq!(added, 1);
    assert_eq!(rows(&world, HOST).await, 1);
}

#[tokio::test]
async fn a_failed_claim_never_blocks_and_is_cleaned_up() {
    let world = World::new().await;
    let squatter = world.workspace(Uuid::now_v7(), "Squatter").await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let mut events = world.events(&["domains.verification_failed"]).await;
    let claim = domains.add(squatter, NewDomain::new(HOST)).await.unwrap();

    // 72 hours later the squatter's claim fails.
    world.advance(72 * HOUR);
    let checked = domains.verify_due().await.unwrap();
    assert_eq!(checked[0].status, DomainStatus::Failed);
    assert_eq!(events.names(), ["domains.verification_failed"]);

    // The owner was never blocked, before or after.
    let mine = world.verified(owner, HOST).await;
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().id,
        mine.id
    );

    // The failed claim stays visible for a week, then is deleted.
    world.advance(24 * HOUR);
    domains.verify_due().await.unwrap();
    assert_eq!(
        domains.get(claim.id).await.unwrap().status,
        DomainStatus::Failed
    );
    world.advance(7 * 24 * HOUR);
    domains.verify_due().await.unwrap();
    assert!(matches!(
        domains.get(claim.id).await,
        Err(DomainsError::DomainNotFound)
    ));
    assert!(domains.list(squatter).await.unwrap().is_empty());
    assert_eq!(rows(&world, HOST).await, 1);
    // The owner's domain is untouched.
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        owner
    );
}

#[tokio::test]
async fn cleaning_up_failed_claims_can_be_turned_off() {
    let world =
        World::with(|builder| builder.config("domains.verification.failed_retention_hours", 0))
            .await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let claim = world.add(workspace, HOST).await;
    let domains = world.domains().await;
    world.advance(72 * HOUR);
    domains.verify_due().await.unwrap();
    world.advance(1000 * HOUR);
    domains.verify_due().await.unwrap();
    assert_eq!(
        domains.get(claim.id).await.unwrap().status,
        DomainStatus::Failed
    );
}

#[tokio::test]
async fn a_failed_claim_is_replaced_when_its_workspace_claims_again() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let old = world.add(workspace, HOST).await;
    world.advance(72 * HOUR);
    domains.verify_due().await.unwrap();
    assert_eq!(
        domains.get(old.id).await.unwrap().status,
        DomainStatus::Failed
    );
    let fresh = domains.add(workspace, NewDomain::new(HOST)).await.unwrap();
    assert_ne!(fresh.id, old.id);
    assert_ne!(fresh.dns_records[0].value, old.dns_records[0].value);
    assert_eq!(fresh.status, DomainStatus::Pending);
    assert_eq!(rows(&world, HOST).await, 1);
}

#[tokio::test]
async fn failed_claims_do_not_count_against_the_workspace_limit() {
    let world = World::with(|builder| builder.config("domains.max_per_workspace", 1)).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    world.add(workspace, HOST).await;
    assert!(matches!(
        domains.add(workspace, NewDomain::new("go.acme-corp.test")).await,
        Err(DomainsError::LimitReached(1))
    ));
    world.advance(72 * HOUR);
    domains.verify_due().await.unwrap();
    domains
        .add(workspace, NewDomain::new("go.acme-corp.test"))
        .await
        .unwrap();
}

#[tokio::test]
async fn closing_or_removing_a_claim_keeps_the_owners_certificate() {
    let world = World::with(|builder| {
        builder
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
    })
    .await;
    let squatter = world.workspace(Uuid::now_v7(), "Squatter").await;
    let owner = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    let claim = domains.add(squatter, NewDomain::new(HOST)).await.unwrap();
    let mine = world.verified(owner, HOST).await;
    domains.issue_pending().await.unwrap();
    assert!(domains.certificates().contains(HOST));

    // The squatter deletes their closed claim: the owner's certificate stays.
    domains.remove(claim.id).await.unwrap();
    assert!(domains.certificates().contains(HOST));
    let stored = models::Certificate::query()
        .where_(models::Certificate::HOST.eq(HOST))
        .first(&*world.db().await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.domain_id, Some(mine.id));
    assert_eq!(
        domains.resolve(HOST).await.unwrap().unwrap().workspace_id,
        owner
    );
}

