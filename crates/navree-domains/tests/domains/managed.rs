//! Managed zones: a domain added inside `domains.dns.zone` gets its record
//! from the DNS provider and is verified at once.

use navree_domains::{DomainStatus, DomainsError, NewDomain, RecordKind};
use navree_testing::TestAppBuilder;
use serde_json::json;
use uuid::Uuid;

use crate::support::{ADDRESS, TARGET, World};

fn managed(builder: TestAppBuilder) -> TestAppBuilder {
    // The fake provider replaces the Cloudflare driver the configuration
    // names.
    builder
        .config("domains.dns.provider", "cloudflare")
        .config("domains.dns.zone", "links.example.com")
        .config("domains.dns.cloudflare.api_token", "token")
        .config("domains.dns.cloudflare.zone_id", "zone")
}

#[tokio::test]
async fn hosts_inside_the_zone_are_created_and_verified_at_once() {
    let world = World::with(managed).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let mut events = world.events(&["domains.added", "domains.verified"]).await;
    let domains = world.domains().await;
    let domain = domains
        .add(
            workspace,
            NewDomain::new("acme.links.example.com").purpose("links"),
        )
        .await
        .unwrap();
    assert!(domain.managed);
    assert_eq!(domain.status, DomainStatus::Verified);
    assert_eq!(domain.points_to_app, Some(true));
    assert!(domain.dns_records.is_empty());
    assert_eq!(
        world
            .fakes
            .zone
            .records(RecordKind::Cname, "acme.links.example.com"),
        [TARGET]
    );
    assert_eq!(events.names(), ["domains.added", "domains.verified"]);
    assert!(domains.allows("acme.links.example.com").await.unwrap());
    // Rechecks skip the TXT record: the app made the record itself.
    let checked = domains.verify(domain.id).await.unwrap();
    assert_eq!(checked.status, DomainStatus::Verified);

    domains.remove(domain.id).await.unwrap();
    assert!(world.fakes.zone.all().is_empty());
}

#[tokio::test]
async fn without_a_target_the_zone_gets_address_records() {
    let world = World::with(|builder| {
        managed(builder)
            .config("domains.target", serde_json::Value::Null)
            .config("domains.addresses", json!([ADDRESS, "2001:db8::10"]))
    })
    .await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    world.add(workspace, "acme.links.example.com").await;
    assert_eq!(
        world
            .fakes
            .zone
            .records(RecordKind::A, "acme.links.example.com"),
        [ADDRESS]
    );
    assert_eq!(
        world
            .fakes
            .zone
            .records(RecordKind::Aaaa, "acme.links.example.com"),
        ["2001:db8::10"]
    );
}

#[tokio::test]
async fn the_zone_itself_is_reserved_and_provider_failures_add_nothing() {
    let world = World::with(managed).await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domains = world.domains().await;
    assert!(matches!(
        domains
            .add(workspace, NewDomain::new("links.example.com"))
            .await,
        Err(DomainsError::NotAllowed(_))
    ));
    world.fakes.zone.fail_next("Authentication error");
    let error = domains
        .add(workspace, NewDomain::new("acme.links.example.com"))
        .await
        .unwrap_err();
    assert!(matches!(error, DomainsError::DnsProvider(_)), "{error}");
    assert!(domains.list(workspace).await.unwrap().is_empty());
    // Outside the zone, domains are verified through DNS as usual.
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    assert!(!domain.managed);
    assert_eq!(domain.status, DomainStatus::Pending);
}
