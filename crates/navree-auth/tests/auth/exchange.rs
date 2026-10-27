//! Trusted token exchange: `POST /api/auth/exchange` accepts a short-lived
//! JWT signed by a configured issuer, once.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use http::Method;
use navree_contracts::{AuditSink, Clock, FixedClock};
use navree_http_client::{FakeResponse, HttpClient};
use navree_testing::{TestApp, TestAppBuilder, TestResponse};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

use crate::support::{self, RecordingSink, tokens};

const ISSUER: &str = "https://legacy.example.com";
const JWKS_URL: &str = "https://legacy.example.com/.well-known/jwks.json";

/// A P-256 signing key of the sibling system.
struct Issuer {
    pair: EcdsaKeyPair,
    rng: SystemRandom,
    kid: Option<String>,
}

impl Issuer {
    fn new(kid: Option<&str>) -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        Self {
            pair,
            rng,
            kid: kid.map(str::to_owned),
        }
    }

    /// The public key as a PEM SubjectPublicKeyInfo.
    fn pem(&self) -> String {
        // SEQUENCE { SEQUENCE { id-ecPublicKey, prime256v1 }, BIT STRING }.
        let mut der = vec![
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01, 0x06,
            0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        der.extend_from_slice(self.pair.public_key().as_ref());
        format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(der)
        )
    }

    /// The public key as a JWK.
    fn jwk(&self) -> Value {
        let point = self.pair.public_key().as_ref();
        json!({
            "kty": "EC",
            "crv": "P-256",
            "kid": self.kid,
            "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&point[33..]),
        })
    }

    /// `claims` signed with ES256.
    fn sign(&self, claims: &Value) -> String {
        let mut header = json!({ "alg": "ES256", "typ": "JWT" });
        if let Some(kid) = &self.kid {
            header["kid"] = json!(kid);
        }
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self.pair.sign(&self.rng, input.as_bytes()).unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
    }
}

/// Valid claims for `email` at `clock`'s time.
fn claims(clock: &FixedClock, email: &str) -> Value {
    let now = clock.now().timestamp();
    json!({
        "iss": ISSUER,
        "aud": "navree-app",
        "sub": "legacy-42",
        "email": email,
        "iat": now,
        "exp": now + 120,
        "jti": uuid::Uuid::now_v7().to_string(),
    })
}

fn issuer_config(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("auth.trusted_issuers.legacy.issuer", ISSUER)
        .config("auth.trusted_issuers.legacy.audience", "navree-app")
}

async fn exchange(app: &TestApp, token: &str) -> TestResponse {
    app.client()
        .post("/api/auth/exchange")
        .json(&json!({ "token": token }))
        .send()
        .await
}

struct Setup {
    app: TestApp,
    clock: Arc<FixedClock>,
    sink: Arc<RecordingSink>,
    issuer: Issuer,
}

async fn with_public_key() -> Setup {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let issuer = Issuer::new(None);
    let app = issuer_config(support::clocked(&clock))
        .config("auth.trusted_issuers.legacy.public_key", issuer.pem())
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    Setup {
        app,
        clock,
        sink,
        issuer,
    }
}

#[tokio::test]
async fn exchange_is_off_without_trusted_issuers() {
    let app = support::app().await;
    exchange(&app, "a.b.c").await.assert_error(404, None);
}

#[tokio::test]
async fn a_signed_token_signs_its_user_in_once() {
    let s = with_public_key().await;
    let token = s.issuer.sign(&claims(&s.clock, "Ada@Example.com"));
    let response = exchange(&s.app, &token)
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ada@example.com"));
    let (access, _) = tokens(&response);
    s.app
        .client()
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);

    exchange(&s.app, &token)
        .await
        .assert_error(401, "Invalid token");
    let exchanged = s.sink.with_action("auth.token_exchanged");
    assert_eq!(exchanged.len(), 1);
    assert_eq!(exchanged[0].metadata["issuer"], "legacy");
    let failed = s.sink.with_action("auth.token_exchange_failed");
    assert_eq!(failed[0].metadata["reason"], "replayed");
}

