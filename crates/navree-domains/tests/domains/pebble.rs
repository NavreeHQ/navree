//! A real ACME CA: Let's Encrypt's test server, Pebble. The real
//! `InstantAcme` client creates an account, orders a certificate, answers
//! the HTTP-01 challenge, finalizes and downloads the chain; a second
//! order reuses the stored account.
//!
//! Opt-in: run Pebble with `PEBBLE_VA_ALWAYS_VALID=1` (it then skips
//! fetching the challenge answers) and set `NAVREE_TEST_PEBBLE_URL` to its
//! directory (`https://localhost:14000/dir`) and `NAVREE_TEST_PEBBLE_CA` to
//! the file of the root its HTTPS uses (`test/certs/pebble.minica.pem`).
//! CI runs it (`.github/workflows/domains.yml`).

use std::sync::Arc;

use navree_domains::{AcmeClient, InstantAcme, TlsStatus, models};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::support::World;

#[tokio::test]
async fn certificates_come_from_a_real_acme_server() {
    let (Ok(url), Ok(ca)) = (
        std::env::var("NAVREE_TEST_PEBBLE_URL"),
        std::env::var("NAVREE_TEST_PEBBLE_CA"),
    ) else {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "pebble: NAVREE_TEST_REQUIRE_SERVICES=1 but NAVREE_TEST_PEBBLE_URL or NAVREE_TEST_PEBBLE_CA is not set"
        );
        println!("SKIPPED pebble: set NAVREE_TEST_PEBBLE_URL and NAVREE_TEST_PEBBLE_CA");
        return;
    };
    let pem = std::fs::read(&ca).unwrap();
    let client = InstantAcme::with_root_pem(&pem).unwrap();
    let world = World::with(|builder| {
        builder
            .config("domains.tls", "acme")
            .config("domains.acme.terms_agreed", true)
            .config("domains.acme.directory_url", url.as_str())
            .config("domains.acme.contact", serde_json::json!(["ops@example.com"]))
            .override_value::<dyn AcmeClient>(Arc::new(client))
    })
    .await;
    let workspace = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(workspace, "crm.acme-corp.test").await;
    let domains = world.domains().await;

    let expires = domains.issue_certificate("crm.acme-corp.test").await.unwrap();
    assert!(expires > chrono::Utc::now());
    let domain = domains.get(domain.id).await.unwrap();
    assert_eq!(domain.tls, TlsStatus::Active);
    assert!(domains.certificates().contains("crm.acme-corp.test"));
    let db = world.db().await;
    let stored = models::Certificate::query()
        .where_(models::Certificate::HOST.eq("crm.acme-corp.test"))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    // The leaf and Pebble's intermediate.
    assert!(stored.chain_pem.matches("BEGIN CERTIFICATE").count() >= 2);
    // The challenge answer was withdrawn.
    assert_eq!(models::Challenge::query().count(&*db).await.unwrap(), 0);

    // A renewal reuses the stored account.
    domains.issue_certificate("crm.acme-corp.test").await.unwrap();
    assert_eq!(models::AcmeAccount::query().count(&*db).await.unwrap(), 1);
    world.app.shutdown().await.unwrap();
}
