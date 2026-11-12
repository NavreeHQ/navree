//! [`AuthModule`] and [`ModuleBuilderAuthExt`].

use std::sync::Arc;

use http::Method;
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::CallTickets;
use navree_crypto::{Encrypter, random_token};
use navree_db::ModuleBuilderDbExt;
use navree_http::{Authenticator, HttpOptions, ModuleBuilderHttpExt};
use navree_kernel::{
    ConfiguredModule, Factory, Injectable, Module, ModuleBuilder, ProviderKey, Resolver,
};

use crate::account_controller::AccountController;
use crate::apps::AppLinksController;
use crate::controller::AuthController;
use crate::error::map_auth_error;
use crate::passkeys::controller::PasskeyController;
use crate::service::clock_of;
use crate::session_controller::SessionController;
use crate::{
    AppsOptions, AuthError, AuthOptions, AuthService, JwtService, JwtStrategy, SealedTickets,
    SignInPolicy, Strategy, StrategyAuthenticator, TwoFactorPolicy, UserResourceExtension,
    migration,
};

/// The purpose the JWT signing key is derived for from the application key.
const JWT_KEY_PURPOSE: &str = "navree-auth.jwt";

/// The auth module: JWT authentication. Global; provides and
/// exports [`AuthOptions`], [`JwtService`], [`AuthService`] and navree-http's
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
/// verification mails when present, and a `dyn AuditSink` (navree-audit)
/// receives the security events when registered. Used plainly
/// (`.module(AuthModule)`) or through [`from_config`](AuthModule::from_config),
/// it reads the `auth` configuration section, so the app must include
/// `ConfigModule`; [`for_root`](AuthModule::for_root) takes the options
/// directly. Invalid options fail the boot.
///
/// ```no_run
/// use navree_auth::AuthModule;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
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
        ConfiguredModule::<AuthModule, _>::for_root(options, register)
    }

    /// The module configured from the `auth` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<AuthModule, AuthOptions>::from_config(register)
    }
}

impl Module for AuthModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
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
    // The native apps' section is read here (not by `m.config`), so it is
    // registered for `config:check` and the reference.
    m.config_section::<AppsOptions>();
    m.exports::<AuthOptions>()
        .factory(Factory::<AppsOptions>::new(|r: Resolver| async move {
            Ok(Arc::new(crate::apps::apps_options(&r).await?))
        }))
        .exports::<AppsOptions>()
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
        // Sealed one-time tickets for results clients post back, on the
        // service's key, cache and clock.
        .factory(
            Factory::<dyn CallTickets>::new(|r: Resolver| async move {
                let service = r.get::<AuthService>().await?;
                let tickets = SealedTickets::new(
                    service.encrypter.clone(),
                    service.cache.clone(),
                    service.clock.clone(),
                );
                Ok(Arc::new(tickets) as Arc<dyn CallTickets>)
            })
            .deps(vec![ProviderKey::of::<AuthService>()]),
        )
        .exports::<dyn CallTickets>()
        .multi(
            Factory::<dyn Strategy>::new(|r: Resolver| async move {
                let jwt = r.get::<JwtService>().await?;
                let options = r.get::<AuthOptions>().await?;
                let api_prefix = match r.app().try_get::<HttpOptions>().await {
                    Some(http) => http.api_prefix.clone(),
                    None => String::new(),
                };
                let strategy =
                    JwtStrategy::new(jwt).with_setup_routes(setup_routes(&api_prefix, &options));
                Ok(Arc::new(strategy) as Arc<dyn Strategy>)
            })
            .deps(vec![
                ProviderKey::of::<JwtService>(),
                ProviderKey::of::<AuthOptions>(),
            ]),
        )
        .factory(Factory::<dyn Authenticator>::new(|r: Resolver| async move {
            let strategies = r.get_all::<dyn Strategy>().await?;
            let authenticator = StrategyAuthenticator::new(strategies);
            tracing::debug!(target: "navree::auth", strategies = ?authenticator.names(), "authentication strategies");
            Ok(Arc::new(authenticator) as Arc<dyn Authenticator>)
        }))
        .exports::<dyn Authenticator>()
        .controller::<AuthController>()
        .controller::<AccountController>()
        .controller::<SessionController>()
        .controller::<PasskeyController>()
        .controller::<AppLinksController>()
        .migrations(migration::migrations())
        .map_error::<AuthError>(map_auth_error);
    // The push channel of native apps asks which sign-ins are live, so a
    // signed-out phone gets nothing.
    #[cfg(feature = "notify")]
    m.factory(
        Factory::<dyn navree_notify::LiveSessions>::new(|r: Resolver| async move {
            let service = r.get::<AuthService>().await?;
            Ok(Arc::new(crate::AuthLiveSessions::new(service))
                as Arc<dyn navree_notify::LiveSessions>)
        })
        .deps(vec![ProviderKey::of::<AuthService>()]),
    )
    .exports::<dyn navree_notify::LiveSessions>();
    // Browsers signed in on the session cookie, after the JWT strategy: a
    // request with a bearer token is the token's.
    #[cfg(feature = "session")]
    m.multi(
        Factory::<dyn Strategy>::new(|r: Resolver| async move {
            let service = r.get::<AuthService>().await?;
            let options = r.get::<AuthOptions>().await?;
            let api_prefix = match r.app().try_get::<HttpOptions>().await {
                Some(http) => http.api_prefix.clone(),
                None => String::new(),
            };
            let mut routes = setup_routes(&api_prefix, &options);
            routes.extend(crate::session_auth::page_setup_routes());
            let strategy = crate::SessionStrategy::new(service).with_setup_routes(routes);
            Ok(Arc::new(strategy) as Arc<dyn Strategy>)
        })
        .deps(vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
        ]),
    );
}

