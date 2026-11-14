//! [`ClientModule`]: a client crate's client as a module of the app, and the
//! options it takes ([`ClientModuleOptions`]).

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_contracts::{HealthCheck, HealthStatus};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, KernelError, Lifecycle, Module, ModuleBuilder,
    ModuleId, PluginInfo, ProviderKey, Resolver,
};

use crate::{ConfigSection, ModuleBuilderConfigExt};

/// Builds the client from its options and a resolver.
type Connect<T, O> =
    Arc<dyn Fn(Arc<O>, Resolver) -> BoxFuture<'static, Result<T, BoxError>> + Send + Sync>;

/// A readiness check or a shutdown hook of the client.
type Hook<T> = Arc<dyn Fn(Arc<T>) -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync>;

/// A client crate as a module: its options section, the client built once at
/// boot, a readiness check, a shutdown hook. The client is exported to every
/// module as `T`, and under the module's name (`get_named::<T>(name)`), which
/// stays unambiguous when two modules build one client type.
///
/// The module is global. Its id is [`ModuleId::named`] of its type and name:
/// two `ClientModule`s of one client type with different names are two
/// modules, each building its own client, and the unnamed `T` is then
/// ambiguous, so each is reached by its name; two with one name are one
/// module, and the first one registered wins.
///
/// A plugin's modules are types of its own crate, or `ClientModule`s whose
/// options type is: `.plugin(navree::plugin_info!())` ties this module to the
/// crate that defines `O`.
///
/// ```no_run
/// use std::sync::atomic::{AtomicBool, Ordering};
///
/// use navree_config::{ClientModule, ConfigModule, ConfigSection, ConfigSources};
/// use navree_kernel::Navree;
///
/// #[derive(Clone, serde::Deserialize, ConfigSection)]
/// #[config(key = "weather")]
/// struct WeatherOptions {
///     feed: String,
/// }
///
/// // The client a weather crate gives.
/// struct Forecasts {
///     feed: String,
///     open: AtomicBool,
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     let feed = ClientModule::<Forecasts>::new("weather")
///         .options::<WeatherOptions>()
///         .connect(|options, _resolver| async move {
///             Ok(Forecasts {
///                 feed: options.feed.clone(),
///                 open: AtomicBool::new(true),
///             })
///         })
///         .health(|forecasts| async move {
///             if forecasts.feed.starts_with("https://mausam.example/") {
///                 Ok(())
///             } else {
///                 Err("the weather feed is not the mausam one".into())
///             }
///         })
///         .shutdown(|forecasts| async move {
///             forecasts.open.store(false, Ordering::SeqCst);
///             Ok(())
///         })
///         .plugin(navree_kernel::plugin_info!(namespace = "weather"));
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(feed)
///         .run()
/// }
/// ```
pub struct ClientModule<T, O = ()> {
    /// The module's name, the readiness check's and the client's export name.
    name: &'static str,
    /// Builds the client; a module without one fails the boot.
    connect: Option<Connect<T, O>>,
    /// The readiness check.
    health: Option<Hook<T>>,
    /// The hook run at graceful shutdown.
    shutdown: Option<Hook<T>>,
    /// The options given in code (`for_root`), in place of the section.
    given: Option<O>,
    /// The plugin the module names itself.
    plugin: Option<PluginInfo>,
}

impl<T: Send + Sync + 'static> ClientModule<T, ()> {
    /// A client module called `name`: the module's name in errors and
    /// reports, the name of its readiness check and the name the client is
    /// exported under. Without [`options`](ClientModule::options), the
    /// connect function gets `()` and no section is read.
    pub fn new(name: &'static str) -> ClientModule<T, ()> {
        ClientModule {
            name,
            connect: None,
            health: None,
            shutdown: None,
            given: None,
            plugin: None,
        }
    }
}

