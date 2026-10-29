//! Secrets stay out of change events. An observer registered in the app
//! sees every change event the unit of work records while the server runs
//! every flow that writes a secret (client secrets, authorization codes,
//! access and refresh tokens, device codes, personal access tokens); none
//! carries a secret or its digest.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_crypto::sha256_hex;
use navree_db::{ChangeEvent, ChangeObserver, DbError, ModuleBuilderDbExt, Uow};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_oauth::testing::pkce_pair;
use navree_oauth::{NewClient, OAuthClients};
use serde_json::json;

use crate::support::{REDIRECT_URI, World, builder, string};

/// Every change event, in the order they were committed.
#[derive(Default)]
struct Seen(Mutex<Vec<ChangeEvent>>);

struct Recorder {
    seen: Arc<Seen>,
}

impl Injectable for Recorder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Seen>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Recorder {
                seen: r.get::<Seen>().await?,
            })
        })
    }
}

impl ChangeObserver for Recorder {
    fn observe<'a>(
        &'a self,
        _uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.seen.0.lock().unwrap().extend(changes.iter().cloned());
            Ok(())
        })
    }
}

struct RecorderModule;

impl Module for RecorderModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Seen::default()).change_observer::<Recorder>();
    }
}

/// The OAuth tables.
const TABLES: [&str; 8] = [
    "oauth_clients",
    "oauth_authorizations",
    "oauth_authorization_requests",
    "oauth_codes",
    "oauth_access_tokens",
    "oauth_refresh_tokens",
    "oauth_device_codes",
    "personal_access_tokens",
];

#[tokio::test]
async fn no_secret_reaches_a_change_event() {
    let world = World::with(builder().module(RecorderModule)).await;
    let mut secrets: Vec<String> = Vec::new();
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;

    // A confidential client, and a new secret for it.
    let confidential = world
        .confidential_client(&["deals:read", "offline_access"])
        .await;
    secrets.extend(confidential.secret.clone());
    let clients = world.app.get::<OAuthClients>().await;
    let id = confidential.client.id;
    let rotated = world
        .work(|uow| async move { Ok(clients.rotate_secret(&uow, id).await?) })
        .await;
    secrets.push(rotated);

    // The authorization code flow with PKCE, a refresh, and a reused
    // refresh token (which revokes the family).
    let public = world.public_client(&["deals:read", "offline_access"]).await;
    let client_id = public.client.id.to_string();
    let (verifier, challenge) = pkce_pair();
    let request = world
        .request(public.client.id, "deals:read offline_access", &challenge)
        .await;
    let code = world.approve(&request, &token).await;
    secrets.push(code.clone());
    let first = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", &verifier),
                ("client_id", &client_id),
            ],
        )
        .await
        .assert_status(200)
        .json_value();
    let first_refresh = string(&first, "refresh_token");
    secrets.push(string(&first, "access_token"));
    secrets.push(first_refresh.clone());
    let refresh = |refresh_token: String| {
        let (world, client_id) = (&world, client_id.clone());
        async move {
            world
                .post_form(
                    "/oauth/token",
                    &[
                        ("grant_type", "refresh_token"),
                        ("refresh_token", &refresh_token),
                        ("client_id", &client_id),
                    ],
                )
                .await
        }
    };
    let second = refresh(first_refresh.clone())
        .await
        .assert_status(200)
        .json_value();
    secrets.push(string(&second, "access_token"));
    secrets.push(string(&second, "refresh_token"));
    refresh(first_refresh).await.assert_status(400);

    // Revoking a token at the revocation endpoint.
    let third = world
        .code_flow(public.client.id, &token, "deals:read offline_access")
        .await;
    secrets.push(string(&third, "access_token"));
    secrets.push(string(&third, "refresh_token"));
    world
        .post_form(
            "/oauth/revoke",
            &[
                ("token", &string(&third, "access_token")),
                ("client_id", &client_id),
            ],
        )
        .await
        .assert_status(200);

    // The device flow.
    let device = world
        .register(
            NewClient::public("Acme CLI")
                .device()
                .scopes(["deals:read", "offline_access"]),
        )
        .await;
    let device_id = device.client.id.to_string();
    let started = world
        .post_form(
            "/oauth/device_authorization",
            &[
                ("client_id", &device_id),
                ("scope", "deals:read offline_access"),
            ],
        )
        .await
        .assert_status(200)
        .json_value();
    let device_code = string(&started, "device_code");
    secrets.push(device_code.clone());
    let poll = || {
        let (world, device_id, device_code) = (&world, device_id.clone(), device_code.clone());
        async move {
            world
                .post_form(
                    "/oauth/token",
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("device_code", &device_code),
                        ("client_id", &device_id),
                    ],
                )
                .await
        }
    };
    poll().await.assert_status(400);
    world.clock.advance(Duration::from_secs(2));
    poll().await.assert_status(400);
    world.clock.advance(Duration::from_secs(11));
    world
        .client()
        .post(&format!(
            "/api/oauth/device/{}/approve",
            string(&started, "user_code")
        ))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    world.clock.advance(Duration::from_secs(11));
    let device_tokens = poll().await.assert_status(200).json_value();
    secrets.push(string(&device_tokens, "access_token"));
    secrets.push(string(&device_tokens, "refresh_token"));

    // A personal access token, created and revoked.
    let pat = world
        .client()
        .post("/api/oauth/personal-access-tokens")
        .bearer(&token)
        .json(&json!({ "name": "Weekly report", "scopes": ["deals:read"] }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    secrets.push(string(&pat, "token"));
    world
        .client()
        .delete(&format!(
            "/api/oauth/personal-access-tokens/{}",
            string(&pat, "id")
        ))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);

    // A machine client, then the client revoked with everything it holds.
    let machine = world
        .register(
            NewClient::confidential("Warehouse sync")
                .client_credentials()
                .owner(user.id)
                .scopes(["deals:read"]),
        )
        .await;
    secrets.extend(machine.secret.clone());
    let machine_id = machine.client.id;
    let clients = world.app.get::<OAuthClients>().await;
    world
        .work(|uow| async move { Ok(clients.revoke(&uow, machine_id).await?) })
        .await;
    world.clock.advance(Duration::from_secs(40 * 86_400));
    world
        .app
        .run_command("oauth:prune", &["--days", "1"])
        .await
        .unwrap();

    let seen = world.app.get::<Seen>().await;
    let events = seen.0.lock().unwrap().clone();
    assert!(
        events.iter().any(|event| event.entity == "users"),
        "the observer sees the changes the app records: {events:?}"
    );
    assert!(secrets.len() >= 14, "{} secrets", secrets.len());
    for event in &events {
        let text = serde_json::to_string(&json!({ "old": event.old, "new": event.new })).unwrap();
        for secret in &secrets {
            assert!(
                !text.contains(secret.as_str()),
                "a secret is in a change event of {}: {text}",
                event.entity
            );
            assert!(
                !text.contains(&sha256_hex(secret.as_bytes())),
                "a digest is in a change event of {}: {text}",
                event.entity
            );
        }
        assert!(
            !TABLES.contains(&event.entity.as_str()) || !text.contains("_hash"),
            "an OAuth change event names a digest column: {text}"
        );
    }
}
