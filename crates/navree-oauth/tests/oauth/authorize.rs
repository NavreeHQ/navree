//! The authorization code flow: `GET /oauth/authorize`, the consent API,
//! the code exchange with PKCE, and their errors.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_oauth::NewClient;
use navree_oauth::testing::{AuthorizeFlow, authorize, pkce_pair};
use serde_json::json;

use crate::support::{CONSENT_URL, ISSUER, REDIRECT_URI, World, param, string};

#[tokio::test]
async fn the_whole_flow_issues_tokens_for_a_public_client() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let id = client.client.id;

    let (verifier, challenge) = pkce_pair();
    let request = world
        .request(id, "deals:read offline_access", &challenge)
        .await;

    // The consent page reads the request.
    world
        .client()
        .get(&format!("/api/oauth/requests/{request}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(request))
        .assert_json_path("client.id", json!(id))
        .assert_json_path("client.name", json!("Acme mobile"))
        .assert_json_path(
            "scopes.0",
            json!({"name": "deals:read", "description": "Read your deals"}),
        )
        .assert_json_path("scopes.1.name", json!("offline_access"))
        .assert_json_path("already_approved", json!(false))
        .assert_json_path("expires_at", json!("2026-10-26T09:10:00Z"));

    // It approves; the browser goes back with the code and the state.
    let approved = world
        .client()
        .post(&format!("/api/oauth/requests/{request}/approve"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let redirect_to = string(&approved, "redirect_to");
    assert!(
        redirect_to.starts_with(&format!("{REDIRECT_URI}?code=nvc_")),
        "{redirect_to}"
    );
    assert_eq!(param(&redirect_to, "state").as_deref(), Some("af0ifjsldkj"));
    assert_eq!(param(&redirect_to, "iss").as_deref(), Some(ISSUER));
    let code = param(&redirect_to, "code").unwrap();

    let id_string = id.to_string();
    let response = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", &verifier),
                ("client_id", &id_string),
            ],
        )
        .await
        .assert_status(200);
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let body = response.json_value();
    assert!(string(&body, "access_token").starts_with("nva_"));
    assert!(string(&body, "refresh_token").starts_with("nvr_"));
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["expires_in"], 3600);
    assert_eq!(body["scope"], "deals:read offline_access");
    assert!(body.get("id_token").is_none());

    // The token works on the API as the user.
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&string(&body, "access_token"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id))
        .assert_json_path("oauth.client_id", json!(id))
        .assert_json_path("oauth.grant", json!("authorization_code"));

    // The approval is remembered.
    let (_, challenge) = pkce_pair();
    let again = world.request(id, "deals:read", &challenge).await;
    world
        .client()
        .get(&format!("/api/oauth/requests/{again}"))
        .bearer(&token)
        .send()
        .await
        .assert_json_path("already_approved", json!(true));
    let (_, challenge) = pkce_pair();
    let more = world
        .request(id, "deals:read offline_access", &challenge)
        .await;
    world
        .client()
        .get(&format!("/api/oauth/requests/{more}"))
        .bearer(&token)
        .send()
        .await
        .assert_json_path("already_approved", json!(true));
}

#[tokio::test]
async fn confidential_clients_authenticate_with_basic_or_the_body() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.confidential_client(&["deals:read"]).await;
    let id = client.client.id.to_string();
    let secret = client.secret.clone().unwrap();
    assert!(secret.starts_with("nvcs_"));

    let exchange = |basic: bool, secret: String| {
        let (id, world, token) = (id.clone(), &world, token.clone());
        async move {
            let (verifier, challenge) = pkce_pair();
            let request = world
                .request(id.parse().unwrap(), "deals:read", &challenge)
                .await;
            let code = world.approve(&request, &token).await;
            let mut form = vec![
                ("grant_type", "authorization_code".to_owned()),
                ("code", code),
                ("code_verifier", verifier),
            ];
            let mut request = world.client().post("/oauth/token");
            if basic {
                let credentials = STANDARD.encode(format!("{id}:{secret}"));
                request = request.header("authorization", format!("Basic {credentials}"));
            } else {
                form.push(("client_id", id.clone()));
                form.push(("client_secret", secret));
            }
            request.form(&form).send().await
        }
    };
    exchange(true, secret.clone()).await.assert_status(200);
    exchange(false, secret.clone()).await.assert_status(200);

    let wrong = exchange(true, "nvcs_wrong".to_owned())
        .await
        .assert_status(401);
    assert_eq!(wrong.json_value()["error"], "invalid_client");
    assert_eq!(
        wrong.header("www-authenticate"),
        Some("Basic realm=\"oauth\"")
    );
    let wrong = exchange(false, "nvcs_wrong".to_owned())
        .await
        .assert_status(401);
    assert_eq!(wrong.json_value()["error"], "invalid_client");
    assert!(wrong.header("www-authenticate").is_none());

    // No secret at all: a confidential client must authenticate.
    let (verifier, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let code = world.approve(&request, &token).await;
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", &verifier),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(401);
}

