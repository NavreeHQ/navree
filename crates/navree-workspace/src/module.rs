//! [`WorkspaceModule`] and [`ModuleBuilderWorkspaceExt`].

use std::sync::Arc;

use chrono::Timelike;
use http::{HeaderName, HeaderValue};
use navree_access::{AccessModule, Subject, WorkspaceGate};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{Clock, ModuleBuilderConformityExt, PermissionCheck, ZoneSource};
use navree_db::{Backend, Db, DbOptions, Executor as _, MigrationSet, ModuleBuilderDbExt};
use navree_http::extract::Request;
use navree_http::middleware::{Next, from_fn};
use navree_http::{AppError, GlobalLayer, IntoResponse, LayerSlot, ModuleBuilderHttpExt, Response};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, Lifecycle, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_orm::hooks::ModelHook;
use navree_orm::{Model, ModuleBuilderOrmExt};
use navree_schedule::ModuleBuilderScheduleExt;

use crate::DemoMode;
use crate::commands::{DEMO_RESET_COMMAND, commands};
use crate::conformity::{self, EntityClass};
use crate::controller::{InvitationController, WorkspaceController};
use crate::core::Core;
use crate::current::{ScopedSubject, TenantCell};
use crate::demo::{DEMO_HEADER, DemoSeeder, DemoWorkspaces};
use crate::directory::{TableUserDirectory, UserDirectory};
use crate::entity::{
    AccessRequest, Invitation, Member, OrgMember, Organization, Partition, Workspace,
};
use crate::error::map_workspace_error;
use crate::gate::Gate;
use crate::isolation::{IsolationDriver, WorkspaceScoped};
use crate::library::{Library, LibraryGuard, LibraryVisibility};
use crate::migration::migrations;
use crate::org_controller::OrganizationController;
use crate::permission_check::WorkspacePermissionCheck;
use crate::tenant::{WorkspaceDatabases, WorkspaceMigrationSet};
use crate::{
    AccessRequests, Invitations, Isolation, Members, OrgMembers, OrganizationZones, Organizations,
    Partitions, WorkspaceConfig, WorkspaceError, WorkspaceZones, Workspaces,
};

/// Multi-tenancy at the workspace level.
///
/// Global; provides and exports [`Workspaces`], [`Members`],
/// [`Invitations`], [`DemoWorkspaces`], [`WorkspaceDatabases`], the `dyn`
/// [`UserDirectory`] and [`WorkspaceConfig`], and contributes:
///
/// - the `workspace` migrations (`workspaces` with its `kind`,
///   `workspace_members`, `workspace_invitations`, after `auth`);
/// - the routes under `{http.api_prefix}` (default `/api`) unless
///   `workspace.routes` is false: `/workspaces...` and
///   `POST /invitations/accept` (see [`WorkspaceController`](crate::WorkspaceController)),
///   and with the `session` feature `POST /workspace/switch`
///   (`workspace.switch_path`, no API prefix), where a browser switches
///   workspace for the `session` resolver;
/// - the subjects `Workspace`, `WorkspaceMember` and `WorkspaceInvitation`
///   for navree-access (permissions `workspace.*`, `workspace.members.*`,
///   `workspace.invitations.*`);
/// - a `dyn WorkspaceGate`, so a module that works in workspaces of its own
///   accord (navree-helm's panel) enters one only for who may;
/// - a `dyn PermissionCheck`, so `navree_contracts::permitted` answers
///   questions about a workspace with the member's role there (evaluated
///   as behind [`InWorkspace`](crate::InWorkspace));
/// - [`WorkspaceZones`] and [`OrganizationZones`], the `dyn ZoneSource`s
///   of workspaces and organizations (their `timezone` and `country`) for
///   navree-time;
/// - a request layer that commits the workspace database's unit of work
///   ([`WorkspaceUow`](crate::WorkspaceUow), database isolation) and adds
///   the `X-Navree-Demo` header to responses about demo workspaces;
/// - the commands `workspace:migrate`, `workspace:prune-invitations`,
///   `workspace:demo-create` and `workspace:demo-reset`;
/// - the scheduled task `workspace:demo-reset` (nightly at
///   `workspace.demo.reset_at` with `workspace.demo.mode = "reset"`; runs
///   when `ScheduleModule` is in the app);
/// - the conformity checks `workspace.entities`, `workspace.repo` and
///   `workspace.tables` that `navree check` runs (see
///   [`ModuleBuilderWorkspaceExt::workspace_entity`]);
/// - the mapping of [`WorkspaceError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule` (for [`from_config`](WorkspaceModule::from_config)
/// and the plain module) and `DatabaseModule`. Its routes declare
/// permissions, so with `workspace.routes` on the app also includes
/// `AccessModule` (navree-access) and an authenticator (navree-auth's
/// `AuthModule`). `EventsModule` receives the events and `MailModule` sends
/// the invitation mails when they are in the app. Schema isolation needs
/// PostgreSQL.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_workspace::WorkspaceModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     // AuthModule, AccessModule, EventsModule, MailModule, ...
///     .module(WorkspaceModule::from_config()) // section `workspace`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceModule;

