//! [`SessionModule`].

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::SessionStore;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::controller::CsrfCookieController;
use crate::database;
use crate::layer::{SessionManager, clock_of, session_layer};
use crate::{DatabaseStore, MemoryStore, SessionOptions, SessionStoreKind};

/// The session module. Global; provides and exports [`SessionOptions`] and
/// the `dyn SessionStore` its options select ([`MemoryStore`],
/// [`DatabaseStore`], or for `store = "redis"` the provider named `redis`
/// that navree-redis's `RedisModule` registers); contributes the session and CSRF layer
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
        ConfiguredSessionModule {
            options: Some(options),
        }
    }

    /// The module configured from the `session` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredSessionModule { options: None }
    }
}

impl Module for SessionModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `SessionModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredSessionModule {
    /// `None`: read from configuration.
    options: Option<SessionOptions>,
}

impl Module for ConfiguredSessionModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<SessionModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SessionOptions>) {
    m.global();
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
            Factory::<dyn SessionStore>::new(|r: Resolver| async move {
                let options = r.get::<SessionOptions>().await?;
                let clock = clock_of(&r).await;
                let store: Arc<dyn SessionStore> = match options.store {
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
                };
                Ok(store)
            })
            .deps(vec![ProviderKey::of::<SessionOptions>()]),
        )
        .exports::<dyn SessionStore>()
        .provider::<SessionManager>()
        .http_layer(session_layer())
        .controller::<CsrfCookieController>()
        .migrations(database::migrations());
}
