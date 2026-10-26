//! [`WorkspaceModule`] and [`ModuleBuilderWorkspaceExt`].

use std::sync::Arc;

use axum::extract::Request;
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use navree_access::{AccessModule, Subject};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderConformityExt;
use navree_db::{Db, DbOptions, MigrationSet, ModuleBuilderDbExt};
use navree_http::{AppError, GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};
use sea_orm::{DbBackend, EntityTrait, IntoActiveModel};
use serde::Serialize;

use crate::commands::commands;
use crate::conformity::{self, EntityClass};
use crate::controller::{InvitationController, WorkspaceController};
use crate::core::Core;
use crate::current::{ScopedSubject, TenantCell};
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
/// [`Invitations`], [`WorkspaceDatabases`], the `dyn`
/// [`UserDirectory`] and [`WorkspaceConfig`], and contributes:
///
/// - the `workspace` migrations (`workspaces`, `workspace_members`,
///   `workspace_invitations`, after `auth`);
/// - the routes under `{http.api_prefix}` (default `/api`) unless
///   `workspace.routes` is false: `/workspaces...` and
///   `POST /invitations/accept` (see [`WorkspaceController`](crate::WorkspaceController));
/// - the subjects `Workspace`, `WorkspaceMember` and `WorkspaceInvitation`
///   for navree-access (permissions `workspace.*`, `workspace.members.*`,
///   `workspace.invitations.*`);
/// - a request layer that commits the workspace database's unit of work
///   ([`WorkspaceUow`](crate::WorkspaceUow), database isolation);
/// - the commands `workspace:migrate` and `workspace:prune-invitations`;
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
        .controller::<WorkspaceController>()
        .controller::<InvitationController>()
        .migrations(migrations())
        .http_layer(tenant_layer())
        .map_error::<WorkspaceError>(map_workspace_error);
    for command in commands() {
        m.command(command);
    }
    for check in conformity::checks() {
        m.conformity_check(check);
    }
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
/// database's unit of work of a request, when one was opened.
fn tenant_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Uow, "workspace-uow", |_ctx, router| {
        Ok(router.layer(from_fn(tenant_unit_of_work)))
    })
}

async fn tenant_unit_of_work(mut request: Request, next: Next) -> Response {
    let cell = TenantCell::default();
    request.extensions_mut().insert(cell.clone());
    let response = next.run(request).await;
    let Some(uow) = cell.0.get() else {
        return response;
    };
    if response.status().as_u16() < 400 {
        match uow.commit().await {
            Ok(()) => response,
            Err(error) => AppError::internal(error).into_response(),
        }
    } else {
        if let Err(error) = uow.rollback().await {
            tracing::warn!(target: "navree::workspace", %error, "rolling back the workspace unit of work failed");
        }
        response
    }
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
    /// current workspace (a 404 otherwise). It also classifies `E` as a
    /// workspace entity, like [`workspace_entity`](Self::workspace_entity).
    fn workspace_scoped<E>(&mut self) -> &mut Self
    where
        E: WorkspaceScoped + Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync;

    /// Classifies `E` as a workspace entity: its rows belong to one
    /// workspace (row isolation). `navree check` requires every entity of
    /// the app to be classified, checks that workspace entities are reached
    /// only through [`WorkspaceRepo`](crate::WorkspaceRepo), and that their
    /// table has an indexed workspace column. Use it for entities that are
    /// not access subjects (child records such as deal notes); subjects use
    /// [`workspace_scoped`](Self::workspace_scoped).
    fn workspace_entity<E: WorkspaceScoped>(&mut self) -> &mut Self;

    /// Classifies `E` as a global entity: one set of rows shared by every
    /// workspace (countries, plans, the app's own settings).
    fn global_entity<E: EntityTrait>(&mut self) -> &mut Self;
}

impl ModuleBuilderWorkspaceExt for ModuleBuilder {
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
            .contribute(EntityClass::workspace::<E>())
    }

    fn workspace_entity<E: WorkspaceScoped>(&mut self) -> &mut Self {
        self.contribute(EntityClass::workspace::<E>())
    }

    fn global_entity<E: EntityTrait>(&mut self) -> &mut Self {
        self.contribute(EntityClass::global::<E>())
    }
}
