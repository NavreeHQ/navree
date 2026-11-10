//! [`DirectoryModule`] and [`ModuleBuilderDirectoryExt`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_events::{Listener, ModuleBuilderEventsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_integrations::{ModuleBuilderIntegrationsExt, Owner, ScopeSet};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, ConfiguredModule, Factory, Injectable, KernelError, Module,
    ModuleBuilder, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_workspace::events::{OrganizationDeleted, WorkspaceDeleted};

use crate::commands::commands;
use crate::controller::DirectoryController;
use crate::error::map_directory_error;
use crate::migration::migrations;
use crate::runs::SyncDirectory;
use crate::runtime::Runtime;
use crate::scim::controller::ScimController;
use crate::sink::DirectorySink;
use crate::source::{DIRECTORY_SCOPE_SET, DirectorySource, SourceRegistration};
use crate::{Directory, DirectoryError, DirectoryOptions};

/// Directory sync: a SCIM 2.0 server and pull connectors.
///
/// Global; provides and exports [`Directory`] and [`DirectoryOptions`],
/// and contributes:
///
/// - the SCIM endpoints under `directory.scim.path` (default `/scim/v2`;
///   [`ScimController`](crate::ScimController)) unless
///   `directory.scim.enabled` is false;
/// - the routes under `{http.api_prefix}/directory` unless
///   `directory.routes` is false
///   ([`DirectoryController`](crate::DirectoryController); permission
///   `directory.manage`);
/// - the `directory` migration set;
/// - the job `directory.sync`, the commands `directory:sync`,
///   `directory:runs`, `directory:connections`, `directory:token` and
///   `directory:prune`, `directory:sync --due` every five minutes and
///   `directory:prune` daily at 03:50;
/// - the integrations scope set `directory` ([`DirectoryScopes`]), which
///   Microsoft Graph, Google Workspace and Okta connections use;
/// - listeners that forget a deleted workspace's or organization's
///   connections;
/// - the mapping of [`DirectoryError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule` and `DatabaseModule`;
/// `CryptoModule` to store connection secrets (the LDAP bind password),
/// `IntegrationsModule` for the Microsoft, Google and Okta sources,
/// `QueueModule` to run syncs as operations, `ScheduleModule` for the
/// schedule, `EventsModule` for the events, `WorkspaceModule` for
/// workspace and organization owners, and the routes need an
/// authenticator and `AccessModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_directory::DirectoryModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(DirectoryModule::from_config()) // section `directory`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct DirectoryModule;

impl DirectoryModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: DirectoryOptions) -> impl Module {
        ConfiguredModule::<DirectoryModule, _>::for_root(options, register)
    }

    /// The module configured from the `directory` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<DirectoryModule, DirectoryOptions>::from_config(register)
    }

    /// Registers sink `S` (import it in the feature module that owns your
    /// model): every directory change is handed to it.
    pub fn for_feature<S: DirectorySink + Injectable>() -> impl Module {
        SinkFeature::<S>(PhantomData)
    }
}

impl Module for DirectoryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<DirectoryOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<DirectoryOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|errors| {
                    format!("invalid directory options: {}", errors.join("; "))
                })?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<DirectoryOptions>(),
    };
    m.exports::<DirectoryOptions>()
        .factory(
            Factory::<Directory>::new(|r: Resolver| async move {
                let runtime = Runtime::build(&r.app(), None).await?;
                Ok(Arc::new(Directory::real(runtime)))
            })
            .deps(vec![
                ProviderKey::of::<DirectoryOptions>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .exports::<Directory>()
        .lifecycle::<Directory>()
        .controller::<ScimController>()
        .controller::<DirectoryController>()
        .migrations(migrations())
        .map_error::<DirectoryError>(map_directory_error)
        .job::<SyncDirectory>()
        .integration_scope_set::<DirectoryScopes>()
        .listen::<WorkspaceDeleted, ForgetDeletedOwners>()
        .on::<OrganizationDeleted, ForgetDeletedOwners>()
        .schedule(|s| {
            s.command("directory:sync", &["--due"])
                .every_five_minutes()
                .without_overlapping();
            s.command("directory:prune", &[]).daily_at("03:50");
        });
    for command in commands() {
        m.command(command);
    }
}

/// `DirectoryModule::for_feature::<S>()`.
struct SinkFeature<S>(PhantomData<fn() -> S>);

impl<S: DirectorySink + Injectable> Module for SinkFeature<S> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.directory_sink::<S>();
    }
}

/// Directory registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderDirectoryExt {
    /// Registers sink `S` (a provider of this module): every directory
    /// change is handed to it, in registration order.
    fn directory_sink<S: DirectorySink + Injectable>(&mut self) -> &mut Self;

    /// Registers a pull source under `name` (lower-case letters, digits,
    /// `-` and `_`): connections with `source = name` read it.
    fn directory_source(
        &mut self,
        name: impl Into<String>,
        source: impl DirectorySource,
    ) -> &mut Self;
}

impl ModuleBuilderDirectoryExt for ModuleBuilder {
    fn directory_sink<S: DirectorySink + Injectable>(&mut self) -> &mut Self {
        self.provider::<S>().multi(
            Factory::<dyn DirectorySink>::new(|r: Resolver| async move {
                Ok(r.get::<S>().await? as Arc<dyn DirectorySink>)
            })
            .deps(vec![ProviderKey::of::<S>()]),
        )
    }

    fn directory_source(
        &mut self,
        name: impl Into<String>,
        source: impl DirectorySource,
    ) -> &mut Self {
        self.contribute(SourceRegistration {
            name: name.into(),
            source: Arc::new(source),
        })
    }
}

/// The scopes the `microsoft`, `google` and `okta` sources need: read
/// users, groups and memberships, nothing else (integrations scope set
/// `directory`).
#[derive(Clone, Copy, Debug, Default)]
pub struct DirectoryScopes;

impl ScopeSet for DirectoryScopes {
    const NAME: &'static str = DIRECTORY_SCOPE_SET;
    const DESCRIPTION: &'static str = "Read users, groups and memberships";

    fn scopes(provider: &str) -> &'static [&'static str] {
        match provider {
            "microsoft" => &[
                "https://graph.microsoft.com/User.Read.All",
                "https://graph.microsoft.com/GroupMember.Read.All",
            ],
            "google" => &[
                "https://www.googleapis.com/auth/admin.directory.user.readonly",
                "https://www.googleapis.com/auth/admin.directory.group.readonly",
                "https://www.googleapis.com/auth/admin.directory.group.member.readonly",
            ],
            "okta" => &["okta.users.read", "okta.groups.read"],
            _ => &[],
        }
    }
}

/// Forgets the connections of a deleted workspace or organization.
struct ForgetDeletedOwners;

impl Injectable for ForgetDeletedOwners {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ForgetDeletedOwners) })
    }
}

impl Listener<WorkspaceDeleted> for ForgetDeletedOwners {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move { forget(ctx, Owner::Workspace(event.workspace_id)).await })
    }
}

impl Listener<OrganizationDeleted> for ForgetDeletedOwners {
    fn handle<'a>(
        &'a self,
        event: &'a OrganizationDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move { forget(ctx, Owner::Organization(event.organization_id)).await })
    }
}

async fn forget(ctx: &AppContext, owner: Owner) -> Result<(), BoxError> {
    let directory = ctx.get::<Directory>().await?;
    match directory.forget(owner).await {
        Ok(_) | Err(DirectoryError::NotBound) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
