//! [`SsoModule`].

use std::sync::Arc;

use navree_auth::{AuthService, ModuleBuilderAuthExt};
use navree_config::ModuleBuilderConfigExt;
use navree_crypto::{Encrypter, Hasher};
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver};

use crate::controller::SsoController;
use crate::error::map_sso_error;
use crate::migration::migrations;
use crate::policy::SsoSignInPolicy;
use crate::{DohResolver, Sso, SsoError, SsoOptions, TxtResolver};

/// Single sign-on. Global; provides and exports [`Sso`], [`SsoOptions`]
/// and `dyn` [`TxtResolver`] (a [`DohResolver`]), and contributes:
///
/// - the routes under `{http.api_prefix}{sso.routes_prefix}` (default
///   `/api/sso`) unless `sso.routes` is false: `POST discover`,
///   `GET {connection}/login`, `POST saml/{connection}/acs`,
///   `GET saml/{connection}/metadata`, `GET oidc/{connection}/callback` and
///   `POST exchange` (public), and the management routes under
///   `connections` and `domains` (permission `sso.manage`);
/// - the `sso` migration set (`sso_connections`, `sso_domains`,
///   `sso_identities`, after `auth`);
/// - a navree-auth `SignInPolicy` that lets the emails of enforcing
///   connections' verified domains sign in through that connection only
///   (every other method answers 403 `sso_required`);
/// - the mapping of [`SsoError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule`, `CryptoModule` (it encrypts client
/// secrets), `DatabaseModule` and `AuthModule`; `WorkspaceModule` for
/// connections owned by organizations or workspaces; a `RouteAuthorizer`
/// (`AccessModule`) for the management routes. `HttpClientModule` is used
/// for identity provider calls when present, `CacheModule` keeps sign-in
/// states, used assertions and one-time codes (an in-process cache
/// otherwise), `EventsModule` receives the events and a `dyn AuditSink` the
/// audit entries.
///
/// ```no_run
/// use navree_auth::AuthModule;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_sso::SsoModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(AuthModule::from_config())
///     .module(SsoModule::from_config()) // section `sso`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SsoModule;

impl SsoModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: SsoOptions) -> impl Module {
        ConfiguredModule::<SsoModule, _>::for_root(options, register)
    }

    /// The module configured from the `sso` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<SsoModule, SsoOptions>::from_config(register)
    }
}

impl Module for SsoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SsoOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<SsoOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid sso options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<SsoOptions>(),
    };
    m.exports::<SsoOptions>()
        .factory(
            Factory::<dyn TxtResolver>::new(|r: Resolver| async move {
                let options = r.get::<SsoOptions>().await?;
                let app = r.app();
                let http = match app.try_get::<HttpClient>().await {
                    Some(http) => (*http).clone(),
                    None => HttpClient::new(HttpClientOptions::default())?,
                };
                Ok(
                    Arc::new(DohResolver::new(http, options.domains.doh_url.clone()))
                        as Arc<dyn TxtResolver>,
                )
            })
            .deps(vec![ProviderKey::of::<SsoOptions>()]),
        )
        .exports::<dyn TxtResolver>()
        .factory(
            Factory::<Sso>::new(
                |r: Resolver| async move { Ok(Arc::new(Sso::build(&r.app()).await?)) },
            )
            .deps(vec![
                ProviderKey::of::<SsoOptions>(),
                ProviderKey::of::<dyn TxtResolver>(),
                ProviderKey::of::<AuthService>(),
                ProviderKey::of::<Encrypter>(),
                ProviderKey::of::<Hasher>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .exports::<Sso>()
        .sign_in_policy::<SsoSignInPolicy>()
        .controller::<SsoController>()
        .migrations(migrations())
        .map_error::<SsoError>(map_sso_error);
}
