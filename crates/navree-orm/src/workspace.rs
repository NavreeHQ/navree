//! Workspace scoping: models whose records belong to a workspace
//! (`#[model(workspace)]`, a `workspace_id` column) see the current
//! workspace's records only, fill it on the records they create, and refuse
//! to move or touch another workspace's records.
//!
//! ```no_run
//! # use navree_orm::prelude::*;
//! # use navree_db::Uow;
//! # use uuid::Uuid;
//! #[derive(Model, Clone, Debug, Default, serde::Serialize)]
//! #[model(workspace)]
//! pub struct Project {
//!     id: Uuid,
//!     workspace_id: Uuid,
//!     name: String,
//! }
//!
//! # async fn demo(uow: Uow, other: Uuid) -> navree_orm::Result<()> {
//! // In a workspace request (CurrentWorkspace or the InWorkspace guard entered
//! // the workspace on the unit of work):
//! let mine = Project::query().get(&uow).await?;            // WHERE workspace_id = <current>
//! let new = Project::create(&uow, Project { name: "Launch".into(), ..Project::default() }).await?;
//! assert_eq!(Some(new.workspace_id), uow.workspace());    // filled
//!
//! // System work names the workspace or opts out explicitly.
//! let theirs = Project::query().for_workspace(other).get(&uow).await?;
//! let every = Project::query().without_workspace_scope().count(&uow).await?;
//! # Ok(()) }
//! ```
//!
//! **The current workspace** is, in order: the one a task runs with
//! ([`scope`]); the one the executor's unit of work entered
//! ([`Uow::workspace`]: navree-workspace enters it for every workspace
//! request, and `Workspaces::enter` for jobs and commands); the one the
//! task's unit of work entered ([`Uow::current`]). With none, a query of a
//! workspace-scoped model fails (it never falls back to every workspace):
//! name one with [`Builder::for_workspace`], or opt out with
//! [`Builder::without_workspace_scope`].
//!
//! **Reads** get `workspace_id = <current>` as a global scope
//! ([`WorkspaceScope`]): queries, finds, counts, pages, plucks, mass
//! updates and deletes, existence and `IN` subqueries, and route model
//! binding (another workspace's record is a 404).
//!
//! **Writes** go through a hook every workspace-scoped model has (it runs
//! in quiet operations too): a new record gets the current workspace
//! (creating one without any workspace fails; one set to another workspace
//! is refused), the workspace of a stored record never changes, and saving,
//! deleting or restoring a record of another workspace answers
//! [`OrmError::ModelNotFound`] (a 404). Mass updates cannot set the
//! workspace column.
//!
//! On PostgreSQL, row-level security (navree-workspace's `RlsPolicy`, which
//! reads the `navree.workspace` setting of the request's transaction) keeps
//! enforcing the same boundary underneath, since every ORM statement runs
//! in that transaction.

use std::borrow::Cow;
use std::future::Future;

use navree_db::Uow;
use navree_kernel::BoxFuture;
use uuid::Uuid;

use crate::column::Filter;
use crate::error::{OrmError, Result};
use crate::exec::{ExecContext, Target};
use crate::hooks::{Flow, ModelEvent, ModelHook};
use crate::ir::{ColumnRef, Expr, Operator};
use crate::model::{Model, PrimaryKey};
use crate::query::Builder;
use crate::record::Record;
use crate::scope::GlobalScope;
use crate::value::{Field, Value};

tokio::task_local! {
    /// The workspace a task runs with ([`scope`]).
    static EXPLICIT: Uuid;
    /// The workspace of the unit of work the statement being built runs
    /// in (set around turning a query into a statement).
    static TARGET: Option<Uuid>;
}

