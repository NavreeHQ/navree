//! [`HttpClientModule`].

use std::sync::Arc;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_kernel::{
    BoxError, ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_telemetry::Telemetry;

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
        ConfiguredModule::<HttpClientModule, _>::for_root(options, register)
    }

    /// The module configured from section `http_client` of the app's
    /// `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<HttpClientModule, HttpClientOptions>::from_config(register)
    }
}

impl Module for HttpClientModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
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

/// Global; provides and exports the singleton `HttpClient`. No options: the
/// module reads section `http_client`.
fn register(m: &mut ModuleBuilder, options: Option<HttpClientOptions>) {
    let source = match options {
        Some(options) => Source::Options(Arc::new(options)),
        None => {
            m.config_section::<HttpClientOptions>();
            Source::Config
        }
    };
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<HttpClient>::new(move |r| {
                let source = source.clone();
                async move {
                    let telemetry = Telemetry::from_app(&r.app()).await;
                    Ok(Arc::new(source.client(&r).await?.with_telemetry(telemetry)))
                }
            })
            .deps(deps),
        )
        .exports::<HttpClient>();
}
