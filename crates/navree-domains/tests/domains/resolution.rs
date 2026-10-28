//! Requests to a verified domain: the `domain` workspace resolver and
//! `CurrentDomain`.

use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::World;

async fn resolving() -> World {
    World::with(|builder| builder.config("workspace.resolvers", json!(["domain", "header"]))).await
}

#[tokio::test]
async fn a_verified_domain_names_its_workspace() {
    let world = resolving().await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    let globex = world.workspace(owner, "Globex").await;
    world.verified(acme, "crm.acme-corp.test").await;
    assert!(world.domains().await.resolves_workspaces());

    world
        .as_user(owner)
        .get("/api/whoami")
        .header("host", "CRM.acme-corp.test:8443")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace", json!(acme));
    // The domain resolver comes first: a header naming another workspace
    // is not read.
    world
        .as_user(owner)
        .get("/api/whoami")
        .header("host", "crm.acme-corp.test")
        .header("x-workspace", globex.to_string())
        .send()
        .await
        .assert_json_path("workspace", json!(acme));
    // Another host falls through to the header.
    world
        .as_user(owner)
        .get("/api/whoami")
        .header("host", "app.example.com")
        .header("x-workspace", globex.to_string())
        .send()
        .await
        .assert_json_path("workspace", json!(globex));
    // A stranger signed in on the domain is not a member: 403.
    world
        .as_user(Uuid::now_v7())
        .get("/api/whoami")
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn pending_domains_name_nothing() {
    let world = resolving().await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    world.add(acme, "crm.acme-corp.test").await;
    world
        .as_user(owner)
        .get("/api/whoami")
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn without_the_domain_resolver_hosts_are_ignored() {
    let world = World::new().await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    world.verified(acme, "crm.acme-corp.test").await;
    assert!(!world.domains().await.resolves_workspaces());
    world
        .as_user(owner)
        .get("/api/whoami")
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn public_pages_know_their_domain() {
    let world = World::new().await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.verified(acme, "crm.acme-corp.test").await;
    world
        .app
        .client()
        .get("/landing")
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(domain.id))
        .assert_json_path("workspace_id", json!(acme))
        .assert_json_path("host", json!("crm.acme-corp.test"))
        .assert_json_path("purpose", json!("app"));
    world
        .app
        .client()
        .get("/landing")
        .header("host", "unknown.test")
        .send()
        .await
        .assert_error_code(404, "domain_not_found");
    let other = world
        .app
        .client()
        .get("/maybe")
        .header("host", "unknown.test")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(other["domain"], Value::Null);
    world
        .app
        .client()
        .get("/maybe")
        .header("host", "crm.acme-corp.test")
        .send()
        .await
        .assert_json_path("domain.workspace_id", json!(acme));
}
