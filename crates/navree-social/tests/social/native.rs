//! Sign-in from native apps against local stand-ins of Apple and Google
//! (wiremock serving a key set of test keys): the nonce, the identity
//! token's checks, Apple's authorization code, revocation and Apple's
//! server-to-server notifications.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use navree_auth::AuthService;
use navree_contracts::{AuditSink, Clock};
use navree_crypto::Encrypter;
use navree_db::Db;
use navree_events::{EventBus, EventsModule};
use navree_orm::Model as _;
use navree_social::{NativeSignIn, Social, SocialUser, social_account};
use navree_testing::{FixedClock, TestApp, TestAppBuilder, TestResponse};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::support::{
    APPLE_P8, Idp, RecordingSink, builder, create_user, fake_app, form_field, http_client_module,
};

/// The bundle id of the site's iPhone app (`auth.apps.apple.main`).
const BUNDLE: &str = "in.navree.dhaba";
/// The Services ID of the web flow.
const SERVICES_ID: &str = "in.navree.dhaba.web";
/// The Google client id the Android app passes as its server client id.
const GOOGLE_CLIENT: &str = "4810-server.apps.googleusercontent.com";

const APPLE_SUB: &str = "001234.8c1f0e2a9b7d4e55.0931";
const RELAY: &str = "k7p2xq9d4m@privaterelay.appleid.com";

struct Native {
    app: TestApp,
    idp: Idp,
    clock: Arc<FixedClock>,
}

