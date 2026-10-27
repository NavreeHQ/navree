//! Lifecycle hooks: what runs around retrieval, saves and deletes.
//!
//! Every persistence flow announces its steps as [`ModelEvent`]s. The `-ing`
//! events run before the database is touched and can cancel the operation
//! ([`Flow::Cancel`]: `save` and `delete` then return `false`); the others
//! run after it. A [`ModelHook<M>`] receives them for model `M`, from two
//! sources, in this order:
//!
//! 1. the model's own hooks, [`Model::hooks`] (written by the derive for
//!    features declared on the model);
//! 2. the app's `dyn ModelHook<M>` multi-bindings, registered by modules
//!    (observers, auditing, search indexing).
//!
//! The app is the one the executor's pool belongs to (`DatabaseModule`'s
//! `Db`, a unit of work on it); a pool opened by hand has no app, so only
//! the model's own hooks run.
//!
//! ```
//! use navree_orm::hooks::{Flow, ModelEvent, ModelHook};
//! use navree_orm::{ExecContext, Model, Record, Result};
//! use navree_kernel::BoxFuture;
//!
//! /// Refuses to delete records whose key is listed as protected.
//! struct Protect(Vec<String>);
//!
//! impl<M: Model> ModelHook<M> for Protect {
//!     fn before<'a>(
//!         &'a self,
//!         event: ModelEvent,
//!         _ctx: &'a ExecContext,
//!         record: &'a mut Record<M>,
//!     ) -> BoxFuture<'a, Result<Flow>> {
//!         Box::pin(async move {
//!             use navree_orm::PrimaryKey;
//!             let protected = self.0.contains(&record.key().to_key_string());
//!             Ok(if event == ModelEvent::Deleting && protected {
//!                 Flow::Cancel
//!             } else {
//!                 Flow::Continue
//!             })
//!         })
//!     }
//! }
//! ```

use std::fmt;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::error::Result;
use crate::exec::ExecContext;
use crate::model::Model;
use crate::record::Record;

/// A step in a record's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModelEvent {
    /// A record was read from the database.
    Retrieved,
    /// A new record is about to be inserted (cancellable).
    Creating,
    /// A new record was inserted.
    Created,
    /// A changed record is about to be updated (cancellable).
    Updating,
    /// A record was updated.
    Updated,
    /// A record is about to be inserted or updated (cancellable). Runs
    /// before `Creating` / `Updating`, and also for a clean record.
    Saving,
    /// A record was inserted or updated (also a clean record).
    Saved,
    /// A record is about to be deleted (cancellable).
    Deleting,
    /// A record was deleted (or soft-deleted).
    Deleted,
    /// A record was soft-deleted.
    Trashed,
    /// A soft-deleting model's record is about to be removed for good
    /// (cancellable).
    ForceDeleting,
    /// A soft-deleting model's record was removed for good.
    ForceDeleted,
    /// A soft-deleted record is about to be restored (cancellable).
    Restoring,
    /// A soft-deleted record was restored.
    Restored,
    /// A copy of a record was made with `replicate`.
    Replicating,
}

impl ModelEvent {
    /// The event's name: `retrieved`, `creating`, `force_deleted`, ...
    pub fn name(self) -> &'static str {
        match self {
            ModelEvent::Retrieved => "retrieved",
            ModelEvent::Creating => "creating",
            ModelEvent::Created => "created",
            ModelEvent::Updating => "updating",
            ModelEvent::Updated => "updated",
            ModelEvent::Saving => "saving",
            ModelEvent::Saved => "saved",
            ModelEvent::Deleting => "deleting",
            ModelEvent::Deleted => "deleted",
            ModelEvent::Trashed => "trashed",
            ModelEvent::ForceDeleting => "force_deleting",
            ModelEvent::ForceDeleted => "force_deleted",
            ModelEvent::Restoring => "restoring",
            ModelEvent::Restored => "restored",
            ModelEvent::Replicating => "replicating",
        }
    }

    /// Whether a hook can cancel the operation at this step (the `-ing`
    /// events that run before the database is touched).
    pub fn is_cancellable(self) -> bool {
        matches!(
            self,
            ModelEvent::Creating
                | ModelEvent::Updating
                | ModelEvent::Saving
                | ModelEvent::Deleting
                | ModelEvent::ForceDeleting
                | ModelEvent::Restoring
        )
    }
}

impl fmt::Display for ModelEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether an operation goes ahead after a cancellable step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Flow {
    /// Go ahead.
    #[default]
    Continue,
    /// Stop: the operation returns `false` and changes nothing.
    Cancel,
}

