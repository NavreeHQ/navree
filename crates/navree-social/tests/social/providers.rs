//! Each built-in provider, an `oidc` provider and a custom one against a
//! mock identity provider (wiremock): the authorization URL, the token
//! request (PKCE verifier, client authentication), ID token verification
//! and the profile requests.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::Utc;
use navree_http_client::HttpClient;
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_social::{
    AuthorizeUrl, ProviderConfig, ProviderToken, Social, SocialError, SocialModule, SocialProvider,
    SocialUser,
};
use navree_testing::{TestApp, TestResponse};
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::support::{
    APPLE_P8, Idp, REDIRECT_BASE, builder, create_user, form_field, http_client_module, query_param,
};

/// An app whose provider `key` is configured with `config` (pointing at
/// the mock provider), calling out through a loopback-allowed client.
async fn app(key: &str, config: Value) -> TestApp {
    builder()
        .config(&format!("social.providers.{key}"), config)
        .module(http_client_module())
        .build()
        .await
}

/// Starts a flow: the state and the authorization URL.
async fn start(app: &TestApp, key: &str) -> (String, String) {
    let body = app
        .client()
        .get(&format!("/api/auth/social/{key}/redirect"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    (
        body["state"].as_str().unwrap().to_owned(),
        body["url"].as_str().unwrap().to_owned(),
    )
}

async fn callback(app: &TestApp, key: &str, body: Value) -> TestResponse {
    app.client()
        .post(&format!("/api/auth/social/{key}/callback"))
        .json(&body)
        .send()
        .await
}

fn now() -> i64 {
    Utc::now().timestamp()
}

/// Checks the token request: the code, the redirect URI, the client id and
/// the PKCE verifier matching the URL's challenge.
fn assert_token_request(
    request: &wiremock::Request,
    key: &str,
    client_id: &str,
    code: &str,
    url: &str,
) {
    assert_eq!(
        form_field(request, "grant_type").as_deref(),
        Some("authorization_code")
    );
    assert_eq!(form_field(request, "code").as_deref(), Some(code));
    assert_eq!(
        form_field(request, "redirect_uri"),
        Some(format!("{REDIRECT_BASE}/{key}"))
    );
    assert_eq!(form_field(request, "client_id").as_deref(), Some(client_id));
    let verifier = form_field(request, "code_verifier").expect("a PKCE verifier");
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    assert_eq!(query_param(url, "code_challenge"), Some(challenge));
}

#[tokio::test]
async fn github_reads_the_primary_verified_email() {
    let idp = Idp::start().await;
    let app = app(
        "github",
        json!({
            "client_id": "gh-client",
            "client_secret": "gh-secret",
            "url": idp.uri(),
            "api_url": format!("{}/api", idp.uri()),
        }),
    )
    .await;
    idp.mount_post(
        "/login/oauth/access_token",
        200,
        json!({ "access_token": "gho_1", "token_type": "bearer", "scope": "read:user,user:email" }),
    )
    .await;
    idp.mount_get(
        "/api/user",
        200,
        json!({ "id": 583231, "login": "anu", "name": "Anu Lakhotia", "email": "public@anu.test", "avatar_url": "https://avatars.test/anu" }),
    )
    .await;
    idp.mount_get(
        "/api/user/emails",
        200,
        json!([
            { "email": "old@anu.test", "primary": false, "verified": true },
            { "email": "anu@example.com", "primary": true, "verified": true },
        ]),
    )
    .await;

    let (state, url) = start(&app, "github").await;
    assert!(
        url.starts_with(&format!("{}/login/oauth/authorize?", idp.uri())),
        "{url}"
    );
    callback(&app, "github", json!({ "code": "code-1", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@example.com"))
        .assert_json_path("user.name", json!("Anu Lakhotia"));

    let token = &idp.requests_to("/login/oauth/access_token").await[0];
    assert_token_request(token, "github", "gh-client", "code-1", &url);
    assert_eq!(
        form_field(token, "client_secret").as_deref(),
        Some("gh-secret")
    );
    assert_eq!(token.headers.get("accept").unwrap(), "application/json");
    let user = &idp.requests_to("/api/user").await[0];
    assert_eq!(user.headers.get("authorization").unwrap(), "Bearer gho_1");
}

#[tokio::test]
async fn github_errors_answered_with_200_are_refusals() {
    let idp = Idp::start().await;
    let app = app(
        "github",
        json!({ "client_id": "gh-client", "url": idp.uri() }),
    )
    .await;
    idp.mount_post(
        "/login/oauth/access_token",
        200,
        json!({ "error": "bad_verification_code", "error_description": "The code passed is incorrect or expired." }),
    )
    .await;
    let (state, _) = start(&app, "github").await;
    callback(&app, "github", json!({ "code": "stale", "state": state }))
        .await
        .assert_error(400, "GitHub refused the sign-in: bad_verification_code");
}

#[tokio::test]
async fn an_unavailable_provider_is_a_502() {
    let idp = Idp::start().await;
    let app = app(
        "github",
        json!({ "client_id": "gh-client", "url": idp.uri() }),
    )
    .await;
    idp.mount_post("/login/oauth/access_token", 503, json!({}))
        .await;
    let (state, _) = start(&app, "github").await;
    callback(&app, "github", json!({ "code": "c", "state": state }))
        .await
        .assert_error(502, "GitHub is unavailable. Please try again.");
}

fn google_config(idp: &Idp) -> Value {
    json!({
        "client_id": "google-client",
        "client_secret": "google-secret",
        "hosted_domain": "acme.test",
        "authorize_url": format!("{}/o/oauth2/v2/auth", idp.uri()),
        "token_url": format!("{}/token", idp.uri()),
        "jwks_url": format!("{}/certs", idp.uri()),
        "userinfo_url": format!("{}/userinfo", idp.uri()),
    })
}

fn google_claims(nonce: &str) -> Value {
    json!({
        "iss": "https://accounts.google.com",
        "aud": "google-client",
        "sub": "109876543210",
        "email": "anu@acme.test",
        "email_verified": true,
        "name": "Anu Lakhotia",
        "picture": "https://lh3.test/anu.png",
        "hd": "acme.test",
        "iat": now(),
        "exp": now() + 3600,
        "nonce": nonce,
    })
}

/// Replaces the claims of a signed token, keeping its signature.
fn tamper(token: &str, claims: &Value) -> String {
    let parts: Vec<&str> = token.split('.').collect();
    format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(claims.to_string()),
        parts[2]
    )
}

#[tokio::test]
async fn google_id_tokens_are_verified() {
    let idp = Idp::start().await;
    idp.mount_jwks("/certs").await;
    let app = app("google", google_config(&idp)).await;

    let (state, url) = start(&app, "google").await;
    assert_eq!(query_param(&url, "hd").as_deref(), Some("acme.test"));
    assert_eq!(
        query_param(&url, "scope").as_deref(),
        Some("openid email profile")
    );
    let nonce = query_param(&url, "nonce").expect("a nonce");
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.a", "expires_in": 3599, "token_type": "Bearer", "id_token": idp.rs256(&google_claims(&nonce)) }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g-1", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@acme.test"));
    assert_token_request(
        &idp.requests_to("/token").await[0],
        "google",
        "google-client",
        "g-1",
        &url,
    );

    // A replayed ID token (another flow's nonce).
    let (state, _) = start(&app, "google").await;
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.b", "id_token": idp.rs256(&google_claims(&nonce)) }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g-2", "state": state }))
        .await
        .assert_error(400, "Google sent an invalid ID token");

    // Changed claims under the original signature.
    let (state, url) = start(&app, "google").await;
    let nonce = query_param(&url, "nonce").unwrap();
    let signed = idp.rs256(&google_claims(&nonce));
    let mut forged_claims = google_claims(&nonce);
    forged_claims["email"] = json!("ceo@acme.test");
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.c", "id_token": tamper(&signed, &forged_claims) }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g-3", "state": state }))
        .await
        .assert_error(400, "Google sent an invalid ID token");

    // Expired.
    let (state, url) = start(&app, "google").await;
    let mut expired = google_claims(&query_param(&url, "nonce").unwrap());
    expired["exp"] = json!(now() - 3600);
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.d", "id_token": idp.rs256(&expired) }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g-4", "state": state }))
        .await
        .assert_status(400);

    // Another client's token.
    let (state, url) = start(&app, "google").await;
    let mut other = google_claims(&query_param(&url, "nonce").unwrap());
    other["aud"] = json!("someone-else");
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.e", "id_token": idp.rs256(&other) }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g-5", "state": state }))
        .await
        .assert_status(400);

    // Another Google Workspace.
    let (state, url) = start(&app, "google").await;
    let mut outsider = google_claims(&query_param(&url, "nonce").unwrap());
    outsider["hd"] = json!("gmail.com");
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.f", "id_token": idp.rs256(&outsider) }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g-6", "state": state }))
        .await
        .assert_error(403, "Sign in with a acme.test Google account");

    // The key set was fetched once.
    assert_eq!(idp.requests_to("/certs").await.len(), 1);
}