impl Native {
    async fn start(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> Native {
        let idp = Idp::start().await;
        idp.mount_jwks("/auth/keys").await;
        idp.mount_jwks("/certs").await;
        let clock = Arc::new(FixedClock::new(
            DateTime::<Utc>::from_timestamp(Utc::now().timestamp(), 0).unwrap(),
        ));
        let builder = builder()
            .config(
                "social.providers.apple",
                json!({
                    "client_id": SERVICES_ID,
                    "team_id": "TEAM123456",
                    "key_id": "KEY1234567",
                    "private_key": APPLE_P8,
                    "url": idp.uri(),
                }),
            )
            .config(
                "social.providers.google",
                json!({
                    "client_id": GOOGLE_CLIENT,
                    "client_secret": "google-secret",
                    "url": idp.uri(),
                    "authorize_url": format!("{}/o/oauth2/v2/auth", idp.uri()),
                    "token_url": format!("{}/token", idp.uri()),
                    "jwks_url": format!("{}/certs", idp.uri()),
                }),
            )
            .config(
                "auth.apps.apple.main",
                json!({ "team_id": "ABCDE12345", "bundle_id": BUNDLE }),
            )
            .module(http_client_module())
            .override_value::<dyn Clock>(clock.clone());
        let app = configure(builder).build().await;
        Native { app, idp, clock }
    }

    fn now(&self) -> i64 {
        self.clock.now().timestamp()
    }

    /// A fresh attempt: the nonce and the sealed state.
    async fn nonce(&self, provider: &str) -> (String, String) {
        let body = self
            .app
            .client()
            .post(&format!("/api/auth/social/{provider}/native/nonce"))
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(body["expires_in"], 300);
        (
            body["nonce"].as_str().unwrap().to_owned(),
            body["state"].as_str().unwrap().to_owned(),
        )
    }

    async fn post(&self, provider: &str, body: Value) -> TestResponse {
        self.app
            .client()
            .post(&format!("/api/auth/social/{provider}/native"))
            .json(&body)
            .send()
            .await
    }

    fn apple_claims(&self, nonce: &str) -> Value {
        json!({
            "iss": self.idp.uri(),
            "aud": BUNDLE,
            "sub": APPLE_SUB,
            "email": RELAY,
            "email_verified": "true",
            "is_private_email": "true",
            "nonce_supported": true,
            "auth_time": self.now(),
            "iat": self.now(),
            "exp": self.now() + 600,
            "nonce": hashed(nonce),
        })
    }

    fn google_claims(&self, nonce: &str, email: &str, verified: bool) -> Value {
        json!({
            "iss": "https://accounts.google.com",
            "aud": GOOGLE_CLIENT,
            "azp": "4810-android.apps.googleusercontent.com",
            "sub": "108836472915530712345",
            "email": email,
            "email_verified": verified,
            "name": "Harpreet Kaur",
            "picture": "https://lh3.test/harpreet.png",
            "iat": self.now(),
            "exp": self.now() + 3600,
            "nonce": nonce,
        })
    }

    /// Apple's token endpoint answers a code with tokens for `sub`.
    async fn mount_apple_code(&self, sub: &str) {
        let id_token = self.idp.es256(&json!({
            "iss": self.idp.uri(),
            "aud": BUNDLE,
            "sub": sub,
            "iat": self.now(),
            "exp": self.now() + 600,
        }));
        self.idp
            .mount_post_once(
                "/auth/token",
                200,
                json!({
                    "access_token": "a.apple-access",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "refresh_token": "r.apple-refresh",
                    "id_token": id_token,
                }),
            )
            .await;
    }

    /// A full Apple sign-in from the app with its code: the response.
    async fn apple_sign_in(&self) -> TestResponse {
        self.mount_apple_code(APPLE_SUB).await;
        let (nonce, state) = self.nonce("apple").await;
        self.post(
            "apple",
            json!({
                "id_token": self.idp.es256(&self.apple_claims(&nonce)),
                "state": state,
                "authorization_code": "c.apple-code",
                "user": { "name": { "firstName": "Simran", "lastName": "Gill" }, "email": "else@example.in" },
            }),
        )
        .await
    }

    async fn accounts(&self) -> Vec<social_account::Model> {
        let db = self.app.get::<Db>().await;
        social_account::Model::query()
            .get(&*db)
            .await
            .unwrap()
            .into_models()
    }
}

/// Apple's form of the nonce: lower-case hex SHA-256.
fn hashed(nonce: &str) -> String {
    hex(&Sha256::digest(nonce.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn claims_of(jwt: &str) -> Value {
    let part = jwt.split('.').nth(1).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
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

fn no_throttle(b: TestAppBuilder) -> TestAppBuilder {
    b.config("auth.login_throttle_per_minute", 0)
}

#[tokio::test]
async fn apple_signs_in_a_new_user_from_the_app_and_keeps_its_tokens() {
    let native = Native::start(|b| b).await;
    let body = native
        .apple_sign_in()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!(RELAY))
        .assert_json_path("user.name", json!("Simran Gill"))
        .assert_json_path("token_type", json!("Bearer"))
        .json_value();
    assert!(body["user"]["email_verified_at"].is_string(), "{body}");
    assert!(body["refresh_token"].is_string());

    // The code was exchanged as the app (its bundle id), with a client
    // secret signed for it, and no redirect URI.
    let request = &native.idp.requests_to("/auth/token").await[0];
    assert_eq!(form_field(request, "client_id").as_deref(), Some(BUNDLE));
    assert_eq!(
        form_field(request, "grant_type").as_deref(),
        Some("authorization_code")
    );
    assert_eq!(form_field(request, "code").as_deref(), Some("c.apple-code"));
    assert_eq!(form_field(request, "redirect_uri"), None);
    let secret = claims_of(&form_field(request, "client_secret").unwrap());
    assert_eq!(secret["sub"], BUNDLE);
    assert_eq!(secret["iss"], "TEAM123456");
    assert_eq!(secret["aud"], native.idp.uri());

    // Kept encrypted although `store_tokens` is off: revocation needs it.
    let rows = native.accounts().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider_user_id, APPLE_SUB);
    assert_eq!(rows[0].name.as_deref(), Some("Simran Gill"));
    let stored = rows[0].refresh_token.clone().expect("a refresh token");
    assert!(!stored.contains("r.apple-refresh"));
    let encrypter = native.app.get::<Encrypter>().await;
    assert_eq!(
        encrypter.decrypt_string(&stored).unwrap(),
        "r.apple-refresh"
    );
    // The name the app sent is display data: the address it sent is not.
    assert_eq!(rows[0].email.as_deref(), Some(RELAY));
}

#[tokio::test]
async fn a_code_of_another_account_is_refused() {
    let native = Native::start(|b| b).await;
    native.mount_apple_code("000999.someone.else").await;
    let (nonce, state) = native.nonce("apple").await;
    native
        .post(
            "apple",
            json!({
                "id_token": native.idp.es256(&native.apple_claims(&nonce)),
                "state": state,
                "authorization_code": "c.other",
            }),
        )
        .await
        .assert_error_code(400, "invalid_id_token");
    assert!(native.accounts().await.is_empty());
}

#[tokio::test]
async fn google_links_the_existing_user_with_a_verified_email_and_signs_it_in_again() {
    let sink = RecordingSink::default();
    let native = Native::start(|b| {
        b.module(EventsModule::for_root())
            .override_value::<dyn AuditSink>(Arc::new(sink.clone()))
    })
    .await;
    let mut linked = native
        .app
        .get::<EventBus>()
        .await
        .subscribe_json("social.linked");
    let harpreet = create_user(&native.app, "Harpreet Kaur", "harpreet@example.in").await;

    let (nonce, state) = native.nonce("google").await;
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce, "harpreet@example.in", true));
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(harpreet.id));
    let event = linked.try_recv().expect("social.linked");
    assert_eq!(event["provider"], "google");
    assert!(sink.entries().iter().any(|e| e.action == "auth.login"
        && e.metadata["provider"] == "google"
        && e.actor_id == Some(harpreet.id)));

