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
///
/// The `-ing` events ([`is_cancellable`](ModelEvent::is_cancellable)) run
/// before the database is touched, inside the operation's transaction, and
/// can cancel it. The others announce what happened; listeners hear the
/// write events among them
/// ([`runs_after_commit`](ModelEvent::runs_after_commit)) once the unit of
/// work commits (see [`HookKind`]).
///
/// | Event | Fired by | Cancellable |
/// |---|---|---|
/// | `Retrieved` | every record a query reads | no |
/// | `Saving`, `Saved` | `save` (also of a clean record), `create`, `update`, `touch`, `restore` | `Saving` |
/// | `Creating`, `Created` | inserting a new record | `Creating` |
/// | `Updating`, `Updated` | updating a changed record, `increment`, `decrement` | `Updating` |
/// | `Deleting`, `Deleted` | `delete`, `force_delete`, `destroy` | `Deleting` |
/// | `Trashed` | a soft delete | no |
/// | `ForceDeleting`, `ForceDeleted` | `force_delete` of a soft-deleting model | `ForceDeleting` |
/// | `Restoring`, `Restored` | `restore` | `Restoring` |
/// | `Replicating` | `replicate_on` | no |
/// | `Custom(name)` | [`Record::fire`](crate::Record::fire) | yes |
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
    /// A copy of a record was made with
    /// [`replicate_on`](crate::Record::replicate_on); listeners may change
    /// the copy.
    Replicating,
    /// An event of the application's own, fired with
    /// [`Record::fire`](crate::Record::fire) (`"archived"`, `"published"`).
    /// Custom events run at once, wherever the record is, and listeners may
    /// cancel them.
    Custom(&'static str),
}

impl ModelEvent {
    /// Every built-in event, in the order a record meets them.
    pub const ALL: [ModelEvent; 15] = [
        ModelEvent::Retrieved,
        ModelEvent::Saving,
        ModelEvent::Creating,
        ModelEvent::Created,
        ModelEvent::Updating,
        ModelEvent::Updated,
        ModelEvent::Saved,
        ModelEvent::Deleting,
        ModelEvent::Deleted,
        ModelEvent::Trashed,
        ModelEvent::ForceDeleting,
        ModelEvent::ForceDeleted,
        ModelEvent::Restoring,
        ModelEvent::Restored,
        ModelEvent::Replicating,
    ];

    /// The built-in event named `name`; `None` for any other name.
    ///
    /// ```
    /// use navree_orm::hooks::ModelEvent;
    ///
    /// assert_eq!(ModelEvent::from_name("trashed"), Some(ModelEvent::Trashed));
    /// assert_eq!(ModelEvent::from_name("archived"), None);
    /// ```
    pub fn from_name(name: &str) -> Option<ModelEvent> {
        ModelEvent::ALL
            .into_iter()
            .find(|event| event.name() == name)
    }

    /// Whether listeners hear this event once the unit of work the write
    /// ran in commits: the events that announce a write (`created`,
    /// `updated`, `saved`, `deleted`, `trashed`, `force_deleted`,
    /// `restored`). Outside a unit of work they hear it at once.
    pub fn runs_after_commit(self) -> bool {
        matches!(
            self,
            ModelEvent::Created
                | ModelEvent::Updated
                | ModelEvent::Saved
                | ModelEvent::Deleted
                | ModelEvent::Trashed
                | ModelEvent::ForceDeleted
                | ModelEvent::Restored
        )
    }

