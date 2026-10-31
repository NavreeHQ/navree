//! # crm-testkit
//!
//! A booted Navree CRM for tests, in one line:
//!
//! ```no_run
//! # use crm_testkit::{Role, TestCrm};
//! # use navree::prelude::*;
//! # #[module] struct DealsModule;
//! # async fn demo() {
//! let crm = TestCrm::builder().module(DealsModule).build().await;
//! let rep = crm.login_as(Role::Rep1).await;
//! rep.get("/api/v1/deals").send().await.assert_status(200);
//! # }
//! ```
//!
//! [`TestCrm::builder`] boots the CRM platform ([`crm_core::platform`]) and
//! [`CrmCoreModule`] plus the feature modules you add, on a temporary
//! SQLite database (migrated on boot), with hermetic settings: mail kept in
//! memory (`array` transport), the database queue driver, fast password
//! hashing, no per-IP rate limit, no metrics recorder, CORS origin
//! [`WEB_ORIGIN`], and a [`FixedClock`] as the app's `dyn Clock` (starting
//! at [`TestCrm::start`]; move it with [`TestCrm::clock`]). The roles and
//! the four demo accounts ([`Role`]) are seeded before `build` returns.
//!
//! [`TestCrm::login_as`] signs in through the real `POST /api/auth/login`
//! and returns a [`CrmClient`] whose requests carry the access token.
//! Access tokens live 15 minutes of the fixed clock: log in again after
//! advancing it further.
#![forbid(unsafe_code)]

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use crm_core::seed::{DEMO_ACCOUNTS, DEMO_PASSWORD, DemoAccount, ensure_demo_accounts};
use crm_core::{CrmCoreModule, UserRole, platform};
use navree::ModuleId;
use navree::auth::User;
use navree::contracts::Clock;
use navree::prelude::*;
use navree::queue::{Queue, WorkStats};
use navree::testing::{
    FixedClock, TempDirGuard, TestApp, TestClient, TestRequest, temp_sqlite_url,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// The origin of crm-web in development: the CORS origin every test
/// request carries.
pub const WEB_ORIGIN: &str = "http://localhost:3000";

/// One of the four demo accounts (password `password123`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// `admin@navree.test`, role `admin` (super-admin).
    Admin,
    /// `manager@navree.test`, role `manager`.
    Manager,
    /// `rep1@navree.test`, role `rep`.
    Rep1,
    /// `rep2@navree.test`, role `rep`.
    Rep2,
}

impl Role {
    /// Every demo account.
    pub const ALL: [Role; 4] = [Role::Admin, Role::Manager, Role::Rep1, Role::Rep2];

    /// The account's name, email and CRM role.
    pub fn account(self) -> DemoAccount {
        DEMO_ACCOUNTS[self as usize]
    }

    /// The account's email address.
    pub fn email(self) -> &'static str {
        self.account().email
    }

    /// The account's display name.
    pub fn name(self) -> &'static str {
        self.account().name
    }

    /// The account's CRM role (`admin`, `manager` or `rep`).
    pub fn role(self) -> UserRole {
        self.account().role
    }
}

/// A booted CRM with the demo accounts; see the [crate docs](crate).
pub struct TestCrm {
    app: TestApp,
    clock: Arc<FixedClock>,
    users: Vec<User>,
    /// The database directory, removed after the app.
    _db: TempDirGuard,
}

impl TestCrm {
    /// Starts building a CRM for a test.
    pub fn builder() -> TestCrmBuilder {
        TestCrmBuilder::default()
    }

