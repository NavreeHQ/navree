//! [`OAuthServerModule`].

use std::sync::Arc;

use navree_auth::ModuleBuilderAuthExt;
use navree_config::{Config, ModuleBuilderConfigExt};
use navree_crypto::Encrypter;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::api::OAuthApiController;
use crate::commands::commands;
use crate::core::{Core, clock_of};
use crate::error::map_oauth_error;
use crate::keys::{SigningKey25519, SigningKeys};
use crate::migration::migrations;
use crate::protocol::OAuthProtocolController;
use crate::strategy::{OAuthStrategy, insufficient_scope_layer};
use crate::{OAuthClients, OAuthError, OAuthOptions, PersonalAccessTokens};

/// The purpose the ID token signing key is derived for from the
/// application key.
const SIGNING_KEY_PURPOSE: &str = "navree-oauth.signing";

/// The OAuth 2.1 authorization server and OpenID Connect provider.
///
/// Global; provides and exports [`OAuthOptions`], [`OAuthClients`] and
/// [`PersonalAccessTokens`], and contributes:
///
/// - the protocol endpoints at `oauth.path` (default `/oauth`, outside
///   `http.api_prefix`): `GET authorize`, `POST token`, `POST
///   device_authorization`, `POST revoke`, `POST introspect`, `GET|POST
///   userinfo`, `GET jwks`, `POST register` (with
///   `oauth.dynamic_registration`), and the discovery documents
///   `/.well-known/openid-configuration` and
///   `/.well-known/oauth-authorization-server`;
/// - the routes under `{http.api_prefix}/oauth` your front end calls (see
///   [`OAuthApiController`](crate::OAuthApiController));
/// - the `oauth` authentication strategy ([`OAuthStrategy`](crate::OAuthStrategy)),
///   which runs after navree-auth's JWT strategy;
/// - the `oauth` migration set (after `auth`);
/// - a layer that adds `WWW-Authenticate: Bearer
///   error="insufficient_scope"` to 403s a token's scopes caused;
/// - the commands `oauth:client`, `oauth:clients`, `oauth:client:secret`,
///   `oauth:client:revoke`, `oauth:keys` and `oauth:prune`.
///
/// The app must include `ConfigModule`, `CryptoModule` (the signing key
/// is derived from `app.key` unless `oauth.signing_key` is set),
/// `DatabaseModule` and `AuthModule`; `AccessModule` turns scopes into
/// abilities, and `WorkspaceModule` resolves workspace-limited clients.
///
/// ```no_run
/// use navree_auth::AuthModule;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_oauth::OAuthServerModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(AuthModule::from_config())
///     .module(OAuthServerModule::from_config()) // section `oauth`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct OAuthServerModule;

impl OAuthServerModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: OAuthOptions) -> impl Module {
        ConfiguredOAuthServerModule {
            options: Some(options),
        }
    }

    /// The module configured from the `oauth` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredOAuthServerModule { options: None }
    }
}

impl Module for OAuthServerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `OAuthServerModule::for_root` / `from_config`: same module id,
/// configured.
struct ConfiguredOAuthServerModule {
    /// `None`: read from configuration.
    options: Option<OAuthOptions>,
}

impl Module for ConfiguredOAuthServerModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<OAuthServerModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<OAuthOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<OAuthOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid oauth options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<OAuthOptions>(),
    };
    m.exports::<OAuthOptions>()
        .factory(
            Factory::<Core>::new(|r: Resolver| async move {
                let options = r.get::<OAuthOptions>().await?;
                let db = r.get::<Db>().await?;
                let app = r.app();
                let issuer = issuer(&options, app.try_get::<Config>().await.as_deref());
                let encrypter = app.try_get::<Encrypter>().await;
                let keys = signing_keys(&options, encrypter.as_deref())?;
                let clock = clock_of(&app).await;
                Ok(Arc::new(Core::new(
                    db,
                    options,
                    issuer,
                    keys,
                    clock,
                    app.downgrade(),
                )))
            })
            .deps(vec![
                ProviderKey::of::<OAuthOptions>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .factory(
            Factory::<OAuthClients>::new(|r: Resolver| async move {
                Ok(Arc::new(OAuthClients::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<OAuthClients>()
        .factory(
            Factory::<PersonalAccessTokens>::new(|r: Resolver| async move {
                Ok(Arc::new(PersonalAccessTokens::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<PersonalAccessTokens>()
        .strategy::<OAuthStrategy>()
        .controller::<OAuthProtocolController>()
        .controller::<OAuthApiController>()
        .migrations(migrations())
        .http_layer(insufficient_scope_layer())
        .map_error::<OAuthError>(map_oauth_error);
    for command in commands() {
        m.command(command);
    }
}

/// `oauth.issuer`, else `app.url`, else `http://localhost` (with a
/// warning).
fn issuer(options: &OAuthOptions, config: Option<&Config>) -> String {
    if let Some(issuer) = &options.issuer {
        return issuer.clone();
    }
    if let Some(url) = config.and_then(|config| config.get_string("app.url"))
        && !url.trim().is_empty()
    {
        return url;
    }
    tracing::warn!(
        target: "navree::oauth",
        "neither oauth.issuer nor app.url is set: the OAuth issuer is http://localhost"
    );
    "http://localhost".to_owned()
}

/// `oauth.signing_key` (and the previous keys), else a key derived from
/// the application key, else a random per-process key (with a warning).
fn signing_keys(
    options: &OAuthOptions,
    encrypter: Option<&Encrypter>,
) -> Result<SigningKeys, BoxError> {
    let previous = options
        .previous_signing_keys
        .iter()
        .map(|key| SigningKey25519::parse(key.expose()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("oauth.previous_signing_keys: {error}"))?;
    let current = match &options.signing_key {
        Some(key) => SigningKey25519::parse(key.expose())
            .map_err(|error| format!("oauth.signing_key: {error}"))?,
        None => match encrypter.map(|encrypter| encrypter.derive_key(SIGNING_KEY_PURPOSE)) {
            Some(Ok(seed)) => SigningKey25519::from_seed(seed),
            _ => {
                tracing::warn!(
                    target: "navree::oauth",
                    "neither oauth.signing_key nor an application key (`app.key` / APP_KEY) is set: \
                     ID tokens are signed with a random key that changes when the process restarts"
                );
                SigningKey25519::generate()
            }
        },
    };
    Ok(SigningKeys::new(current, previous))
}

#[cfg(test)]
mod tests {
    use navree_contracts::Secret;
    use navree_crypto::{AppKey, generate_key};

    use super::*;

    #[test]
    fn the_issuer_falls_back_to_app_url_then_localhost() {
        let options = OAuthOptions {
            issuer: Some("https://api.acme.test".into()),
            ..OAuthOptions::default()
        };
        assert_eq!(issuer(&options, None), "https://api.acme.test");
        assert_eq!(issuer(&OAuthOptions::default(), None), "http://localhost");
    }

    #[test]
    fn signing_keys_come_from_config_or_the_app_key() {
        let encrypter = Encrypter::new(AppKey::parse(&generate_key()).unwrap());
        let derived = signing_keys(&OAuthOptions::default(), Some(&encrypter)).unwrap();
        let again = signing_keys(&OAuthOptions::default(), Some(&encrypter)).unwrap();
        assert_eq!(derived.current().kid(), again.current().kid());

        let key = SigningKey25519::generate();
        let previous = SigningKey25519::generate();
        let options = OAuthOptions {
            signing_key: Some(Secret::new(key.encode())),
            previous_signing_keys: vec![Secret::new(previous.encode())],
            ..OAuthOptions::default()
        };
        let keys = signing_keys(&options, Some(&encrypter)).unwrap();
        assert_eq!(keys.current().kid(), key.kid());
        assert_eq!(keys.jwks()["keys"][1]["kid"], previous.kid());

        let bad = OAuthOptions {
            signing_key: Some(Secret::new("ed25519:nope".into())),
            ..OAuthOptions::default()
        };
        let error = signing_keys(&bad, None).unwrap_err().to_string();
        assert!(error.starts_with("oauth.signing_key"), "{error}");

        // No key at all: a random one.
        let random = signing_keys(&OAuthOptions::default(), None).unwrap();
        assert_ne!(random.current().kid(), derived.current().kid());
    }
}