    // The linked account signs in again, on a new attempt.
    let (nonce, state) = native.nonce("google").await;
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce, "harpreet@example.in", true));
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(harpreet.id));
    assert_eq!(native.accounts().await.len(), 1);
    // The key set was fetched once.
    assert_eq!(native.idp.requests_to("/certs").await.len(), 1);
}

#[tokio::test]
async fn an_unverified_email_never_takes_over_an_account() {
    let native = Native::start(|b| b).await;
    create_user(&native.app, "Harpreet Kaur", "harpreet@example.in").await;
    let (nonce, state) = native.nonce("google").await;
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce, "harpreet@example.in", false));
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_error_code(409, "email_taken");
    assert!(native.accounts().await.is_empty());
}

/// Posts a Google token whose claims `change` made, on a fresh attempt.
async fn google_with(native: &Native, change: impl FnOnce(&mut Value)) -> TestResponse {
    let (nonce, state) = native.nonce("google").await;
    let mut claims = native.google_claims(&nonce, "simran@example.in", true);
    change(&mut claims);
    let token = native.idp.rs256(&claims);
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
}

#[tokio::test]
async fn tokens_for_another_app_or_issuer_are_refused() {
    let native = Native::start(no_throttle).await;
    google_with(&native, |c| {
        c["aud"] = json!("4810-other.apps.googleusercontent.com")
    })
    .await
    .assert_error_code(400, "invalid_id_token");
    google_with(&native, |c| c["iss"] = json!("https://accounts.evil.test"))
        .await
        .assert_error_code(400, "invalid_id_token");
    // Apple: the web flow's Services ID is not an app.
    let (nonce, state) = native.nonce("apple").await;
    let mut claims = native.apple_claims(&nonce);
    claims["aud"] = json!(SERVICES_ID);
    native
        .post(
            "apple",
            json!({ "id_token": native.idp.es256(&claims), "state": state }),
        )
        .await
        .assert_error_code(400, "invalid_id_token");
    assert!(native.accounts().await.is_empty());
}

