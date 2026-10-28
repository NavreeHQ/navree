//! Adding and removing domains: normalization, the DNS records to create,
//! reserved hosts, taken hosts, the per-workspace limit, events, and a
//! deleted workspace's domains.

use navree_domains::{DomainStatus, DomainsError, NewDomain, TlsStatus, models};
use navree_orm::Model as _;
use navree_workspace::Workspaces;
use serde_json::json;
use uuid::Uuid;

use crate::support::{ADDRESS, PREFIX, TARGET, World};

#[tokio::test]
async fn a_domain_is_added_pending_with_the_records_to_create() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let mut events = world.events(&["domains.added"]).await;
    let domains = world.domains().await;
    let domain = domains
        .add(
            workspace,
            NewDomain::new(" CRM.Acme-Corp.test. ").created_by(Uuid::from_u128(7)),
        )
        .await
        .unwrap();
    assert_eq!(domain.host, "crm.acme-corp.test");
    assert_eq!(domain.workspace_id, workspace);
    assert_eq!(domain.purpose, "app");
    assert_eq!(domain.status, DomainStatus::Pending);
    assert_eq!(domain.tls, TlsStatus::Pending);
    assert!(!domain.primary && !domain.managed);
    assert_eq!(domain.points_to_app, None);
    assert_eq!(domain.next_check_at, Some(world.now()));
    assert_eq!(domain.created_by, Some(Uuid::from_u128(7)));
    let token = &domain.dns_records[0].value;
    assert_eq!(token.len(), 32);
    assert_eq!(
        serde_json::to_value(&domain.dns_records).unwrap(),
        json!([
            {
                "type": "TXT",
                "name": format!("{PREFIX}.crm.acme-corp.test"),
                "value": token,
                "purpose": "verification"
            },
            { "type": "CNAME", "name": "crm.acme-corp.test", "value": TARGET, "purpose": "routing" }
        ])
    );
    let stored = models::Domain::query()
        .where_(models::Domain::ID.eq(domain.id))
        .first(&*world.db().await)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.verification_token, *token);
    let added = events.drain();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].1["domain"]["host"], json!("crm.acme-corp.test"));
    // Not verified: it names no workspace yet.
    assert_eq!(domains.resolve("crm.acme-corp.test").await.unwrap(), None);
    assert!(!domains.allows("crm.acme-corp.test").await.unwrap());
}

#[tokio::test]
async fn apex_domains_point_with_address_records() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "acme-corp.test").await;
    let kinds: Vec<(&str, &str)> = domain
        .dns_records
        .iter()
        .map(|record| (record.kind.as_str(), record.value.as_str()))
        .collect();
    assert_eq!(kinds[1], ("A", ADDRESS));
    assert_eq!(kinds.len(), 2);
}

#[tokio::test]
async fn reserved_taken_and_invalid_domains_are_refused() {
    let world = World::with(|builder| {
        builder
            .config("domains.reserved", json!(["example.org"]))
            .config("domains.max_per_workspace", 2)
            .config("workspace.base_domain", "app.example.net")
    })
    .await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    let globex = world.workspace(Uuid::now_v7(), "Globex").await;
    let domains = world.domains().await;
    let refuse = |host: &'static str| {
        let domains = domains.clone();
        async move { domains.add(acme, NewDomain::new(host)).await.unwrap_err() }
    };
    // The app's own domain (app.url), the workspace subdomains' base, the
    // target, and configured reserved zones, with their subdomains.
    for host in [
        "app.example.com",
        "admin.app.example.com",
        "evil.app.example.net",
        TARGET,
        "example.org",
        "www.example.org",
    ] {
        assert!(
            matches!(refuse(host).await, DomainsError::NotAllowed(_)),
            "{host}"
        );
    }
    assert!(matches!(
        refuse("https://crm.acme-corp.test").await,
        DomainsError::InvalidDomain(_)
    ));
    let bad_purpose = domains
        .add(
            acme,
            NewDomain::new("crm.acme-corp.test").purpose("Landing Pages"),
        )
        .await
        .unwrap_err();
    assert!(matches!(bad_purpose, DomainsError::InvalidDomain(_)));

    world.add(acme, "crm.acme-corp.test").await;
    let taken = domains
        .add(globex, NewDomain::new("CRM.acme-corp.test"))
        .await
        .unwrap_err();
    assert!(matches!(taken, DomainsError::Taken(host) if host == "crm.acme-corp.test"));
    let links = domains
        .add(acme, NewDomain::new("go.acme-corp.test").purpose("links"))
        .await
        .unwrap();
    assert_eq!(links.purpose, "links");
    let full = domains
        .add(acme, NewDomain::new("help.acme-corp.test"))
        .await
        .unwrap_err();
    assert!(matches!(full, DomainsError::LimitReached(2)));
    // The limit is per workspace.
    world.add(globex, "globex.test").await;
}

