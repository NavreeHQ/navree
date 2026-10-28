//! [`LibraryGuard`]: what a workspace may write in a library, and
//! [`unguarded`] for the deliberate exceptions.

use std::future::Future;

use navree_kernel::BoxFuture;
use navree_orm::hooks::{Flow, ModelEvent, ModelHook};
use navree_orm::{ExecContext, OrmError, PrimaryKey, Record};
use uuid::Uuid;

use super::{Library, LibraryOwner, LibraryScope, owner_columns};
use crate::WorkspaceError;

tokio::task_local! {
    /// Set while [`unguarded`] runs.
    static UNGUARDED: ();
}

/// Runs `future` without the [`LibraryGuard`]: inside it, a workspace
/// request may write items of other owners (an organization admin
/// publishing a workspace's item to the organization's library from a
/// workspace screen, say). The owner columns must still agree with each
/// other. Check the user's permission before you call it.
///
/// ```no_run
/// # use navree_db::Uow;
/// # use navree_orm::prelude::*;
/// # use navree_workspace::library::{Library, LibraryOwner, LibraryRepo, LibraryScope, unguarded};
/// # use uuid::Uuid;
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// # #[model(table = "templates")]
/// # pub struct Template { pub id: Uuid, pub scope: LibraryScope, pub organization_id: Option<Uuid>,
/// #     pub workspace_id: Option<Uuid>, pub cloned_from_id: Option<Uuid>, pub name: String }
/// # impl Library for Template {}
/// # async fn demo(uow: Uow, id: Uuid, org: Uuid) -> Result<(), navree_workspace::WorkspaceError> {
/// // Publish a copy of the workspace's template to its organization.
/// let published = unguarded(LibraryRepo::<Template>::clone_to(&uow, id, LibraryOwner::Organization(org))).await?;
/// # Ok(()) }
/// ```
pub async fn unguarded<F: Future>(future: F) -> F::Output {
    UNGUARDED.scope((), future).await
}

/// Whether the task runs inside [`unguarded`].
fn is_unguarded() -> bool {
    UNGUARDED.try_with(|_| ()).is_ok()
}

/// The write rules of library models
/// ([`m.library::<M>()`](crate::ModuleBuilderWorkspaceExt::library)
/// registers it as a model hook; it runs also when events are silenced):
///
/// - **Inside a workspace** (navree-orm's current workspace for the
///   write): a new item without an owner joins the workspace's library; a
///   new item of another owner is refused. Changing, deleting or restoring
///   a global or an organization's item is refused with a 403 (code
///   `library_read_only`: clone it into the workspace to change it), as is
///   moving one of the workspace's items to another owner; another
///   workspace's item is a 404.
/// - **Everywhere**: the owner columns must agree with each other (a 400 on
///   `scope`), as the table's `CHECK` constraint requires.
///
/// Work outside a workspace (seeders, commands, organization and admin
/// requests) may write any owner's items; check permissions in the route.
/// [`unguarded`] lifts the workspace rules for a block.
#[derive(Clone, Copy, Debug, Default)]
pub struct LibraryGuard;

impl<M: Library> ModelHook<M> for LibraryGuard {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, navree_orm::Result<Flow>> {
        Box::pin(async move {
            let workspace = if is_unguarded() {
                None
            } else {
                navree_orm::workspace::current_in(ctx)
            };
            match event {
                ModelEvent::Creating => creating(record, workspace)?,
                ModelEvent::Updating => updating(record, workspace)?,
                ModelEvent::Deleting | ModelEvent::ForceDeleting | ModelEvent::Restoring => {
                    let stored = stored_owner(record)?;
                    if let Some(workspace) = workspace {
                        owned_by(record, &stored, workspace)?;
                    }
                }
                _ => {}
            }
            Ok(Flow::Continue)
        })
    }
}

/// A new item: filled with the workspace when it has no owner, else its
/// owner must be the workspace (inside one) and its columns consistent.
fn creating<M: Library>(record: &mut Record<M>, workspace: Option<Uuid>) -> navree_orm::Result<()> {
    if let Some(workspace) = workspace
        && owner_columns(record.model())? == (LibraryScope::Workspace, None, None)
    {
        return record
            .model_mut()
            .set_library_owner(LibraryOwner::Workspace(workspace));
    }
    let owner = record.model().library_owner()?;
    match workspace {
        Some(workspace) if owner != LibraryOwner::Workspace(workspace) => Err(read_only::<M>(
            format!("a new {} of {owner}", M::meta().name()),
        )),
        _ => Ok(()),
    }
}

/// A changed item: consistent columns; inside a workspace, one of its own
/// items that stays its own.
fn updating<M: Library>(record: &mut Record<M>, workspace: Option<Uuid>) -> navree_orm::Result<()> {
    let stored = stored_owner(record)?;
    let owner = record.model().library_owner()?;
    let Some(workspace) = workspace else {
        return Ok(());
    };
    owned_by(record, &stored, workspace)?;
    if owner != stored {
        return Err(read_only::<M>(format!(
            "{} {} (moving it to {owner})",
            M::meta().name(),
            record.key().to_key_string()
        )));
    }
    Ok(())
}

/// The owner of the record as it was read (the model itself for one never
/// read).
fn stored_owner<M: Library>(record: &Record<M>) -> navree_orm::Result<LibraryOwner> {
    match record.get_original() {
        Some(original) => original.library_owner(),
        None => record.model().library_owner(),
    }
}

/// `Ok` when `stored` is `workspace`; a 404 for another workspace's item, a
/// 403 for a shared one.
fn owned_by<M: Library>(
    record: &Record<M>,
    stored: &LibraryOwner,
    workspace: Uuid,
) -> navree_orm::Result<()> {
    match stored {
        LibraryOwner::Workspace(owner) if *owner == workspace => Ok(()),
        LibraryOwner::Workspace(_) => Err(OrmError::not_found(
            M::meta().name(),
            vec![record.key().to_key_string()],
        )),
        shared => Err(read_only::<M>(format!(
            "{} {} ({shared})",
            M::meta().name(),
            record.key().to_key_string()
        ))),
    }
}

/// The 403 refusal of a write the workspace may not make.
fn read_only<M: Library>(what: String) -> OrmError {
    OrmError::refused(M::meta().name(), WorkspaceError::LibraryReadOnly(what))
}
