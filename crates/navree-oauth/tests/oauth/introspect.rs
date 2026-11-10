//! Introspection (RFC 7662) and revocation (RFC 7009).

use std::time::Duration;

use navree_oauth::NewClient;
use navree_oauth::testing::authorize;
use serde_json::json;

use crate::support::{ISSUER, World};

#[tokio::test]
async fn resource_servers_introspect_tokens() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let tokens = authorize(
        &world.client(),
        client.client.id,
        &token,
        &["deals:read", "offline_access"],
    )
    .await;
    let resource = world
        .register(NewClient::confidential("Billing service").introspect())
        .await;
    let (rid, secret) = (
        resource.client.id.to_string(),
        resource.secret.clone().unwrap(),
    );
    let introspect = |token: String| {
        let (world, rid, secret) = (&world, rid.clone(), secret.clone());
        async move {
            world
                .post_form(
                    "/oauth/introspect",
                    &[
                        ("token", &token),
                        ("client_id", &rid),
                        ("client_secret", &secret),
                    ],
                )
                .await
                .assert_status(200)
                .json_value()
        }
    };

    let active = introspect(tokens.access_token.clone()).await;
    assert_eq!(active["active"], true);
    assert_eq!(active["scope"], "deals:read offline_access");
    assert_eq!(active["client_id"], json!(client.client.id));
    assert_eq!(active["sub"], json!(user.id));
    assert_eq!(active["username"], "rep@acme.test");
    assert_eq!(active["iss"], ISSUER);
    assert_eq!(active["token_type"], "Bearer");
    assert_eq!(
        active["exp"].as_i64().unwrap() - active["iat"].as_i64().unwrap(),
        3600
    );
    let refresh = introspect(tokens.refresh_token.clone().unwrap()).await;
    assert_eq!(refresh["active"], true);
    assert_eq!(refresh["token_type"], "refresh_token");

    for unknown in ["nva_nope", "nvr_nope", "nvp_nope", "garbage"] {
        assert_eq!(
            introspect(unknown.to_owned()).await,
            json!({"active": false})
        );
    }
    world.clock.advance(Duration::from_secs(3601));
    assert_eq!(
        introspect(tokens.access_token).await,
        json!({"active": false})
    );

    // Only clients created to introspect may.
    let other = world.confidential_client(&["deals:read"]).await;
    let refused = world
        .post_form(
            "/oauth/introspect",
            &[
                ("token", "nva_x"),
                ("client_id", &other.client.id.to_string()),
                ("client_secret", other.secret.as_deref().unwrap()),
            ],
        )
        .await
        .assert_status(401)
        .json_value();
    assert_eq!(refused["error"], "invalid_client");
    world
        .post_form("/oauth/introspect", &[("token", "nva_x")])
        .await
        .assert_status(401);
}

#[tokio::test]
async fn personal_access_tokens_introspect_without_a_client() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let pat = world
        .client()
        .post("/api/oauth/personal-access-tokens")
        .bearer(&token)
        .json(&json!({ "name": "script", "scopes": ["deals:read"] }))
        .send()
        .await
        .assert_status(201)
        .json_value()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let resource = world
        .register(NewClient::confidential("Service").introspect())
        .await;
    let body = world
        .post_form(
            "/oauth/introspect",
            &[
                ("token", &pat),
                ("client_id", &resource.client.id.to_string()),
                ("client_secret", resource.secret.as_deref().unwrap()),
            ],
        )
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["active"], true);
    assert_eq!(body["sub"], json!(user.id));
    assert!(body.get("client_id").is_none());
}

#[tokio::test]
async fn clients_revoke_their_own_tokens() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let id = client.client.id.to_string();
    let tokens = authorize(
        &world.client(),
        client.client.id,
        &token,
        &["deals:read", "offline_access"],
    )
    .await;

    // Another client can't revoke them (the answer is 200 regardless).
    let other = world.public_client(&["deals:read"]).await;
    world
        .post_form(
            "/oauth/revoke",
            &[
                ("token", &tokens.access_token),
                ("client_id", &other.client.id.to_string()),
            ],
        )
        .await
        .assert_status(200);
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);

    // Revoking the refresh token ends the grant: its access tokens too.
    world
        .post_form(
            "/oauth/revoke",
            &[
                ("token", tokens.refresh_token.as_deref().unwrap()),
                ("token_type_hint", "refresh_token"),
                ("client_id", &id),
            ],
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
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", tokens.refresh_token.as_deref().unwrap()),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(400);
    // Unknown tokens: 200 too.
    world
        .post_form(
            "/oauth/revoke",
            &[("token", "nva_nope"), ("client_id", &id)],
        )
        .await
        .assert_status(200);
    // A token is required, and so is the client.
    world
        .post_form("/oauth/revoke", &[("client_id", &id)])
        .await
        .assert_status(400);
    world
        .post_form("/oauth/revoke", &[("token", "nva_nope")])
        .await
        .assert_status(401);
}