    /// Where the fixed clock starts unless [`TestCrmBuilder::now`] says
    /// otherwise: Monday 2026-10-26, 10:00 UTC.
    pub fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, 10, 0, 0)
            .single()
            .expect("a valid instant")
    }

    /// Signs in as `role`'s demo account through `POST /api/auth/login`.
    ///
    /// # Panics
    ///
    /// When the login does not answer 200 with a token pair.
    pub async fn login_as(&self, role: Role) -> CrmClient {
        let body = self
            .app
            .client()
            .post("/api/auth/login")
            .json(&json!({ "email": role.email(), "password": DEMO_PASSWORD }))
            .send()
            .await
            .assert_status(200)
            .json_value();
        let token = |key: &str| {
            body[key]
                .as_str()
                .unwrap_or_else(|| panic!("the login response has no `{key}`: {body}"))
                .to_owned()
        };
        CrmClient {
            client: self.app.client(),
            user: self.user(role).clone(),
            role,
            access_token: token("access_token"),
            refresh_token: token("refresh_token"),
        }
    }

    /// The demo account of `role`.
    pub fn user(&self, role: Role) -> &User {
        &self.users[role as usize]
    }

    /// The id of `role`'s demo account.
    pub fn user_id(&self, role: Role) -> Uuid {
        self.user(role).id
    }

    /// The app's clock (also its `dyn Clock`): `set` or `advance` it.
    pub fn clock(&self) -> &FixedClock {
        &self.clock
    }

    /// Runs every queued job that is due (by the fixed clock), as a worker
    /// would, and returns how many succeeded and failed.
    ///
    /// # Panics
    ///
    /// When the queue cannot be read.
    pub async fn run_queue(&self) -> WorkStats {
        self.get::<Queue>()
            .await
            .run_pending()
            .await
            .unwrap_or_else(|error| panic!("the queue failed to run: {error}"))
    }

    /// A client without credentials (a guest).
    pub fn client(&self) -> TestClient {
        self.app.client()
    }

    /// Resolves the provider of `T` (ignoring visibility).
    ///
    /// # Panics
    ///
    /// When `T` cannot be resolved.
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.app.get::<T>().await
    }

    /// The app context.
    pub fn ctx(&self) -> &AppContext {
        self.app.ctx()
    }

    /// The underlying [`TestApp`] (commands, providers, clients).
    pub fn app(&self) -> &TestApp {
        &self.app
    }

    /// Shuts the app down and deletes its database. Prefer it to dropping:
    /// on Windows an open pool keeps the database files.
    ///
    /// # Panics
    ///
    /// When a shutdown hook fails.
    pub async fn shutdown(self) {
        let TestCrm { app, _db, .. } = self;
        app.shutdown()
            .await
            .unwrap_or_else(|error| panic!("the CRM failed to shut down: {error}"));
    }
}

/// Builds a [`TestCrm`]: the app, feature modules, configuration overrides
/// and the clock's start.
pub struct TestCrmBuilder {
    app: fn(ConfigSources) -> AppBuilder,
    modules: Vec<Box<dyn Module>>,
    config: Vec<(String, Value)>,
    now: DateTime<Utc>,
}

impl Default for TestCrmBuilder {
    fn default() -> Self {
        Self {
            app: platform_only,
            modules: Vec::new(),
            config: Vec::new(),
            now: TestCrm::start(),
        }
    }
}

/// The default app: the platform modules alone.
fn platform_only(sources: ConfigSources) -> AppBuilder {
    platform(Navree::app(), sources)
}

impl TestCrmBuilder {
    /// Boots `app(sources)` instead of the platform alone, e.g. the whole
    /// CRM API for end-to-end tests: `TestCrm::builder().app(crm_api::app)`.
    /// `app` must build on [`platform`] with the sources it is given (the
    /// hermetic test settings); the fixed clock, [`CrmCoreModule`] and any
    /// [`module`](Self::module) are added on top.
    pub fn app(mut self, app: fn(ConfigSources) -> AppBuilder) -> Self {
        self.app = app;
        self
    }

    /// Adds a feature module (with whatever it imports).
    pub fn module(mut self, module: impl Module) -> Self {
        self.modules.push(Box::new(module));
        self
    }

    /// Sets configuration key `key` (dotted path) above every default.
    pub fn config(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.config.push((key.to_owned(), value.into()));
        self
    }

