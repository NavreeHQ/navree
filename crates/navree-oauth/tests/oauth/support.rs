//! Shared fixtures: an app with auth, access, a `deals` subject with probe
//! routes, and the OAuth server; users with roles; clients; an audit sink
//! that records in memory.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, TimeZone, Utc};
use http::Method;
use navree_access::{AccessModule, AccessOptions, AccessService};
use navree_auth::{AuthModule, AuthService, User};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture, Clock, FixedClock};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::{DatabaseModule, Db, Uow};
use navree_http::{AppError, Controller, CurrentPrincipal, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_oauth::{CreatedClient, NewClient, OAuthClients, OAuthServerModule, OAuthToken};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use serde_json::{Value, json};
use uuid::Uuid;

/// A strong enough password for every test.
pub const PASSWORD: &str = "correct horse battery";
/// The issuer of the test apps.
pub const ISSUER: &str = "https://api.acme.test";
/// The consent page.
pub const CONSENT_URL: &str = "https://app.acme.test/oauth/consent";
/// The device page.
pub const DEVICE_URL: &str = "https://app.acme.test/device";
/// The redirect URI of the test clients.
pub const REDIRECT_URI: &str = "https://zapier.test/oauth/return";

/// When every test app's clock starts.
pub fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap()
}

/// The `deals` subject (no table: the probe routes only check access).
pub mod deal {
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
    pub struct Model {
        pub id: Uuid,
        pub title: String,
    }
}

/// Probe routes under `/api/probe`.
pub struct ProbeController;

impl Injectable for ProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProbeController) })
    }
}

async fn whoami(
    CurrentPrincipal(principal): CurrentPrincipal,
    token: Option<OAuthToken>,
) -> Json<Value> {
    Json(json!({
        "user_id": principal.user_id,
        "token_abilities": principal.token_abilities,
        "oauth": token.map(|token| json!({
            "client_id": token.client_id,
            "client_name": token.client_name,
            "scopes": token.scopes,
            "grant": token.grant,
            "user_id": token.user_id,
            "workspace_id": token.workspace_id,
        })),
    }))
}

async fn list_deals() -> Json<Value> {
    Json(json!({ "data": [] }))
}

async fn create_deal() -> Json<Value> {
    Json(json!({ "created": true }))
}

async fn export(token: Option<OAuthToken>) -> Result<Json<Value>, AppError> {
    if let Some(token) = &token {
        token.require_scope("deals:export")?;
    }
    Ok(Json(json!({ "exported": true })))
}

async fn needs_token(token: OAuthToken) -> Json<Value> {
    Json(json!({ "grant": token.grant }))
}

impl Controller for ProbeController {
    fn prefix(&self) -> &'static str {
        "/probe"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/whoami", whoami).authenticated(),
            RouteDef::on(Method::GET, "/deals", list_deals).permissions(&["deals.read"]),
            RouteDef::on(Method::POST, "/deals", create_deal).permissions(&["deals.create"]),
            RouteDef::on(Method::PATCH, "/deals", create_deal).can("update", "Deal", None),
            RouteDef::on(Method::POST, "/export", export).authenticated(),
            RouteDef::on(Method::GET, "/token", needs_token).authenticated(),
        ]
    }
}

/// The probe routes and the `Deal` subject.
pub struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<deal::Model>())
            .controller::<ProbeController>();
    }
}

/// The app's scopes.
pub fn scopes() -> Value {
    json!({
        "deals:read": { "description": "Read your deals", "permissions": ["deals.read"] },
        "deals:write": {
            "description": "Create, update and move your deals",
            "permissions": ["deals.read", "deals.create", "deals.update"]
        },
        "deals:export": { "description": "Export your deals", "permissions": ["deals.read"] },
        "contacts:read": { "description": "Read your contacts", "permissions": ["contacts.read"] },
        "everything": { "description": "Do anything you can", "permissions": ["*"] }
    })
}

/// A test app: crypto (fast hashing), a fresh database, auth, access with
/// the default abilities, the probe routes and the OAuth server.
pub fn builder() -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("app.url", ISSUER)
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("oauth.consent_url", CONSENT_URL)
        .config("oauth.device_url", DEVICE_URL)
        .config("oauth.scopes", scopes())
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(ProbeModule)
        .module(OAuthServerModule::from_config())
}

/// Records audit entries in memory.
#[derive(Default)]
pub struct RecordingSink(Mutex<Vec<AuditEntry>>);

impl RecordingSink {
    /// The entries with `action`.
    pub fn with_action(&self, action: &str) -> Vec<AuditEntry> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|entry| entry.action == action)
            .cloned()
            .collect()
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

/// A booted test app with its clock and audit sink.
pub struct World {
    pub app: TestApp,
    pub clock: Arc<FixedClock>,
    pub audit: Arc<RecordingSink>,
}

impl World {
    pub async fn new() -> World {
        World::with(builder()).await
    }

    pub async fn with(builder: TestAppBuilder) -> World {
        let clock = Arc::new(FixedClock::new(start()));
        let audit = Arc::new(RecordingSink::default());
        let app = builder
            .override_value::<dyn Clock>(clock.clone())
            .override_value::<dyn AuditSink>(audit.clone())
            .build()
            .await;
        World { app, clock, audit }
    }

