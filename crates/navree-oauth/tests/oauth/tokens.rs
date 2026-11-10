//! Calling the API with OAuth tokens: abilities narrowed to the scopes,
//! the `WWW-Authenticate` answers, `OAuthToken`, the lookup cache.

use std::time::Duration;

use navree_oauth::testing::authorize;
use serde_json::json;

use crate::support::{World, builder};

#[tokio::test]
async fn a_token_s_abilities_are_the_user_s_narrowed_to_its_scopes() {
    let world = World::new().await;
    let (user, token) = world
        .user(
            "rep@acme.test",
            &["deals.read", "deals.create", "deals.update"],
        )
        .await;
    let client = world
        .public_client(&["deals:read", "deals:write", "everything"])
        .await;
    let read = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;

    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&read.access_token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id))
        .assert_json_path("token_abilities", json!(["deals.read"]));
    world
        .client()
        .get("/api/probe/deals")
        .bearer(&read.access_token)
        .send()
        .await
        .assert_status(200);

    // A permission the token's scopes don't grant: 403 insufficient_scope,
    // naming the scopes that would.
    let refused = world
        .client()
        .post("/api/probe/deals")
        .bearer(&read.access_token)
        .send()
        .await
        .assert_error(403, None);
    assert_eq!(
        refused.header("www-authenticate"),
        Some(r#"Bearer error="insufficient_scope", scope="deals:write everything""#)
    );
    let refused = world
        .client()
        .patch("/api/probe/deals")
        .bearer(&read.access_token)
        .send()
        .await
        .assert_status(403);
    assert!(
        refused
            .header("www-authenticate")
            .unwrap()
            .contains("insufficient_scope")
    );

    // With the scope, the user's own permissions apply.
    let write = authorize(&world.client(), client.client.id, &token, &["deals:write"]).await;
    world
        .client()
        .post("/api/probe/deals")
        .bearer(&write.access_token)
        .send()
        .await
        .assert_status(200);
    let everything = authorize(&world.client(), client.client.id, &token, &["everything"]).await;
    world
        .client()
        .patch("/api/probe/deals")
        .bearer(&everything.access_token)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_scope_never_adds_to_what_the_user_may_do() {
    let world = World::new().await;
    let (_, token) = world.user("viewer@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:write"]).await;
    let write = authorize(&world.client(), client.client.id, &token, &["deals:write"]).await;
    // The token allows it, the user doesn't: a plain 403.
    let refused = world
        .client()
        .post("/api/probe/deals")
        .bearer(&write.access_token)
        .send()
        .await
        .assert_status(403);
    assert!(refused.header("www-authenticate").is_none());
}

#[tokio::test]
async fn invalid_tokens_are_401_with_a_bearer_challenge() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;

    for bad in ["nva_unknown", "nvp_unknown"] {
        let response = world
            .client()
            .get("/api/probe/whoami")
            .bearer(bad)
            .send()
            .await
            .assert_error(401, "Invalid token");
        assert_eq!(
            response.header("www-authenticate"),
            Some(r#"Bearer error="invalid_token""#)
        );
    }
    // Even on public routes: an invalid credential never falls back to a
    // guest.
    world
        .client()
        .get("/oauth/jwks")
        .bearer("nva_unknown")
        .send()
        .await
        .assert_status(401);

    // Expired.
    world.clock.advance(Duration::from_secs(3601));
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_error(401, "Invalid token");
}

#[tokio::test]
async fn handlers_read_the_token_with_oauth_token() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "deals:export"]).await;
    let read = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;

    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&read.access_token)
        .send()
        .await
        .assert_json_path("oauth.client_name", json!("Agra mobile"))
        .assert_json_path("oauth.scopes", json!(["deals:read"]))
        .assert_json_path("oauth.user_id", json!(user.id))
        .assert_json_path("oauth.workspace_id", json!(null));
    // A first-party JWT: no OAuthToken.
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&token)
        .send()
        .await
        .assert_json_path("oauth", json!(null))
        .assert_json_path("token_abilities", json!(null));
    world
        .client()
        .get("/api/probe/token")
        .bearer(&token)
        .send()
        .await
        .assert_status(401);

    // require_scope.
    let refused = world
        .client()
        .post("/api/probe/export")
        .bearer(&read.access_token)
        .send()
        .await
        .assert_status(403);
    assert_eq!(
        refused.header("www-authenticate"),
        Some(r#"Bearer error="insufficient_scope", scope="deals:export""#)
    );
    let export = authorize(&world.client(), client.client.id, &token, &["deals:export"]).await;
    world
        .client()
        .post("/api/probe/export")
        .bearer(&export.access_token)
        .send()
        .await
        .assert_status(200);
    world
        .client()
        .post("/api/probe/export")
        .bearer(&token)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn revocation_is_immediate_on_the_instance_that_revoked() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    // Cached now.
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);
    let id = client.client.id.to_string();
    world
        .post_form(
            "/oauth/revoke",
            &[("token", &tokens.access_token), ("client_id", &id)],
        )
        .await
        .assert_status(200);
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn without_the_cache_every_request_reads_the_database() {
    let world = World::with(builder().config("oauth.token_cache_secs", 0)).await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    world
        .client()
        .get("/api/probe/deals")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn revoking_a_client_kills_its_tokens() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);
    world
        .app
        .run_command("oauth:client:revoke", &[&client.client.id.to_string()])
        .await
        .unwrap();
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(401);
}