/// The routes a two-factor setup token reaches: setting two-factor
/// authentication up, reading the user, signing out and refreshing.
fn setup_routes(api_prefix: &str, options: &AuthOptions) -> Vec<(Method, String)> {
    let prefix = format!(
        "/{}/{}",
        api_prefix.trim_matches('/'),
        options.routes_prefix.trim_matches('/')
    )
    .replace("//", "/");
    let prefix = prefix.trim_end_matches('/');
    [
        (Method::POST, "/2fa/enable"),
        (Method::POST, "/2fa/confirm"),
        (Method::GET, "/me"),
        (Method::POST, "/logout"),
        (Method::POST, "/refresh"),
    ]
    .into_iter()
    .map(|(method, path)| (method, format!("{prefix}{path}")))
    .collect()
}

/// `auth.jwt.secret`, else a key derived from the application key, else a
/// random per-process key (with a warning: tokens die with the process).
async fn signing_secret(
    r: &Resolver,
    options: &AuthOptions,
) -> Result<Vec<u8>, navree_kernel::BoxError> {
    if let Some(secret) = &options.jwt.secret {
        return Ok(secret.expose().as_bytes().to_vec());
    }
    let encrypter = r.get::<Encrypter>().await?;
    match encrypter.derive_key(JWT_KEY_PURPOSE) {
        Ok(key) => Ok(key.to_vec()),
        Err(_) => {
            tracing::warn!(
                target: "navree::auth",
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

    /// Registers `P` as a `dyn SignInPolicy` multi-binding: every sign-in
    /// flow asks it before it issues a session.
    fn sign_in_policy<P: SignInPolicy + Injectable>(&mut self) -> &mut Self;
    /// Registers `P` as a `dyn TwoFactorPolicy` multi-binding: users it
    /// requires two-factor authentication of must set it up first.
    fn two_factor_policy<P: TwoFactorPolicy + Injectable>(&mut self) -> &mut Self;
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
        // The fields it adds join `UserResource` in the OpenAPI document.
        self.contribute(crate::resource::user_resource_properties::<E>());
        self.multi(
            Factory::<dyn UserResourceExtension>::new(|r: Resolver| async move {
                Ok(Arc::new(E::construct(r).await?) as Arc<dyn UserResourceExtension>)
            })
            .deps(E::dependencies()),
        )
    }

    fn sign_in_policy<P: SignInPolicy + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn SignInPolicy>::new(|r: Resolver| async move {
                Ok(Arc::new(P::construct(r).await?) as Arc<dyn SignInPolicy>)
            })
            .deps(P::dependencies()),
        )
    }

    fn two_factor_policy<P: TwoFactorPolicy + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn TwoFactorPolicy>::new(|r: Resolver| async move {
                Ok(Arc::new(P::construct(r).await?) as Arc<dyn TwoFactorPolicy>)
            })
            .deps(P::dependencies()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_routes_live_under_the_api_and_auth_prefixes() {
        let routes = setup_routes("/api", &AuthOptions::default());
        assert!(routes.contains(&(Method::POST, "/api/auth/2fa/enable".to_owned())));
        assert!(routes.contains(&(Method::GET, "/api/auth/me".to_owned())));
        let options = AuthOptions {
            routes_prefix: "/identity/".to_owned(),
            ..AuthOptions::default()
        };
        let routes = setup_routes("", &options);
        assert!(routes.contains(&(Method::POST, "/identity/refresh".to_owned())));
        let routes = setup_routes(
            "/api/",
            &AuthOptions {
                routes_prefix: String::new(),
                ..AuthOptions::default()
            },
        );
        assert!(routes.contains(&(Method::POST, "/api/logout".to_owned())));
    }
}