#[tokio::test]
async fn google_fills_a_missing_email_from_userinfo() {
    let idp = Idp::start().await;
    idp.mount_jwks("/certs").await;
    let mut config = google_config(&idp);
    config["hosted_domain"] = Value::Null;
    let app = app("google", config).await;
    let (state, url) = start(&app, "google").await;
    let mut claims = google_claims(&query_param(&url, "nonce").unwrap());
    claims.as_object_mut().unwrap().remove("email");
    idp.mount_post(
        "/token",
        200,
        json!({ "access_token": "ya29.a", "id_token": idp.rs256(&claims) }),
    )
    .await;
    idp.mount_get(
        "/userinfo",
        200,
        json!({ "sub": "109876543210", "email": "anu@acme.test", "email_verified": true }),
    )
    .await;
    callback(&app, "google", json!({ "code": "g", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@acme.test"));
}

#[tokio::test]
async fn apple_signs_its_client_secret_and_keeps_the_first_name() {
    let idp = Idp::start().await;
    idp.mount_jwks("/auth/keys").await;
    let app = app(
        "apple",
        json!({
            "client_id": "com.agra.web",
            "team_id": "TEAM123456",
            "key_id": "KEY1234567",
            "private_key": APPLE_P8,
            "url": idp.uri(),
        }),
    )
    .await;
    let (state, url) = start(&app, "apple").await;
    assert_eq!(
        query_param(&url, "response_mode").as_deref(),
        Some("form_post")
    );
    assert_eq!(query_param(&url, "scope").as_deref(), Some("name email"));
    let nonce = query_param(&url, "nonce").unwrap();
    let claims = json!({
        "iss": idp.uri(),
        "aud": "com.agra.web",
        "sub": "001234.5f0b.1234",
        "email": "x7k2@privaterelay.appleid.com",
        "email_verified": "true",
        "is_private_email": "true",
        "iat": now(),
        "exp": now() + 600,
        "nonce": nonce,
    });
    idp.mount_post(
        "/auth/token",
        200,
        json!({ "access_token": "a.1", "id_token": idp.es256(&claims) }),
    )
    .await;
    callback(
        &app,
        "apple",
        json!({
            "code": "c-apple",
            "state": state,
            "user": "{\"name\":{\"firstName\":\"Anu\",\"lastName\":\"Lakhotia\"},\"email\":\"x7k2@privaterelay.appleid.com\"}",
        }),
    )
    .await
    .assert_status(200)
    .assert_json_path("user.name", json!("Anu Lakhotia"))
    .assert_json_path("user.email", json!("x7k2@privaterelay.appleid.com"));

    // The client secret is an ES256 JWT signed with the .p8 key.
    let request = &idp.requests_to("/auth/token").await[0];
    let secret = form_field(request, "client_secret").expect("a client secret");
    let parts: Vec<&str> = secret.split('.').collect();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(header, json!({ "alg": "ES256", "kid": "KEY1234567" }));
    assert_eq!(claims["iss"], "TEAM123456");
    assert_eq!(claims["sub"], "com.agra.web");
    assert_eq!(claims["aud"], idp.uri());
    assert!(claims["exp"].as_i64().unwrap() > claims["iat"].as_i64().unwrap());
    let der: Vec<u8> = STANDARD
        .decode(
            APPLE_P8
                .lines()
                .filter(|l| !l.starts_with("-----"))
                .collect::<String>(),
        )
        .unwrap();
    let pair = ring::signature::EcdsaKeyPair::from_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &der,
        &ring::rand::SystemRandom::new(),
    )
    .unwrap();
    use ring::signature::KeyPair as _;
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, pair.public_key().as_ref())
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
        )
        .expect("signed with the .p8 key");
}

