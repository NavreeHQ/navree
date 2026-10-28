//! The routes: `/api/domains` for a workspace's settings page, and the ask
//! endpoint of an on-demand TLS proxy.

use navree_domains::DomainStatus;
use navree_workspace::Members;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{TARGET, World};

#[tokio::test]
async fn owners_manage_their_workspace_domains() {
    let world = World::new().await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    let client = world.as_user(owner);
    let ws = acme.to_string();

    let created = client
        .post("/api/domains")
        .header("x-workspace", ws.as_str())
        .json(&json!({ "host": "CRM.acme-corp.test", "purpose": "app" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("host", json!("crm.acme-corp.test"))
        .assert_json_path("status", json!("pending"))
        .assert_json_path("tls", json!("pending"))
        .assert_json_path("dns_records.1.type", json!("CNAME"))
        .assert_json_path("dns_records.1.value", json!(TARGET))
        .assert_json_path("created_by", json!(owner));
    let body = created.json_value();
    let id = body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        created.header("location"),
        Some(format!("/api/domains/{id}").as_str())
    );

    client
        .get("/api/domains")
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.id", json!(id));
    client
        .get(&format!("/api/domains/{id}"))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("host", json!("crm.acme-corp.test"));

    // Not verified yet: it can't be the primary domain.
    client
        .post(&format!("/api/domains/{id}/primary"))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_error_code(409, "domain_not_verified");

    // The settings page's "Check now" button.
    client
        .post(&format!("/api/domains/{id}/verify"))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("pending"));
    let domain = world.domains().await.get(id.parse().unwrap()).await.unwrap();
    world.publish(&domain);
    client
        .post(&format!("/api/domains/{id}/verify"))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("verified"))
        .assert_json_path("tls", json!("active"));
    client
        .post(&format!("/api/domains/{id}/primary"))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("primary", json!(true));
    let domains = world.domains().await;
    assert_eq!(
        domains.primary_url(acme).await.unwrap().as_deref(),
        Some("https://crm.acme-corp.test")
    );

    client
        .delete(&format!("/api/domains/{id}"))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_status(204);
    assert!(domains.list(acme).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_new_primary_domain_replaces_the_old_one() {
    let world = World::new().await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    let first = world.verified(acme, "crm.acme-corp.test").await;
    let second = world.verified(acme, "app.acme-corp.test").await;
    let domains = world.domains().await;
    domains.make_primary(first.id).await.unwrap();
    let now = domains.make_primary(second.id).await.unwrap();
    assert!(now.primary);
    assert!(!domains.get(first.id).await.unwrap().primary);
    assert_eq!(domains.primary(acme).await.unwrap().unwrap().id, second.id);
}

#[tokio::test]
async fn refusals_carry_their_codes() {
    let world = World::with(|builder| builder.config("domains.max_per_workspace", 1)).await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    let globex = world.workspace(Uuid::now_v7(), "Globex").await;
    let theirs = world.add(globex, "globex.test").await;
    let client = world.as_user(owner);
    let ws = acme.to_string();
    let add = |host: &str| {
        client
            .post("/api/domains")
            .header("x-workspace", ws.as_str())
            .json(&json!({ "host": host }))
    };
    add("https://crm.acme-corp.test/").send().await.assert_error_code(400, "invalid_domain");
    add("app.example.com").send().await.assert_error_code(400, "domain_not_allowed");
    add("globex.test").send().await.assert_error_code(409, "domain_taken");
    add("crm.acme-corp.test").send().await.assert_status(201);
    add("go.acme-corp.test").send().await.assert_error_code(409, "domain_limit_reached");
    // Another workspace's domain does not exist here.
    client
        .get(&format!("/api/domains/{}", theirs.id))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_error_code(404, "domain_not_found");
    client
        .delete(&format!("/api/domains/{}", theirs.id))
        .header("x-workspace", ws.as_str())
        .send()
        .await
        .assert_error_code(404, "domain_not_found");
    assert_eq!(
        world.domains().await.get(theirs.id).await.unwrap().status,
        DomainStatus::Pending
    );
}

#[tokio::test]
async fn members_without_the_permission_are_refused() {
    let world = World::new().await;
    let owner = Uuid::now_v7();
    let member = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    let members = world.app.get::<Members>().await;
    let db = world.db().await;
    let uow = db.uow(None, None).await;
    members
        .add(&uow, acme, member, "member".into())
        .await
        .unwrap();
    uow.commit().await.unwrap();
    world
        .as_user(member)
        .get("/api/domains")
        .header("x-workspace", acme.to_string())
        .send()
        .await
        .assert_status(403);
    world
        .app
        .client()
        .get("/api/domains")
        .header("x-workspace", acme.to_string())
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn the_ask_endpoint_allows_verified_domains_only() {
    let world = World::new().await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    let domain = world.add(acme, "crm.acme-corp.test").await;
    let client = world.app.client();
    client
        .get("/_domains/allow?domain=crm.acme-corp.test")
        .send()
        .await
        .assert_error_code(404, "domain_not_found");
    world.publish(&domain);
    world.domains().await.verify(domain.id).await.unwrap();
    client
        .get("/_domains/allow?domain=CRM.acme-corp.test")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("domain", json!("crm.acme-corp.test"));
    client
        .get("/_domains/allow")
        .send()
        .await
        .assert_error_code(400, "invalid_domain");
    client
        .get("/_domains/allow?domain=app.example.com")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_ask_path_is_configurable_and_off_without_on_demand_tls() {
    let world = World::with(|builder| builder.config("domains.ask_path", "/caddy/ask")).await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    world.verified(acme, "crm.acme-corp.test").await;
    let client = world.app.client();
    client
        .get("/caddy/ask?domain=crm.acme-corp.test")
        .send()
        .await
        .assert_status(200);
    client
        .get("/_domains/allow?domain=crm.acme-corp.test")
        .send()
        .await
        .assert_status(404);

    let off = World::with(|builder| builder.config("domains.tls", "off")).await;
    let acme = off.workspace(Uuid::now_v7(), "Acme").await;
    let domain = off.verified(acme, "crm.acme-corp.test").await;
    assert_eq!(serde_json::to_value(domain.tls).unwrap(), Value::from("off"));
    off.app
        .client()
        .get("/_domains/allow?domain=crm.acme-corp.test")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_routes_can_be_turned_off() {
    let world = World::with(|builder| builder.config("domains.routes", false)).await;
    let owner = Uuid::now_v7();
    let acme = world.workspace(owner, "Acme").await;
    world
        .as_user(owner)
        .get("/api/domains")
        .header("x-workspace", acme.to_string())
        .send()
        .await
        .assert_status(404);
}
