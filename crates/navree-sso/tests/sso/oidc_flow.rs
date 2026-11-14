//! OpenID Connect sign-in against a mock provider (wiremock): discovery,
//! PKCE, the code exchange and ID token checks.

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_crypto::jose::{JwkSet, SigningKey};
use navree_sso::{NewConnection, Provisioning, SsoConnection, SsoOwner};
use navree_workspace::{NewWorkspace, Workspaces};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{self, World};

const CLIENT_ID: &str = "navree-app";
const CLIENT_SECRET: &str = "provider-secret";

/// A mock provider: discovery, keys and userinfo; the token endpoint is
/// mounted per test.
struct Provider {
    server: MockServer,
    key: SigningKey,
}

impl Provider {
    async fn start() -> Provider {
        let provider = Provider {
            server: MockServer::start().await,
            key: SigningKey::from_pem(include_str!("../fixtures/other-key.pem"))
                .unwrap()
                .with_kid("k1"),
        };
        provider.mount_documents().await;
        provider
    }

    /// Discovery and the key set.
    async fn mount_documents(&self) {
        let issuer = self.issuer();
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/authorize"),
                "token_endpoint": format!("{issuer}/token"),
                "jwks_uri": format!("{issuer}/keys"),
                "userinfo_endpoint": format!("{issuer}/userinfo"),
            })))
            .mount(&self.server)
            .await;
        let jwks: JwkSet = self.key.public_jwk().into_iter().collect();
        Mock::given(method("GET"))
            .and(path("/keys"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&jwks))
            .mount(&self.server)
            .await;
    }

    fn issuer(&self) -> String {
        self.server.uri()
    }

    /// An ID token with `claims` over the standard ones (`null` removes
    /// one).
    fn id_token(&self, nonce: &str, claims: Value) -> String {
        let now = chrono::Utc::now().timestamp();
        let mut all = json!({
            "iss": self.issuer(),
            "aud": CLIENT_ID,
            "sub": "user-123",
            "email": "jane@acme.test",
            "email_verified": true,
            "name": "Jane Doe",
            "nonce": nonce,
            "iat": now,
            "exp": now + 300,
        });
        if let (Value::Object(all), Value::Object(extra)) = (&mut all, claims) {
            for (key, value) in extra {
                if value.is_null() {
                    all.remove(&key);
                } else {
                    all.insert(key, value);
                }
            }
        }
        self.key.sign_jwt(&all).unwrap()
    }

    /// Answers the next code exchange with `body` (the other mocks are
    /// mounted again).
    async fn token_response(&self, body: Value) {
        self.server.reset().await;
        self.mount_documents().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }
}

async fn connection(
    world: &World,
    provider: &Provider,
    provisioning: Provisioning,
) -> SsoConnection {
    let sso = world.sso().await;
    let input = NewConnection::oidc(
        SsoOwner::App,
        "Acme Login",
        provider.issuer(),
        CLIENT_ID,
        CLIENT_SECRET,
    )
    .slug("acme")
    .provisioning(provisioning);
    let connection = world
        .write(|uow| async move { sso.create_connection(&uow, input).await })
        .await
        .unwrap();
    world.verified_domain(&connection, "acme.test").await;
    connection
}

/// The connection `acme` of a workspace of Gurpreet Singh's, with the
/// verified domain `acme.test`.
async fn workspace_connection(
    world: &World,
    provider: &Provider,
    provisioning: Provisioning,
) -> SsoConnection {
    let owner = world
        .create_verified_user("Gurpreet Singh", "gurpreet@ludhiana.test")
        .await;
    let workspaces = world.app.get::<Workspaces>().await;
    let workspace = world
        .write(|uow| async move {
            workspaces
                .create(&uow, owner.id, NewWorkspace::named("Ludhiana Traders"))
                .await
        })
        .await
        .unwrap();
    let sso = world.sso().await;
    let input = NewConnection::oidc(
        SsoOwner::Workspace(workspace.id),
        "Ludhiana Login",
        provider.issuer(),
        CLIENT_ID,
        CLIENT_SECRET,
    )
    .slug("acme")
    .provisioning(provisioning);
    let connection = world
        .write(|uow| async move { sso.create_connection(&uow, input).await })
        .await
        .unwrap();
    world.verified_domain(&connection, "acme.test").await;
    connection
}

