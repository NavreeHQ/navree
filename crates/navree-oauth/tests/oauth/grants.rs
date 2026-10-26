//! The other grants: client credentials, device authorization and refresh
//! tokens (rotation, reuse detection, narrower scopes).

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_oauth::NewClient;
use navree_oauth::testing::authorize;
use serde_json::{Value, json};

use crate::support::{DEVICE_URL, World, builder, string};

#[tokio::test]
async fn client_credentials_act_as_the_owner_narrowed_to_the_scopes() {
    let world = World::new().await;
    let (owner, _) = world
        .user("sync@acme.test", &["deals.read", "deals.create"])
        .await;
    let client = world
        .register(
            NewClient::confidential("Warehouse sync")
                .client_credentials()
                .owner(owner.id)
                .scopes(["deals:read", "deals:write"]),
        )
        .await;
    let basic = STANDARD.encode(format!(
        "{}:{}",
        client.client.id,
        client.secret.as_deref().unwrap()
    ));
    let response = world
        .client()
        .post("/oauth/token")
        .header("authorization", format!("Basic {basic}"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "deals:read"),
        ])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(response["scope"], "deals:read");
    assert!(response.get("refresh_token").is_none());
    let access = string(&response, "access_token");
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_json_path("user_id", json!(owner.id))
        .assert_json_path("oauth.grant", json!("client_credentials"));
    world
        .client()
        .post("/api/probe/deals")
        .bearer(&access)
        .send()
        .await
        .assert_status(403);

    // No scope: the client's scopes. A scope it doesn't have: invalid_scope.
    let all = world
        .client()
        .post("/oauth/token")
        .header("authorization", format!("Basic {basic}"))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(all["scope"], "deals:read deals:write");
    let refused = world
        .client()
        .post("/oauth/token")
        .header("authorization", format!("Basic {basic}"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "everything"),
        ])
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(refused["error"], "invalid_scope");

    // A client without the grant.
    let other = world.confidential_client(&["deals:read"]).await;
    let refused = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "client_credentials"),
                ("client_id", &other.client.id.to_string()),
                ("client_secret", other.secret.as_deref().unwrap()),
            ],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(refused["error"], "unauthorized_client");
}