/// Receives model `M`'s lifecycle events. Both methods have no-op defaults;
/// implement the ones you need.
///
/// Register one for the whole app as a multi-binding:
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use std::sync::Arc;
///
/// use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
/// use navree_orm::hooks::{Flow, ModelEvent, ModelHook};
/// use navree_orm::{ExecContext, Record, Result};
///
/// /// Refuses to save a deal without a title.
/// struct RequireTitle;
///
/// impl ModelHook<Deal> for RequireTitle {
///     fn before<'a>(
///         &'a self,
///         event: ModelEvent,
///         _ctx: &'a ExecContext,
///         deal: &'a mut Record<Deal>,
///     ) -> BoxFuture<'a, Result<Flow>> {
///         Box::pin(async move {
///             Ok(if event == ModelEvent::Saving && deal.title.trim().is_empty() {
///                 Flow::Cancel
///             } else {
///                 Flow::Continue
///             })
///         })
///     }
/// }
///
/// struct DealsModule;
///
/// impl Module for DealsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.multi(Factory::<dyn ModelHook<Deal>>::new(|_| async {
///             Ok(Arc::new(RequireTitle) as Arc<dyn ModelHook<Deal>>)
///         }));
///     }
/// }
/// ```
pub trait ModelHook<M: Model>: Send + Sync + 'static {
    /// Runs before a cancellable step ([`ModelEvent::is_cancellable`]);
    /// may change the record (a `Saving` hook can fill a column) or cancel.
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<Flow>> {
        let _ = (event, ctx, record);
        Box::pin(async { Ok(Flow::Continue) })
    }

    /// Runs after a step that happened (`Retrieved`, `Created`, `Saved`,
    /// ...). An error fails the operation (inside a unit of work, which
    /// then rolls back).
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        let _ = (event, ctx, record);
        Box::pin(async { Ok(()) })
    }

    /// The hook's name, as `model:show` lists it (its type's).
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// The hooks of model `M` for one operation: the model's own, then the
/// app's. Resolved once per operation (a query's records share them).
pub(crate) struct Hooks<M: Model> {
    hooks: Vec<Arc<dyn ModelHook<M>>>,
}

impl<M: Model> Hooks<M> {
    /// The hooks that apply in `ctx`: the model's intrinsic behaviour
    /// first (workspace scoping), then its own hooks, then the app's (which
    /// are skipped inside a quiet operation, see `crate::factory::quiet`).
    pub(crate) async fn resolve(ctx: &ExecContext) -> Result<Self> {
        let mut hooks = M::hooks();
        if M::meta().is_workspace_scoped() {
            hooks.insert(0, Arc::new(crate::workspace::WorkspaceHook));
        }
        if let Some(app) = ctx.app()
            && !crate::factory::quiet::is_active()
        {
            let registered = app.get_all::<dyn ModelHook<M>>().await.map_err(|error| {
                crate::error::OrmError::Hook {
                    model: M::meta().name().into(),
                    message: format!("cannot resolve the app's model hooks: {error}"),
                }
            })?;
            hooks.extend(registered);
        }
        Ok(Hooks { hooks })
    }

    /// Whether no hook applies (the flows skip the calls).
    pub(crate) fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Runs the `before` hooks for `event` until one cancels.
    pub(crate) async fn before(
        &self,
        event: ModelEvent,
        ctx: &ExecContext,
        record: &mut Record<M>,
    ) -> Result<Flow> {
        for hook in &self.hooks {
            if hook.before(event, ctx, record).await? == Flow::Cancel {
                return Ok(Flow::Cancel);
            }
        }
        Ok(Flow::Continue)
    }

    /// Runs the `after` hooks for `event`.
    pub(crate) async fn after(
        &self,
        event: ModelEvent,
        ctx: &ExecContext,
        record: &mut Record<M>,
    ) -> Result<()> {
        for hook in &self.hooks {
            hook.after(event, ctx, record).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ing_events_are_the_cancellable_ones() {
        let cancellable: Vec<&str> = [
            ModelEvent::Retrieved,
            ModelEvent::Creating,
            ModelEvent::Created,
            ModelEvent::Updating,
            ModelEvent::Updated,
            ModelEvent::Saving,
            ModelEvent::Saved,
            ModelEvent::Deleting,
            ModelEvent::Deleted,
            ModelEvent::Trashed,
            ModelEvent::ForceDeleting,
            ModelEvent::ForceDeleted,
            ModelEvent::Restoring,
            ModelEvent::Restored,
            ModelEvent::Replicating,
        ]
        .into_iter()
        .filter(|event| event.is_cancellable())
        .map(ModelEvent::name)
        .collect();
        assert_eq!(
            cancellable,
            [
                "creating",
                "updating",
                "saving",
                "deleting",
                "force_deleting",
                "restoring"
            ]
        );
        assert_eq!(ModelEvent::ForceDeleted.to_string(), "force_deleted");
    }
}
