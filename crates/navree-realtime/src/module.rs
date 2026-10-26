//! [`RealtimeModule`], channel authorizers ([`ModuleBuilderRealtimeExt::channel`])
//! and [`ChannelDecision`].

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{Broadcaster, Principal};
use navree_crypto::Encrypter;
use navree_http::{HttpOptions, HttpServer, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Module, ModuleBuilder, ModuleId, ProviderKey,
    Resolver,
};
use serde_json::Value;

use crate::controller::RealtimeController;
use crate::realtime::Relay;
use crate::relay::{self, RelayClient};
use crate::{Realtime, RealtimeOptions};

/// The realtime module: a Pusher-protocol (version 7) WebSocket server in
/// the app's own HTTP server, so `laravel-echo` and `pusher-js` work
/// unchanged.
///
/// Global; provides and exports [`Realtime`] and `dyn Broadcaster` (the same
/// instance), and mounts three routes:
///
/// - `GET /app/{key}` (public; the `Origin` is checked): the WebSocket;
/// - `POST /apps/{app_id}/events` (public; signed): broadcasts relayed by
///   the app's other processes (`realtime.relay_url`);
/// - `POST /broadcasting/auth` (authenticated; form or JSON `socket_id`,
///   `channel_name`): signs private and presence subscriptions after the
///   channel authorizer registered with [`ModuleBuilderRealtimeExt::channel`]
///   allows them; 403 otherwise.
///
/// The auth route needs a `dyn Authenticator` (the auth module), like every
/// non-public route.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_realtime::{RealtimeModule, RealtimeOptions};
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(RealtimeModule::for_root(RealtimeOptions {
///         key: "navree-crm-key".to_owned(),
///         allowed_origins: vec!["http://localhost:3000".to_owned()],
///         ..RealtimeOptions::default()
///     }))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(RealtimeModule)`) or through
/// [`from_config`](RealtimeModule::from_config), it reads the `realtime`
/// configuration section ([`RealtimeOptions`]), so the app must include
/// `ConfigModule`. Invalid options, malformed channel patterns and a pattern
/// registered twice fail the boot.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealtimeModule;

impl RealtimeModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: RealtimeOptions) -> impl Module {
        ConfiguredRealtimeModule {
            options: Some(options),
        }
    }

    /// The module configured from the `realtime` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredRealtimeModule { options: None }
    }
}