/// Runs `future` with `workspace` as the current workspace of
/// workspace-scoped models (system work: a command or job working for one
/// workspace, without a unit of work that entered it).
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, workspace: Uuid) -> Result<()> {
/// let deals = navree_orm::workspace::scope(workspace, async {
///     Deal::query().count(&db).await
/// })
/// .await?;
/// # Ok(()) }
/// ```
pub async fn scope<F: Future>(workspace: Uuid, future: F) -> F::Output {
    EXPLICIT.scope(workspace, future).await
}

/// The current workspace of the running task (see the [module
/// documentation](self)), if any.
pub fn current() -> Option<Uuid> {
    EXPLICIT
        .try_with(|workspace| *workspace)
        .ok()
        .or_else(|| TARGET.try_with(|workspace| *workspace).ok().flatten())
        .or_else(|| Uow::current().and_then(|uow| uow.workspace()))
}

/// The current workspace for an operation running in `ctx`: the task's
/// explicit one, else the unit of work's, else the task's unit of work's.
fn current_in(ctx: &ExecContext) -> Option<Uuid> {
    EXPLICIT
        .try_with(|workspace| *workspace)
        .ok()
        .or_else(|| ctx.uow().and_then(Uow::workspace))
        .or_else(current)
}

/// Runs `build` (turning a query into a statement) knowing the workspace of
/// the unit of work `target` runs in.
pub(crate) fn with_target<R>(target: &Target<'_>, build: impl FnOnce() -> R) -> R {
    let workspace = target.uow().and_then(Uow::workspace);
    TARGET.sync_scope(workspace, build)
}

/// The global scope of workspace-scoped models: `workspace_id = <the
/// current workspace>`, or a failing query when there is none. Removed by
/// [`Builder::without_workspace_scope`] and [`Builder::for_workspace`].
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceScope;

impl<M: Model> GlobalScope<M> for WorkspaceScope {
    fn apply(&self, query: Builder<M>) -> Builder<M> {
        let Some(column) = M::meta().workspace_column() else {
            return query;
        };
        match current() {
            Some(workspace) => query.where_(workspace_filter::<M>(column, workspace)),
            None => query.with_error(no_workspace::<M>("query")),
        }
    }
}

/// `table.column = workspace`.
fn workspace_filter<M: Model>(column: &'static str, workspace: Uuid) -> Filter {
    Filter::compare(
        Expr::Column(ColumnRef::new(Some(M::meta().table()), column)),
        Operator::Eq,
        Expr::Value(Value::from(workspace)),
    )
}

/// The message for an operation on a workspace-scoped model without a
/// current workspace.
fn no_workspace<M: Model>(operation: &str) -> String {
    let name = M::meta().name();
    format!(
        "{name} is scoped to workspaces, but there is no current workspace to {operation} it in: \
         run it in a workspace request (CurrentWorkspace or the InWorkspace guard), on a unit of work \
         that entered a workspace, or inside navree_orm::workspace::scope(id, ..); or name the \
         workspace with for_workspace(id) / opt out with without_workspace_scope()"
    )
}

impl<M: Model> Builder<M> {
    /// Leaves out the workspace scope: every workspace's records (system
    /// work, reports across workspaces).
    pub fn without_workspace_scope(self) -> Self {
        self.without_global_scope::<WorkspaceScope>()
    }

    /// The records of `workspace` instead of the current workspace's. Fails
    /// when the query runs if the model is not scoped to workspaces.
    pub fn for_workspace(self, workspace: Uuid) -> Self {
        match M::meta().workspace_column() {
            Some(column) => self
                .without_workspace_scope()
                .where_(workspace_filter::<M>(column, workspace)),
            None => self.with_error(format!(
                "{} is not scoped to workspaces: for_workspace() needs #[model(workspace)]",
                M::meta().name()
            )),
        }
    }
}

/// The workspace a model's workspace column holds (`None` when it is
/// empty: NULL or the nil UUID).
fn workspace_of<M: Model>(model: &M, column: &str) -> Result<Option<Uuid>> {
    let value = model
        .value(column)?
        .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(column))?;
    let workspace = <Option<Uuid>>::from_value(value).map_err(|error| OrmError::Decode {
        model: Cow::Borrowed(M::meta().name()),
        column: column.to_owned(),
        expected: Cow::Borrowed("a workspace id (Uuid)"),
        message: error.to_string(),
    })?;
    Ok(workspace.filter(|id| !id.is_nil()))
}