#[tokio::test]
async fn tokens_that_fail_a_check_are_refused_with_the_reason_audited() {
    let s = with_public_key().await;
    let now = s.clock.now().timestamp();
    let base = claims(&s.clock, "ada@example.com");
    let with = |key: &str, value: Value| {
        let mut claims = base.clone();
        claims["jti"] = json!(uuid::Uuid::now_v7().to_string());
        if value.is_null() {
            claims.as_object_mut().unwrap().remove(key);
        } else {
            claims[key] = value;
        }
        claims
    };
    let other = Issuer::new(None);
    let cases: Vec<(String, &str)> = vec![
        (
            s.issuer.sign(&with("aud", json!("someone-else"))),
            "wrong audience",
        ),
        (s.issuer.sign(&with("aud", json!(["a", "navree-app"]))), ""),
        (s.issuer.sign(&with("exp", json!(now - 31))), "expired"),
        (
            s.issuer.sign(&with("iat", json!(now + 60))),
            "not yet valid",
        ),
        (
            s.issuer.sign(&with("nbf", json!(now + 60))),
            "not yet valid",
        ),
        (
            s.issuer.sign(&with("exp", json!(now + 3600))),
            "lifetime too long",
        ),
        (s.issuer.sign(&with("jti", Value::Null)), "jti is required"),
        (
            s.issuer.sign(&with("email", Value::Null)),
            "email is required",
        ),
        (
            s.issuer.sign(&with("email", json!("nobody@example.com"))),
            "unknown user",
        ),
        (
            s.issuer.sign(&with("email_verified", json!(false))),
            "email not verified",
        ),
        (
            s.issuer
                .sign(&with("iss", json!("https://evil.example.com"))),
            "unknown issuer",
        ),
        (other.sign(&with("iat", json!(now))), "bad signature"),
        ("not.a.token".to_owned(), "malformed"),
    ];
    for (token, reason) in cases {
        let response = exchange(&s.app, &token).await;
        if reason.is_empty() {
            response.assert_status(200);
            continue;
        }
        response.assert_error(401, "Invalid token");
        let failed = s.sink.with_action("auth.token_exchange_failed");
        assert_eq!(
            failed.last().unwrap().metadata["reason"],
            reason,
            "{reason}"
        );
    }
}

#[tokio::test]
async fn the_kill_switch_refuses_an_issuer() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let issuer = Issuer::new(None);
    let app = issuer_config(support::clocked(&clock))
        .config("auth.trusted_issuers.legacy.public_key", issuer.pem())
        .config("auth.trusted_issuers.legacy.enabled", false)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;
    exchange(&app, &issuer.sign(&claims(&clock, "ada@example.com")))
        .await
        .assert_error(401, "Invalid token");
    assert_eq!(
        sink.with_action("auth.token_exchange_failed")[0].metadata["reason"],
        "issuer disabled"
    );
}

#[tokio::test]
async fn key_sets_are_fetched_cached_and_refetched_for_new_keys() {
    let clock = support::clock();
    let (http, server) = HttpClient::fake();
    let first = Issuer::new(Some("k1"));
    let second = Issuer::new(Some("k2"));
    // The issuer rotates its key after the first fetch.
    server.on_sequence(
        Method::GET,
        JWKS_URL,
        [
            FakeResponse::new(200).json(&json!({ "keys": [first.jwk()] })),
            FakeResponse::new(200).json(&json!({ "keys": [first.jwk(), second.jwk()] })),
        ],
    );
    let app = issuer_config(support::clocked(&clock))
        .config("auth.trusted_issuers.legacy.jwks_url", JWKS_URL)
        .override_value::<HttpClient>(Arc::new(http))
        .build()
        .await;
    support::create_user(&app, "ada@example.com").await;

    exchange(&app, &first.sign(&claims(&clock, "ada@example.com")))
        .await
        .assert_status(200);
    exchange(&app, &first.sign(&claims(&clock, "ada@example.com")))
        .await
        .assert_status(200);
    server.assert_sent_count(1);

    // A token with an unknown kid fetches again (once a minute at most).
    exchange(&app, &second.sign(&claims(&clock, "ada@example.com")))
        .await
        .assert_error(401, "Invalid token");
    clock.advance(Duration::from_secs(61));
    exchange(&app, &second.sign(&claims(&clock, "ada@example.com")))
        .await
        .assert_status(200);
    server.assert_sent_count(2);
}
