//! [`platform`]: every framework module the app uses, configured once.

use std::collections::HashMap;

use navree::contracts::Principal;
use navree::realtime::ChannelDecision;
use navree::social::SocialModule;
use navree::workspace::Workspaces;
use navree::{BoxFuture, prelude::*};
use uuid::Uuid;

use crate::abilities;

/// The private channel of a workspace's live updates
/// (`private-workspace.{id}`); its members may subscribe.
pub const WORKSPACE_CHANNEL: &str = "workspace.{id}";

/// Adds the framework modules to `builder`, configured from `sources`:
///
/// Config, Logger, Crypto, Http, Database, Cache, Events, View, Mail,
/// Queue, Schedule, Validation, Auth, Session, Ui, AuthPages (the sign-in
/// pages of the tools), Access ([`abilities::define`], the
/// super-admin role `admin`), Workspace, Billing, Social, Notify,
/// Realtime, Audit, Api, Observe and Helm, plus the workspace channel
/// authorizer. Each `from_config()` module reads its section of
/// `config/app.toml` at boot.
pub fn platform(builder: AppBuilder, sources: ConfigSources) -> AppBuilder {
    builder
        .module(ConfigModule::for_root(sources))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(CacheModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(QueueModule::from_config())
        .module(ScheduleModule::for_root())
        .module(ValidationModule::from_config())
        // Accounts: JWT bearer tokens, refresh tokens, two-factor, email
        // verification and password reset (`/api/auth/*`).
        .module(AuthModule::from_config())
        // A browser signs in on the server-side session: the pages that Helm and
        // Lighthouse send guests to.
        .module(SessionModule::from_config())
        .module(UiModule::from_config())
        .module(navree::auth::AuthPages::new())
        // Roles, permissions and abilities (`GET /api/me/abilities`); the
        // `admin` role passes every check (Helm's super admins).
        .module(AccessModule::for_root(AccessOptions {
            abilities: abilities::define,
            super_admin_role: Some(abilities::SUPER_ADMIN.to_owned()),
        }))
        // Workspaces, members, roles and invitations (`/api/workspaces/*`).
        .module(WorkspaceModule::from_config())
        // Plans, checkout, the customer portal and invoices, per workspace
        // (`/api/billing/*`).
        .module(BillingModule::from_config())
        // "Continue with ..." for every provider under `social.providers`.
        .module(SocialModule::from_config())
        .module(NotifyModule::from_config())
        // WebSockets (Pusher protocol) at /app/{key}; channel auth at
        // /broadcasting/auth.
        .module(RealtimeModule::from_config())
        .module(AuditModule::from_config())
        // OpenAPI (`/api/openapi.json`, `/api/docs`), health and metrics.
        .module(ApiModule::from_config())
        .module(ObserveModule::from_config())
        // The admin panel at /admin (resources in `modules/admin`).
        .module(HelmModule::from_config())
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
