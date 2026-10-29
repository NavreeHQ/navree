//! [`DirectorySink`]: how your app maps the directory into its own model.

use std::collections::HashMap;

use navree_db::Uow;
use navree_integrations::Owner;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::models;
use crate::types::{ChangeAction, DirectoryConnection};
use crate::user::{DirectoryGroup, DirectoryUser};

/// Maps directory changes into your app's model: recipients, employees,
/// members. Register it with
/// [`m.directory_sink::<S>()`](crate::ModuleBuilderDirectoryExt::directory_sink);
/// it is a provider, so it gets its dependencies injected.
///
/// Each method runs inside the change's unit of work
/// ([`SinkContext::uow`]): write through it, and the directory row, your
/// rows and the events commit (or roll back) together. A sync applies its
/// changes in chunks, one transaction per chunk; when a chunk fails, its
/// changes are tried one by one and the failing ones are recorded on the
/// run. SCIM changes run in the request's unit of work, and an error is a
/// 500 to the identity provider, which retries.
///
/// ```no_run
/// use navree_directory::{ChangeAction, DirectorySink, SinkContext, UserChange};
/// use navree_kernel::{BoxError, BoxFuture, Injectable, KernelError, Resolver};
///
/// /// Keeps `recipients` in step with the directory.
/// struct RecipientSink;
///
/// impl Injectable for RecipientSink {
///     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
///         Box::pin(async { Ok(RecipientSink) })
///     }
/// }
///
/// impl DirectorySink for RecipientSink {
///     fn upsert_user<'a>(
///         &'a self,
///         ctx: &'a SinkContext,
///         change: &'a UserChange,
///     ) -> BoxFuture<'a, Result<Option<String>, BoxError>> {
///         Box::pin(async move {
///             let workspace = ctx.owner().id();
///             let email = change.user.primary_email().unwrap_or(&change.user.user_name);
///             // Insert or update the recipient keyed by (workspace, change.directory_user_id)
///             // through ctx.uow(), and return its id.
///             let _ = (workspace, email, change.action == ChangeAction::Create);
///             Ok(Some("recipient-id".to_owned()))
///         })
///     }
///
///     fn deactivate_user<'a>(
///         &'a self,
///         _ctx: &'a SinkContext,
///         change: &'a UserChange,
///     ) -> BoxFuture<'a, Result<(), BoxError>> {
///         Box::pin(async move {
///             // Archive the recipient change.local_id points at.
///             let _ = &change.local_id;
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait DirectorySink: Send + Sync + 'static {
    /// A user was created, updated or reactivated (`change.action`).
    /// Create or update your record and return its id: it is kept as the
    /// user's `local_id` and handed back on later changes. A user can
    /// arrive inactive (`change.user.active`), for example a disabled
    /// account in a first pull.
    fn upsert_user<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a UserChange,
    ) -> BoxFuture<'a, Result<Option<String>, BoxError>>;

    /// A user was deactivated: disabled in the directory, removed from it,
    /// missing from a full pull, or deleted over SCIM (`change.deleted`).
    fn deactivate_user<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a UserChange,
    ) -> BoxFuture<'a, Result<(), BoxError>>;

    /// A group was created or changed, its members included
    /// (`change.added`, `change.removed`). Return your record's id, if
    /// any. Does nothing unless you implement it.
    fn upsert_group<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a GroupChange,
    ) -> BoxFuture<'a, Result<Option<String>, BoxError>> {
        let _ = (ctx, change);
        Box::pin(async { Ok(None) })
    }

    /// A group was deleted. Does nothing unless you implement it.
    fn delete_group<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a GroupChange,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        let _ = (ctx, change);
        Box::pin(async { Ok(()) })
    }
}

/// Where a change is applied: the connection, the unit of work and the
/// app.
#[derive(Clone)]
pub struct SinkContext {
    pub(crate) connection: DirectoryConnection,
    pub(crate) uow: Uow,
    pub(crate) app: AppContext,
    pub(crate) run_id: Option<Uuid>,
}

impl SinkContext {
    /// The connection the change comes from.
    pub fn connection(&self) -> &DirectoryConnection {
        &self.connection
    }

    /// Who the connection belongs to: the workspace or organization your
    /// records go in.
    pub fn owner(&self) -> Owner {
        self.connection.owner()
    }

    /// The change's unit of work: write through it.
    pub fn uow(&self) -> &Uow {
        &self.uow
    }

    /// The app, to resolve services.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The run applying the change (`None` for SCIM requests).
    pub fn run_id(&self) -> Option<Uuid> {
        self.run_id
    }

    /// The `local_id`s your sink returned for users of this connection, by
    /// their `external_id` (users without one are left out): what a group
    /// change needs to map its members.
    pub async fn local_ids(
        &self,
        external_ids: &[String],
    ) -> Result<HashMap<String, String>, BoxError> {
        if external_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut found = HashMap::new();
        for chunk in external_ids.chunks(500) {
            let rows = models::User::query()
                .where_(models::User::CONNECTION_ID.eq(self.connection.id))
                .where_(models::User::EXTERNAL_ID.is_in(chunk.iter().cloned()))
                .get(&self.uow)
                .await?
                .into_models();
            for row in rows {
                if let Some(local_id) = row.local_id {
                    found.insert(row.external_id, local_id);
                }
            }
        }
        Ok(found)
    }
}

impl std::fmt::Debug for SinkContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SinkContext")
            .field("connection", &self.connection.id)
            .field("run_id", &self.run_id)
            .finish_non_exhaustive()
    }
}

/// A change to one user.
#[derive(Clone, Debug, PartialEq)]
pub struct UserChange {
    /// `Create`, `Update`, `Reactivate` or `Deactivate`.
    pub action: ChangeAction,
    /// The id Navree gave the user (the SCIM id): stable for the person
    /// within the connection.
    pub directory_user_id: Uuid,
    /// The user after the change.
    pub user: DirectoryUser,
    /// The user before it (`None` when created).
    pub previous: Option<DirectoryUser>,
    /// The fields that changed.
    pub changed: Vec<String>,
    /// Your record's id, as your sink last returned it.
    pub local_id: Option<String>,
    /// Whether the SCIM client deleted the user.
    pub deleted: bool,
}

/// A change to one group (members by user `external_id`).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupChange {
    /// `Create`, `Update` or `Delete`.
    pub action: ChangeAction,
    /// The id Navree gave the group (the SCIM id).
    pub directory_group_id: Uuid,
    /// The group after the change, with every member.
    pub group: DirectoryGroup,
    /// The group before it.
    pub previous: Option<DirectoryGroup>,
    /// The attribute fields that changed (and `members`).
    pub changed: Vec<String>,
    /// Members added.
    pub added: Vec<String>,
    /// Members removed.
    pub removed: Vec<String>,
    /// Your record's id, as your sink last returned it.
    pub local_id: Option<String>,
}
