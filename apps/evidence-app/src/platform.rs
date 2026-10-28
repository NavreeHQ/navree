//! [`platform`]: every framework module the evidence app uses, configured
//! once from `config/app.toml` (and `APP_*` environment variables, which
//! the e2e harness sets in `e2e/docker/evidence.env`).

use std::collections::HashMap;

use navree::contracts::Principal;
use navree::realtime::ChannelDecision;
use navree::workspace::Workspaces;
use navree::{BoxFuture, prelude::*};
use uuid::Uuid;

use crate::abilities;

/// The private channel of a workspace's live updates
/// (`private-workspace.{id}`); its members may subscribe.
pub const WORKSPACE_CHANNEL: &str = "workspace.{id}";

/// Adds the framework modules to `builder`, configured from `sources`.
///
/// The foundation first (config, logs, crypto, HTTP, the database and the
/// ORM, cache, events, views, mail, queue, schedule, validation), then
/// accounts and tenancy (auth, access, workspaces, sessions), then every
/// package and tool the evidence app shows. Each `from_config()` module
/// reads its section of `config/app.toml` at boot.
pub fn platform(builder: AppBuilder, sources: ConfigSources) -> AppBuilder {
    builder
        // Foundation.
        .module(ConfigModule::for_root(sources))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(HttpClientModule::from_config())
        .module(DatabaseModule::from_config())
        .module(OrmModule::from_config())
        .module(RedisModule::from_config())
        .module(CacheModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::from_config())
        .module(navree::ui::UiModule::from_config())
        .module(TimeModule::from_config())
        .module(MailModule::from_config())
        .module(QueueModule::from_config())
        .module(ScheduleModule::for_root())
        .module(ValidationModule::from_config())
        .module(StorageModule::from_config())
        // Accounts, access and tenancy.
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities: abilities::define,
            super_admin_role: Some(abilities::SUPER_ADMIN.to_owned()),
        }))
        .module(WorkspaceModule::from_config())
        .module(SessionModule::from_config())
        .module(AuditModule::from_config())
        // Packages.
        .module(NotifyModule::from_config())
        .module(RealtimeModule::from_config())
        .module(SearchModule::from_config())
        .module(BillingModule::from_config())
        .module(SocialModule::from_config())
        .module(ApiModule::from_config())
        .module(ObserveModule::from_config())
        // Tools.
        .module(HelmModule::from_config())
        .module(LighthouseModule::from_config())
        .module(LogbookModule::from_config())
        .module(FleetModule::from_config())
        .module(ChannelsModule)
}

/// The private channels: `private-workspace.{id}` for the workspace's
/// members.
struct ChannelsModule;

impl Module for ChannelsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel(WORKSPACE_CHANNEL, workspace_channel);
    }
}

/// `private-workspace.{id}`: members of that workspace.
fn workspace_channel(
    principal: &Principal,
    params: &HashMap<String, String>,
    ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let user = principal.user_id;
    let workspace = params.get("id").and_then(|id| id.parse::<Uuid>().ok());
    let ctx = ctx.clone();
    Box::pin(async move {
        let Some(workspace) = workspace else {
            return Ok(ChannelDecision::Deny);
        };
        let workspaces = ctx.get::<Workspaces>().await?;
        Ok(match workspaces.membership(workspace, user).await? {
            Some(_) => ChannelDecision::Allow,
            None => ChannelDecision::Deny,
        })
    })
}