#[tokio::test]
async fn expired_and_stale_tokens_are_refused() {
    let native = Native::start(no_throttle).await;
    let now = native.now();
    google_with(&native, |c| c["exp"] = json!(now - 120))
        .await
        .assert_error_code(400, "invalid_id_token");
    // Issued before the attempt started: not this attempt's token.
    google_with(&native, |c| c["iat"] = json!(now - 600))
        .await
        .assert_error_code(400, "invalid_id_token");
    google_with(&native, |c| c["iat"] = json!(now + 600))
        .await
        .assert_error_code(400, "invalid_id_token");
    // The attempt itself expires after five minutes.
    let (nonce, state) = native.nonce("google").await;
    native.clock.advance(Duration::from_secs(301));
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce, "simran@example.in", true));
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_error_code(400, "invalid_state");
}

#[tokio::test]
async fn a_bad_signature_is_refused() {
    let native = Native::start(no_throttle).await;
    let (nonce, state) = native.nonce("google").await;
    let claims = native.google_claims(&nonce, "simran@example.in", true);
    let signed = native.idp.rs256(&claims);
    let mut forged = claims.clone();
    forged["email"] = json!("owner@example.in");
    native
        .post(
            "google",
            json!({ "id_token": tamper(&signed, &forged), "state": state }),
        )
        .await
        .assert_error_code(400, "invalid_id_token");
}

#[tokio::test]
async fn the_nonce_is_required_once_and_for_this_attempt() {
    let native = Native::start(no_throttle).await;
    // Missing.
    google_with(&native, |c| {
        c.as_object_mut().unwrap().remove("nonce");
    })
    .await
    .assert_error_code(400, "invalid_id_token");

    // Another attempt's nonce.
    let (nonce_a, _state_a) = native.nonce("google").await;
    let (_nonce_b, state_b) = native.nonce("google").await;
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce_a, "simran@example.in", true));
    native
        .post("google", json!({ "id_token": token, "state": state_b }))
        .await
        .assert_error_code(400, "invalid_id_token");

    // Reused: the attempt works once.
    let (nonce, state) = native.nonce("google").await;
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce, "simran@example.in", true));
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_status(200);
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_error_code(400, "invalid_state");

    // Apple's nonce is the hash the app gave the sheet, not the nonce.
    let (nonce, state) = native.nonce("apple").await;
    let mut claims = native.apple_claims(&nonce);
    claims["nonce"] = json!(nonce);
    native
        .post(
            "apple",
            json!({ "id_token": native.idp.es256(&claims), "state": state }),
        )
        .await
        .assert_error_code(400, "invalid_id_token");

    // An Apple attempt does not sign in with Google, and a web flow's
    // state does not start a native sign-in.
    let (nonce, apple_state) = native.nonce("apple").await;
    let token = native
        .idp
        .rs256(&native.google_claims(&nonce, "simran@example.in", true));
    native
        .post("google", json!({ "id_token": token, "state": apple_state }))
        .await
        .assert_error_code(400, "invalid_state");
    let web = native
        .app
        .client()
        .get("/api/auth/social/google/redirect")
        .send()
        .await
        .assert_status(200)
        .json_value();
    native
        .post(
            "google",
            json!({ "id_token": token, "state": web["state"] }),
        )
        .await
        .assert_error_code(400, "invalid_state");
}

#[tokio::test]
async fn an_unknown_key_id_is_looked_up_once_and_stays_refused() {
    let native = Native::start(no_throttle).await;
    native.apple_sign_in().await.assert_status(200);
    assert_eq!(native.idp.requests_to("/auth/keys").await.len(), 1);

    // Later, a token naming a key the set does not have: refetched once.
    native.clock.advance(Duration::from_secs(31));
    let (nonce, state) = native.nonce("apple").await;
    let token = native
        .idp
        .es256_kid(&native.apple_claims(&nonce), "rotated-away");
    native
        .post("apple", json!({ "id_token": token, "state": state }))
        .await
        .assert_error_code(400, "invalid_id_token");
    assert_eq!(native.idp.requests_to("/auth/keys").await.len(), 2);

    // Right after, the same unknown key does not fetch the set again.
    let (nonce, state) = native.nonce("apple").await;
    let token = native
        .idp
        .es256_kid(&native.apple_claims(&nonce), "rotated-away");
    native
        .post("apple", json!({ "id_token": token, "state": state }))
        .await
        .assert_error_code(400, "invalid_id_token");
    assert_eq!(native.idp.requests_to("/auth/keys").await.len(), 2);
}