impl<T: Send + Sync + 'static, O: ClientModuleOptions> ClientModule<T, O> {
    /// Reads section `P::KEY` at boot (or takes `P` from code with
    /// [`for_root`](ClientModule::for_root)), and hands it to the connect
    /// function. Call it first: it keeps the name, the readiness check, the
    /// shutdown hook and the plugin, and drops a connect function or options
    /// given for the earlier options type.
    pub fn options<P: ConfigSection + Clone>(self) -> ClientModule<T, P> {
        ClientModule {
            name: self.name,
            connect: None,
            health: self.health,
            shutdown: self.shutdown,
            given: None,
            plugin: self.plugin,
        }
    }

    /// Builds the client once at boot, from the options and a resolver for
    /// the app's other providers. Required: without it the boot fails,
    /// naming the module. An error stops the boot with that error.
    pub fn connect<F, Fut>(mut self, connect: F) -> Self
    where
        F: Fn(Arc<O>, Resolver) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, BoxError>> + Send + 'static,
    {
        let build: Connect<T, O> = Arc::new(move |options, r| Box::pin(connect(options, r)));
        self.connect = Some(build);
        self
    }

    /// A readiness check named after the module: `Ok` is up, an error is
    /// down with its message. The message is shown in the readiness report,
    /// so it must hold no secret.
    pub fn health<F, Fut>(mut self, check: F) -> Self
    where
        F: Fn(Arc<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
    {
        let hook: Hook<T> = Arc::new(move |client| Box::pin(check(client)));
        self.health = Some(hook);
        self
    }

    /// Runs at graceful shutdown, after the app's tracked tasks have
    /// drained. An error is reported with the module's name; its message
    /// must hold no secret.
    pub fn shutdown<F, Fut>(mut self, close: F) -> Self
    where
        F: Fn(Arc<T>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
    {
        let hook: Hook<T> = Arc::new(move |client| Box::pin(close(client)));
        self.shutdown = Some(hook);
        self
    }

    /// The options in code: the section is not read.
    pub fn for_root(mut self, options: O) -> Self {
        self.given = Some(options);
        self
    }
}

impl<T: Send + Sync + 'static, O: ConfigSection + Clone> ClientModule<T, O> {
    /// Names the module a plugin. The options type, a type of the plugin's
    /// crate, ties the module to it; the boot fails when it is another
    /// crate's.
    ///
    /// `.plugin(..)` needs an options type: without one, no type of the
    /// plugin's crate would tie the module to it.
    ///
    /// ```compile_fail
    /// use navree_config::ClientModule;
    ///
    /// struct Forecasts;
    /// let module = ClientModule::<Forecasts>::new("weather")
    ///     .plugin(navree_kernel::plugin_info!(namespace = "weather"));
    /// ```
    pub fn plugin(mut self, info: PluginInfo) -> Self {
        self.plugin = Some(info);
        self
    }
}

impl<T: Send + Sync + 'static, O: ClientModuleOptions> Module for ClientModule<T, O> {
    fn id(&self) -> ModuleId {
        ModuleId::named::<Self>(self.name)
    }

    fn register(&self, m: &mut ModuleBuilder) {
        let name = self.name;
        if let Some(info) = &self.plugin {
            m.plugin(info.clone());
        }
        m.global();
        O::register_options(m, self.given.as_ref());

        // The client, built once at boot and exported.
        let client = match self.connect.clone() {
            Some(connect) => Factory::<T>::new(move |r: Resolver| {
                let connect = connect.clone();
                async move {
                    let options = O::resolve(r.clone()).await?;
                    connect(options, r).await.map(Arc::new)
                }
            })
            .deps(O::dependencies()),
            None => Factory::<T>::new(move |_| async move {
                Err(BoxError::from(format!(
                    "ClientModule `{name}` has no connect function: call .connect(..)"
                )))
            }),
        };
        m.factory(client).exports::<T>();

        // The same client under the module's name: the module's own `T`
        // comes before every other module's.
        m.factory(
            Factory::<T>::new(|r: Resolver| async move { Ok(r.get::<T>().await?) })
                .named(name)
                .deps(vec![ProviderKey::of::<T>()]),
        )
        .export_key(ProviderKey::named::<T>(name));

        if let Some(check) = self.health.clone() {
            m.multi(
                Factory::<dyn HealthCheck>::new(move |r: Resolver| {
                    let check = check.clone();
                    async move {
                        let client = r.get::<T>().await?;
                        Ok(Arc::new(ClientHealth {
                            name,
                            client,
                            check,
                        }) as Arc<dyn HealthCheck>)
                    }
                })
                .deps(vec![ProviderKey::of::<T>()]),
            );
        }

        if let Some(close) = self.shutdown.clone() {
            m.factory(
                Factory::<ClientShutdown<T>>::new(move |r: Resolver| {
                    let close = close.clone();
                    async move {
                        let client = r.get::<T>().await?;
                        Ok(Arc::new(ClientShutdown { client, close }))
                    }
                })
                .deps(vec![ProviderKey::of::<T>()]),
            )
            .lifecycle::<ClientShutdown<T>>();
        }
    }
}

