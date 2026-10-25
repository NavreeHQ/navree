//! [`AuthModule`] and [`ModuleBuilderAuthExt`].

use std::sync::Arc;

use keel_config::ModuleBuilderConfigExt;
use keel_crypto::{Encrypter, random_token};
use keel_db::ModuleBuilderDbExt;
use keel_http::{Authenticator, ModuleBuilderHttpExt};
use keel_kernel::{Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::account_controller::AccountController;
use crate::controller::AuthController;
use crate::error::map_auth_error;
use crate::service::clock_of;
use crate::{
    AuthError, AuthOptions, AuthService, JwtService, JwtStrategy, Strategy, StrategyAuthenticator,
    UserResourceExtension, migration,
};

/// The purpose the JWT signing key is derived for from the application key.
const JWT_KEY_PURPOSE: &str = "keel-auth.jwt";

/// The auth module: NestJS-style JWT authentication. Global; provides and
/// exports [`AuthOptions`], [`JwtService`], [`AuthService`] and keel-http's
/// `dyn Authenticator` (a [`StrategyAuthenticator`] running every
/// `dyn Strategy`, the built-in [`JwtStrategy`] first), and contributes:
///
/// - the routes under `{http.api_prefix}{auth.routes_prefix}` (default
///   `/api/auth`): `POST register` (public; not mounted when
///   `auth.registration` is false), `POST login` (public; throttled per
///   email and IP), `POST refresh` (public), `POST logout` and `GET me`
///   (authenticated), and the account flows: `PATCH me`, `PATCH
///   me/password`, `POST forgot-password`, `POST reset-password`, `POST
///   email/verification`, `GET email/verify/{id}/{hash}` (signed link) and
///   `POST 2fa/{enable,confirm,disable,recovery-codes,verify}` (see the
///   crate documentation);
/// - the `auth` migration set (`users`, `refresh_tokens`,
///   `password_reset_tokens`); feature modules referencing `users` run
///   `.after(["auth"])`;
/// - the mapping of [`AuthError`]s returned from handlers to error responses.
///
/// The app must include `CryptoModule` (password hashing; the JWT key is
/// derived from `app.key` unless `auth.jwt.secret` is set; two-factor
/// secrets are encrypted with its `Encrypter`) and `DatabaseModule`;
/// `CacheModule` is optional (the login and two-factor throttles use an
/// in-process cache without it), `MailModule` sends the password reset and
/// verification mails when present, and a `dyn AuditSink` (keel-audit)
/// receives the security events when registered. Used plainly
/// (`.module(AuthModule)`) or through [`from_config`](AuthModule::from_config),
/// it reads the `auth` configuration section, so the app must include
/// `ConfigModule`; [`for_root`](AuthModule::for_root) takes the options
/// directly. Invalid options fail the boot.
///
/// ```no_run
/// use keel_auth::AuthModule;
/// use keel_config::{ConfigModule, ConfigSources};
/// use keel_crypto::CryptoModule;
/// use keel_db::DatabaseModule;
/// use keel_http::HttpModule;
/// use keel_kernel::Keel;
///
/// # fn main() -> Result<(), keel_kernel::BoxError> {
/// Keel::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(AuthModule::from_config()) // section `auth`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct AuthModule;

impl AuthModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: AuthOptions) -> impl Module {
        ConfiguredAuthModule {
            options: Some(options),
        }
    }

    /// The module configured from the `auth` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredAuthModule { options: None }
    }
}

impl Module for AuthModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `AuthModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredAuthModule {
    /// `None`: read from configuration.
    options: Option<AuthOptions>,
}

impl Module for ConfiguredAuthModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<AuthModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<AuthOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<AuthOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|errors| errors.join("; "))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<AuthOptions>(),
    };
    m.exports::<AuthOptions>()
        .factory(
            Factory::<JwtService>::new(|r: Resolver| async move {
                let options = r.get::<AuthOptions>().await?;
                let secret = signing_secret(&r, &options).await?;
                let clock = clock_of(&r.app()).await;
                Ok(Arc::new(
                    JwtService::new(&secret, &options.jwt).with_clock(clock),
                ))
            })
            .deps(vec![
                ProviderKey::of::<AuthOptions>(),
                ProviderKey::of::<Encrypter>(),
            ]),
        )
        .exports::<JwtService>()
        .provider::<AuthService>()
        .exports::<AuthService>()
        .multi(
            Factory::<dyn Strategy>::new(|r: Resolver| async move {
                let jwt = r.get::<JwtService>().await?;
                Ok(Arc::new(JwtStrategy::new(jwt)) as Arc<dyn Strategy>)
            })
            .deps(vec![ProviderKey::of::<JwtService>()]),
        )
        .factory(Factory::<dyn Authenticator>::new(|r: Resolver| async move {
            let strategies = r.get_all::<dyn Strategy>().await?;
            let authenticator = StrategyAuthenticator::new(strategies);
            tracing::debug!(target: "keel::auth", strategies = ?authenticator.names(), "authentication strategies");
            Ok(Arc::new(authenticator) as Arc<dyn Authenticator>)
        }))
        .exports::<dyn Authenticator>()
        .controller::<AuthController>()
        .controller::<AccountController>()
        .migrations(migration::migrations())
        .map_error::<AuthError>(map_auth_error);
}

/// `auth.jwt.secret`, else a key derived from the application key, else a
/// random per-process key (with a warning: tokens die with the process).
async fn signing_secret(
    r: &Resolver,
    options: &AuthOptions,
) -> Result<Vec<u8>, keel_kernel::BoxError> {
    if let Some(secret) = &options.jwt.secret {
        return Ok(secret.expose().as_bytes().to_vec());
    }
    let encrypter = r.get::<Encrypter>().await?;
    match encrypter.derive_key(JWT_KEY_PURPOSE) {
        Ok(key) => Ok(key.to_vec()),
        Err(_) => {
            tracing::warn!(
                target: "keel::auth",
                "neither auth.jwt.secret nor an application key (`app.key` / APP_KEY) is set: \
                 JWTs are signed with a random key and stop working when the process restarts"
            );
            Ok(random_token(64).into_bytes())
        }
    }
}

/// Auth registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderAuthExt {
    /// Registers `S` as a `dyn Strategy` multi-binding (run after the
    /// strategies of the modules this one depends on, the built-in JWT
    /// strategy first).
    fn strategy<S: Strategy + Injectable>(&mut self) -> &mut Self;

    /// Registers `E` as a `dyn UserResourceExtension` multi-binding.
    fn user_resource_extension<E: UserResourceExtension + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderAuthExt for ModuleBuilder {
    fn strategy<S: Strategy + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn Strategy>::new(|r: Resolver| async move {
                Ok(Arc::new(S::construct(r).await?) as Arc<dyn Strategy>)
            })
            .deps(S::dependencies()),
        )
    }

    fn user_resource_extension<E: UserResourceExtension + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn UserResourceExtension>::new(|r: Resolver| async move {
                Ok(Arc::new(E::construct(r).await?) as Arc<dyn UserResourceExtension>)
            })
            .deps(E::dependencies()),
        )
    }
}
