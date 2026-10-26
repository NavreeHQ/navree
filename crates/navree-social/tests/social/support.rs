//! Helpers shared by the navree-social integration tests: app builders, the
//! sign-in steps, an audit sink that records entries in memory, and a mock
//! identity provider on wiremock that signs ID tokens with fixture keys.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_auth::{AuthModule, AuthService, ClientMeta, User};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::DatabaseModule;
use navree_http_client::{HttpClientModule, HttpClientOptions, SsrfPolicy};
use navree_social::{Social, SocialFake, SocialModule};
use navree_testing::{TestApp, TestAppBuilder, TestResponse};
use ring::rand::SystemRandom;
use ring::signature::{
    self, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair, RSA_PKCS1_SHA256, RsaKeyPair,
};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// The front end's callback page.
pub const REDIRECT_BASE: &str = "https://app.acme.test/auth/callback";

/// A password for users created in tests.
pub const PASSWORD: &str = "correct horse battery";

/// A test app with crypto (fast hashing), a fresh database, auth and the
/// social module read from configuration, with a GitHub provider.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("social.redirect_base", REDIRECT_BASE)
        .config("social.providers.github.client_id", "Iv1.8a61f9b3a7aba766")
        .config("social.providers.github.client_secret", "github-secret")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(SocialModule::from_config())
}

/// [`builder`] with a [`Social::fake`].
pub async fn fake_app(
    configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder,
) -> (TestApp, SocialFake) {
    let (social, fake) = Social::fake();
    let app = configure(builder())
        .override_value(Arc::new(social))
        .build()
        .await;
    (app, fake)
}

/// The `HttpClientModule` of the provider tests: loopback allowed (the
/// mock servers), no retries.
pub fn http_client_module() -> impl navree_kernel::Module {
    HttpClientModule::for_root(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".to_owned()],
        },
        ..HttpClientOptions::default()
    })
}

/// `GET /api/auth/social/{provider}/redirect` (with an optional query and
/// bearer token): the response.
pub async fn redirect(
    app: &TestApp,
    provider: &str,
    query: &str,
    bearer: Option<&str>,
) -> TestResponse {
    let mut request = app
        .client()
        .get(&format!("/api/auth/social/{provider}/redirect{query}"));
    if let Some(token) = bearer {
        request = request.bearer(token);
    }
    request.send().await
}

/// The sealed state of a new sign-in flow.
pub async fn state(app: &TestApp, provider: &str) -> String {
    redirect(app, provider, "", None)
        .await
        .assert_status(200)
        .json_value()["state"]
        .as_str()
        .expect("a state")
        .to_owned()
}

/// `POST /api/auth/social/{provider}/callback`.
pub async fn callback(app: &TestApp, provider: &str, code: &str, state: &str) -> TestResponse {
    app.client()
        .post(&format!("/api/auth/social/{provider}/callback"))
        .json(&json!({ "code": code, "state": state }))
        .send()
        .await
}

/// A full sign-in flow: a fresh state, then the callback with `code`.
pub async fn sign_in(app: &TestApp, provider: &str, code: &str) -> TestResponse {
    let state = state(app, provider).await;
    callback(app, provider, code, &state).await
}

/// Creates a user with a password (as a seeder would).
pub async fn create_user(app: &TestApp, name: &str, email: &str) -> User {
    app.get::<AuthService>()
        .await
        .create_user(name, email, PASSWORD)
        .await
        .expect("the user is created")
}

/// An access token for `user`.
pub async fn access_token(app: &TestApp, user: &User) -> String {
    app.get::<AuthService>()
        .await
        .issue_tokens(user, ClientMeta::default())
        .await
        .expect("tokens")
        .access_token
}