    /// The event's name: `retrieved`, `creating`, `force_deleted`, ... (a
    /// custom event's own name).
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
            ModelEvent::Custom(name) => name,
        }
    }

    /// Whether a hook can cancel the operation at this step: the `-ing`
    /// events that run before the database is touched, and custom events.
    /// Hooks hear these through [`ModelHook::before`], the others through
    /// [`ModelHook::after`].
    pub fn is_cancellable(self) -> bool {
        matches!(
            self,
            ModelEvent::Creating
                | ModelEvent::Updating
                | ModelEvent::Saving
                | ModelEvent::Deleting
                | ModelEvent::ForceDeleting
                | ModelEvent::Restoring
                | ModelEvent::Custom(_)
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

/// `()` goes ahead, so listeners of events that cannot be cancelled return
/// `Ok(())`.
impl From<()> for Flow {
    fn from((): ()) -> Flow {
        Flow::Continue
    }
}

/// What kind of hook a [`ModelHook`] is, which decides when it hears an
/// event and whether quiet operations silence it.
///
/// | Kind | `-ing` events | "-ed" write events in a unit of work | quiet operations, [`without_events`](crate::without_events) |
/// |---|---|---|---|
/// | `Behaviour` | at once | at once, in the transaction | still run |
/// | `TransactionalListener` | at once | at once, in the transaction | silenced |
/// | `Listener` | at once | after the commit, on a snapshot of the record | silenced |
///
/// Behaviours are part of what a model does (touching parents, filling a
/// workspace column): they run whatever happens. Listeners react to events
/// (observers, closure listeners, typed application events, broadcasts):
/// they hear about writes once those are durable, and never about writes
/// that were rolled back. Outside a unit of work every hook runs at once.
///
/// A failing `-ing` hook stops the operation with its error; a listener
/// failing after the commit is logged and reported to the app's telemetry,
/// and never undoes the commit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HookKind {
    /// Part of the model's behaviour: runs at once, also when events are
    /// silenced.
    #[default]
    Behaviour,
    /// A listener that writes inside the transaction (related rows that
    /// commit or roll back with the record): hears every event at once.
    TransactionalListener,
    /// A listener: hears "-ed" write events after the unit of work commits.
    Listener,
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
    /// then rolls back), except for a [`HookKind::Listener`] that hears the
    /// event after the commit: its error is logged and reported.
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        let _ = (event, ctx, record);
        Box::pin(async { Ok(()) })
    }

    /// What kind of hook this is: when it hears "-ed" events inside a unit
    /// of work and whether quiet operations silence it (see [`HookKind`]).
    /// A hook implemented directly is a [`HookKind::Behaviour`]: it runs at
    /// once, every time.
    fn kind(&self) -> HookKind {
        HookKind::Behaviour
    }

    /// The hook's name, as `model:show` lists it (its type's).
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// The hooks of model `M` for one operation: the model's own, then the
/// app's. Resolved once per operation (a query's records share them).
///
/// Every hook runs with the operation's unit of work as the task's current
/// one ([`Uow::current`](navree_db::Uow::current)), so jobs and events it
/// dispatches join that unit of work. A [`HookKind::Listener`] hears "-ed"
/// write events after the commit ([`ModelEvent::runs_after_commit`]).
pub(crate) struct Hooks<M: Model> {
    hooks: Vec<Arc<dyn ModelHook<M>>>,
}

impl<M: Model> Hooks<M> {
    /// The hooks that apply in `ctx`: the model's intrinsic behaviour
    /// first (workspace scoping), then its own hooks, then the app's (which
    /// are skipped inside a factory's quiet operation, see
    /// `crate::factory::quiet`); behaviours only inside
    /// [`without_events`](crate::without_events); behaviours and a
    /// recorder when the app has a
    /// [`ModelEventFake`](crate::events::ModelEventFake).
    pub(crate) async fn resolve(ctx: &ExecContext) -> Result<Self> {
        let mut hooks = M::hooks();
        if M::meta().is_workspace_scoped() {
            hooks.insert(0, Arc::new(crate::workspace::WorkspaceHook));
        }
        let mut fake = None;
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
            fake = app.try_get::<crate::events::ModelEventFake>().await;
        }
        if crate::events::events_are_silenced() {
            hooks.retain(|hook| hook.kind() == HookKind::Behaviour);
        } else if let Some(fake) = fake {
            hooks.retain(|hook| hook.kind() == HookKind::Behaviour);
            hooks.push(Arc::new(crate::events::FakeListener(fake)));
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
            let flow = crate::events::in_unit_of_work(ctx, hook.before(event, ctx, record)).await?;
            if flow == Flow::Cancel {
                return Ok(Flow::Cancel);
            }
        }
        Ok(Flow::Continue)
    }

    /// Runs the `after` hooks for `event`: at once, except listeners of a
    /// write event inside a unit of work, which hear it after the commit
    /// (on a snapshot of the record as it is now).
    pub(crate) async fn after(
        &self,
        event: ModelEvent,
        ctx: &ExecContext,
        record: &mut Record<M>,
    ) -> Result<()> {
        let after_commit = if event.runs_after_commit() {
            ctx.uow()
        } else {
            None
        };
        let mut deferred = Vec::new();
        for hook in &self.hooks {
            if after_commit.is_some() && hook.kind() == HookKind::Listener {
                deferred.push(hook.clone());
                continue;
            }
            crate::events::in_unit_of_work(ctx, hook.after(event, ctx, record)).await?;
        }
        if let Some(uow) = after_commit
            && !deferred.is_empty()
        {
            crate::events::after_commit(uow, deferred, event, ctx.clone(), record.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ing_events_are_the_cancellable_ones() {
        let cancellable: Vec<&str> = ModelEvent::ALL
            .into_iter()
            .filter(|event| event.is_cancellable())
            .map(ModelEvent::name)
            .collect();
        assert_eq!(
            cancellable,
            [
                "saving",
                "creating",
                "updating",
                "deleting",
                "force_deleting",
                "restoring"
            ]
        );
        assert_eq!(ModelEvent::ForceDeleted.to_string(), "force_deleted");
    }

    #[test]
    fn every_built_in_event_is_found_by_its_name() {
        for event in ModelEvent::ALL {
            assert_eq!(ModelEvent::from_name(event.name()), Some(event));
        }
        assert_eq!(Flow::from(()), Flow::Continue);
    }
}
