//! [`WorkspaceModule`] and [`ModuleBuilderWorkspaceExt`].

use std::sync::Arc;

use axum::extract::Request;
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use chrono::Timelike;
use http::{HeaderName, HeaderValue};
use navree_access::{AccessModule, Subject};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::Clock;
use navree_db::{Db, DbOptions, MigrationSet, ModuleBuilderDbExt};
use navree_http::{AppError, GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, Lifecycle, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use sea_orm::{DbBackend, IntoActiveModel};
use serde::Serialize;

use crate::DemoMode;
use crate::commands::{DEMO_RESET_COMMAND, commands};
use crate::controller::{InvitationController, WorkspaceController};
use crate::core::Core;
use crate::current::{ScopedSubject, TenantCell};
use crate::demo::{DEMO_HEADER, DemoSeeder, DemoWorkspaces};
use crate::directory::{TableUserDirectory, UserDirectory};
use crate::entity::{invitation, member, workspace};
use crate::error::map_workspace_error;
use crate::isolation::{IsolationDriver, WorkspaceScoped};
use crate::migration::migrations;
use crate::tenant::{WorkspaceDatabases, WorkspaceMigrationSet};
use crate::{Invitations, Isolation, Members, WorkspaceConfig, WorkspaceError, Workspaces};

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
///   `POST /invitations/accept` (see [`WorkspaceController`](crate::WorkspaceController));
/// - the subjects `Workspace`, `WorkspaceMember` and `WorkspaceInvitation`
///   for navree-access (permissions `workspace.*`, `workspace.members.*`,
///   `workspace.invitations.*`);
/// - a request layer that commits the workspace database's unit of work
///   ([`WorkspaceUow`](crate::WorkspaceUow), database isolation) and adds
///   the `X-Navree-Demo` header to responses about demo workspaces;
/// - the commands `workspace:migrate`, `workspace:prune-invitations`,
///   `workspace:demo-create` and `workspace:demo-reset`;
/// - the scheduled task `workspace:demo-reset` (nightly at
///   `workspace.demo.reset_at` with `workspace.demo.mode = "reset"`; runs
///   when `ScheduleModule` is in the app);
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
            AccessModule::for_feature::<workspace::Entity>(),
            AccessModule::for_feature::<member::Entity>(),
            AccessModule::for_feature::<invitation::Entity>(),
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
                if config.isolation == Isolation::Schema && db.backend() != DbBackend::Postgres {
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
    for command in commands() {
        m.command(command);
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

    /// Registers `E` as workspace-scoped: on routes guarded by
    /// [`InWorkspace`](crate::InWorkspace), a `can(.., from = "id")` check
    /// on `E`'s subject first confirms that the record belongs to the
    /// current workspace (a 404 otherwise).
    fn workspace_scoped<E>(&mut self) -> &mut Self
    where
        E: WorkspaceScoped + Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync;

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

    fn workspace_scoped<E>(&mut self) -> &mut Self
    where
        E: WorkspaceScoped + Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
    {
        self.contribute(ScopedSubject::of::<E>())
    }
}
