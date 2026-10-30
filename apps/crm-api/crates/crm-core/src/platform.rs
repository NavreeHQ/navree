//! [`platform`]: every framework module the CRM uses, configured once.

use std::collections::HashMap;

use navree::access::{AbilityFactory, READ};
use navree::config::Config;
use navree::contracts::{Clock, Principal, SystemClock};
use navree::realtime::ChannelDecision;
use navree::{BoxFuture, prelude::*};

use crate::abilities;
use crate::subjects::DEAL;

/// The realtime application key: clients connect to `ws://host/app/navree-crm-key`.
/// A default; `realtime.key` overrides it.
pub const REALTIME_KEY: &str = "navree-crm-key";

/// The private channel deal changes are broadcast on (`private-crm.deals`,
/// event `deal.updated`); any user who can read deals may subscribe.
pub const DEALS_CHANNEL: &str = "private-crm.deals";

/// The event deal changes are broadcast as.
pub const DEAL_UPDATED: &str = "deal.updated";

/// The proxies whose `X-Forwarded-For` names the client, unless
/// `http.trusted_proxies` says otherwise: crm-web's backend-for-frontend
/// calls the API from loopback in the documented setup, for
/// every browser, and names the browser in that header. Without this, all
/// web users share one per-IP rate limit, and one browser's wrong passwords
/// throttle everyone's logins to that account. Clients that reach the API
/// directly from elsewhere cannot set their address. Set
/// `APP_HTTP__TRUSTED_PROXIES` to the BFF's (and any reverse proxy's)
/// address when they run elsewhere, or to an empty string to trust none.
pub const TRUSTED_PROXIES: &str = "127.0.0.1,::1";

/// Defaults the CRM gives settings that the configuration does not set.
const DEFAULTS: &[(&str, &str)] = &[
    ("realtime.key", REALTIME_KEY),
    ("realtime.app_id", "navree-crm"),
    ("api.title", "Navree CRM API"),
    ("mail.from", "Navree CRM <crm@navree.test>"),
    ("http.trusted_proxies", TRUSTED_PROXIES),
    // The MCP server (`crm_api::app` adds `McpModule`).
    ("mcp.name", "navree-crm"),
    ("mcp.title", "Navree CRM"),
    ("mcp.instructions", MCP_INSTRUCTIONS),
];

/// What MCP clients tell the model about the CRM's MCP server, unless
/// `mcp.instructions` says otherwise.
pub const MCP_INSTRUCTIONS: &str = "Tools for the Navree CRM. Deals move through the stages lead, \
     qualified, proposal, negotiation, won and lost. Amounts are in cents.";

/// Adds every platform module the CRM uses to `builder`, configured from
/// `sources`:
///
/// Config, Logger, Crypto, Http, Database, Orm (strict outside
/// production: a relation loaded record by record, a column a query left
/// out or an attribute that cannot be filled is an error), Cache, Events,
/// View, Mail, Queue, Schedule, Validation, Auth (public registration off:
/// accounts are created by admins), Access ([`abilities::define`],
/// super-admin role `admin`), Audit, Notify, Realtime (key
/// [`REALTIME_KEY`]; channels `private-crm.deals` for users who can read
/// deals and `private-users.{id}` for that user only), Api and Observe;
/// plus the app's `dyn Clock` ([`SystemClock`]), which CRM services inject
/// for "now" (`Arc<dyn Clock>`) so tests can fix it.
///
/// The CRM modules come on top: `CrmCoreModule` and the feature modules
/// (`crm_api::app` adds them all).
///
/// ```no_run
/// use crm_core::{CrmCoreModule, platform};
/// use navree::prelude::*;
///
/// # fn main() -> Result<(), BoxError> {
/// platform(Navree::app(), ConfigSources::defaults())
///     .module(CrmCoreModule)
///     .run()
/// # }
/// ```
pub fn platform(builder: AppBuilder, sources: ConfigSources) -> AppBuilder {
    let sources = with_defaults(sources).set("auth.registration", false);
    builder
        .module(ConfigModule::for_root(sources))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(OrmModule::from_config())
        .module(CacheModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(QueueModule::from_config())
        .module(ScheduleModule::for_root())
        .module(ValidationModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(abilities::access_options()))
        .module(AuditModule::from_config())
        .module(NotifyModule::from_config())
        .module(RealtimeModule::from_config())
        .module(ApiModule::from_config())
        .module(ObserveModule::from_config())
        .module(CrmPlatformModule)
}

/// `sources` plus [`DEFAULTS`] for the keys they leave unset. When the
/// sources cannot be read, they are returned as they are (the boot then
/// reports the problem).
fn with_defaults(sources: ConfigSources) -> ConfigSources {
    let Ok(config) = Config::load(&sources) else {
        return sources;
    };
    // Any value counts as set (a list, a number, an empty string).
    let is_set = |key: &str| config.section::<serde_json::Value>(key).is_ok();
    DEFAULTS
        .iter()
        .filter(|(key, _)| !is_set(key))
        .fold(sources, |sources, (key, value)| sources.set(key, *value))
}

/// What the CRM adds to the platform: the app's `dyn Clock` (the system
/// clock; tests override it with a fixed one) and the private channel
/// authorizers. Global, so every module can inject `Arc<dyn Clock>`.
struct CrmPlatformModule;

impl Module for CrmPlatformModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global()
            .value_arc::<dyn Clock>(Arc::new(SystemClock))
            .exports::<dyn Clock>()
            .channel("crm.deals", deals_channel)
            .channel("users.{id}", user_channel);
    }
}

/// `private-crm.deals`: users who can read deals.
fn deals_channel(
    principal: &Principal,
    _params: &HashMap<String, String>,
    ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let (principal, ctx) = (principal.clone(), ctx.clone());
    Box::pin(async move {
        let factory = ctx.get::<AbilityFactory>().await?;
        let ability = factory.for_principal(&Some(principal)).await?;
        Ok(if ability.can(READ, DEAL, None) {
            ChannelDecision::Allow
        } else {
            ChannelDecision::Deny
        })
    })
}

/// `private-users.{id}` (notifications): only that user.
fn user_channel(
    principal: &Principal,
    params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let own = params.get("id") == Some(&principal.user_id.to_string());
    Box::pin(async move {
        Ok(if own {
            ChannelDecision::Allow
        } else {
            ChannelDecision::Deny
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config(sources: ConfigSources) -> Config {
        Config::load(&with_defaults(sources)).expect("the configuration loads")
    }

    #[test]
    fn defaults_fill_only_the_keys_left_unset() {
        let defaults = config(ConfigSources::new());
        assert_eq!(
            defaults.get_string("http.trusted_proxies").as_deref(),
            Some(TRUSTED_PROXIES)
        );
        assert_eq!(
            defaults.get_string("realtime.key").as_deref(),
            Some(REALTIME_KEY)
        );

        // A list is a setting too (not only strings).
        let listed = config(ConfigSources::new().set("http.trusted_proxies", json!(["10.0.0.5"])));
        assert_eq!(
            listed
                .section::<Vec<String>>("http.trusted_proxies")
                .unwrap(),
            ["10.0.0.5"]
        );
        // An empty value trusts no proxy.
        let none =
            config(ConfigSources::new().env_vars("APP_", [("APP_HTTP__TRUSTED_PROXIES", "")]));
        assert_eq!(none.get_string("http.trusted_proxies").as_deref(), Some(""));
    }
}