/// Provisioning that creates accounts for addresses the identity provider
/// doesn't vouch for.
fn open() -> Provisioning {
    Provisioning {
        allow_unverified_emails: true,
        ..Provisioning::default()
    }
}

/// Answers the next code exchange with an ID token of `claims`, over the
/// standard ones.
async fn token_with(provider: &Provider, query: &HashMap<String, String>, claims: Value) {
    let id_token = provider.id_token(&query["nonce"], claims);
    provider
        .token_response(json!({ "id_token": id_token }))
        .await;
}

/// The query of the authorization URL the login route gave.
async fn authorize(world: &World) -> HashMap<String, String> {
    let url = world.login_url("acme", Some("/home")).await;
    url::Url::parse(&url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

async fn callback(world: &World, query: &[(&str, &str)]) -> navree_testing::TestResponse {
    world
        .app
        .client()
        .get("/api/sso/oidc/acme/callback")
        .query(query)
        .send()
        .await
}

#[tokio::test]
async fn a_full_sign_in_with_pkce_and_a_nonce() {
    let world = support::world().await;
    let provider = Provider::start().await;
    connection(&world, &provider, Provisioning::default()).await;

    let query = authorize(&world).await;
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["scope"], "openid email profile");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["redirect_uri"],
        "https://api.acme.test/api/sso/oidc/acme/callback"
    );
    provider
        .token_response(json!({
            "access_token": "at",
            "token_type": "Bearer",
            "id_token": provider.id_token(&query["nonce"], json!({ "groups": ["Admins"] })),
        }))
        .await;

    callback(&world, &[("code", "the-code"), ("state", &query["state"])])
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("jane@acme.test"))
        .assert_json_path("user.name", json!("Jane Doe"))
        .assert_json_path("return_to", json!("/home"));

    // The token request carried the code, the verifier of the challenge
    // and the client's credentials (HTTP Basic).
    let requests = provider.server.received_requests().await.unwrap();
    let token = requests
        .iter()
        .find(|r| r.url.path() == "/token")
        .expect("a token request");
    let form: HashMap<String, String> = form_urlencoded::parse(&token.body).into_owned().collect();
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], "the-code");
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())),
        query["code_challenge"]
    );
    let basic = token
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(
        basic,
        format!(
            "Basic {}",
            STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
        )
    );

    // The state works once.
    callback(&world, &[("code", "the-code"), ("state", &query["state"])])
        .await
        .assert_error_code(400, "invalid_state");
}

#[tokio::test]
async fn id_tokens_must_match_the_nonce_client_and_keys() {
    let world = support::world().await;
    let provider = Provider::start().await;
    connection(&world, &provider, Provisioning::default()).await;

    for claims in [
        json!({ "nonce": "another" }),
        json!({ "aud": "someone-else" }),
        json!({ "iss": "https://evil.test" }),
        json!({ "exp": 1_000_000 }),
    ] {
        let query = authorize(&world).await;
        let nonce = query["nonce"].clone();
        let mut token_claims = claims.clone();
        if token_claims.get("nonce").is_none() {
            token_claims["nonce"] = json!(nonce);
        }
        provider
            .token_response(json!({ "id_token": provider.id_token(&nonce, token_claims) }))
            .await;
        callback(&world, &[("code", "c"), ("state", &query["state"])])
            .await
            .assert_error_code(400, "invalid_id_token");
    }

    // Signed by a key the provider never published.
    let query = authorize(&world).await;
    let stranger = SigningKey::from_pem(include_str!("../../src/fake/idp-key.pem"))
        .unwrap()
        .with_kid("k1");
    let now = chrono::Utc::now().timestamp();
    let forged = stranger
        .sign_jwt(&json!({
            "iss": provider.issuer(), "aud": CLIENT_ID, "sub": "x", "nonce": query["nonce"],
            "email": "jane@acme.test", "iat": now, "exp": now + 300,
        }))
        .unwrap();
    provider.token_response(json!({ "id_token": forged })).await;
    callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_error_code(400, "invalid_id_token");
}

