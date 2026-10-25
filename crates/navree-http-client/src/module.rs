//! [`HttpClientModule`].

use std::sync::Arc;

use navree_config::Config;
use navree_kernel::{BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::{HttpClient, HttpClientOptions};

/// The global HTTP client module: provides and exports one shared
/// [`HttpClient`].
///
/// - [`HttpClientModule::for_root`] takes [`HttpClientOptions`] in code;
/// - [`HttpClientModule::from_config`] (and a plain
///   `.module(HttpClientModule)` or import with no configured instance in the
///   app) reads section `http_client` of the app's `Config` at boot, so the
///   app must include `ConfigModule`. Invalid options fail the boot, naming
///   the section and the key.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http_client::HttpClientModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// // .env: APP_HTTP_CLIENT__TIMEOUT_SECS=10
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpClientModule::from_config())
///     .run()
/// # }
/// ```
///
/// In tests, replace the client with a fake:
/// `Navree::app()...override_value(Arc::new(fake_client))`, where
/// `(fake_client, server) = HttpClient::fake()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpClientModule;

impl HttpClientModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: HttpClientOptions) -> impl Module {
        ConfiguredHttpClientModule {
            source: Source::Options(Arc::new(options)),
        }
    }

    /// The module configured from section `http_client` of the app's
    /// `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredHttpClientModule {
            source: Source::Config,
        }
    }
}

impl Module for HttpClientModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, Source::Config);
    }
}

/// Where the options come from.
#[derive(Clone)]
enum Source {
    Options(Arc<HttpClientOptions>),
    Config,
}

impl Source {
    /// Builds the client.
    async fn client(&self, r: &Resolver) -> Result<HttpClient, BoxError> {
        let options = match self {
            Source::Options(options) => HttpClientOptions::clone(options),
            Source::Config => r.get::<Config>().await?.typed::<HttpClientOptions>()?,
        };
        Ok(HttpClient::new(options)?)
    }

    /// The keys the factory resolves.
    fn deps(&self) -> Vec<ProviderKey> {
        match self {
            Source::Options(_) => Vec::new(),
            Source::Config => vec![ProviderKey::of::<Config>()],
        }
    }
}

/// `HttpClientModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredHttpClientModule {
    source: Source,
}

impl Module for ConfiguredHttpClientModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<HttpClientModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.source.clone());
    }
}

/// Global; provides and exports the singleton `HttpClient`.
fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<HttpClient>::new(move |r| {
                let source = source.clone();
                async move { Ok(Arc::new(source.client(&r).await?)) }
            })
            .deps(deps),
        )
        .exports::<HttpClient>();
}
