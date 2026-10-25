//! `GET /api/me/abilities` and the `/api/access` management routes.

use keel_access::{AbilityFactory, AccessService};
use keel_testing::principal;
use serde_json::json;
use uuid::Uuid;

use crate::support::{app, give_role, in_uow};

#[tokio::test]
async fn abilities_endpoint_returns_casl_json_with_etag_and_304() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "publisher", &["posts.publish"]).await;
    let version = app.get::<AccessService>().await.version().await.unwrap();
    let client = app.acting_as(principal(user));

    let response = client
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("0", json!({ "action": "publish", "subject": "Post" }))
        .assert_json_path(
            "1",
            json!({ "action": "read", "subject": "Post", "conditions": { "published": true } }),
        );
    let expected = app
        .get::<AbilityFactory>()
        .await
        .for_principal(&Some(principal(user)))
        .await
        .unwrap()
        .to_casl_json();
    assert_eq!(response.json_value(), expected);
    assert_eq!(response.header("cache-control"), Some("private, no-cache"));
    let etag = response.header("etag").expect("an ETag").to_owned();
    assert!(
        etag.starts_with(&format!("\"{user}-{version}-")) && etag.ends_with('"'),
        "{etag}"
    );

    // Unchanged: 304 with no body.
    let not_modified = client
        .get("/api/me/abilities")
        .header("if-none-match", etag.as_str())
        .send()
        .await
        .assert_status(304);
    assert!(not_modified.bytes().is_empty());
    assert_eq!(not_modified.header("etag"), Some(etag.as_str()));
    client
        .get("/api/me/abilities")
        .header("if-none-match", format!("W/{etag}, \"other\""))
        .send()
        .await
        .assert_status(304);

    // A role change gives a new ETag (and the new rules).
    in_uow(&app, |service, uow| async move {
        service.revoke(&uow, "publisher", "posts.publish").await
    })
    .await;
    let changed = client
        .get("/api/me/abilities")
        .header("if-none-match", etag.as_str())
        .send()
        .await
        .assert_status(200);
    assert_ne!(changed.header("etag"), Some(etag.as_str()));
    assert_ne!(changed.json_value()[0]["action"], json!("publish"));

    app.client()
        .get("/api/me/abilities")
        .send()
        .await
        .assert_problem(401, "unauthenticated");
}

#[tokio::test]
async fn management_routes_need_the_roles_manage_permission() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "writer", &["posts.update"]).await;
    let client = app.acting_as(principal(user));

    client
        .get("/api/access/roles")
        .send()
        .await
        .assert_problem(403, "forbidden");
    client
        .post("/api/access/roles")
        .json(&json!({ "name": "sneaky" }))
        .send()
        .await
        .assert_problem(403, "forbidden");
    client
        .put(&format!("/api/access/users/{user}/roles"))
        .json(&json!({ "roles": ["writer"] }))
        .send()
        .await
        .assert_problem(403, "forbidden");
}

#[tokio::test]
async fn management_routes_create_roles_and_sync_permissions_and_user_roles() {
    let (app, _audit) = app().await;
    let (admin, member) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, admin, "access-admin", &["roles.manage"]).await;
    let client = app.acting_as(principal(admin));

    client
        .post("/api/access/roles")
        .json(&json!({ "name": "support", "description": "Answers tickets" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("name", json!("support"))
        .assert_json_path("description", json!("Answers tickets"))
        .assert_json_path("permissions", json!([]));
    client
        .post("/api/access/roles")
        .json(&json!({ "name": "support" }))
        .send()
        .await
        .assert_problem(409, "conflict");
    client
        .post("/api/access/roles")
        .json(&json!({ "name": "has space" }))
        .send()
        .await
        .assert_problem(422, "validation_failed");

    client
        .put("/api/access/roles/support/permissions")
        .json(&json!({ "permissions": ["posts.update", "posts.read"] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("permissions", json!(["posts.read", "posts.update"]));
    client
        .put("/api/access/roles/nobody/permissions")
        .json(&json!({ "permissions": [] }))
        .send()
        .await
        .assert_problem(404, "not_found");

    client
        .put(&format!("/api/access/users/{member}/roles"))
        .json(&json!({ "roles": ["support"] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(member))
        .assert_json_path("roles", json!(["support"]));
    client
        .put(&format!("/api/access/users/{member}/roles"))
        .json(&json!({ "roles": ["support", "ghost"] }))
        .send()
        .await
        .assert_problem(422, "validation_failed")
        .assert_json_path("errors.roles.0", json!("The role `ghost` does not exist."));

    let service = app.get::<AccessService>().await;
    assert_eq!(
        service.permissions_of(member).await.unwrap(),
        ["posts.read", "posts.update"]
    );

    client
        .get("/api/access/roles")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.name", json!("access-admin"))
        .assert_json_path("data.1.name", json!("support"))
        .assert_json_path("data.1.permissions", json!(["posts.read", "posts.update"]));

    client
        .delete("/api/access/roles/support")
        .send()
        .await
        .assert_status(204);
    assert!(service.roles_of(member).await.unwrap().is_empty());
    client
        .delete("/api/access/roles/support")
        .send()
        .await
        .assert_problem(404, "not_found");
}