impl WorkspaceModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(config: WorkspaceConfig) -> impl Module {
        ConfiguredWorkspaceModule {
            config: Some(config),
        }
    }

    /// The module configured from the `workspace` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredWorkspaceModule { config: None }
    }
}

impl Module for WorkspaceModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `WorkspaceModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredWorkspaceModule {
    /// `None`: read from configuration.
    config: Option<WorkspaceConfig>,
}

impl Module for ConfiguredWorkspaceModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<WorkspaceModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.config.clone());
    }
}

fn register(m: &mut ModuleBuilder, config: Option<WorkspaceConfig>) {
    m.global();
    let demo = config.as_ref().map(|config| config.demo.clone());
    match config {
        Some(config) => m.factory(Factory::<WorkspaceConfig>::new(move |_| {
            let config = config.clone();
            async move {
                config.check().map_err(|errors| {
                    format!("invalid workspace options: {}", errors.join("; "))
                })?;
                Ok(Arc::new(config))
            }
        })),
        None => m.config::<WorkspaceConfig>(),
    };
    m.exports::<WorkspaceConfig>()
        .imports((
            AccessModule::for_feature::<Workspace>(),
            AccessModule::for_feature::<Member>(),
            AccessModule::for_feature::<Invitation>(),
            AccessModule::for_feature::<Organization>(),
            AccessModule::for_feature::<OrgMember>(),
            AccessModule::for_feature::<AccessRequest>(),
            AccessModule::for_feature::<Partition>(),
        ))
        .factory(
            Factory::<dyn UserDirectory>::new(|r: Resolver| async move {
                let db = r.get::<Db>().await?;
                let config = r.get::<WorkspaceConfig>().await?;
                let table = config.users_table().map(str::to_owned);
                Ok(Arc::new(TableUserDirectory::new(db, table)) as Arc<dyn UserDirectory>)
            })
            .deps(vec![
                ProviderKey::of::<Db>(),
                ProviderKey::of::<WorkspaceConfig>(),
            ]),
        )
        .exports::<dyn UserDirectory>()
        .factory(
            Factory::<WorkspaceDatabases>::new(|r: Resolver| async move {
                let options = r.get::<DbOptions>().await?;
                let config = r.get::<WorkspaceConfig>().await?;
                Ok(Arc::new(WorkspaceDatabases::new(
                    config.database_url.clone(),
                    DbOptions {
                        migrate_on_boot: false,
                        ..(*options).clone()
                    },
                    r.app().downgrade(),
                )))
            })
            .deps(vec![
                ProviderKey::of::<DbOptions>(),
                ProviderKey::of::<WorkspaceConfig>(),
            ]),
        )
        .exports::<WorkspaceDatabases>()
        .lifecycle::<WorkspaceDatabases>()
        .factory(
            Factory::<IsolationDriver>::new(|r: Resolver| async move {
                let config = r.get::<WorkspaceConfig>().await?;
                let db = r.get::<Db>().await?;
                if config.isolation == Isolation::Schema && db.backend() != Backend::Postgres {
                    return Err(
                        "workspace.isolation = \"schema\" needs PostgreSQL (one schema per workspace)"
                            .into(),
                    );
                }
                Ok(Arc::new(IsolationDriver::new(
                    config.isolation,
                    config.schema_prefix.clone(),
                    r.get::<WorkspaceDatabases>().await?,
                    r.app().downgrade(),
                )))
            })
            .deps(vec![
                ProviderKey::of::<WorkspaceConfig>(),
                ProviderKey::of::<Db>(),
                ProviderKey::of::<WorkspaceDatabases>(),
            ]),
        )
        .factory(
            Factory::<Core>::new(|r: Resolver| async move {
                Ok(Arc::new(
                    Core::new(
                        r.app(),
                        r.get::<Db>().await?,
                        r.get::<WorkspaceConfig>().await?,
                        r.get::<dyn UserDirectory>().await?,
                        r.get::<IsolationDriver>().await?,
                    )
                    .await,
                ))
            })
            .deps(vec![
                ProviderKey::of::<Db>(),
                ProviderKey::of::<WorkspaceConfig>(),
                ProviderKey::of::<dyn UserDirectory>(),
                ProviderKey::of::<IsolationDriver>(),
            ]),
        )
        .factory(
            Factory::<Workspaces>::new(|r: Resolver| async move {
                Ok(Arc::new(Workspaces::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<Workspaces>()
        .factory(
            Factory::<Members>::new(|r: Resolver| async move {
                Ok(Arc::new(Members::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<Members>()
        // Who may work in a workspace, for modules that work in them.
        .factory(
            Factory::<dyn WorkspaceGate>::new(|r: Resolver| async move {
                Ok(Arc::new(Gate(r.get::<Core>().await?)) as Arc<dyn WorkspaceGate>)
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<dyn WorkspaceGate>()
        // Permission questions about a workspace asked outside a route
        // (`navree_contracts::permitted`).
        .multi(
            Factory::<dyn PermissionCheck>::new(|r: Resolver| async move {
                Ok(Arc::new(WorkspacePermissionCheck(r.get::<Core>().await?))
                    as Arc<dyn PermissionCheck>)
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .factory(
            Factory::<Invitations>::new(|r: Resolver| async move {
                Ok(Arc::new(Invitations::new(
                    r.get::<Core>().await?,
                    r.get::<Members>().await?,
                )))
            })
            .deps(vec![ProviderKey::of::<Core>(), ProviderKey::of::<Members>()]),
        )
        .exports::<Invitations>()
        .factory(
            Factory::<Organizations>::new(|r: Resolver| async move {
                Ok(Arc::new(Organizations::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<Organizations>()
        .factory(
            Factory::<OrgMembers>::new(|r: Resolver| async move {
                Ok(Arc::new(OrgMembers::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<OrgMembers>()
        .factory(
            Factory::<AccessRequests>::new(|r: Resolver| async move {
                Ok(Arc::new(AccessRequests::new(
                    r.get::<Core>().await?,
                    r.get::<OrgMembers>().await?,
                )))
            })
            .deps(vec![
                ProviderKey::of::<Core>(),
                ProviderKey::of::<OrgMembers>(),
            ]),
        )
        .exports::<AccessRequests>()
        .factory(
            Factory::<Partitions>::new(|r: Resolver| async move {
                Ok(Arc::new(Partitions::new(r.get::<Core>().await?)))
            })
            .deps(vec![ProviderKey::of::<Core>()]),
        )
        .exports::<Partitions>()
        .multi(
            Factory::<dyn ZoneSource>::new(|r: Resolver| async move {
                Ok(Arc::new(WorkspaceZones::new(r.get::<Workspaces>().await?))
                    as Arc<dyn ZoneSource>)
            })
            .deps(vec![ProviderKey::of::<Workspaces>()]),
        )
        // Silent unless `workspace.organizations.enabled`.
        .multi(
            Factory::<dyn ZoneSource>::new(|r: Resolver| async move {
                Ok(
                    Arc::new(OrganizationZones::new(r.get::<Organizations>().await?))
                        as Arc<dyn ZoneSource>,
                )
            })
            .deps(vec![ProviderKey::of::<Organizations>()]),
        )
        .factory(
            Factory::<DemoWorkspaces>::new(|r: Resolver| async move {
                Ok(Arc::new(DemoWorkspaces::new(
                    r.get::<Core>().await?,
                    r.get::<Workspaces>().await?,
                )))
            })
            .deps(vec![ProviderKey::of::<Core>(), ProviderKey::of::<Workspaces>()]),
        )
        .exports::<DemoWorkspaces>()
        .controller::<WorkspaceController>()
        .controller::<InvitationController>()
        .controller::<OrganizationController>()
        .migrations(migrations())
        .http_layer(tenant_layer())
        .map_error::<WorkspaceError>(map_workspace_error)
        .schedule(|s| match demo {
            // Known now: the command, daily at `reset_at`, in reset mode.
            Some(demo) => {
                if let (DemoMode::Reset, Some((hour, minute))) = (demo.mode, demo.reset_time()) {
                    s.command(DEMO_RESET_COMMAND, &[])
                        .daily_at(&format!("{hour:02}:{minute:02}"))
                        .without_overlapping();
                }
            }
            // Read at boot: check every minute whether it is time.
            None => {
                s.task(DEMO_RESET_COMMAND, reset_demos_if_due)
                    .every_minute()
                    .without_overlapping();
            }
        });
    // `POST {workspace.switch_path}`: a browser switches workspace (the
    // `session` resolver reads the choice).
    #[cfg(feature = "session")]
    m.controller::<crate::switch::SwitchController>();
    for command in commands() {
        m.command(command);
    }
    for check in conformity::checks() {
        m.conformity_check(check);
    }
}

/// The scheduled demo reset of an app configured from `workspace`: resets
/// every demo workspace when `workspace.demo.mode` is `reset` and the
/// app's clock shows `workspace.demo.reset_at` (UTC).
async fn reset_demos_if_due(ctx: AppContext) -> Result<(), BoxError> {
    let config = ctx.get::<WorkspaceConfig>().await?;
    let demo = &config.demo;
    let Some((hour, minute)) = demo.reset_time() else {
        return Ok(());
    };
    if demo.mode != DemoMode::Reset {
        return Ok(());
    }
    let now = match ctx.try_get::<dyn Clock>().await {
        Some(clock) => clock.now(),
        None => chrono::Utc::now(),
    };
    if now.hour() != hour || now.minute() != minute {
        return Ok(());
    }
    let reset = ctx.get::<DemoWorkspaces>().await?.reset_all().await?;
    tracing::info!(target: "navree::workspace", demos = reset.len(), "nightly demo reset");
    Ok(())
}

impl Lifecycle for WorkspaceDatabases {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.close_all().await;
            Ok(())
        })
    }
}

/// The layer that commits (status below 400) or rolls back the workspace
/// database's unit of work of a request, when one was opened, and adds the
/// `X-Navree-Demo` header to responses about a demo workspace.
fn tenant_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Uow, "workspace-uow", |_ctx, router| {
        Ok(router.layer(from_fn(tenant_unit_of_work)))
    })
}

async fn tenant_unit_of_work(mut request: Request, next: Next) -> Response {
    let cell = TenantCell::default();
    request.extensions_mut().insert(cell.clone());
    let response = next.run(request).await;
    let mut response = match cell.0.uow.get() {
        None => response,
        Some(uow) if response.status().as_u16() < 400 => match uow.commit().await {
            Ok(()) => response,
            Err(error) => AppError::internal(error).into_response(),
        },
        Some(uow) => {
            if let Err(error) = uow.rollback().await {
                tracing::warn!(target: "navree::workspace", %error, "rolling back the workspace unit of work failed");
            }
            response
        }
    };
    if let Some(mode) = cell.0.demo.get() {
        response.headers_mut().insert(
            HeaderName::from_static(DEMO_HEADER),
            HeaderValue::from_static(mode.as_str()),
        );
    }
    response
}

/// Workspace registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderWorkspaceExt {
    /// Contributes migrations that run in every workspace's own schema or
    /// database (schema and database isolation): when a workspace is
    /// created (schema isolation), when its database is first opened
    /// (database isolation), and with `workspace:migrate`. Under row
    /// isolation, workspace-owned tables are ordinary migrations
    /// (`m.migrations(..)`).
    fn workspace_migrations(&mut self, set: MigrationSet) -> &mut Self;

    /// Registers model `M` as workspace-scoped: on routes guarded by
    /// [`InWorkspace`](crate::InWorkspace), a `can(.., from = "id")` check
    /// on `M`'s subject first confirms that the record belongs to the
    /// current workspace (a 404 otherwise). It also classifies `M` as a
    /// workspace entity, like [`workspace_entity`](Self::workspace_entity).
    fn workspace_scoped<M: WorkspaceScoped + Subject>(&mut self) -> &mut Self;

    /// Classifies model `M` as a workspace entity: its rows belong to one
    /// workspace (row isolation). `navree check` requires every entity of
    /// the app to be classified, checks that workspace entities are reached
    /// only through [`WorkspaceRepo`](crate::WorkspaceRepo), and that their
    /// table has an indexed workspace column. Use it for models that are
    /// not access subjects (child records such as deal notes); subjects use
    /// [`workspace_scoped`](Self::workspace_scoped). A model declared
    /// `#[model(workspace)]` scopes its own queries and is a workspace
    /// entity to `navree check` without this registration (which still
    /// adds the table check and `WorkspaceRepo`).
    fn workspace_entity<M: WorkspaceScoped>(&mut self) -> &mut Self;

    /// Classifies model `M` as a global entity: one set of rows shared by
    /// every workspace (countries, plans, the app's own settings).
    fn global_entity<M: Model>(&mut self) -> &mut Self;

    /// Registers library model `M` (see [`library`](crate::library)): its
    /// queries see what the current workspace sees
    /// ([`LibraryVisibility`](crate::library::LibraryVisibility)), its
    /// writes inside a workspace keep to the workspace's own items
    /// ([`LibraryGuard`](crate::library::LibraryGuard), a model hook), and
    /// `navree check` classifies its table as a library. Also registers the
    /// model with navree-orm (`m.model::<M>()`).
    ///
    /// ```ignore
    /// impl Module for TemplatesModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.library::<Template>();
    ///     }
    /// }
    /// ```
    fn library<M: Library>(&mut self) -> &mut Self;
    /// Registers demo seeder `S` (a `dyn DemoSeeder` multi-binding): it
    /// fills every demo workspace when [`DemoWorkspaces`] creates or
    /// resets one.
    fn demo_seeder<S: DemoSeeder + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderWorkspaceExt for ModuleBuilder {
    fn demo_seeder<S: DemoSeeder + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn DemoSeeder>::new(|r: Resolver| async move {
                Ok(Arc::new(S::construct(r).await?) as Arc<dyn DemoSeeder>)
            })
            .deps(S::dependencies()),
        )
    }

    fn workspace_migrations(&mut self, set: MigrationSet) -> &mut Self {
        self.contribute(WorkspaceMigrationSet(set))
    }

    fn workspace_scoped<M: WorkspaceScoped + Subject>(&mut self) -> &mut Self {
        self.contribute(ScopedSubject::of::<M>())
            .contribute(EntityClass::workspace::<M>())
    }

    fn workspace_entity<M: WorkspaceScoped>(&mut self) -> &mut Self {
        self.contribute(EntityClass::workspace::<M>())
    }

    fn global_entity<M: Model>(&mut self) -> &mut Self {
        self.contribute(EntityClass::global(M::meta().table()))
    }

    fn library<M: Library>(&mut self) -> &mut Self {
        self.model::<M>()
            .global_scope_of::<M>(LibraryVisibility)
            .multi(Factory::<dyn ModelHook<M>>::new(|_| async {
                Ok(Arc::new(LibraryGuard) as Arc<dyn ModelHook<M>>)
            }))
            .contribute(EntityClass::library(M::meta().table()))
    }
}