#[tokio::test]
async fn provider_errors_and_missing_states() {
    let world = support::world().await;
    let provider = Provider::start().await;
    connection(&world, &provider, Provisioning::default()).await;

    let query = authorize(&world).await;
    callback(
        &world,
        &[
            ("error", "access_denied"),
            ("error_description", "The user cancelled"),
            ("state", &query["state"]),
        ],
    )
    .await
    .assert_error_code(400, "sso_provider_refused");
    callback(&world, &[("code", "c")])
        .await
        .assert_error_code(400, "invalid_state");
    callback(&world, &[("code", "c"), ("state", "made-up")])
        .await
        .assert_error_code(400, "invalid_state");

    // The token endpoint refuses the code.
    let query = authorize(&world).await;
    provider
        .token_response(json!({ "error": "invalid_grant" }))
        .await;
    callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_error_code(400, "sso_provider_refused");
}

#[tokio::test]
async fn the_email_comes_from_userinfo_when_the_id_token_has_none() {
    let world = support::world().await;
    let provider = Provider::start().await;
    connection(&world, &provider, Provisioning::default()).await;
    let query = authorize(&world).await;
    provider
        .token_response(json!({
            "access_token": "at-123",
            "id_token": provider.id_token(&query["nonce"], json!({ "email": null, "email_verified": null })),
        }))
        .await;
    Mock::given(method("GET"))
        .and(path("/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sub": "user-123",
            "email": "jane@acme.test",
        })))
        .mount(&provider.server)
        .await;
    callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("jane@acme.test"));
}

#[tokio::test]
async fn an_unverified_provider_email_never_links_an_account() {
    let world = support::world().await;
    let provider = Provider::start().await;
    connection(&world, &provider, Provisioning::default()).await;
    world.create_verified_user("Jane", "jane@acme.test").await;
    let query = authorize(&world).await;
    provider
        .token_response(json!({
            "id_token": provider.id_token(&query["nonce"], json!({ "email_verified": false })),
        }))
        .await;
    callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_error_code(409, "email_taken");
}

#[tokio::test]
async fn a_workspace_connection_refuses_an_address_outside_its_domains_through_openid_connect() {
    let world = support::world().await;
    let provider = Provider::start().await;
    workspace_connection(&world, &provider, open()).await;

    let query = authorize(&world).await;
    token_with(
        &provider,
        &query,
        json!({ "email": "simran.kaur@gmail.test" }),
    )
    .await;
    callback(&world, &[("code", "the-code"), ("state", &query["state"])])
        .await
        .assert_error_code(403, "unverified_domain");
}

#[tokio::test]
async fn a_workspace_connection_treats_an_unverified_address_in_its_domains_as_before() {
    let world = support::world().await;
    let provider = Provider::start().await;
    workspace_connection(&world, &provider, open()).await;

    // In the verified domain, a token that doesn't vouch for the address
    // creates the account, with an unverified email.
    let query = authorize(&world).await;
    token_with(
        &provider,
        &query,
        json!({ "email": "simran@acme.test", "email_verified": false }),
    )
    .await;
    let body = callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("simran@acme.test"))
        .json_value();
    assert_eq!(body["user"]["email_verified_at"], Value::Null);
    let actions = world.audit.actions();
    assert!(!actions.contains(&"sso.login_refused".to_owned()));

    // Outside it, the same kind of token is refused, and the refusal audited.
    let query = authorize(&world).await;
    token_with(
        &provider,
        &query,
        json!({
            "sub": "user-456",
            "email": "simran.kaur@gmail.test",
            "email_verified": false,
        }),
    )
    .await;
    callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_error_code(403, "unverified_domain");
    let actions = world.audit.actions();
    assert!(actions.contains(&"sso.login_refused".to_owned()));
}

#[tokio::test]
async fn without_the_setting_a_workspace_connection_refuses_an_unverified_address_as_before() {
    let world = support::world().await;
    let provider = Provider::start().await;
    workspace_connection(&world, &provider, Provisioning::default()).await;
    let query = authorize(&world).await;
    token_with(
        &provider,
        &query,
        json!({ "email": "simran@acme.test", "email_verified": false }),
    )
    .await;
    callback(&world, &[("code", "c"), ("state", &query["state"])])
        .await
        .assert_error_code(403, "unverified_domain");
    let actions = world.audit.actions();
    assert!(!actions.contains(&"sso.login_refused".to_owned()));
}
