//! Rate limits on the protocol endpoints that take client credentials:
//! token, introspection, revocation and device authorization, per client
//! and per client IP (`oauth.rate_limits`).

use navree_oauth::{CreatedClient, EndpointLimit, NewClient, OAuthOptions};
use navree_testing::{TestAppBuilder, TestResponse};
use serde_json::{Value, json};

use crate::support::{self, World};

/// The four throttled endpoints.
const ENDPOINTS: [&str; 4] = ["token", "introspect", "revoke", "device_authorization"];

/// A confidential client that may use every throttled endpoint.
async fn client(world: &World, name: &str) -> CreatedClient {
    world
        .register(
            NewClient::confidential(name)
                .redirect_uri(support::REDIRECT_URI)
                .scopes(["deals:read"])
                .device()
                .introspect(),
        )
        .await
}

/// A valid request to `/oauth/{endpoint}` for `client` (the answer may be
/// an OAuth error such as `invalid_grant`: what matters is that the client
/// authenticated), from `ip` when given.
async fn call(
    world: &World,
    endpoint: &str,
    client: &CreatedClient,
    ip: Option<&str>,
) -> TestResponse {
    let id = client.client.id.to_string();
    let secret = client.secret.clone().unwrap();
    let mut form = vec![
        ("client_id", id.as_str()),
        ("client_secret", secret.as_str()),
    ];
    match endpoint {
        "token" => form.extend([
            ("grant_type", "refresh_token"),
            ("refresh_token", "nvr_unknown"),
        ]),
        "introspect" | "revoke" => form.push(("token", "nva_unknown")),
        _ => {}
    }
    let mut request = world
        .client()
        .post(&format!("/oauth/{endpoint}"))
        .form(&form);
    if let Some(ip) = ip {
        request = request.header("x-forwarded-for", ip);
    }
    request.send().await
}

/// A request with an unknown client from `ip`.
async fn anonymous(world: &World, endpoint: &str, ip: &str) -> TestResponse {
    world
        .client()
        .post(&format!("/oauth/{endpoint}"))
        .header("x-forwarded-for", ip)
        .form(&[
            ("client_id", "00000000-0000-7000-8000-000000000000"),
            ("client_secret", "wrong"),
            ("grant_type", "refresh_token"),
            ("refresh_token", "nvr_unknown"),
            ("token", "nva_unknown"),
        ])
        .send()
        .await
}

/// Asserts the 429 answer: an OAuth error body, `Retry-After`, no caching.
fn assert_limited(response: TestResponse) -> Value {
    let response = response.assert_status(429);
    let retry_after: u64 = response
        .header("retry-after")
        .expect("Retry-After")
        .parse()
        .expect("seconds");
    assert!((1..=60).contains(&retry_after), "{retry_after}");
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let body = response.json_value();
    assert_eq!(body["error"], "too_many_requests");
    assert!(
        body["error_description"]
            .as_str()
            .is_some_and(|text| text.contains("retry")),
        "{body}"
    );
    body
}

fn limits(per_client: u32, per_ip: u32) -> TestAppBuilder {
    let mut builder = support::builder().config("http.trust_proxy", true);
    for endpoint in ENDPOINTS {
        builder = builder
            .config(
                &format!("oauth.rate_limits.{endpoint}.per_client"),
                per_client,
            )
            .config(&format!("oauth.rate_limits.{endpoint}.per_ip"), per_ip);
    }
    builder
}

#[tokio::test]
async fn each_endpoint_limits_each_client() {
    let world = World::with(limits(2, 0)).await;
    let (zapier, ifttt) = (
        client(&world, "Zapier").await,
        client(&world, "IFTTT").await,
    );
    for endpoint in ENDPOINTS {
        for _ in 0..2 {
            let status = call(&world, endpoint, &zapier, None).await.status();
            assert_ne!(status, 429, "{endpoint}");
        }
        assert_limited(call(&world, endpoint, &zapier, None).await);
        // Another client has its own budget, as has each endpoint.
        let status = call(&world, endpoint, &ifttt, None).await.status();
        assert_ne!(status, 429, "{endpoint}");
    }
}