#[tokio::test]
async fn the_device_flow_polls_until_the_user_approves() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world
        .register(
            NewClient::public("Acme CLI")
                .device()
                .scopes(["deals:read", "offline_access"]),
        )
        .await;
    let id = client.client.id.to_string();

    let started = world
        .post_form(
            "/oauth/device_authorization",
            &[("client_id", &id), ("scope", "deals:read offline_access")],
        )
        .await
        .assert_status(200)
        .json_value();
    let device_code = string(&started, "device_code");
    let user_code = string(&started, "user_code");
    assert!(device_code.starts_with("nvd_"));
    assert_eq!(user_code.len(), 9);
    assert_eq!(&user_code[4..5], "-");
    assert_eq!(started["verification_uri"], DEVICE_URL);
    assert_eq!(
        started["verification_uri_complete"],
        format!("{DEVICE_URL}?user_code={user_code}")
    );
    assert_eq!(started["expires_in"], 600);
    assert_eq!(started["interval"], 5);

    let poll = || {
        let (world, id, device_code) = (&world, id.clone(), device_code.clone());
        async move {
            world
                .post_form(
                    "/oauth/token",
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("device_code", &device_code),
                        ("client_id", &id),
                    ],
                )
                .await
        }
    };
    let error = |response: navree_testing::TestResponse| -> Value {
        response.assert_status(400).json_value()["error"].clone()
    };
    assert_eq!(error(poll().await), "authorization_pending");
    // Too fast.
    world.clock.advance(Duration::from_secs(2));
    assert_eq!(error(poll().await), "slow_down");
    world.clock.advance(Duration::from_secs(11));
    assert_eq!(error(poll().await), "authorization_pending");

    // The device page reads and approves the code, typed in lower case.
    let typed = user_code.to_lowercase();
    world
        .client()
        .get(&format!("/api/oauth/device/{typed}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_code", json!(user_code))
        .assert_json_path("client.name", json!("Acme CLI"))
        .assert_json_path("scopes.0.name", json!("deals:read"));
    world
        .client()
        .post(&format!("/api/oauth/device/{typed}/approve"))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);

    world.clock.advance(Duration::from_secs(11));
    let tokens = poll().await.assert_status(200).json_value();
    assert!(tokens.get("refresh_token").is_some());
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&string(&tokens, "access_token"))
        .send()
        .await
        .assert_json_path("user_id", json!(user.id))
        .assert_json_path("oauth.grant", json!("device_code"));
    // Once only.
    world.clock.advance(Duration::from_secs(11));
    assert_eq!(error(poll().await), "invalid_grant");
    // The code is no longer on the device page.
    world
        .client()
        .get(&format!("/api/oauth/device/{user_code}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn a_denied_or_expired_device_code_ends_the_polling() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world
        .register(NewClient::public("TV").device().scopes(["deals:read"]))
        .await;
    let id = client.client.id.to_string();
    let start = || {
        let (world, id) = (&world, id.clone());
        async move {
            world
                .post_form("/oauth/device_authorization", &[("client_id", &id)])
                .await
                .assert_status(200)
                .json_value()
        }
    };
    let poll = |device_code: String| {
        let (world, id) = (&world, id.clone());
        async move {
            world
                .post_form(
                    "/oauth/token",
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("device_code", &device_code),
                        ("client_id", &id),
                    ],
                )
                .await
                .assert_status(400)
                .json_value()["error"]
                .clone()
        }
    };

    let denied = start().await;
    world
        .client()
        .post(&format!(
            "/api/oauth/device/{}/deny",
            string(&denied, "user_code")
        ))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    assert_eq!(poll(string(&denied, "device_code")).await, "access_denied");

    let expired = start().await;
    world.clock.advance(Duration::from_secs(601));
    assert_eq!(poll(string(&expired, "device_code")).await, "expired_token");

    // A client without the grant can't start one.
    let other = world.public_client(&["deals:read"]).await;
    let refused = world
        .post_form(
            "/oauth/device_authorization",
            &[("client_id", &other.client.id.to_string())],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(refused["error"], "unauthorized_client");
}

#[tokio::test]
async fn the_device_grant_needs_a_device_page() {
    let world = World::with(builder().config("oauth.device_url", serde_json::Value::Null)).await;
    let client = world
        .register(NewClient::public("TV").device().scopes(["deals:read"]))
        .await;
    let response = world
        .post_form(
            "/oauth/device_authorization",
            &[("client_id", &client.client.id.to_string())],
        )
        .await
        .assert_status(503)
        .json_value();
    assert_eq!(response["error"], "temporarily_unavailable");
}

#[tokio::test]
async fn refresh_tokens_rotate_and_a_reused_one_revokes_the_family() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world
        .public_client(&["deals:read", "deals:export", "offline_access"])
        .await;
    let id = client.client.id.to_string();
    let first = authorize(
        &world.client(),
        client.client.id,
        &token,
        &["deals:read", "deals:export", "offline_access"],
    )
    .await;
    let refresh = |refresh_token: String, scope: Option<&'static str>| {
        let (world, id) = (&world, id.clone());
        async move {
            let mut form = vec![
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", id.as_str()),
            ];
            if let Some(scope) = scope {
                form.push(("scope", scope));
            }
            world.post_form("/oauth/token", &form).await
        }
    };

    let r1 = first.refresh_token.clone().unwrap();
    let second = refresh(r1.clone(), None)
        .await
        .assert_status(200)
        .json_value();
    let r2 = string(&second, "refresh_token");
    assert_ne!(r1, r2);
    assert_eq!(second["scope"], "deals:read deals:export offline_access");

    // Narrower scopes; never more.
    let narrow = refresh(r2.clone(), Some("deals:read"))
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(narrow["scope"], "deals:read");
    let r3 = string(&narrow, "refresh_token");
    let wider = refresh(r3.clone(), Some("deals:read everything"))
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(wider["error"], "invalid_scope");
    // The narrowed refresh keeps the original grant.
    let back = refresh(r3, Some("deals:export"))
        .await
        .assert_status(200)
        .json_value();
    let r4 = string(&back, "refresh_token");
    let live = string(&back, "access_token");
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&live)
        .send()
        .await
        .assert_status(200);

    // r1 again: reuse. The family dies, the live tokens with it.
    let reused = refresh(r1, None).await.assert_status(400).json_value();
    assert_eq!(reused["error"], "invalid_grant");
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&live)
        .send()
        .await
        .assert_status(401);
    refresh(r4, None).await.assert_status(400);
    let detected = world.audit.with_action("oauth.refresh_reuse_detected");
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].actor_id, Some(user.id));
}

#[tokio::test]
async fn refresh_tokens_need_offline_access_and_expire() {
    let world = World::with(builder().config("oauth.refresh_token_ttl_secs", 120)).await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let without = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    assert!(without.refresh_token.is_none());

    let with = authorize(
        &world.client(),
        client.client.id,
        &token,
        &["deals:read", "offline_access"],
    )
    .await;
    let id = client.client.id.to_string();
    // Sliding: each refresh starts the lifetime again.
    world.clock.advance(Duration::from_secs(100));
    let next = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", with.refresh_token.as_deref().unwrap()),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(200)
        .json_value();
    world.clock.advance(Duration::from_secs(100));
    let next = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &string(&next, "refresh_token")),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(200)
        .json_value();
    world.clock.advance(Duration::from_secs(121));
    let expired = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &string(&next, "refresh_token")),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(
        expired["error_description"],
        "The refresh token has expired"
    );

    // Another client's refresh token is invalid for this one.
    let other = world.public_client(&["deals:read", "offline_access"]).await;
    let theirs = authorize(
        &world.client(),
        other.client.id,
        &world.login("rep@acme.test").await,
        &["deals:read", "offline_access"],
    )
    .await;
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", theirs.refresh_token.as_deref().unwrap()),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(400);
}