    /// Starts the fixed clock at `now` instead of [`TestCrm::start`].
    pub fn now(mut self, now: DateTime<Utc>) -> Self {
        self.now = now;
        self
    }

    /// Boots the CRM and seeds the roles and demo accounts.
    ///
    /// # Panics
    ///
    /// When the app fails to boot or the seeding fails.
    pub async fn build(self) -> TestCrm {
        let (url, db) = temp_sqlite_url();
        let mut sources = ConfigSources::new()
            .set("app.key", navree::crypto::generate_key())
            .set("database.url", url)
            .set("database.migrate_on_boot", true)
            .set("http.per_ip_rate_limit", Value::Null)
            .set("http.cors.allowed_origins", json!([WEB_ORIGIN]))
            .set("mail.transport", "array")
            .set("queue.driver", "database")
            .set("hashing.memory_kib", 1024)
            .set("hashing.iterations", 1)
            .set("observe.metrics", false)
            .set("log.level", "warn");
        for (key, value) in self.config {
            sources = sources.set(&key, value);
        }
        let clock = Arc::new(FixedClock::new(self.now));
        let mut builder = (self.app)(sources)
            .module(CrmCoreModule)
            .override_value::<dyn Clock>(clock.clone());
        for module in self.modules {
            builder = builder.module(Boxed(module));
        }
        let app = TestApp::from_builder(builder).await;
        let users = ensure_demo_accounts(app.ctx())
            .await
            .unwrap_or_else(|error| panic!("the demo accounts failed to seed: {error}"));
        TestCrm {
            app,
            clock,
            users,
            _db: db,
        }
    }
}

/// A client signed in as a demo account: every request carries
/// `Authorization: Bearer <access token>`.
#[derive(Clone)]
pub struct CrmClient {
    client: TestClient,
    user: User,
    role: Role,
    access_token: String,
    refresh_token: String,
}

impl CrmClient {
    /// A request with `method` to `path`, with the bearer token.
    pub fn request(&self, method: http::Method, path: &str) -> TestRequest {
        self.client.request(method, path).bearer(&self.access_token)
    }

    /// A `GET` request with the bearer token.
    pub fn get(&self, path: &str) -> TestRequest {
        self.client.get(path).bearer(&self.access_token)
    }

    /// A `POST` request with the bearer token.
    pub fn post(&self, path: &str) -> TestRequest {
        self.client.post(path).bearer(&self.access_token)
    }

    /// A `PUT` request with the bearer token.
    pub fn put(&self, path: &str) -> TestRequest {
        self.client.put(path).bearer(&self.access_token)
    }

    /// A `PATCH` request with the bearer token.
    pub fn patch(&self, path: &str) -> TestRequest {
        self.client.patch(path).bearer(&self.access_token)
    }

    /// A `DELETE` request with the bearer token.
    pub fn delete(&self, path: &str) -> TestRequest {
        self.client.delete(path).bearer(&self.access_token)
    }

    /// The signed-in user's id.
    pub fn id(&self) -> Uuid {
        self.user.id
    }

    /// The signed-in user.
    pub fn user(&self) -> &User {
        &self.user
    }

    /// The demo account signed in.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The access token (JWT).
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// The refresh token.
    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    /// The underlying client (no bearer token on its requests).
    pub fn inner(&self) -> &TestClient {
        &self.client
    }
}

impl std::fmt::Debug for CrmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrmClient")
            .field("role", &self.role)
            .field("user", &self.user.id)
            .finish_non_exhaustive()
    }
}

/// A boxed module, so modules collected by the builder can be handed to
/// `AppBuilder::module` (same id, so imports of the same module dedupe).
struct Boxed(Box<dyn Module>);

impl Module for Boxed {
    fn id(&self) -> ModuleId {
        self.0.id()
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn is_configured(&self) -> bool {
        self.0.is_configured()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        self.0.register(m);
    }
}
