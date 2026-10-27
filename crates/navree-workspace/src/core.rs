//! What the services share: the database, the options, the role table, the
//! clock, the user directory and the after-commit helpers.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::{Clock, SystemClock};
use navree_db::{Db, Uow};
use navree_events::{Event, EventBus};
use navree_kernel::{AppContext, WeakAppContext};
use navree_mail::{Mailable, Mailer};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::directory::UserDirectory;
use crate::entity::{
    Member, OrgMember, Organization, Workspace, member, organization, organization_member,
    workspace,
};
use crate::isolation::IsolationDriver;
use crate::{
    OrgRole, OrgRoleTable, RoleTable, WorkspaceConfig, WorkspaceError, WorkspaceRole,
};

/// The setting that turns organizations on.
pub(crate) const ORGANIZATIONS_ENABLED: &str = "workspace.organizations.enabled";
/// The setting that turns partitions on.
pub(crate) const PARTITIONS_ENABLED: &str = "workspace.partitions.enabled";

/// The state the [`Workspaces`](crate::Workspaces),
/// [`Members`](crate::Members), [`Invitations`](crate::Invitations),
/// [`Organizations`](crate::Organizations), [`OrgMembers`](crate::OrgMembers),
/// [`AccessRequests`](crate::AccessRequests) and
/// [`Partitions`](crate::Partitions) services share (a private provider of
/// the module).
pub(crate) struct Core {
    pub(crate) db: Arc<Db>,
    pub(crate) config: Arc<WorkspaceConfig>,
    pub(crate) roles: RoleTable,
    pub(crate) org_roles: OrgRoleTable,
    pub(crate) app: WeakAppContext,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) directory: Arc<dyn UserDirectory>,
    pub(crate) isolation: Arc<IsolationDriver>,
}

impl Core {
    pub(crate) async fn new(
        app: AppContext,
        db: Arc<Db>,
        config: Arc<WorkspaceConfig>,
        directory: Arc<dyn UserDirectory>,
        isolation: Arc<IsolationDriver>,
    ) -> Core {
        let clock = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        Core {
            db,
            roles: RoleTable::from_config(&config),
            org_roles: OrgRoleTable::from_config(&config),
            config,
            app: app.downgrade(),
            clock,
            directory,
            isolation,
        }
    }

    /// The current time, from the app's `dyn Clock` when there is one.
    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Checks that `role` is a built-in or configured role.
    pub(crate) fn check_role(&self, role: &WorkspaceRole) -> Result<(), WorkspaceError> {
        if self.roles.contains(role) {
            return Ok(());
        }
        let known: Vec<&str> = self.roles.names().collect();
        Err(WorkspaceError::invalid(
            "role",
            format!(
                "role must be one of the following values: {}",
                known.join(", ")
            ),
        ))
    }

    /// `FeatureDisabled` unless organizations are on.
    pub(crate) fn require_organizations(&self) -> Result<(), WorkspaceError> {
        if self.config.organizations.enabled {
            Ok(())
        } else {
            Err(WorkspaceError::FeatureDisabled(ORGANIZATIONS_ENABLED))
        }
    }

    /// `FeatureDisabled` unless partitions are on.
    pub(crate) fn require_partitions(&self) -> Result<(), WorkspaceError> {
        if self.config.partitions.enabled {
            Ok(())
        } else {
            Err(WorkspaceError::FeatureDisabled(PARTITIONS_ENABLED))
        }
    }

    /// Checks that `role` is a built-in or configured organization role.
    pub(crate) fn check_org_role(&self, role: &OrgRole) -> Result<(), WorkspaceError> {
        if self.org_roles.contains(role) {
            return Ok(());
        }
        let known: Vec<&str> = self.org_roles.names().collect();
        Err(WorkspaceError::invalid(
            "role",
            format!(
                "role must be one of the following values: {}",
                known.join(", ")
            ),
        ))
    }

    /// The organization with `id` read through `conn`;
    /// `OrganizationNotFound` when there is none.
    pub(crate) async fn organization_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        id: Uuid,
    ) -> Result<Organization, WorkspaceError> {
        organization::Entity::find_by_id(id)
            .one(conn)
            .await?
            .ok_or_else(|| WorkspaceError::OrganizationNotFound(id.to_string()))
    }

    /// The membership of `user` in organization `org`, read through `conn`.
    pub(crate) async fn org_member_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        org: Uuid,
        user: Uuid,
    ) -> Result<Option<OrgMember>, WorkspaceError> {
        Ok(organization_member::Entity::find()
            .filter(organization_member::Column::OrganizationId.eq(org))
            .filter(organization_member::Column::UserId.eq(user))
            .one(conn)
            .await?)
    }

    /// The workspace with `id` read through `conn`; `WorkspaceNotFound` when
    /// there is none.
    pub(crate) async fn workspace_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        id: Uuid,
    ) -> Result<Workspace, WorkspaceError> {
        workspace::Entity::find_by_id(id)
            .one(conn)
            .await?
            .ok_or_else(|| WorkspaceError::WorkspaceNotFound(id.to_string()))
    }

    /// The membership of `user` in `workspace`, read through `conn`.
    pub(crate) async fn member_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        workspace: Uuid,
        user: Uuid,
    ) -> Result<Option<Member>, WorkspaceError> {
        Ok(member::Entity::find()
            .filter(member::Column::WorkspaceId.eq(workspace))
            .filter(member::Column::UserId.eq(user))
            .one(conn)
            .await?)
    }

    /// Emits `event` once `uow` commits, when the app has an `EventBus`.
    pub(crate) async fn emit_after_commit<E: Event>(&self, uow: &Uow, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = bus.emit(event).await {
                    tracing::warn!(target: "navree::workspace", event = E::NAME, %error, "a workspace event listener failed");
                }
            })
        });
    }

    /// The app's `Mailer`, when `MailModule` is in the app.
    pub(crate) async fn mailer(&self) -> Option<Arc<Mailer>> {
        self.app.upgrade()?.try_get::<Mailer>().await
    }

    /// Queues `mail` once `uow` commits; a failure is logged.
    pub(crate) fn mail_after_commit(
        &self,
        uow: &Uow,
        mailer: Arc<Mailer>,
        mail: Box<dyn Mailable>,
    ) {
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = mailer.queue(&*mail).await {
                    tracing::warn!(target: "navree::workspace", %error, "could not send a workspace invitation mail");
                }
            })
        });
    }
}
