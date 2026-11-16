//! [`SessionModule`].

use std::sync::Arc;

use navree_config::__private::{build_driver, check_registrations, contribute_slot};
use navree_config::{ConfigSection, ModuleBuilderConfigExt};
use navree_contracts::SessionStore;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver};

use crate::controller::CsrfCookieController;
use crate::database;
use crate::drivers::{CheckedStores, SLOT};
use crate::layer::{SessionManager, clock_of, session_layer};
use crate::{DatabaseStore, MemoryStore, SessionOptions, SessionStoreKind};

/// The session module. Global; provides and exports [`SessionOptions`] and
/// the `dyn SessionStore` its options select ([`MemoryStore`],
/// [`DatabaseStore`], or for `store = "redis"` the provider named `redis`
/// that navree-redis's `RedisModule` registers, or a store a module registers
/// with [`m.session_store`](crate::ModuleBuilderSessionExt::session_store));
/// contributes the session and CSRF layer
/// ([`LayerSlot::Session`](navree_http::LayerSlot::Session)), the public route
/// `GET /csrf-cookie` and the `session` migration set (the
/// `sessions` table, used by the database store).
///
/// The app must include `CryptoModule` (the cookie holds the session id
/// encrypted with the app key; without `app.key` the app boots, but
/// requests that write a session fail with 500) and, for
/// `store = "database"`, `DatabaseModule`. Used plainly
/// (`.module(SessionModule)`) or through
/// [`from_config`](SessionModule::from_config), it reads the `session`
/// configuration section, so the app must include `ConfigModule`;
/// [`for_root`](SessionModule::for_root) needs no configuration. Invalid
/// options fail the boot.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_session::SessionModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .module(HttpModule::from_config())
///     .module(SessionModule::from_config()) // section `session`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SessionModule;

impl SessionModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: SessionOptions) -> impl Module {
        ConfiguredModule::<SessionModule, _>::for_root(options, register)
    }

    /// The module configured from the `session` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<SessionModule, SessionOptions>::from_config(register)
    }
}

impl Module for SessionModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SessionOptions>) {
    m.global();
    contribute_slot(m, SLOT);
    match options {
        Some(options) => m.factory(Factory::<SessionOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|errors| errors.join("; "))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<SessionOptions>(),
    };
    m.exports::<SessionOptions>()
        .factory(
            // Every registration of a session store is checked, whichever is
            // selected and whether or not `dyn SessionStore` is replaced.
            Factory::<CheckedStores>::new(|r: Resolver| async move {
                r.get::<SessionOptions>().await?;
                check_registrations(&r.app(), &SLOT, SessionOptions::schema())?;
                Ok(Arc::new(CheckedStores))
            })
            .deps(vec![ProviderKey::of::<SessionOptions>()]),
        )
        .factory(
            Factory::<dyn SessionStore>::new(|r: Resolver| async move {
                let options = r.get::<SessionOptions>().await?;
                r.get::<CheckedStores>().await?;
                let clock = clock_of(&r).await;
                let store: Arc<dyn SessionStore> = match options.store.resolved() {
                    SessionStoreKind::Memory => Arc::new(MemoryStore::new().with_clock(clock)),
                    SessionStoreKind::Database => {
                        let db = r.get::<Db>().await.map_err(|error| {
                            format!(
                                "session.store = \"database\" needs DatabaseModule in the app: {error}"
                            )
                        })?;
                        Arc::new(DatabaseStore::new((*db).clone()).with_clock(clock))
                    }
                    SessionStoreKind::Redis => {
                        r.app().get_named::<dyn SessionStore>("redis").await.map_err(|error| {
                            format!(
                                "session.store = \"redis\" needs RedisModule (navree-redis) in the app: {error}"
                            )
                        })?
                    }
                    SessionStoreKind::Registered(name) => {
                        build_driver::<dyn SessionStore>(
                            &r,
                            &SLOT,
                            "session.store",
                            name,
                            format!("session.{name}"),
                            None,
                            None,
                        )
                        .await?
                    }
                };
                Ok(store)
            })
            .deps(vec![
                ProviderKey::of::<SessionOptions>(),
                ProviderKey::of::<CheckedStores>(),
            ]),
        )
        .exports::<dyn SessionStore>()
        .provider::<SessionManager>()
        .http_layer(session_layer())
        .controller::<CsrfCookieController>()
        .migrations(database::migrations());
}
