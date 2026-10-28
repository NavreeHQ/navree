//! Helpers shared by the navree-auth integration tests: app builders, a probe
//! controller showing who a request is authenticated as, and an audit sink
//! that records entries in memory.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::http::Method;
use navree_access::{AccessModule, AccessOptions, AccessService};
use navree_auth::{
    AuthModule, AuthService, ClientMeta, CurrentUser, User, not_while_impersonating,
    platform_session, recent_auth,
};
use navree_contracts::{AuditEntry, AuditSink, AuthState, BoxError, BoxFuture, Clock, FixedClock};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::DatabaseModule;
use navree_db::Db;
use navree_http::{Controller, CurrentPrincipal, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_mail::{MailModule, MailOptions, Mailer, Message, TransportConfig};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_view::{ViewModule, ViewOptions};
use serde_json::{Value, json};

/// A strong enough password for every test.
pub const PASSWORD: &str = "correct horse battery";

/// A test app with crypto (fast hashing), a fresh database and the auth
/// module read from configuration, plus the probe routes.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(ProbeModule)
}

/// [`builder`], booted.
pub async fn app() -> TestApp {
    builder().build().await
}

/// [`builder`] plus ViewModule and MailModule with the array transport, so
/// the account flows send mail the tests can read with [`sent_mail`].
pub fn mail_builder() -> TestAppBuilder {
    builder()
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailModule::for_root(MailOptions {
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
}

/// Every message the array transport recorded so far.
pub async fn sent_mail(app: &TestApp) -> Vec<Message> {
    app.get::<Mailer>()
        .await
        .array_transport()
        .expect("the array transport")
        .sent()
}

/// The value of query parameter `name` in the first link of `text` that
/// carries it (URL-decoded).
pub fn link_param(text: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=");
    let start = text.find(&marker)? + marker.len();
    let raw: String = text[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '&' && *c != '"' && *c != '<')
        .collect();
    Some(
        form_urlencoded::parse(format!("v={raw}").as_bytes())
            .next()
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default(),
    )
}

/// The first `http(s)://` URL in `text`.
pub fn first_url(text: &str) -> Option<String> {
    let start = text.find("http://").or_else(|| text.find("https://"))?;
    Some(
        text[start..]
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '<')
            .collect(),
    )
}

/// Creates a user through the service (as a seeder would).
pub async fn create_user(app: &TestApp, email: &str) -> User {
    app.get::<AuthService>()
        .await
        .create_user("Ada Lovelace", email, PASSWORD)
        .await
        .expect("the user is created")
}

/// `POST /api/auth/login`.
pub async fn login(client: &TestClient, email: &str, password: &str) -> TestResponse {
    client
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await
}

/// The access and refresh tokens of a token response.
pub fn tokens(response: &TestResponse) -> (String, String) {
    let body = response.json_value();
    let access = body["access_token"]
        .as_str()
        .expect("access_token is a string")
        .to_owned();
    let refresh = body["refresh_token"]
        .as_str()
        .expect("refresh_token is a string")
        .to_owned();
    (access, refresh)
}

/// Creates `email` and logs in: the access and refresh tokens.
pub async fn signed_in(app: &TestApp, email: &str) -> (User, String, String) {
    let user = create_user(app, email).await;
    let response = login(&app.client(), email, PASSWORD)
        .await
        .assert_status(200);
    let (access, refresh) = tokens(&response);
    (user, access, refresh)
}

/// `POST /api/auth/refresh`.
pub async fn refresh(client: &TestClient, refresh_token: &str) -> TestResponse {
    client
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
}

/// Records audit entries in memory.
#[derive(Default)]
pub struct RecordingSink(Mutex<Vec<AuditEntry>>);

impl RecordingSink {
    /// Every entry recorded so far.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The entries with `action`.
    pub fn with_action(&self, action: &str) -> Vec<AuditEntry> {
        self.entries()
            .into_iter()
            .filter(|entry| entry.action == action)
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

/// Routes showing how a request is authenticated, under `/api/probe`.
pub struct ProbeController;

impl Injectable for ProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProbeController) })
    }
}

async fn whoami(CurrentPrincipal(principal): CurrentPrincipal) -> Json<Value> {
    Json(json!({
        "user_id": principal.user_id,
        "via": format!("{:?}", principal.via),
        "token_abilities": principal.token_abilities,
    }))
}