impl<T, O> fmt::Debug for ClientModule<T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientModule")
            .field("name", &self.name)
            .field("client", &std::any::type_name::<T>())
            .field("options", &std::any::type_name::<O>())
            .field("connect", &self.connect.is_some())
            .field("health", &self.health.is_some())
            .field("shutdown", &self.shutdown.is_some())
            .field("options_in_code", &self.given.is_some())
            .field("plugin", &self.plugin.as_ref().map(|info| info.name))
            .finish()
    }
}

/// The options a `ClientModule` takes: `()` or any `ConfigSection` that is
/// `Clone`. Nothing else can implement it.
///
/// ```compile_fail
/// #[derive(Clone)]
/// struct LedgerOptions;
///
/// impl navree_config::ClientModuleOptions for LedgerOptions {}
/// ```
pub trait ClientModuleOptions: Send + Sync + 'static + sealed::Sealed {}

impl ClientModuleOptions for () {}

impl<P: ConfigSection + Clone> ClientModuleOptions for P {}

mod sealed {
    use std::sync::Arc;

    use navree_kernel::{BoxFuture, KernelError, ModuleBuilder, ProviderKey, Resolver};

    /// What a `ClientModule` does with its options type.
    pub trait Sealed: Sized {
        /// Registers the options with the module: `given` as a value, or
        /// else the section.
        fn register_options(m: &mut ModuleBuilder, given: Option<&Self>);

        /// The options, at boot.
        fn resolve(r: Resolver) -> BoxFuture<'static, Result<Arc<Self>, KernelError>>;

        /// The keys `resolve` reads, for the boot-time graph check.
        fn dependencies() -> Vec<ProviderKey>;
    }
}

/// No options: nothing is registered, and the connect function gets `()`.
impl sealed::Sealed for () {
    fn register_options(_m: &mut ModuleBuilder, _given: Option<&()>) {}

    fn resolve(_r: Resolver) -> BoxFuture<'static, Result<Arc<()>, KernelError>> {
        Box::pin(async { Ok(Arc::new(())) })
    }

    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }
}

/// A section: the options given in code as a value of the module, or else
/// the section read at boot.
impl<P: ConfigSection + Clone> sealed::Sealed for P {
    fn register_options(m: &mut ModuleBuilder, given: Option<&P>) {
        match given {
            Some(options) => {
                m.value(options.clone());
            }
            None => {
                m.config::<P>();
            }
        }
    }

    fn resolve(r: Resolver) -> BoxFuture<'static, Result<Arc<P>, KernelError>> {
        Box::pin(async move { r.get::<P>().await })
    }

    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<P>()]
    }
}

/// A `ClientModule`'s readiness check, named after the module.
struct ClientHealth<T> {
    /// The module's name.
    name: &'static str,
    /// The client.
    client: Arc<T>,
    /// The check.
    check: Hook<T>,
}

impl<T: Send + Sync + 'static> HealthCheck for ClientHealth<T> {
    fn name(&self) -> &str {
        self.name
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        let check = (self.check)(self.client.clone());
        Box::pin(async move {
            match check.await {
                Ok(()) => HealthStatus::Up,
                Err(error) => HealthStatus::Down(error.to_string()),
            }
        })
    }
}

/// A `ClientModule`'s shutdown hook, run by the kernel at graceful shutdown.
struct ClientShutdown<T> {
    /// The client.
    client: Arc<T>,
    /// The hook.
    close: Hook<T>,
}

impl<T: Send + Sync + 'static> Lifecycle for ClientShutdown<T> {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        (self.close)(self.client.clone())
    }
}