#[tokio::test]
async fn native_sign_in_needs_a_configured_app() {
    let idp = Idp::start().await;
    let app = builder()
        .config(
            "social.providers.apple",
            json!({ "client_id": SERVICES_ID, "team_id": "T", "key_id": "K", "private_key": APPLE_P8, "url": idp.uri() }),
        )
        .module(http_client_module())
        .build()
        .await;
    for provider in ["apple", "github"] {
        app.client()
            .post(&format!("/api/auth/social/{provider}/native/nonce"))
            .send()
            .await
            .assert_error_code(404, "native_sign_in_unavailable");
    }
    app.client()
        .post("/api/auth/social/okta/native/nonce")
        .send()
        .await
        .assert_error_code(404, "unknown_provider");
}

#[tokio::test]
async fn the_post_is_validated_and_throttled_and_errors_carry_no_secrets() {
    let native = Native::start(|b| b).await;
    native
        .post("google", json!({ "id_token": "", "state": "" }))
        .await
        .assert_error_code(400, "validation_failed");
    native
        .post(
            "apple",
            json!({ "id_token": "x".repeat(20_000), "state": "nvs1.x", "user": [1] }),
        )
        .await
        .assert_error_code(400, "validation_failed");

    let (_, state) = native.nonce("apple").await;
    let secret_code = "c.do-not-print-me";
    let token = "eyJhbGciOiJFUzI1NiJ9.e30.c2ln";
    let body = native
        .post(
            "apple",
            json!({ "id_token": token, "state": state, "authorization_code": secret_code }),
        )
        .await
        .assert_status(400)
        .json_value()
        .to_string();
    assert!(
        !body.contains(secret_code) && !body.contains(token),
        "{body}"
    );

    // The login throttle (5 a minute) counts native posts.
    let mut last = 0;
    for _ in 0..6 {
        last = native
            .post(
                "google",
                json!({ "id_token": "e.x.y", "state": "nvs1.made-up" }),
            )
            .await
            .status()
            .as_u16();
    }
    assert_eq!(last, 429);
}

async fn bearer(body: &Value) -> String {
    body["access_token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn unlinking_apple_revokes_its_token_at_apple() {
    let native = Native::start(|b| b).await;
    let body = native.apple_sign_in().await.assert_status(200).json_value();
    let user_id: Uuid = serde_json::from_value(body["user"]["id"].clone()).unwrap();
    // A second way to sign in, so Apple can be disconnected: Google with
    // the same verified address links to the same user.
    let (nonce, state) = native.nonce("google").await;
    let token = native.idp.rs256(&native.google_claims(&nonce, RELAY, true));
    native
        .post("google", json!({ "id_token": token, "state": state }))
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(user_id));

    // Apple cannot be reached: the account stays linked.
    native
        .idp
        .mount_post_once("/auth/revoke", 503, json!({}))
        .await;
    native
        .app
        .client()
        .delete("/api/auth/social/accounts/apple")
        .bearer(&bearer(&body).await)
        .send()
        .await
        .assert_error_code(502, "provider_unavailable");
    assert_eq!(native.accounts().await.len(), 2);

    native.idp.mount_post("/auth/revoke", 200, json!({})).await;
    native
        .app
        .client()
        .delete("/api/auth/social/accounts/apple")
        .bearer(&bearer(&body).await)
        .send()
        .await
        .assert_status(204);
    let revoke = native.idp.requests_to("/auth/revoke").await.pop().unwrap();
    assert_eq!(form_field(&revoke, "client_id").as_deref(), Some(BUNDLE));
    assert_eq!(
        form_field(&revoke, "token").as_deref(),
        Some("r.apple-refresh")
    );
    assert_eq!(
        form_field(&revoke, "token_type_hint").as_deref(),
        Some("refresh_token")
    );
    let secret = claims_of(&form_field(&revoke, "client_secret").unwrap());
    assert_eq!(secret["sub"], BUNDLE);
    let rows = native.accounts().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, "google");
}