fn microsoft_claims(idp: &Idp, tid: &str, nonce: &str) -> Value {
    json!({
        "iss": format!("{}/{tid}/v2.0", idp.uri()),
        "tid": tid,
        "aud": "ms-client",
        "sub": "AAAAAAAAAAAAAAAAAAAAAIkzqFVrSaSaFHy782bbtaQ",
        "oid": "00000000-0000-0000-66f3-3332eca7ea81",
        "name": "Anu Lakhotia",
        "email": "anu@contoso.test",
        "preferred_username": "anu@contoso.test",
        "iat": now(),
        "exp": now() + 3600,
        "nonce": nonce,
    })
}

#[tokio::test]
async fn microsoft_checks_the_tenant_and_does_not_trust_the_email() {
    let idp = Idp::start().await;
    idp.mount_jwks("/organizations/discovery/v2.0/keys").await;
    let app = app(
        "microsoft",
        json!({ "client_id": "ms-client", "client_secret": "s", "tenant": "organizations", "url": idp.uri() }),
    )
    .await;
    let tid = "72f988bf-86f1-41af-91ab-2d7cd011db47";

    let (state, url) = start(&app, "microsoft").await;
    assert!(url.starts_with(&format!(
        "{}/organizations/oauth2/v2.0/authorize?",
        idp.uri()
    )));
    let claims = microsoft_claims(&idp, tid, &query_param(&url, "nonce").unwrap());
    idp.mount_post_once(
        "/organizations/oauth2/v2.0/token",
        200,
        json!({ "access_token": "eyJ0", "id_token": idp.rs256(&claims) }),
    )
    .await;
    callback(&app, "microsoft", json!({ "code": "m-1", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@contoso.test"))
        .assert_json_path("user.email_verified_at", Value::Null);

    // A personal account is refused by `organizations`.
    let (state, url) = start(&app, "microsoft").await;
    let personal = microsoft_claims(
        &idp,
        "9188040d-6c67-4c5b-b112-36a304b66dad",
        &query_param(&url, "nonce").unwrap(),
    );
    idp.mount_post_once(
        "/organizations/oauth2/v2.0/token",
        200,
        json!({ "access_token": "eyJ0", "id_token": idp.rs256(&personal) }),
    )
    .await;
    callback(&app, "microsoft", json!({ "code": "m-2", "state": state }))
        .await
        .assert_error(403, "Sign in with a work or school Microsoft account");

    // An unverified email never links to an existing user.
    create_user(&app, "Gauri", "gauri@contoso.test").await;
    let (state, url) = start(&app, "microsoft").await;
    let mut grace = microsoft_claims(&idp, tid, &query_param(&url, "nonce").unwrap());
    grace["sub"] = json!("gauri-sub");
    grace["email"] = json!("gauri@contoso.test");
    idp.mount_post_once(
        "/organizations/oauth2/v2.0/token",
        200,
        json!({ "access_token": "eyJ0", "id_token": idp.rs256(&grace) }),
    )
    .await;
    callback(&app, "microsoft", json!({ "code": "m-3", "state": state }))
        .await
        .assert_status(409);
}

#[tokio::test]
async fn gitlab_reads_userinfo_on_a_self_hosted_instance() {
    let idp = Idp::start().await;
    let app = app(
        "gitlab",
        json!({ "client_id": "gl-client", "client_secret": "s", "url": idp.uri() }),
    )
    .await;
    idp.mount_post("/oauth/token", 200, json!({ "access_token": "glpat", "token_type": "Bearer", "expires_in": 7200, "refresh_token": "glrt" }))
        .await;
    idp.mount_get(
        "/oauth/userinfo",
        200,
        json!({ "sub": "42", "name": "Anu Lakhotia", "nickname": "anu", "email": "anu@example.com", "email_verified": true, "picture": "https://gitlab.test/anu.png" }),
    )
    .await;
    let (state, url) = start(&app, "gitlab").await;
    assert!(url.starts_with(&format!("{}/oauth/authorize?", idp.uri())));
    assert_eq!(
        query_param(&url, "scope").as_deref(),
        Some("openid profile email")
    );
    callback(&app, "gitlab", json!({ "code": "c", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@example.com"));
}

#[tokio::test]
async fn slack_errors_and_workspaces() {
    let idp = Idp::start().await;
    let app = app("slack", json!({ "client_id": "sl-client", "client_secret": "s", "team": "T0123", "url": idp.uri() })).await;

    let (state, url) = start(&app, "slack").await;
    assert_eq!(query_param(&url, "team").as_deref(), Some("T0123"));
    idp.mount_post_once(
        "/api/openid.connect.token",
        200,
        json!({ "ok": false, "error": "invalid_code" }),
    )
    .await;
    callback(&app, "slack", json!({ "code": "c", "state": state }))
        .await
        .assert_error(400, "Slack refused the sign-in: invalid_code");

    idp.mount_post(
        "/api/openid.connect.token",
        200,
        json!({ "ok": true, "access_token": "xoxp-1", "token_type": "Bearer" }),
    )
    .await;
    let profile = |team: &str| json!({ "ok": true, "sub": "U0R7JM", "email": "anu@acme.test", "email_verified": true, "name": "Anu", "https://slack.com/team_id": team });
    idp.mount_get_once("/api/openid.connect.userInfo", 200, profile("T9999"))
        .await;
    let (state, _) = start(&app, "slack").await;
    callback(&app, "slack", json!({ "code": "c", "state": state }))
        .await
        .assert_error(403, "Sign in with an account of the T0123 Slack workspace");

    idp.mount_get_once("/api/openid.connect.userInfo", 200, profile("T0123"))
        .await;
    let (state, _) = start(&app, "slack").await;
    callback(&app, "slack", json!({ "code": "c", "state": state }))
        .await
        .assert_status(200);
}

#[tokio::test]
async fn linkedin_reads_userinfo() {
    let idp = Idp::start().await;
    let app = app(
        "linkedin",
        json!({
            "client_id": "li-client",
            "client_secret": "s",
            "authorize_url": format!("{}/oauth/v2/authorization", idp.uri()),
            "token_url": format!("{}/oauth/v2/accessToken", idp.uri()),
            "userinfo_url": format!("{}/v2/userinfo", idp.uri()),
        }),
    )
    .await;
    idp.mount_post(
        "/oauth/v2/accessToken",
        200,
        json!({ "access_token": "AQX", "expires_in": 5183999 }),
    )
    .await;
    idp.mount_get(
        "/v2/userinfo",
        200,
        json!({ "sub": "782bbtaQ", "given_name": "Anu", "family_name": "Lakhotia", "email": "anu@example.com", "email_verified": true, "picture": "https://media.licdn.test/anu" }),
    )
    .await;
    let (state, _) = start(&app, "linkedin").await;
    callback(&app, "linkedin", json!({ "code": "c", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.name", json!("Anu Lakhotia"));
}

#[tokio::test]
async fn discord_links_only_verified_emails() {
    let idp = Idp::start().await;
    let app = app(
        "discord",
        json!({ "client_id": "dc-client", "client_secret": "s", "url": idp.uri() }),
    )
    .await;
    create_user(&app, "Anu", "anu@example.com").await;
    idp.mount_post("/api/oauth2/token", 200, json!({ "access_token": "dc", "token_type": "Bearer", "expires_in": 604800, "refresh_token": "r", "scope": "identify email" }))
        .await;
    idp.mount_get_once(
        "/api/users/@me",
        200,
        json!({ "id": "80351110224678912", "username": "anu", "global_name": "Anu", "email": "anu@example.com", "verified": false }),
    )
    .await;
    let (state, url) = start(&app, "discord").await;
    assert_eq!(
        query_param(&url, "scope").as_deref(),
        Some("identify email")
    );
    callback(&app, "discord", json!({ "code": "c", "state": state }))
        .await
        .assert_status(409);
    idp.mount_get_once(
        "/api/users/@me",
        200,
        json!({ "id": "80351110224678912", "username": "anu", "email": "anu@example.com", "verified": true }),
    )
    .await;
    let (state, _) = start(&app, "discord").await;
    callback(&app, "discord", json!({ "code": "c", "state": state }))
        .await
        .assert_status(200);
}

#[tokio::test]
async fn x_uses_basic_authentication_and_the_confirmed_email() {
    let idp = Idp::start().await;
    let app = app(
        "x",
        json!({
            "client_id": "x-client",
            "client_secret": "x-secret",
            "scopes": ["users.email"],
            "authorize_url": format!("{}/i/oauth2/authorize", idp.uri()),
            "token_url": format!("{}/2/oauth2/token", idp.uri()),
            "userinfo_url": format!("{}/2/users/me", idp.uri()),
        }),
    )
    .await;
    idp.mount_post(
        "/2/oauth2/token",
        200,
        json!({ "token_type": "bearer", "access_token": "x-at", "expires_in": 7200, "scope": "tweet.read users.read users.email" }),
    )
    .await;
    idp.mount_get(
        "/2/users/me",
        200,
        json!({ "data": { "id": "2244994945", "name": "Anu Lakhotia", "username": "anu", "confirmed_email": "anu@example.com" } }),
    )
    .await;
    let (state, url) = start(&app, "x").await;
    assert_eq!(
        query_param(&url, "scope").as_deref(),
        Some("tweet.read users.read users.email")
    );
    callback(&app, "x", json!({ "code": "c", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@example.com"));
    let token = &idp.requests_to("/2/oauth2/token").await[0];
    assert_eq!(
        token
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("Basic {}", STANDARD.encode("x-client:x-secret"))
    );
    assert_eq!(form_field(token, "client_secret"), None);
    let me = &idp.requests_to("/2/users/me").await[0];
    assert_eq!(
        query_param(me.url.as_str(), "user.fields").as_deref(),
        Some("profile_image_url,confirmed_email")
    );
}

fn discovery(idp: &Idp, issuer: &str) -> Value {
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{}/oauth2/v1/authorize", idp.uri()),
        "token_endpoint": format!("{}/oauth2/v1/token", idp.uri()),
        "userinfo_endpoint": format!("{}/oauth2/v1/userinfo", idp.uri()),
        "jwks_uri": format!("{}/oauth2/v1/keys", idp.uri()),
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
    })
}

#[tokio::test]
async fn oidc_providers_are_read_from_their_discovery_document() {
    let idp = Idp::start().await;
    idp.mount_get(
        "/.well-known/openid-configuration",
        200,
        discovery(&idp, &idp.uri()),
    )
    .await;
    idp.mount_jwks("/oauth2/v1/keys").await;
    let app = app(
        "okta",
        json!({
            "type": "oidc",
            "name": "Okta",
            "issuer": idp.uri(),
            "client_id": "okta-client",
            "client_secret": "okta-secret",
            "scopes": ["openid", "profile", "email", "groups"],
        }),
    )
    .await;
    let (state, url) = start(&app, "okta").await;
    assert!(
        url.starts_with(&format!("{}/oauth2/v1/authorize?", idp.uri())),
        "{url}"
    );
    assert_eq!(
        query_param(&url, "scope").as_deref(),
        Some("openid email profile groups")
    );
    let claims = json!({
        "iss": idp.uri(),
        "aud": "okta-client",
        "sub": "00u1a2b3",
        "name": "Anu Lakhotia",
        "groups": ["crm-users"],
        "iat": now(),
        "exp": now() + 3600,
        "nonce": query_param(&url, "nonce").unwrap(),
    });
    idp.mount_post(
        "/oauth2/v1/token",
        200,
        json!({ "access_token": "okta-at", "id_token": idp.rs256(&claims) }),
    )
    .await;
    idp.mount_get(
        "/oauth2/v1/userinfo",
        200,
        json!({ "sub": "00u1a2b3", "email": "anu@acme.test", "email_verified": true }),
    )
    .await;

    let social = app.get::<Social>().await;
    let profile = social
        .user("okta", &navree_social::SocialCallback::new("code", state))
        .await
        .unwrap();
    assert_eq!(profile.email.as_deref(), Some("anu@acme.test"));
    assert_eq!(profile.raw["groups"], json!(["crm-users"]));
    assert_eq!(social.provider_name("okta").unwrap(), "Okta");
    // Basic authentication, as the discovery document allows.
    let token = &idp.requests_to("/oauth2/v1/token").await[0];
    assert_eq!(
        token
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("Basic {}", STANDARD.encode("okta-client:okta-secret"))
    );
    // Read at boot, once.
    assert_eq!(
        idp.requests_to("/.well-known/openid-configuration")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn a_discovery_document_for_another_issuer_is_refused() {
    let idp = Idp::start().await;
    idp.mount_get(
        "/.well-known/openid-configuration",
        200,
        discovery(&idp, "https://evil.test"),
    )
    .await;
    let app = app(
        "okta",
        json!({ "type": "oidc", "issuer": idp.uri(), "client_id": "okta-client" }),
    )
    .await;
    app.client()
        .get("/api/auth/social/okta/redirect")
        .send()
        .await
        .assert_status(500);
}

/// A provider that is not built in, read through its own API.
struct Zoho {
    http: Arc<HttpClient>,
}

impl Injectable for Zoho {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<HttpClient>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Zoho {
                http: r.get::<HttpClient>().await?,
            })
        })
    }
}

impl SocialProvider for Zoho {
    fn key(&self) -> &'static str {
        "zoho"
    }

    fn authorize_url(&self, config: &ProviderConfig, url: AuthorizeUrl) -> AuthorizeUrl {
        url.endpoint(config.authorize_url.clone().unwrap_or_default())
            .scopes(["AaaServer.profile.Read"])
    }

    fn user<'a>(
        &'a self,
        config: &'a ProviderConfig,
        token: ProviderToken,
    ) -> BoxFuture<'a, Result<SocialUser, SocialError>> {
        Box::pin(async move {
            let profile: Value = self
                .http
                .get(config.userinfo_url.clone().unwrap_or_default())
                .bearer(token.access_token.expose())
                .send()
                .await?
                .json()?;
            Ok(SocialUser::new("zoho", profile["ZUID"].to_string())
                .name(profile["Display_Name"].as_str().unwrap_or_default())
                .email(profile["Email"].as_str(), true)
                .token(token)
                .raw(profile))
        })
    }
}

#[tokio::test]
async fn custom_providers_plug_in_with_for_feature() {
    let idp = Idp::start().await;
    let app = builder()
        .config(
            "social.providers.zoho",
            json!({
                "type": "custom",
                "name": "Zoho",
                "client_id": "zoho-client",
                "client_secret": "zoho-secret",
                "authorize_url": format!("{}/oauth/v2/auth", idp.uri()),
                "token_url": format!("{}/oauth/v2/token", idp.uri()),
                "userinfo_url": format!("{}/oauth/user/info", idp.uri()),
            }),
        )
        .module(http_client_module())
        .module(SocialModule::for_feature::<Zoho>())
        .build()
        .await;
    idp.mount_post(
        "/oauth/v2/token",
        200,
        json!({ "access_token": "zoho-at", "expires_in": 3600 }),
    )
    .await;
    idp.mount_get(
        "/oauth/user/info",
        200,
        json!({ "ZUID": 800012345, "Display_Name": "Anu Lakhotia", "Email": "anu@zoho.test" }),
    )
    .await;
    let (state, url) = start(&app, "zoho").await;
    assert!(url.starts_with(&format!(
        "{}/oauth/v2/auth?response_type=code&client_id=zoho-client",
        idp.uri()
    )));
    assert_eq!(
        query_param(&url, "scope").as_deref(),
        Some("AaaServer.profile.Read")
    );
    callback(&app, "zoho", json!({ "code": "z", "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("anu@zoho.test"));
    let token = &idp.requests_to("/oauth/v2/token").await[0];
    assert_eq!(
        form_field(token, "client_secret").as_deref(),
        Some("zoho-secret")
    );
    assert!(form_field(token, "code_verifier").is_some());
}

#[tokio::test]
async fn provider_tokens_are_refreshed_at_the_token_endpoint() {
    let idp = Idp::start().await;
    idp.mount_jwks("/certs").await;
    let mut config = google_config(&idp);
    config["hosted_domain"] = Value::Null;
    let app = builder()
        .config("social.providers.google", config)
        .config("social.store_tokens", true)
        .module(http_client_module())
        .build()
        .await;
    let (state, url) = start(&app, "google").await;
    let claims = google_claims(&query_param(&url, "nonce").unwrap());
    // Expires within the minute: the first `token` call refreshes it.
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.old", "expires_in": 30, "refresh_token": "1//refresh", "id_token": idp.rs256(&claims) }),
    )
    .await;
    let body = callback(&app, "google", json!({ "code": "g", "state": state }))
        .await
        .assert_status(200)
        .json_value();
    let user_id: uuid::Uuid = serde_json::from_value(body["user"]["id"].clone()).unwrap();

    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.new", "expires_in": 3599 }),
    )
    .await;
    let social = app.get::<Social>().await;
    assert_eq!(
        social.token(user_id, "google").await.unwrap().expose(),
        "ya29.new"
    );
    let refresh = idp.requests_to("/token").await.pop().unwrap();
    assert_eq!(
        form_field(&refresh, "grant_type").as_deref(),
        Some("refresh_token")
    );
    assert_eq!(
        form_field(&refresh, "refresh_token").as_deref(),
        Some("1//refresh")
    );
    assert_eq!(
        form_field(&refresh, "client_secret").as_deref(),
        Some("google-secret")
    );
    // Fresh now.
    assert_eq!(
        social.token(user_id, "google").await.unwrap().expose(),
        "ya29.new"
    );
    assert_eq!(idp.requests_to("/token").await.len(), 2);
}

#[tokio::test]
async fn a_refused_refresh_asks_for_a_new_authorization() {
    let idp = Idp::start().await;
    idp.mount_jwks("/certs").await;
    let mut config = google_config(&idp);
    config["hosted_domain"] = Value::Null;
    let app = builder()
        .config("social.providers.google", config)
        .config("social.store_tokens", true)
        .module(http_client_module())
        .build()
        .await;
    let (state, url) = start(&app, "google").await;
    let claims = google_claims(&query_param(&url, "nonce").unwrap());
    idp.mount_post_once(
        "/token",
        200,
        json!({ "access_token": "ya29.old", "expires_in": 1, "refresh_token": "1//revoked", "id_token": idp.rs256(&claims) }),
    )
    .await;
    let body = callback(&app, "google", json!({ "code": "g", "state": state }))
        .await
        .assert_status(200)
        .json_value();
    let user_id: uuid::Uuid = serde_json::from_value(body["user"]["id"].clone()).unwrap();
    idp.mount_post_once("/token", 400, json!({ "error": "invalid_grant", "error_description": "Token has been expired or revoked." }))
        .await;
    let social = app.get::<Social>().await;
    let error = social.token(user_id, "google").await.unwrap_err();
    assert!(matches!(error, SocialError::Reauthorize { .. }), "{error}");
}