    pub fn client(&self) -> TestClient {
        self.app.client()
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    /// Runs `f` in a new unit of work and commits it.
    pub async fn work<T, F, Fut>(&self, f: F) -> T
    where
        F: FnOnce(Uow) -> Fut,
        Fut: std::future::Future<Output = Result<T, BoxError>>,
    {
        let uow = self.db().await.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await.unwrap();
                value
            }
            Err(error) => {
                uow.rollback().await.unwrap();
                panic!("work failed: {error}");
            }
        }
    }

    /// Creates `email` with `permissions` (through a role of the same
    /// name as the email's local part) and signs in: the user and a JWT.
    pub async fn user(&self, email: &str, permissions: &[&str]) -> (User, String) {
        let auth = self.app.get::<AuthService>().await;
        let user = auth
            .create_user("Ada Lovelace", email, PASSWORD)
            .await
            .expect("the user is created");
        let access = self.app.get::<AccessService>().await;
        let role = email.split('@').next().unwrap().replace('.', "-");
        let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
        let user_id = user.id;
        self.work(|uow| async move {
            access.create_role(&uow, &role, None).await?;
            for permission in &permissions {
                access.grant(&uow, &role, permission).await?;
            }
            access.assign_role(&uow, user_id, &role).await?;
            Ok(())
        })
        .await;
        let token = self.login(email).await;
        (user, token)
    }

    /// Signs `email` in: a JWT.
    pub async fn login(&self, email: &str) -> String {
        let response = self
            .client()
            .post("/api/auth/login")
            .json(&json!({ "email": email, "password": PASSWORD }))
            .send()
            .await
            .assert_status(200);
        response.json_value()["access_token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Registers `new`.
    pub async fn register(&self, new: NewClient) -> CreatedClient {
        let clients = self.app.get::<OAuthClients>().await;
        self.work(|uow| async move { Ok(clients.create(&uow, new).await?) })
            .await
    }

    /// A public client with [`REDIRECT_URI`] and `scopes`.
    pub async fn public_client(&self, scopes: &[&str]) -> CreatedClient {
        self.register(
            NewClient::public("Acme mobile")
                .redirect_uri(REDIRECT_URI)
                .scopes(scopes.iter().copied()),
        )
        .await
    }

    /// A confidential client with [`REDIRECT_URI`] and `scopes`.
    pub async fn confidential_client(&self, scopes: &[&str]) -> CreatedClient {
        self.register(
            NewClient::confidential("Zapier")
                .redirect_uri(REDIRECT_URI)
                .website("https://zapier.test")
                .scopes(scopes.iter().copied()),
        )
        .await
    }

    /// `POST {path}` with a form body.
    pub async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> TestResponse {
        self.client().post(path).form(form).send().await
    }

    /// `GET /oauth/authorize` with `params`: the response.
    pub async fn authorize(&self, params: &[(&str, &str)]) -> TestResponse {
        self.client()
            .get("/oauth/authorize")
            .query(params)
            .send()
            .await
    }

    /// Starts an authorization request for `client_id` with `scope` and the
    /// PKCE `challenge`: the request id from the consent redirect.
    pub async fn request(&self, client_id: Uuid, scope: &str, challenge: &str) -> String {
        let id = client_id.to_string();
        let response = self
            .authorize(&[
                ("response_type", "code"),
                ("client_id", &id),
                ("redirect_uri", REDIRECT_URI),
                ("scope", scope),
                ("state", "af0ifjsldkj"),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
            ])
            .await;
        assert_eq!(response.status(), 302, "{}", response.text());
        let location = response.header("location").unwrap().to_owned();
        assert!(location.starts_with(CONSENT_URL), "{location}");
        param(&location, "request").expect("a request id")
    }

    /// Approves request `id` as `token`'s user: the code.
    pub async fn approve(&self, id: &str, token: &str) -> String {
        let response = self
            .client()
            .post(&format!("/api/oauth/requests/{id}/approve"))
            .bearer(token)
            .send()
            .await
            .assert_status(200);
        let redirect_to = response.json_value()["redirect_to"]
            .as_str()
            .unwrap()
            .to_owned();
        param(&redirect_to, "code").expect("a code")
    }

    /// The whole authorization code flow for a public client: a token
    /// response.
    pub async fn code_flow(&self, client_id: Uuid, user_token: &str, scope: &str) -> Value {
        let (verifier, challenge) = navree_oauth::testing::pkce_pair();
        let request = self.request(client_id, scope, &challenge).await;
        let code = self.approve(&request, user_token).await;
        let id = client_id.to_string();
        self.post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", &verifier),
                ("client_id", &id),
            ],
        )
        .await
        .assert_status(200)
        .json_value()
    }
}

/// Query parameter `name` of `url`.
pub fn param(url: &str, name: &str) -> Option<String> {
    navree_oauth::testing::query_param(url, name)
}

/// The string at `key` of `value`.
pub fn string(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} missing in {value}"))
        .to_owned()
}