#[tokio::test]
async fn each_endpoint_limits_each_ip() {
    let world = World::with(limits(0, 2)).await;
    let zapier = client(&world, "Zapier").await;
    for endpoint in ENDPOINTS {
        // Failed client authentications count against the address too.
        anonymous(&world, endpoint, "203.0.113.7")
            .await
            .assert_status(401);
        call(&world, endpoint, &zapier, Some("203.0.113.7")).await;
        assert_limited(call(&world, endpoint, &zapier, Some("203.0.113.7")).await);
        assert_limited(anonymous(&world, endpoint, "203.0.113.7").await);
        // Another address is not limited.
        let status = call(&world, endpoint, &zapier, Some("198.51.100.9"))
            .await
            .status();
        assert_ne!(status, 429, "{endpoint}");
    }
}

#[tokio::test]
async fn a_flood_of_unknown_clients_cannot_use_up_a_clients_budget() {
    // The per-client count starts once the client has authenticated, so
    // requests naming its id with a wrong secret don't lock it out.
    let world = World::with(limits(2, 0)).await;
    let zapier = client(&world, "Zapier").await;
    let id = zapier.client.id.to_string();
    for _ in 0..5 {
        world
            .post_form(
                "/oauth/token",
                &[
                    ("client_id", &id),
                    ("client_secret", "wrong"),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", "nvr_unknown"),
                ],
            )
            .await
            .assert_status(401);
    }
    let status = call(&world, "token", &zapier, None).await.status();
    assert_ne!(status, 429);
}

#[tokio::test]
async fn nobody_can_use_up_a_public_clients_budget() {
    // A public client has no secret: anyone can name it, so a per-client
    // count would let one attacker lock every user of the app out. Each
    // address keeps its own limit.
    let world = World::with(limits(2, 3)).await;
    let mobile = world
        .register(
            NewClient::public("Acme mobile")
                .redirect_uri(support::REDIRECT_URI)
                .scopes(["deals:read"])
                .device(),
        )
        .await;
    let id = mobile.client.id.to_string();
    let request = |ip: &'static str| {
        let id = id.clone();
        let world = &world;
        async move {
            world
                .client()
                .post("/oauth/token")
                .header("x-forwarded-for", ip)
                .form(&[
                    ("client_id", id.as_str()),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", "nvr_unknown"),
                ])
                .send()
                .await
        }
    };
    for attacker in ["203.0.113.7", "203.0.113.8"] {
        for _ in 0..3 {
            assert_ne!(request(attacker).await.status(), 429);
        }
        assert_limited(request(attacker).await);
    }
    let status = request("198.51.100.9").await.status();
    assert_ne!(status, 429, "a user of the app is locked out");
}

#[tokio::test]
async fn a_refusal_is_audited_once_per_window() {
    let world = World::with(limits(1, 0)).await;
    let zapier = client(&world, "Zapier").await;
    call(&world, "token", &zapier, None).await;
    for _ in 0..3 {
        assert_limited(call(&world, "token", &zapier, None).await);
    }
    let limited = world.audit.with_action("oauth.rate_limited");
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].metadata["endpoint"], "token");
    assert_eq!(limited[0].metadata["limit"], "per_client");
    assert_eq!(
        limited[0].metadata["client_id"],
        json!(zapier.client.id.to_string())
    );
}

#[tokio::test]
async fn zero_turns_a_limit_off() {
    let world = World::with(limits(0, 0)).await;
    let zapier = client(&world, "Zapier").await;
    for _ in 0..30 {
        let status = call(&world, "introspect", &zapier, None).await.status();
        assert_eq!(status, 200);
    }
}

#[tokio::test]
async fn the_defaults_apply_without_configuration() {
    // 30 device authorizations a minute per IP by default.
    let world = World::with(support::builder()).await;
    let tv = client(&world, "TV").await;
    for _ in 0..30 {
        call(&world, "device_authorization", &tv, None)
            .await
            .assert_status(200);
    }
    assert_limited(call(&world, "device_authorization", &tv, None).await);
}

#[test]
fn the_defaults_limit_every_endpoint() {
    let limits = OAuthOptions::default().rate_limits;
    for (name, limit) in [
        ("token", limits.token),
        ("introspect", limits.introspect),
        ("revoke", limits.revoke),
        ("device_authorization", limits.device_authorization),
    ] {
        assert!(limit.per_client > 0, "{name}");
        assert!(limit.per_ip > 0, "{name}");
    }
    assert_eq!(limits.token, EndpointLimit::new(300, 120));
    assert_eq!(limits.introspect, EndpointLimit::new(1200, 600));
    assert_eq!(limits.revoke, EndpointLimit::new(300, 60));
    assert_eq!(limits.device_authorization, EndpointLimit::new(60, 30));
}
