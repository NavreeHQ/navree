//! The commands `domains:list`, `domains:verify` and
//! `domains:certificates`.

use chrono::TimeDelta;
use navree_domains::{DomainStatus, TlsStatus};
use uuid::Uuid;

use crate::support::World;

#[tokio::test]
async fn domains_verify_checks_a_domain_by_host_or_id_or_every_due_one() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let first = world.add(workspace, "crm.acme-corp.test").await;
    let second = world.add(workspace, "go.acme-corp.test").await;
    world.publish(&first);
    world.publish(&second);
    let domains = world.domains().await;

    world
        .app
        .run_command("domains:verify", &["CRM.acme-corp.test"])
        .await
        .unwrap();
    assert_eq!(
        domains.get(first.id).await.unwrap().status,
        DomainStatus::Verified
    );
    assert_eq!(
        domains.get(second.id).await.unwrap().status,
        DomainStatus::Pending
    );
    world
        .app
        .run_command("domains:verify", &["--due"])
        .await
        .unwrap();
    assert_eq!(
        domains.get(second.id).await.unwrap().status,
        DomainStatus::Verified
    );
    let id = second.id.to_string();
    world
        .app
        .run_command("domains:verify", &[id.as_str()])
        .await
        .unwrap();
    world
        .app
        .run_command("domains:verify", &["--all"])
        .await
        .unwrap();
    assert!(
        world
            .app
            .run_command("domains:verify", &["unknown.test"])
            .await
            .is_err()
    );
    assert!(world.app.run_command("domains:verify", &[]).await.is_err());
}

#[tokio::test]
async fn domains_verify_due_issues_the_new_certificates_with_acme() {
    let world = World::with(|builder| {
        builder
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
    })
    .await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(workspace, "crm.acme-corp.test").await;
    world.publish(&domain);
    world
        .app
        .run_command("domains:verify", &["--due"])
        .await
        .unwrap();
    let domain = world.domains().await.get(domain.id).await.unwrap();
    assert_eq!(domain.status, DomainStatus::Verified);
    assert_eq!(domain.tls, TlsStatus::Active);
    assert_eq!(world.fakes.ca.orders(), ["crm.acme-corp.test"]);
}

#[tokio::test]
async fn domains_certificates_probes_and_domains_list_lists() {
    let world = World::new().await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    world
        .fakes
        .probe
        .expires("crm.acme-corp.test", world.now() + TimeDelta::days(70));
    world
        .app
        .run_command("domains:certificates", &["--due"])
        .await
        .unwrap();
    assert!(
        world
            .domains()
            .await
            .get(domain.id)
            .await
            .unwrap()
            .certificate_expires_at
            .is_some()
    );
    // A host given by name that can't be read fails the command.
    world.fakes.probe.fail("crm.acme-corp.test", "refused");
    assert!(
        world
            .app
            .run_command("domains:certificates", &["crm.acme-corp.test"])
            .await
            .is_err()
    );
    for args in [
        &[][..],
        &["--status", "verified"][..],
        &["--workspace", &workspace.to_string()][..],
    ] {
        world.app.run_command("domains:list", args).await.unwrap();
    }
    assert!(
        world
            .app
            .run_command("domains:list", &["--status", "lost"])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_commands_are_registered() {
    let world = World::new().await;
    let mut names: Vec<&str> = world
        .app
        .ctx()
        .commands()
        .iter()
        .map(|command| command.name)
        .filter(|name| name.starts_with("domains:"))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "domains:certificates",
            "domains:list",
            "domains:transfer",
            "domains:verify"
        ]
    );
}
