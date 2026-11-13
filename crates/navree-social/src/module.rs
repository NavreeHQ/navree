//! [`SocialModule`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_auth::{AuthOptions, AuthService};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderRevocationExt;
use navree_crypto::{Encrypter, Hasher};
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    ConfiguredModule, Factory, Injectable, Module, ModuleBuilder, ProviderKey, Resolver,
};

use crate::controller::SocialController;
use crate::error::map_social_error;
use crate::migration::migrations;
use crate::native::SocialRevoker;
use crate::runtime::Runtime;
use crate::{Social, SocialError, SocialOptions, SocialProvider};

/// Social sign-in. Global; provides and exports [`Social`] and
/// [`SocialOptions`], and contributes:
///
/// - the routes under `{http.api_prefix}{auth.routes_prefix}/social`
///   (default `/api/auth/social`) unless `social.routes` is false:
///   `GET {provider}/redirect` and `POST {provider}/callback` (public; the
///   callback counts against `auth.login_throttle_per_minute` per client
///   IP), `GET accounts`, `POST {provider}/link` and `DELETE
///   accounts/{provider}` (signed in), the native apps' `POST
///   {provider}/native/nonce`, `POST {provider}/native` (and `.../session`
///   with the feature `session`), and Apple's `POST apple/notifications`
///   (public);
/// - the `social` migration set (`social_accounts` and
///   `social_account_clients`, after `auth`);
/// - a credential revoker (`social`): navree-auth's offboarding revokes the
///   users' stored Apple and Google tokens at the provider;
/// - the mapping of [`SocialError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule` (for [`from_config`](SocialModule::from_config)
/// and the plain module), `CryptoModule` (it seals the states and encrypts
/// stored provider tokens), `DatabaseModule` and `AuthModule`.
/// `HttpClientModule` is used for provider calls when present (a default
/// client otherwise), `CacheModule` remembers used states (an in-process
/// cache otherwise), `EventsModule` receives the events and a
/// `dyn AuditSink` the audit entries.
///
/// ```no_run
/// use navree_auth::AuthModule;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_http_client::HttpClientModule;
/// use navree_kernel::Navree;
/// use navree_social::SocialModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .module(HttpModule::from_config())
///     .module(HttpClientModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(AuthModule::from_config())
///     .module(SocialModule::from_config()) // section `social`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SocialModule;

impl SocialModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: SocialOptions) -> impl Module {
        ConfiguredModule::<SocialModule, _>::for_root(options, register)
    }

    /// The module configured from the `social` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<SocialModule, SocialOptions>::from_config(register)
    }

    /// Registers the provider `P` (configured as
    /// `[social.providers.<P::key()>]` with `type = "custom"`).
    pub fn for_feature<P: SocialProvider + Injectable>() -> impl Module {
        SocialFeature::<P>(PhantomData)
    }
}

impl Module for SocialModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SocialOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<SocialOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid social options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<SocialOptions>(),
    };
    m.exports::<SocialOptions>()
        .factory(
            Factory::<Social>::new(|r: Resolver| async move {
                let runtime = Runtime::build(&r.app()).await?;
                Ok(Arc::new(Social::real(runtime)))
            })
            .deps(vec![
                ProviderKey::of::<SocialOptions>(),
                ProviderKey::of::<AuthService>(),
                ProviderKey::of::<AuthOptions>(),
                ProviderKey::of::<Encrypter>(),
                ProviderKey::of::<Hasher>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .exports::<Social>()
        .lifecycle::<Social>()
        .controller::<SocialController>()
        .credential_revoker::<SocialRevoker>()
        .migrations(migrations())
        .map_error::<SocialError>(map_social_error);
}

/// `SocialModule::for_feature::<P>()`.
struct SocialFeature<P>(PhantomData<fn() -> P>);

impl<P: SocialProvider + Injectable> Module for SocialFeature<P> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(
            Factory::<dyn SocialProvider>::new(|r: Resolver| async move {
                Ok(Arc::new(P::construct(r).await?) as Arc<dyn SocialProvider>)
            })
            .deps(P::dependencies()),
        );
    }
}