async fn ping(state: axum::Extension<AuthState>) -> Json<Value> {
    Json(json!({ "pong": true, "guest": state.0.0.is_none() }))
}

async fn current_user(CurrentUser(user): CurrentUser) -> Json<Value> {
    Json(json!({ "id": user.id, "email": user.email }))
}

async fn actor(actor: navree_auth::Actor) -> Json<Value> {
    Json(json!({
        "user_id": actor.user_id,
        "impersonator_id": actor.impersonator_id,
        "impersonating": actor.is_impersonating(),
    }))
}

impl Controller for ProbeController {
    fn prefix(&self) -> &'static str {
        "/probe"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/whoami", whoami).authenticated(),
            RouteDef::on(Method::GET, "/ping", ping).public(),
            RouteDef::on(Method::GET, "/user", current_user).authenticated(),
            RouteDef::on(Method::GET, "/actor", actor).authenticated(),
            RouteDef::on(Method::POST, "/sensitive", || async {
                Json(json!({"ok": true}))
            })
            .authenticated()
            .guard(recent_auth(Duration::from_secs(600))),
            RouteDef::on(Method::POST, "/no-impersonation", || async {
                Json(json!({"ok": true}))
            })
            .authenticated()
            .guard(not_while_impersonating()),
            RouteDef::on(Method::GET, "/platform", || async {
                Json(json!({"ok": true}))
            })
            .authenticated()
            .guard(platform_session()),
            RouteDef::on(
                Method::GET,
                "/platform-admin",
                |navree_auth::PlatformAdmin(admin): navree_auth::PlatformAdmin| async move {
                    Json(json!({ "email": admin.email }))
                },
            )
            .authenticated(),
        ]
    }
}

/// Mounts [`ProbeController`].
pub struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ProbeController>();
    }
}

/// A fixed clock at the current time.
pub fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(chrono::Utc::now()))
}

/// [`builder`] with `clock` as the app's clock.
pub fn clocked(clock: &Arc<FixedClock>) -> TestAppBuilder {
    builder().override_value::<dyn Clock>(clock.clone())
}

/// [`builder`] with `clock` and `AccessModule` (roles and permissions).
pub fn with_access(clock: &Arc<FixedClock>) -> TestAppBuilder {
    clocked(clock).module(AccessModule::for_root(AccessOptions::default()))
}

/// Creates `role` with `permissions` (when it does not exist) and gives it
/// to `user`.
pub async fn grant_role(app: &TestApp, user: uuid::Uuid, role: &str, permissions: &[&str]) {
    let db = app.get::<Db>().await;
    let access = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    if access.role(role).await.unwrap().is_none() {
        access.create_role(&uow, role, None).await.unwrap();
    }
    for permission in permissions {
        access.grant(&uow, role, permission).await.unwrap();
    }
    access.assign_role(&uow, user, role).await.unwrap();
    uow.commit().await.unwrap();
}

/// The TOTP code of `secret` (base32) at `clock`'s time.
pub fn totp_code(secret: &str, clock: &FixedClock) -> String {
    use navree_crypto::totp::{Secret, Totp};
    Totp::builder(Secret::from_base32(secret).expect("a base32 secret"))
        .build()
        .expect("a valid TOTP")
        .generate(u64::try_from(clock.now().timestamp()).unwrap())
}

/// Turns two-factor authentication on for `user` through the service; the
/// TOTP secret. The clock moves to the next step (the code is used up).
pub async fn enable_two_factor(app: &TestApp, clock: &FixedClock, user: &User) -> String {
    let service = app.get::<AuthService>().await;
    let meta = ClientMeta::default();
    let setup = service.enable_two_factor(user.id, &meta).await.unwrap();
    service
        .confirm_two_factor(user.id, &totp_code(&setup.secret, clock), &meta)
        .await
        .unwrap();
    clock.advance(Duration::from_secs(30));
    setup.secret
}

/// The claims of an access token.
pub fn claims_of(app_jwt: &navree_auth::JwtService, token: &str) -> navree_auth::Claims {
    app_jwt
        .verify_any(
            token,
            &[
                navree_auth::TokenType::Access,
                navree_auth::TokenType::TwoFactorSetup,
                navree_auth::TokenType::Platform,
            ],
        )
        .expect("a valid access token")
}
