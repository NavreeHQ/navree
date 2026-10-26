//! [`TestApp`] and [`TestAppBuilder`].

use std::sync::Arc;

use http::HeaderValue;
use navis_config::{ConfigModule, ConfigSources};
use navis_contracts::Principal;
use navis_http::{HttpModule, HttpServer, build_router};
use navis_kernel::{App, AppBuilder, AppContext, BoxError, Module, ModuleBuilder, ModuleId};

use crate::auth::TestAuthenticator;
use crate::client::Target;
use crate::{TempDirGuard, TestClient, temp_sqlite_url};

/// A booted app under test.
///
/// Build one with [`TestApp::builder`] (modules, configuration, a temporary
/// database, overrides) or boot an app root built elsewhere with
/// [`TestApp::from_builder`]. Then send requests with [`client`](TestApp::client)
/// or [`acting_as`](TestApp::acting_as), resolve providers with
/// [`get`](TestApp::get) and run commands with
/// [`run_command`](TestApp::run_command).
///
/// When the app registers no `dyn Authenticator`, a
/// [`TestAuthenticator`] is installed, so routes that need a signed-in user
/// boot and answer 401 to guests and 200 to [`acting_as`](TestApp::acting_as)
/// clients.
///
/// Dropping the app cancels its shutdown token and deletes its temporary
/// directories; [`shutdown`](TestApp::shutdown) also runs the shutdown hooks.
pub struct TestApp {
    app: App,
    /// What its clients share; `None` without `HttpModule`.
    target: Option<Arc<Target>>,
    /// Temporary directories (databases) removed with the app; declared
    /// last so they are dropped after it.
    temp: Vec<TempDirGuard>,
}

impl TestApp {
    /// Starts building a test app.
    pub fn builder() -> TestAppBuilder {
        TestAppBuilder::default()
    }

    /// Boots an app whose root the caller built (with its own
    /// `ConfigModule`; use [`temp_sqlite_url`] for its database). Nothing
    /// is added except a [`TestAuthenticator`] when the app has no
    /// authenticator.
    ///
    /// ```no_run
    /// # async fn demo() {
    /// use navis_config::{ConfigModule, ConfigSources};
    /// use navis_http::HttpModule;
    /// use navis_kernel::Navis;
    /// use navis_testing::{TestApp, temp_sqlite_url};
    ///
    /// let (url, _db) = temp_sqlite_url();
    /// let sources = ConfigSources::new().set("database.url", url);
    /// let app = TestApp::from_builder(
    ///     Navis::app()
    ///         .module(ConfigModule::for_root(sources))
    ///         .module(HttpModule),
    /// )
    /// .await;
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// When the app fails to boot.
    pub async fn from_builder(builder: AppBuilder) -> TestApp {
        Self::boot(builder)
            .await
            .unwrap_or_else(|error| panic!("the test app failed to boot:\n{error}"))
    }

    async fn boot(builder: AppBuilder) -> Result<TestApp, BoxError> {
        let builder = if TestAuthenticator::is_missing(&builder) {
            TestAuthenticator::install(builder)
        } else {
            builder
        };
        let app = builder.boot().await?;
        let ctx = app.ctx();
        let target = match ctx.try_get::<HttpServer>().await {
            Some(server) => Some(Arc::new(Target {
                router: build_router(ctx).await?,
                origin: server
                    .options()
                    .cors
                    .allowed_origins
                    .first()
                    .and_then(|origin| HeaderValue::from_str(origin.trim()).ok()),
                test_auth: TestAuthenticator::is_active(ctx).await,
            })),
            None => None,
        };
        Ok(TestApp {
            app,
            target,
            temp: Vec::new(),
        })
    }

    /// Resolves the provider of `T` (ignoring visibility, like
    /// [`AppContext::get`]).
    ///
    /// # Panics
    ///
    /// When `T` cannot be resolved.
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.ctx().get::<T>().await.unwrap_or_else(|error| {
            panic!(
                "TestApp::get::<{}>() failed: {error}",
                std::any::type_name::<T>()
            )
        })
    }

    /// Runs the registered command `name` with `args` against this app (as
    /// `navis <name> <args...>` would, without booting a second app).
    pub async fn run_command(&self, name: &str, args: &[&str]) -> Result<(), BoxError> {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.app.run_command(name, args).await
    }

    /// Shuts the app down gracefully (cancels the shutdown token, waits for
    /// tracked tasks, runs the `on_shutdown` hooks), then deletes its
    /// temporary directories. Dropping a `TestApp` only cancels the token.
    pub async fn shutdown(self) -> Result<(), BoxError> {
        let TestApp { app, target, temp } = self;
        drop(target);
        let result = app.shutdown().await;
        drop(temp);
        result
    }

    /// The app context.
    pub fn ctx(&self) -> &AppContext {
        self.app.ctx()
    }

    /// A new client with an empty cookie jar.
    ///
    /// # Panics
    ///
    /// When the app has no `HttpModule`.
    pub fn client(&self) -> TestClient {
        let target = self
            .target
            .clone()
            .expect("TestApp::client needs HttpModule in the app");
        TestClient::new(target)
    }

    /// A new client (empty cookie jar) whose requests are authenticated as
    /// `principal`; see [`TestClient::acting_as`].
    ///
    /// # Panics
    ///
    /// When the app has no `HttpModule`, or authenticates with an
    /// authenticator of its own (see [`TestAppBuilder::fake_auth`]).
    pub fn acting_as(&self, principal: Principal) -> TestClient {
        self.client().acting_as(principal)
    }
}