#[tokio::test]
async fn offboarding_and_deletion_revoke_apples_tokens() {
    let native = Native::start(|b| b).await;
    let body = native.apple_sign_in().await.assert_status(200).json_value();
    let user_id: Uuid = serde_json::from_value(body["user"]["id"].clone()).unwrap();
    // A token Apple no longer knows counts as revoked.
    native
        .idp
        .mount_post("/auth/revoke", 400, json!({ "error": "invalid_grant" }))
        .await;
    let ended = native
        .app
        .get::<AuthService>()
        .await
        .revoke_all_credentials(user_id)
        .await
        .unwrap();
    assert_eq!(ended.revoked("social_provider_tokens"), 1);
    let revoke = &native.idp.requests_to("/auth/revoke").await[0];
    assert_eq!(form_field(revoke, "client_id").as_deref(), Some(BUNDLE));
    let rows = native.accounts().await;
    assert_eq!(rows.len(), 1, "the link stays");
    assert_eq!(rows[0].refresh_token, None, "the tokens are forgotten");
    assert_eq!(rows[0].access_token, None);
    // Nothing left to revoke.
    let social = native.app.get::<Social>().await;
    assert_eq!(social.revoke_tokens(user_id).await.unwrap(), 0);
    assert_eq!(native.idp.requests_to("/auth/revoke").await.len(), 1);
}

/// A notification from Apple about `sub`, signed with the test key.
fn notification(native: &Native, kind: &str, sub: &str, jti: &str) -> String {
    let events = json!({ "type": kind, "sub": sub, "event_time": native.now() });
    native.idp.es256(&json!({
        "iss": native.idp.uri(),
        "aud": BUNDLE,
        "iat": native.now(),
        "jti": jti,
        // Apple sends the events as a JSON string.
        "events": events.to_string(),
    }))
}

async fn notify(native: &Native, payload: &str) -> TestResponse {
    native
        .app
        .client()
        .post("/api/auth/social/apple/notifications")
        .json(&json!({ "payload": payload }))
        .send()
        .await
}

#[tokio::test]
async fn apples_consent_revoked_forgets_the_tokens_and_signs_the_person_out() {
    let native = Native::start(|b| b.module(EventsModule::for_root())).await;
    let mut notices = native
        .app
        .get::<EventBus>()
        .await
        .subscribe_json("social.provider_notice");
    let body = native.apple_sign_in().await.assert_status(200).json_value();
    let payload = notification(&native, "consent-revoked", APPLE_SUB, "jti-1");
    notify(&native, &payload).await.assert_status(200);

    let rows = native.accounts().await;
    assert_eq!(rows.len(), 1, "still linked");
    assert_eq!(rows[0].refresh_token, None);
    native
        .app
        .client()
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": body["refresh_token"] }))
        .send()
        .await
        .assert_status(401);
    let event = notices.try_recv().expect("social.provider_notice");
    assert_eq!(event["notice"], "consent_revoked");
    // A second delivery of the same notification changes nothing.
    notify(&native, &payload).await.assert_status(200);
    assert!(notices.try_recv().is_err());
}

#[tokio::test]
async fn apples_account_deleted_removes_the_link() {
    let native = Native::start(|b| b.module(EventsModule::for_root())).await;
    let mut unlinked = native
        .app
        .get::<EventBus>()
        .await
        .subscribe_json("social.unlinked");
    native.apple_sign_in().await.assert_status(200);
    notify(
        &native,
        &notification(&native, "account-deleted", APPLE_SUB, "jti-2"),
    )
    .await
    .assert_status(200);
    assert!(native.accounts().await.is_empty());
    assert_eq!(
        unlinked.try_recv().expect("social.unlinked")["provider"],
        "apple"
    );
}

