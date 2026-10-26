//! [`AgentRun`]: a tool's view of who is asking and where it runs.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_access::{Ability, Accessible, ActionMarker, Authorized, PrincipalAccess, Subject};
use navree_auth::User;
use navree_contracts::Clock;
use navree_db::{Repo, Uow};
use navree_http::AppError;
use navree_kernel::AppContext;
use sea_orm::{EntityTrait, IntoActiveModel};
use serde::Serialize;
use uuid::Uuid;

/// What every tool call of a run shares.
pub(crate) struct RunShared {
    pub(crate) ctx: AppContext,
    pub(crate) user: User,
    pub(crate) access: Arc<PrincipalAccess>,
    pub(crate) prefixes: Arc<BTreeMap<String, String>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) conversation_id: Option<Uuid>,
}

/// The run as a tool sees it: the user it acts for (with their roles,
/// permissions and token restrictions), the tool call's unit of work, the
/// clock and the conversation.
#[derive(Clone)]
pub struct AgentRun {
    shared: Arc<RunShared>,
    uow: Uow,
}

impl AgentRun {
    pub(crate) fn new(shared: Arc<RunShared>, uow: Uow) -> Self {
        Self { shared, uow }
    }

    /// The signed-in user.
    pub fn user(&self) -> &User {
        &self.shared.user
    }

    /// Their id.
    pub fn user_id(&self) -> Uuid {
        self.shared.user.id
    }

    /// Their roles, permissions and compiled abilities, with any token
    /// restrictions applied.
    pub fn access(&self) -> &PrincipalAccess {
        &self.shared.access
    }

    /// Their abilities, for checks in services.
    pub fn ability(&self) -> &Arc<Ability> {
        &self.shared.access.ability
    }

    /// A list filter: the rows of `E` the user may do `A` on.
    pub fn accessible<E, A>(&self) -> Accessible<E, A>
    where
        E: EntityTrait + Subject,
        A: ActionMarker,
    {
        Accessible::new(self.shared.access.ability.clone())
    }

    /// Loads record `id` of `E` in the tool call's unit of work and checks
    /// `A` on it: the record, or a 404 (no such record) or a 403.
    pub async fn authorize<E, A>(
        &self,
        id: impl Into<sea_orm::Value>,
    ) -> Result<Authorized<E, A>, AppError>
    where
        E: EntityTrait + Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
        A: ActionMarker,
    {
        let model = Repo::<E>::find(&self.uow, id)
            .await
            .map_err(|error| {
                navree_db::map_db_error(&error).unwrap_or_else(|| AppError::internal(error))
            })?
            .ok_or_else(AppError::not_found)?;
        Authorized::check(model, self.shared.access.ability.clone())
    }

    /// The tool call's unit of work: it commits when the tool returns `Ok`
    /// and rolls back when it returns `Err`.
    pub fn uow(&self) -> &Uow {
        &self.uow
    }

    /// The time from the app's `dyn Clock`.
    pub fn now(&self) -> DateTime<Utc> {
        self.shared.clock.now()
    }

    /// The conversation, when the run has memory.
    pub fn conversation_id(&self) -> Option<Uuid> {
        self.shared.conversation_id
    }

    /// The app, to resolve anything else.
    pub fn ctx(&self) -> &AppContext {
        &self.shared.ctx
    }
}

impl fmt::Debug for AgentRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRun")
            .field("user_id", &self.user_id())
            .field("conversation_id", &self.shared.conversation_id)
            .finish_non_exhaustive()
    }
}