impl std::fmt::Debug for TestApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestApp")
            .field("app", &self.app)
            .field("http", &self.target.is_some())
            .finish()
    }
}

/// Builds a [`TestApp`]: the root modules are
/// `ConfigModule::for_root(sources)` (no sources unless
/// [`sources`](TestAppBuilder::sources) says otherwise, plus every
/// [`config`](TestAppBuilder::config) value), `HttpModule` (a configured
/// `HttpModule::for_root(..)` added with [`module`](TestAppBuilder::module)
/// takes its place) and the added modules, in order. Do not add a
/// `ConfigModule` yourself: it would be configured twice.
#[derive(Default)]
pub struct TestAppBuilder {
    app: AppBuilder,
    modules: Vec<Box<dyn Module>>,
    sources: ConfigSources,
    /// `config` values, set over `sources` at build time.
    config: Vec<(String, serde_json::Value)>,
    fake_auth: bool,
    temp: Vec<TempDirGuard>,
}

impl TestAppBuilder {
    /// Replaces the base configuration sources. The default is none at all,
    /// so tests never read `.env`, `config/app.toml` or the process
    /// environment; [`config`](TestAppBuilder::config) values still go on top.
    pub fn sources(mut self, sources: ConfigSources) -> Self {
        self.sources = sources;
        self
    }

    /// Points `database.url` at a SQLite file in a fresh temporary
    /// directory (deleted with the app) and sets
    /// `database.migrate_on_boot = true`, so every app gets its own,
    /// migrated database.
    pub fn temp_database(mut self) -> Self {
        let (url, guard) = temp_sqlite_url();
        self.temp.push(guard);
        self.config("database.url", url)
            .config("database.migrate_on_boot", true)
    }

    /// Replaces the unnamed provider of `T` with `value` wherever it is
    /// registered (or registers it globally); see `AppBuilder::override_value`.
    pub fn override_value<T: ?Sized + Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.app = self.app.override_value(value);
        self
    }

    /// Sets configuration key `key` (a dotted path) to `value`, above every
    /// other source ([`ConfigSources::set`]); later calls win.
    pub fn config(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.config.push((key.to_owned(), value.into()));
        self
    }

    /// Adds a root module.
    pub fn module(mut self, m: impl Module) -> Self {
        self.modules.push(Box::new(m));
        self
    }

    /// Authenticates with the [`TestAuthenticator`] even if a module
    /// registers its own `dyn Authenticator`, so
    /// [`TestApp::acting_as`] works (and the app's own authentication does not).
    pub fn fake_auth(mut self) -> Self {
        self.fake_auth = true;
        self
    }

    /// Boots the app.
    ///
    /// # Panics
    ///
    /// When the app fails to boot.
    pub async fn build(self) -> TestApp {
        self.try_build()
            .await
            .unwrap_or_else(|error| panic!("the test app failed to boot:\n{error}"))
    }

    /// Boots the app, returning the boot error instead of panicking.
    pub async fn try_build(self) -> Result<TestApp, BoxError> {
        let sources = self
            .config
            .into_iter()
            .fold(self.sources, |sources, (key, value)| {
                sources.set(&key, value)
            });
        let mut builder = self
            .app
            .module(ConfigModule::for_root(sources))
            .module(HttpModule);
        for module in self.modules {
            builder = builder.module(Boxed(module));
        }
        if self.fake_auth {
            builder = TestAuthenticator::install(builder);
        }
        let mut app = TestApp::boot(builder).await?;
        app.temp = self.temp;
        Ok(app)
    }
}

impl std::fmt::Debug for TestAppBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestAppBuilder")
            .field(
                "modules",
                &self.modules.iter().map(|m| m.name()).collect::<Vec<_>>(),
            )
            .field("sources", &self.sources)
            .field(
                "config",
                &self.config.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            )
            .field("fake_auth", &self.fake_auth)
            .field("temp", &self.temp)
            .finish_non_exhaustive()
    }
}

/// A boxed module, so modules collected before the build can be handed to
/// `AppBuilder::module` in order.
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