/// The query parameter `name` of `url`.
pub fn query_param(url: &str, name: &str) -> Option<String> {
    let url = url::Url::parse(url).expect("a URL");
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// The form field `name` of a recorded request body.
pub fn form_field(request: &Request, name: &str) -> Option<String> {
    form_urlencoded::parse(&request.body)
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// Collects audit entries.
#[derive(Clone, Default)]
pub struct RecordingSink(pub Arc<Mutex<Vec<AuditEntry>>>);

impl RecordingSink {
    /// The actions recorded so far.
    pub fn actions(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.action).collect()
    }

    /// The entries recorded so far.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl AuditSink for RecordingSink {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// The fixture RSA key (PKCS#8 PEM) the mock provider signs RS256 tokens
/// with.
const RSA_PEM: &str = include_str!("../fixtures/rsa-2048.pem");

/// The fixture P-256 key, in the format of Sign in with Apple's `.p8`.
pub const APPLE_P8: &str = include_str!("../fixtures/apple-authkey.p8");

fn pem_der(pem: &str) -> Vec<u8> {
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    STANDARD.decode(body).expect("PEM")
}

/// A mock identity provider: a wiremock server plus the keys it signs ID
/// tokens with (RS256 `rsa-1`, ES256 `ec-1`).
pub struct Idp {
    pub server: MockServer,
    rsa: RsaKeyPair,
    ec: EcdsaKeyPair,
}

impl Idp {
    pub async fn start() -> Idp {
        let rng = SystemRandom::new();
        let ec_pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        Idp {
            server: MockServer::start().await,
            rsa: RsaKeyPair::from_pkcs8(&pem_der(RSA_PEM)).unwrap(),
            ec: EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, ec_pkcs8.as_ref(), &rng)
                .unwrap(),
        }
    }

    pub fn uri(&self) -> String {
        self.server.uri()
    }

    /// The key set with both keys.
    pub fn jwks(&self) -> Value {
        let rsa: signature::RsaPublicKeyComponents<Vec<u8>> = self.rsa.public().into();
        let point = self.ec.public_key().as_ref();
        json!({ "keys": [
            {
                "kty": "RSA", "kid": "rsa-1", "use": "sig", "alg": "RS256",
                "n": URL_SAFE_NO_PAD.encode(&rsa.n), "e": URL_SAFE_NO_PAD.encode(&rsa.e),
            },
            {
                "kty": "EC", "kid": "ec-1", "use": "sig", "crv": "P-256",
                "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
            },
        ]})
    }

    fn encode(header: &Value, claims: &Value) -> String {
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    /// An RS256 ID token with `claims`.
    pub fn rs256(&self, claims: &Value) -> String {
        let input = Self::encode(
            &json!({ "alg": "RS256", "kid": "rsa-1", "typ": "JWT" }),
            claims,
        );
        let mut sig = vec![0; self.rsa.public().modulus_len()];
        self.rsa
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                input.as_bytes(),
                &mut sig,
            )
            .unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig))
    }

    /// An ES256 ID token with `claims`.
    pub fn es256(&self, claims: &Value) -> String {
        let input = Self::encode(&json!({ "alg": "ES256", "kid": "ec-1" }), claims);
        let sig = self
            .ec
            .sign(&SystemRandom::new(), input.as_bytes())
            .unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
    }

    /// Serves the key set at `at`.
    pub async fn mount_jwks(&self, at: &str) {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(200).set_body_json(self.jwks()))
            .mount(&self.server)
            .await;
    }

    /// Answers `POST at` with `status` and `body`.
    pub async fn mount_post(&self, at: &str, status: u16, body: Value) {
        Mock::given(method("POST"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    /// Answers the next `POST at` (once) with `status` and `body`.
    pub async fn mount_post_once(&self, at: &str, status: u16, body: Value) {
        Mock::given(method("POST"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .up_to_n_times(1)
            .mount(&self.server)
            .await;
    }

    /// Answers the next `GET at` (once) with `status` and `body`.
    pub async fn mount_get_once(&self, at: &str, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .up_to_n_times(1)
            .mount(&self.server)
            .await;
    }

    /// Answers `GET at` with `status` and `body`.
    pub async fn mount_get(&self, at: &str, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    /// The requests received at `at`.
    pub async fn requests_to(&self, at: &str) -> Vec<Request> {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path() == at)
            .collect()
    }
}