#[tokio::test]
async fn requests_that_cannot_redirect_get_an_error_page() {
    let world = World::new().await;
    let client = world.public_client(&["deals:read"]).await;
    let id = client.client.id.to_string();
    let (_, challenge) = pkce_pair();

    for params in [
        vec![("response_type", "code"), ("client_id", "nope")],
        vec![
            ("response_type", "code"),
            ("client_id", "0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23"),
        ],
        vec![
            ("response_type", "code"),
            ("client_id", id.as_str()),
            ("redirect_uri", "https://evil.test/steal"),
        ],
        vec![
            ("response_type", "code"),
            ("client_id", id.as_str()),
            ("redirect_uri", "https://zapier.test/oauth/return/extra"),
        ],
    ] {
        let response = world.authorize(&params).await.assert_status(400);
        assert!(response.header("location").is_none());
        assert!(
            response
                .header("content-type")
                .unwrap()
                .starts_with("text/html"),
            "{params:?}"
        );
    }

    // Other problems go back to the client.
    let cases = [
        (
            vec![
                ("response_type", "token"),
                ("code_challenge", challenge.as_str()),
            ],
            "unsupported_response_type",
        ),
        (vec![("response_type", "code")], "invalid_request"),
        (
            vec![
                ("response_type", "code"),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "plain"),
            ],
            "invalid_request",
        ),
        (
            vec![
                ("response_type", "code"),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("scope", "deals:write"),
            ],
            "invalid_scope",
        ),
        (
            vec![
                ("response_type", "code"),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("scope", "unknown"),
            ],
            "invalid_scope",
        ),
    ];
    for (mut params, error) in cases {
        params.push(("client_id", id.as_str()));
        params.push(("state", "xyz"));
        let response = world.authorize(&params).await.assert_status(302);
        let location = response.header("location").unwrap();
        assert!(location.starts_with(REDIRECT_URI), "{location}");
        assert_eq!(
            param(location, "error").as_deref(),
            Some(error),
            "{params:?}"
        );
        assert_eq!(param(location, "state").as_deref(), Some("xyz"));
    }
}

#[tokio::test]
async fn the_client_s_scopes_are_the_default_and_redirect_uri_is_optional_with_one() {
    let world = World::new().await;
    let client = world.public_client(&["deals:read"]).await;
    let id = client.client.id.to_string();
    let (_, challenge) = pkce_pair();
    let response = world
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &id),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ])
        .await
        .assert_status(302);
    let location = response.header("location").unwrap();
    assert!(
        location.starts_with(&format!("{CONSENT_URL}?request=")),
        "{location}"
    );

    // With several redirect URIs, the request must name one.
    let several = world
        .register(
            NewClient::public("Two")
                .redirect_uri(REDIRECT_URI)
                .redirect_uri("https://zapier.test/other")
                .scopes(["deals:read"]),
        )
        .await;
    world
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &several.client.id.to_string()),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ])
        .await
        .assert_status(400);
}

#[tokio::test]
async fn loopback_redirect_uris_may_use_any_port() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world
        .register(
            NewClient::public("Acme CLI")
                .redirect_uri("http://127.0.0.1/callback")
                .scopes(["deals:read"]),
        )
        .await;
    let tokens = AuthorizeFlow::new(client.client.id)
        .redirect_uri("http://127.0.0.1:51234/callback")
        .run(&world.client(), &token, &["deals:read"])
        .await;
    assert!(tokens.access_token.starts_with("nva_"));
    // Another path is still refused.
    let (_, challenge) = pkce_pair();
    world
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &client.client.id.to_string()),
            ("redirect_uri", "http://127.0.0.1:51234/other"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ])
        .await
        .assert_status(400);
}

#[tokio::test]
async fn denying_sends_access_denied_back() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let denied = world
        .client()
        .post(&format!("/api/oauth/requests/{request}/deny"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let redirect_to = string(&denied, "redirect_to");
    assert!(redirect_to.starts_with(REDIRECT_URI));
    assert_eq!(
        param(&redirect_to, "error").as_deref(),
        Some("access_denied")
    );
    assert_eq!(param(&redirect_to, "state").as_deref(), Some("af0ifjsldkj"));
    // A decided request is gone.
    world
        .client()
        .post(&format!("/api/oauth/requests/{request}/approve"))
        .bearer(&token)
        .send()
        .await
        .assert_error(404, "Authorization request not found.");
    let denied = world.audit.with_action("oauth.denied");
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].actor_id, Some(user.id));
    assert_eq!(denied[0].metadata["scopes"], json!(["deals:read"]));
}

