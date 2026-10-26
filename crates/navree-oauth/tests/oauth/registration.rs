//! Dynamic client registration (RFC 7591), behind
//! `oauth.dynamic_registration`.

use navree_oauth::testing::{AuthorizeFlow, authorize};
use serde_json::json;
use uuid::Uuid;

use crate::support::{ISSUER, REDIRECT_URI, World, builder, string};

#[tokio::test]
async fn registration_is_off_by_default() {
    let world = World::new().await;
    world
        .client()
        .post("/oauth/register")
        .json(&json!({ "client_name": "Claude", "redirect_uris": [REDIRECT_URI] }))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn clients_register_themselves_and_use_the_code_flow() {
    let world = World::with(builder().config("oauth.dynamic_registration", true)).await;
    world
        .client()
        .get("/.well-known/oauth-authorization-server")
        .send()
        .await
        .assert_json_path(
            "registration_endpoint",
            json!(format!("{ISSUER}/oauth/register")),
        );

    let public = world
        .client()
        .post("/oauth/register")
        .json(&json!({
            "client_name": "Claude",
            "redirect_uris": [REDIRECT_URI],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "scope": "deals:read offline_access"
        }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("client_name", json!("Claude"))
        .assert_json_path("token_endpoint_auth_method", json!("none"))
        .assert_json_path("redirect_uris", json!([REDIRECT_URI]))
        .assert_json_path("scope", json!("deals:read offline_access"))
        .json_value();
    assert!(public.get("client_secret").is_none());
    let id: Uuid = string(&public, "client_id").parse().unwrap();
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let tokens = authorize(
        &world.client(),
        id,
        &token,
        &["deals:read", "offline_access"],
    )
    .await;
    assert!(tokens.refresh_token.is_some());

    let confidential = world
        .client()
        .post("/oauth/register")
        .json(&json!({ "client_name": "Server app", "redirect_uris": [REDIRECT_URI] }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("client_secret_expires_at", json!(0))
        .json_value();
    let secret = string(&confidential, "client_secret");
    assert!(secret.starts_with("nvcs_"));
    let id: Uuid = string(&confidential, "client_id").parse().unwrap();
    // Every scope by default.
    AuthorizeFlow::new(id)
        .secret(secret)
        .run(&world.client(), &token, &["everything"])
        .await;
}

#[tokio::test]
async fn invalid_metadata_is_refused() {
    let world = World::with(builder().config("oauth.dynamic_registration", true)).await;
    let register = |body: serde_json::Value| {
        let world = &world;
        async move {
            world
                .client()
                .post("/oauth/register")
                .json(&body)
                .send()
                .await
                .assert_status(400)
                .json_value()["error"]
                .clone()
        }
    };
    assert_eq!(
        register(json!({ "redirect_uris": [REDIRECT_URI] })).await,
        "invalid_client_metadata"
    );
    assert_eq!(
        register(json!({ "client_name": "x" })).await,
        "invalid_redirect_uri"
    );
    assert_eq!(
        register(json!({ "client_name": "x", "redirect_uris": ["http://evil.test/cb"] })).await,
        "invalid_redirect_uri"
    );
    assert_eq!(
        register(json!({ "client_name": "x", "redirect_uris": [REDIRECT_URI], "grant_types": ["client_credentials"] })).await,
        "invalid_client_metadata"
    );
    assert_eq!(
        register(json!({ "client_name": "x", "redirect_uris": [REDIRECT_URI], "scope": "admin" }))
            .await,
        "invalid_client_metadata"
    );
    assert_eq!(
        register(json!({ "client_name": "x", "redirect_uris": [REDIRECT_URI], "token_endpoint_auth_method": "private_key_jwt" })).await,
        "invalid_client_metadata"
    );
    assert_eq!(
        register(json!("not an object")).await,
        "invalid_client_metadata"
    );
}