#[tokio::test]
async fn removing_a_domain_forgets_it() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    assert!(
        domains
            .resolve("crm.acme-corp.test")
            .await
            .unwrap()
            .is_some()
    );
    let mut events = world.events(&["domains.removed"]).await;
    domains.remove(domain.id).await.unwrap();
    assert_eq!(domains.find("crm.acme-corp.test").await.unwrap(), None);
    // The cached answer is gone with it.
    assert_eq!(domains.resolve("crm.acme-corp.test").await.unwrap(), None);
    let removed = events.drain();
    assert_eq!(removed[0].1["host"], json!("crm.acme-corp.test"));
    assert_eq!(removed[0].1["workspace_id"], json!(workspace));
    assert!(matches!(
        domains.remove(domain.id).await,
        Err(DomainsError::DomainNotFound)
    ));
    // The host can be added again, by anyone.
    world
        .add(
            world.workspace(Uuid::now_v7(), "Globex").await,
            "crm.acme-corp.test",
        )
        .await;
}

#[tokio::test]
async fn a_deleted_workspace_takes_its_domains_along() {
    let world = World::new().await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    let globex = world.workspace(Uuid::now_v7(), "Globex").await;
    world.add(acme, "crm.acme-corp.test").await;
    world.add(acme, "go.acme-corp.test").await;
    world.add(globex, "globex.test").await;
    let workspaces = world.app.get::<Workspaces>().await;
    let db = world.db().await;
    let uow = db.uow(Some(owner), None).await;
    workspaces.delete(&uow, acme).await.unwrap();
    uow.commit().await.unwrap();
    let domains = world.domains().await;
    // The listener runs after the commit.
    for _ in 0..50 {
        if domains.list(acme).await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(domains.list(acme).await.unwrap().is_empty());
    assert_eq!(domains.list(globex).await.unwrap().len(), 1);
}

#[tokio::test]
async fn domains_are_listed_and_found() {
    let world = World::new().await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    world.add(acme, "go.acme-corp.test").await;
    world.verified(acme, "crm.acme-corp.test").await;
    let domains = world.domains().await;
    let hosts: Vec<String> = domains
        .list(acme)
        .await
        .unwrap()
        .into_iter()
        .map(|domain| domain.host)
        .collect();
    assert_eq!(hosts, ["crm.acme-corp.test", "go.acme-corp.test"]);
    assert_eq!(domains.all(None).await.unwrap().len(), 2);
    assert_eq!(
        domains.all(Some(DomainStatus::Verified)).await.unwrap()[0].host,
        "crm.acme-corp.test"
    );
    let found = domains.find("GO.acme-corp.test").await.unwrap().unwrap();
    assert_eq!(found.status, DomainStatus::Pending);
    assert_eq!(domains.find("not a domain").await.unwrap(), None);
    assert!(matches!(
        domains.get_for(Uuid::now_v7(), found.id).await,
        Err(DomainsError::DomainNotFound)
    ));
    assert_eq!(domains.get(found.id).await.unwrap(), found);
}
