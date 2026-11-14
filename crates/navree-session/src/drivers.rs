//! Session stores a module registers under a name: [`ModuleBuilderSessionExt`].

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::register_driver;
use navree_config::{DriverContext, DriverSlot};
use navree_contracts::SessionStore;
use navree_kernel::{BoxError, ModuleBuilder};

/// The selector `session.store`, its built-in stores and the method that
/// registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "session.store",
    &["memory", "database", "redis"],
    "m.session_store",
    "session store",
);

/// What the boot builds first: it holds that every registration of a session
/// store was checked. A provider of its own, so an app that replaces
/// `dyn SessionStore` (in a test) still has its registrations checked.
pub(crate) struct CheckedStores;

/// Registers session stores on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_http::HttpModule;
/// use navree_kernel::{BoxError, Module, ModuleBuilder, Navree};
/// use navree_session::{MemoryStore, ModuleBuilderSessionExt, SessionModule};
/// use serde::Deserialize;
///
/// /// The settings of the store: `[session.tijori]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct TijoriSettings {
///     shelf: String,
/// }
///
/// struct TijoriModule;
///
/// impl Module for TijoriModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Selected with `session.store = "tijori"`. Any type implementing
///         // `SessionStore` can be returned; a store over memory stands in here
///         // for one that opens its backend with the settings.
///         m.session_store("tijori", |ctx| async move {
///             let _settings: TijoriSettings = ctx.settings()?;
///             Ok(MemoryStore::new())
///         });
///     }
/// }
///
/// fn main() -> Result<(), BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(CryptoModule::from_config())
///         .module(HttpModule::from_config())
///         .module(SessionModule::from_config())
///         .module(TijoriModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderSessionExt {
    /// Registers a session store under `name` (a lower-case letter followed
    /// by up to 63 lower-case letters, digits or `_`). Then
    /// `session.store = "{name}"` selects it, and `factory` builds it at boot
    /// from its [`DriverContext`], whose settings are `session.{name}`.
    ///
    /// A name that is not such a word, a built-in name (`memory`,
    /// `database`, `redis`), a key of the `session` section (`cookie`) and a
    /// name two modules register fail the boot, naming `m.session_store` and
    /// the module, also when another store is selected; `config:check` says
    /// the same. A factory that fails fails the boot, naming the store and
    /// the module.
    fn session_store<S, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        S: SessionStore,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, BoxError>> + Send + 'static;
}

impl ModuleBuilderSessionExt for ModuleBuilder {
    fn session_store<S, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        S: SessionStore,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, BoxError>> + Send + 'static,
    {
        register_driver::<dyn SessionStore, _, _>(
            self,
            &SLOT,
            Some("session"),
            name,
            move |ctx| {
                let built = factory(ctx);
                async move {
                    built
                        .await
                        .map(|store| Arc::new(store) as Arc<dyn SessionStore>)
                }
            },
        )
    }
}