impl Module for RealtimeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `RealtimeModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredRealtimeModule {
    options: Option<RealtimeOptions>,
}

impl Module for ConfiguredRealtimeModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<RealtimeModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<RealtimeOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.config::<RealtimeOptions>(),
    };
    m.factory(
        Factory::<Realtime>::new(|r: Resolver| async move {
            let options = r.get::<RealtimeOptions>().await?;
            let app = r.app();
            let cors_origins = match app.try_get::<HttpOptions>().await {
                Some(http) => http.cors.allowed_origins.clone(),
                None => Vec::new(),
            };
            let realtime = Realtime::new(
                (*options).clone(),
                cors_origins,
                app.contributions::<ChannelRegistration>(),
                app.shutdown_token(),
                app.tracker(),
            )?
            .with_relay(relay_of(&options, &app).await?);
            Ok(Arc::new(realtime))
        })
        .deps(vec![ProviderKey::of::<RealtimeOptions>()]),
    )
    .exports::<Realtime>()
    .factory(
        Factory::<dyn Broadcaster>::new(|r: Resolver| async move {
            let realtime: Arc<dyn Broadcaster> = r.get::<Realtime>().await?;
            Ok(realtime)
        })
        .deps(vec![ProviderKey::of::<Realtime>()]),
    )
    .exports::<dyn Broadcaster>()
    .controller::<RealtimeController>();
}

/// The relay of this process: the key (from `realtime.secret`, else the
/// app key) and, with `realtime.relay_url`, the client that posts
/// broadcasts to the serving process.
async fn relay_of(options: &RealtimeOptions, app: &AppContext) -> Result<Relay, BoxError> {
    let encrypter = app.try_get::<Encrypter>().await;
    let key = relay::relay_key(options.secret.expose(), encrypter.as_deref());
    let server = app.try_get::<HttpServer>().await;
    let url = match options.relay_url.trim() {
        "" => None,
        "auto" => server
            .as_ref()
            .map(|server| relay::auto_url(&server.options().addr)),
        url => Some(url.to_owned()),
    };
    let client = match (url, &key) {
        (Some(url), Some(key)) => Some(RelayClient::new(&url, &options.app_id, key.clone())?),
        (Some(url), None) => {
            tracing::warn!(
                target: "navree::realtime",
                relay_url = %url,
                "realtime.relay_url is set, but there is neither realtime.secret nor an app key to sign relayed broadcasts with; broadcasts stay in this process"
            );
            None
        }
        (None, _) => None,
    };
    Ok(Relay {
        key,
        client,
        server,
    })
}

/// What a channel authorizer decides for a private or presence channel.
#[derive(Clone, Debug, PartialEq)]
pub enum ChannelDecision {
    /// The user may not subscribe (the auth endpoint answers 403).
    Deny,
    /// The user may subscribe. On a presence channel the member has no
    /// `user_info`.
    Allow,
    /// The user may subscribe; on a presence channel `user_info` is shown to
    /// the other members (`{"name": "Ada"}`). On a private channel this is
    /// the same as [`Allow`](ChannelDecision::Allow).
    Presence(Value),
}

/// A channel authorizer: the principal asking, the parameters of the
/// matched pattern (`{"id": "42"}` for `users.{id}`) and the app.
pub(crate) type ChannelAuthorizer = Arc<
    dyn Fn(
            &Principal,
            &HashMap<String, String>,
            &AppContext,
        ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>>
        + Send
        + Sync,
>;

/// A channel authorizer registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct ChannelRegistration {
    pub(crate) pattern: &'static str,
    pub(crate) authorize: ChannelAuthorizer,
}

impl fmt::Debug for ChannelRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelRegistration")
            .field("pattern", &self.pattern)
            .finish_non_exhaustive()
    }
}

/// Channel authorizers on a [`ModuleBuilder`] (Laravel's `Broadcast::channel`).
pub trait ModuleBuilderRealtimeExt {
    /// Authorizes private and presence channels whose name, without the
    /// `private-` / `presence-` prefix, matches `pattern`: literal text and
    /// `{name}` parameters, each matching one or more characters other than
    /// `.` (`"crm.deals"`, `"users.{id}"`). The first matching pattern, in
    /// module dependency order, decides; no match means 403.
    ///
    /// The module does not need to import `RealtimeModule`.
    ///
    /// ```
    /// use std::collections::HashMap;
    ///
    /// use navree_contracts::Principal;
    /// use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};
    /// use navree_realtime::{ChannelDecision, ModuleBuilderRealtimeExt};
    ///
    /// /// `private-users.{id}`: only the user themself.
    /// fn own_channel(
    ///     principal: &Principal,
    ///     params: &HashMap<String, String>,
    ///     _ctx: &AppContext,
    /// ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    ///     let own = params.get("id") == Some(&principal.user_id.to_string());
    ///     Box::pin(async move {
    ///         Ok(if own { ChannelDecision::Allow } else { ChannelDecision::Deny })
    ///     })
    /// }
    ///
    /// struct UsersModule;
    ///
    /// impl Module for UsersModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.channel("users.{id}", own_channel);
    ///     }
    /// }
    /// ```
    fn channel<F>(&mut self, pattern: &'static str, authorize: F) -> &mut Self
    where
        F: Fn(
                &Principal,
                &HashMap<String, String>,
                &AppContext,
            ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>>
            + Send
            + Sync
            + 'static;
}

impl ModuleBuilderRealtimeExt for ModuleBuilder {
    fn channel<F>(&mut self, pattern: &'static str, authorize: F) -> &mut Self
    where
        F: Fn(
                &Principal,
                &HashMap<String, String>,
                &AppContext,
            ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>>
            + Send
            + Sync
            + 'static,
    {
        self.contribute(ChannelRegistration {
            pattern,
            authorize: Arc::new(authorize),
        })
    }
}