/// Gives a new `model` the `current` workspace, or checks the one it has.
pub(crate) fn fill_new<M: Model>(model: &mut M, current: Option<Uuid>) -> Result<()> {
    let Some(column) = M::meta().workspace_column() else {
        return Ok(());
    };
    match (workspace_of(model, column)?, current) {
        (None, Some(workspace)) => model.set_value(column, Value::from(workspace)),
        (None, None) => Err(OrmError::Workspace {
            model: Cow::Borrowed(M::meta().name()),
            message: no_workspace::<M>("create"),
        }),
        (Some(own), Some(workspace)) if own != workspace => Err(OrmError::Workspace {
            model: Cow::Borrowed(M::meta().name()),
            message: format!(
                "cannot create a {} in workspace {own} while working in workspace {workspace}",
                M::meta().name()
            ),
        }),
        (Some(_), _) => Ok(()),
    }
}

/// Fills the workspace of new models inserted in bulk on `target`.
pub(crate) fn fill_for_insert<M: Model>(ctx: &ExecContext, model: &mut M) -> Result<()> {
    fill_new(model, current_in(ctx))
}

/// Refuses mass-update assignments that set the workspace column.
pub(crate) fn check_assignments<M: Model>(assignments: &[(Cow<'static, str>, Expr)]) -> Result<()> {
    let Some(column) = M::meta().workspace_column() else {
        return Ok(());
    };
    if assignments.iter().any(|(name, _)| name == column) {
        return Err(OrmError::Workspace {
            model: Cow::Borrowed(M::meta().name()),
            message: format!(
                "the `{column}` of {} records cannot be changed by a mass update",
                M::meta().name()
            ),
        });
    }
    Ok(())
}

/// The hook every workspace-scoped model has: fills the workspace of new
/// records, keeps it from changing, and keeps writes to the current
/// workspace's records.
pub(crate) struct WorkspaceHook;

impl<M: Model> ModelHook<M> for WorkspaceHook {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<Flow>> {
        Box::pin(async move {
            let Some(column) = M::meta().workspace_column() else {
                return Ok(Flow::Continue);
            };
            let current = current_in(ctx);
            match event {
                ModelEvent::Creating => fill_new(&mut record.model, current)?,
                ModelEvent::Updating => {
                    let stored = match &record.original {
                        Some(original) => workspace_of(original, column)?,
                        None => workspace_of(&record.model, column)?,
                    };
                    if workspace_of(&record.model, column)? != stored {
                        return Err(OrmError::Workspace {
                            model: Cow::Borrowed(M::meta().name()),
                            message: format!(
                                "the workspace of a {} cannot change (`{column}` was changed)",
                                M::meta().name()
                            ),
                        });
                    }
                    check_belongs(record, stored, current)?;
                }
                ModelEvent::Deleting | ModelEvent::ForceDeleting | ModelEvent::Restoring => {
                    let stored = match &record.original {
                        Some(original) => workspace_of(original, column)?,
                        None => workspace_of(&record.model, column)?,
                    };
                    check_belongs(record, stored, current)?;
                }
                _ => {}
            }
            Ok(Flow::Continue)
        })
    }
}

/// A 404 when `record` (whose workspace is `stored`) is not in the
/// `current` workspace.
fn check_belongs<M: Model>(
    record: &Record<M>,
    stored: Option<Uuid>,
    current: Option<Uuid>,
) -> Result<()> {
    match (stored, current) {
        (Some(own), Some(workspace)) if own != workspace => Err(OrmError::not_found(
            M::meta().name(),
            vec![record.key().to_key_string()],
        )),
        _ => Ok(()),
    }
}