#[tokio::test]
async fn requests_and_codes_expire() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let id = client.client.id.to_string();

    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    world.clock.advance(Duration::from_secs(601));
    world
        .client()
        .get(&format!("/api/oauth/requests/{request}"))
        .bearer(&token)
        .send()
        .await
        .assert_error(410, "The authorization request has expired.");

    let token = world.login("rep@acme.test").await;
    let (verifier, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let code = world.approve(&request, &token).await;
    world.clock.advance(Duration::from_secs(61));
    let expired = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", &verifier),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(
        expired,
        json!({"error": "invalid_grant", "error_description": "The authorization code has expired"})
    );
}

#[tokio::test]
async fn pkce_is_checked_and_a_reused_code_revokes_its_tokens() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let id = client.client.id.to_string();

    let (verifier, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read offline_access", &challenge)
        .await;
    let code = world.approve(&request, &token).await;
    let exchange = |verifier: Option<String>| {
        let (world, code, id) = (&world, code.clone(), id.clone());
        async move {
            let mut form = vec![
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("client_id", id.as_str()),
            ];
            if let Some(verifier) = &verifier {
                form.push(("code_verifier", verifier.as_str()));
            }
            world.post_form("/oauth/token", &form).await
        }
    };

    let missing = exchange(None).await.assert_status(400).json_value();
    assert_eq!(missing["error"], "invalid_request");
    let (other, _) = pkce_pair();
    let wrong = exchange(Some(other)).await.assert_status(400).json_value();
    assert_eq!(wrong["error"], "invalid_grant");

    let tokens = exchange(Some(verifier.clone()))
        .await
        .assert_status(200)
        .json_value();
    let access = string(&tokens, "access_token");
    let refresh = string(&tokens, "refresh_token");
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);

    // The code again: refused, and what it issued is revoked.
    let reused = exchange(Some(verifier))
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(
        reused["error_description"],
        "The authorization code was already used"
    );
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_error(401, "Invalid token");
    let refreshed = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(refreshed["error"], "invalid_grant");

    // A code belongs to its client.
    let other_client = world.public_client(&["deals:read"]).await;
    let (verifier, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let code = world.approve(&request, &token).await;
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", &verifier),
                ("client_id", &other_client.client.id.to_string()),
            ],
        )
        .await
        .assert_status(400);
}

#[tokio::test]
async fn the_token_endpoint_answers_in_the_oauth_format() {
    let world = World::new().await;
    let client = world.public_client(&["deals:read"]).await;
    let id = client.client.id.to_string();

    let unsupported = world
        .post_form(
            "/oauth/token",
            &[("grant_type", "password"), ("client_id", &id)],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(unsupported["error"], "unsupported_grant_type");
    let missing = world
        .post_form("/oauth/token", &[("client_id", &id)])
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(missing["error"], "invalid_request");
    let unknown = world
        .post_form(
            "/oauth/token",
            &[("grant_type", "authorization_code"), ("client_id", "nope")],
        )
        .await
        .assert_status(401)
        .json_value();
    assert_eq!(unknown["error"], "invalid_client");
    let json_body = world
        .client()
        .post("/oauth/token")
        .json(&json!({"grant_type": "authorization_code"}))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(json_body["error"], "invalid_request");
    let bad_code = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &id),
                ("code", "nvc_nope"),
                ("code_verifier", &pkce_pair().0),
            ],
        )
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(bad_code["error"], "invalid_grant");
}

#[tokio::test]
async fn trusted_clients_are_already_approved() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world
        .register(
            NewClient::public("Acme web")
                .redirect_uri(REDIRECT_URI)
                .scopes(["deals:read"])
                .trusted(),
        )
        .await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    world
        .client()
        .get(&format!("/api/oauth/requests/{request}"))
        .bearer(&token)
        .send()
        .await
        .assert_json_path("already_approved", json!(true));
}

#[tokio::test]
async fn the_consent_api_needs_a_first_party_sign_in() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;

    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    world
        .client()
        .get(&format!("/api/oauth/requests/{request}"))
        .send()
        .await
        .assert_status(401);
    world
        .client()
        .post(&format!("/api/oauth/requests/{request}/approve"))
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(403);
    world
        .client()
        .get("/api/oauth/requests/0199c1d4-0a51-7c0e-a8f3-5b9c2d4e6f70")
        .bearer(&token)
        .send()
        .await
        .assert_error(404, "Authorization request not found.");
}

#[tokio::test]
async fn approvals_are_audited() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    let authorized = world.audit.with_action("oauth.authorized");
    assert_eq!(authorized.len(), 1);
    assert_eq!(authorized[0].actor_id, Some(user.id));
    assert_eq!(authorized[0].metadata["scopes"], json!(["deals:read"]));
    assert_eq!(authorized[0].metadata["client_id"], json!(client.client.id));
}