#[tokio::test]
async fn apples_notifications_are_verified() {
    let sink = RecordingSink::default();
    let native = Native::start(|b| b.override_value::<dyn AuditSink>(Arc::new(sink.clone()))).await;
    native.apple_sign_in().await.assert_status(200);

    // Forged: changed claims under Apple's signature.
    let signed = notification(&native, "email-disabled", APPLE_SUB, "jti-3");
    let mut claims = claims_of(&signed);
    claims["events"] = json!(
        json!({ "type": "account-deleted", "sub": APPLE_SUB, "event_time": native.now() })
            .to_string()
    );
    notify(&native, &tamper(&signed, &claims))
        .await
        .assert_error_code(400, "invalid_notification");
    // For another app.
    let mut other = claims_of(&signed);
    other["aud"] = json!("in.someone.else");
    notify(&native, &native.idp.es256(&other))
        .await
        .assert_error_code(400, "invalid_notification");
    // Old.
    let mut old = claims_of(&signed);
    old["iat"] = json!(native.now() - 8 * 86_400);
    notify(&native, &native.idp.es256(&old))
        .await
        .assert_error_code(400, "invalid_notification");
    assert_eq!(native.accounts().await.len(), 1, "nothing was done");

    // About nobody linked here: accepted, ignored.
    notify(
        &native,
        &notification(&native, "account-deleted", "000000.nobody.0000", "jti-4"),
    )
    .await
    .assert_status(200);
    assert_eq!(native.accounts().await.len(), 1);

    // Mail forwarding: recorded.
    notify(&native, &signed).await.assert_status(200);
    assert!(sink.actions().contains(&"social.email_disabled".to_owned()));
}

#[tokio::test]
async fn the_fake_answers_native_sign_ins() {
    let (app, fake) =
        fake_app(|b| b.config("social.providers.google.client_id", GOOGLE_CLIENT)).await;
    fake.on_id_token(
        "google",
        "token-gurpreet",
        SocialUser::new("google", "1")
            .name("Gurpreet Sandhu")
            .email(Some("gurpreet@example.in"), true),
    );
    let body = app
        .client()
        .post("/api/auth/social/google/native/nonce")
        .send()
        .await
        .assert_status(200)
        .json_value();
    app.client()
        .post("/api/auth/social/google/native")
        .json(&json!({ "id_token": "token-gurpreet", "state": body["state"] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("gurpreet@example.in"));
    let social = app.get::<Social>().await;
    let post = NativeSignIn::new("token-unknown", body["state"].as_str().unwrap());
    assert!(social.native_user("google", &post).await.is_err());
    assert!(fake.revocations().is_empty());
}

#[tokio::test]
async fn an_app_on_the_session_cookie_is_signed_in_on_its_session() {
    let native = Native::start(|b| {
        b.module(navree_session::SessionModule::for_root(
            navree_session::SessionOptions::default(),
        ))
    })
    .await;
    let phone = native.app.client();
    phone.csrf_cookie().await;
    native.mount_apple_code(APPLE_SUB).await;
    let body = phone
        .post("/api/auth/social/apple/native/nonce")
        .xsrf()
        .send()
        .await
        .assert_status(200)
        .json_value();
    let nonce = body["nonce"].as_str().unwrap();
    let body = phone
        .post("/api/auth/social/apple/native/session")
        .xsrf()
        .json(&json!({
            "id_token": native.idp.es256(&native.apple_claims(nonce)),
            "state": body["state"],
            "authorization_code": "c.apple-code",
            "remember": true,
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["email"], RELAY);
    assert!(body.get("access_token").is_none(), "no token on a session");
    phone
        .get("/api/auth/me")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!(RELAY));
}
