//! [`LibraryRepo`]: reading one viewer's library, creating and seeding
//! items at a level, and cloning them.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use navree_db::Uow;
use navree_events::EventBus;
use navree_orm::{Builder, Executor, Field, Model as _, OrmError, PrimaryKey, Record, col};
use uuid::Uuid;

use super::visibility::{owner_filter, viewer_filter};
use super::{Library, LibraryOwner, LibraryViewer, LibraryVisibility};
use crate::WorkspaceError;
use crate::entity::{Organization, Workspace};
use crate::events::LibraryItemCloned;

/// What [`LibraryRepo::seed_global`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeededGlobal {
    /// Items that were not there and were created.
    pub created: usize,
    /// Items that were there with other values and were updated.
    pub updated: usize,
    /// Items that were there as given.
    pub unchanged: usize,
}

/// The library of model `M`: queries for one viewer or one owner, writes
/// at a chosen level, global items for seeders, and clones.
///
/// Every query it builds replaces [`LibraryVisibility`] with the viewer or
/// owner it names, so it reads the same inside and outside a workspace
/// request. Writes go through the model's hooks, [`LibraryGuard`](super::LibraryGuard)
/// included, and the unit of work's change events (the audit trail).
///
/// ```no_run
/// # use navree_db::{Db, Uow};
/// # use navree_orm::prelude::*;
/// # use navree_workspace::library::{Library, LibraryOwner, LibraryRepo, LibraryScope, LibraryViewer};
/// # use navree_workspace::{CurrentOrganization, CurrentWorkspace};
/// # use uuid::Uuid;
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// # #[model(table = "templates")]
/// # pub struct Template { pub id: Uuid, pub scope: LibraryScope, pub organization_id: Option<Uuid>,
/// #     pub workspace_id: Option<Uuid>, pub cloned_from_id: Option<Uuid>, pub name: String }
/// # impl Library for Template {}
/// # async fn demo(ws: CurrentWorkspace, org: CurrentOrganization, uow: Uow, id: Uuid) -> Result<(), navree_workspace::WorkspaceError> {
/// type Templates = LibraryRepo<Template>;
///
/// // What a workspace picks from: the global templates, its organization's, its own.
/// let library = Templates::visible_to(&ws).order_by(Template::NAME).get(&uow).await?;
/// // An organization's own templates, for its admins.
/// let shared = Templates::owned_by(LibraryOwner::Organization(org.id())).get(&uow).await?;
/// // A copy the workspace can change.
/// let mine = Templates::clone_into(&uow, id, ws.id()).await?;
/// assert_eq!(mine.cloned_from()?, Some(id));
/// # Ok(()) }
/// ```
pub struct LibraryRepo<M>(PhantomData<M>);

impl<M: Library> LibraryRepo<M> {
    /// The items `viewer` sees: the global ones, its organization's and its
    /// workspace's. `&CurrentWorkspace`, `&Workspace` and
    /// `&CurrentOrganization` convert.
    pub fn visible_to(viewer: impl Into<LibraryViewer>) -> Builder<M> {
        let viewer = viewer.into();
        M::query()
            .without_global_scope::<LibraryVisibility>()
            .where_(viewer_filter::<M>(&viewer))
    }

    /// The items of `owner` only.
    pub fn owned_by(owner: LibraryOwner) -> Builder<M> {
        M::query()
            .without_global_scope::<LibraryVisibility>()
            .where_(owner_filter::<M>(&owner))
    }

    /// The global items.
    pub fn global() -> Builder<M> {
        Self::owned_by(LibraryOwner::Global)
    }

    /// The clones made from item `key`, among the items the query sees:
    /// with [`LibraryVisibility`], the current workspace's (or, outside a
    /// workspace, the global ones). Chain
    /// `.without_global_scope::<LibraryVisibility>()` to count every
    /// owner's clones (reports for the app's admins).
    pub fn clones_of(key: impl Into<M::Key>) -> Builder<M> {
        let key = key.into();
        M::query().where_(
            col(format!("{}.{}", M::meta().table(), M::CLONED_FROM_COLUMN)).eq(key.to_value()),
        )
    }

    /// Item `key` if `viewer` sees it.
    pub async fn find_visible<E: Executor + ?Sized>(
        exec: &E,
        viewer: impl Into<LibraryViewer>,
        key: impl Into<M::Key>,
    ) -> Result<Option<Record<M>>, WorkspaceError> {
        Ok(Self::visible_to(viewer).find(exec, key).await?)
    }

    /// Item `key` if `viewer` sees it, else a 404.
    pub async fn find_visible_or_fail<E: Executor + ?Sized>(
        exec: &E,
        viewer: impl Into<LibraryViewer>,
        key: impl Into<M::Key>,
    ) -> Result<Record<M>, WorkspaceError> {
        Ok(Self::visible_to(viewer).find_or_fail(exec, key).await?)
    }

    /// Item `key` if `viewer` may change it: a 404 when the viewer does not
    /// see it, a 403 (code `library_read_only`) when it sees it but the
    /// item belongs to a wider level (a workspace looking at a global
    /// item). A viewer changes the items of its [`owner`](LibraryViewer::owner).
    pub async fn find_editable_or_fail<E: Executor + ?Sized>(
        exec: &E,
        viewer: impl Into<LibraryViewer>,
        key: impl Into<M::Key>,
    ) -> Result<Record<M>, WorkspaceError> {
        let viewer = viewer.into();
        let record = Self::find_visible_or_fail(exec, viewer, key).await?;
        let owner = record.model().library_owner()?;
        if owner != viewer.owner() {
            return Err(WorkspaceError::LibraryReadOnly(format!(
                "{} {} ({owner})",
                M::meta().name(),
                record.key().to_key_string()
            )));
        }
        Ok(record)
    }

    /// Creates `item` as an item of `owner` (its owner columns are set).
    /// Inside a workspace request only the workspace's own items may be
    /// created this way (see [`LibraryGuard`](super::LibraryGuard)).
    pub async fn create<E: Executor + ?Sized>(
        exec: &E,
        owner: LibraryOwner,
        mut item: M,
    ) -> Result<Record<M>, WorkspaceError> {
        item.set_library_owner(owner)?;
        Ok(M::create(exec, item).await?)
    }

    /// Places the global items a seeder ships: each item is matched to a
    /// global row by its value in column `unique_by` (a slug, a code); a
    /// missing one is created, an existing one takes the item's values
    /// (its key, its timestamps and a soft delete stay). Run it again after
    /// changing the items: it creates and updates what changed.
    ///
    /// ```no_run
    /// # use navree_db::Db;
    /// # use navree_orm::prelude::*;
    /// # use navree_workspace::library::{Library, LibraryRepo, LibraryScope};
    /// # use uuid::Uuid;
    /// # #[derive(Model, Clone, Debug, Default, serde::Serialize)]
    /// # #[model(table = "templates")]
    /// # pub struct Template { pub id: Uuid, pub scope: LibraryScope, pub organization_id: Option<Uuid>,
    /// #     pub workspace_id: Option<Uuid>, pub cloned_from_id: Option<Uuid>, pub slug: String, pub name: String }
    /// # impl Library for Template {}
    /// # async fn demo(db: &Db) -> Result<(), navree_workspace::WorkspaceError> {
    /// let items = [("welcome", "Welcome aboard"), ("reset", "Reset your password")]
    ///     .map(|(slug, name)| Template { slug: slug.into(), name: name.into(), ..Template::default() });
    /// let seeded = LibraryRepo::<Template>::seed_global(db, "slug", items).await?;
    /// println!("{} created, {} updated", seeded.created, seeded.updated);
    /// # Ok(()) }
    /// ```
    pub async fn seed_global<E: Executor + ?Sized>(
        exec: &E,
        unique_by: &'static str,
        items: impl IntoIterator<Item = M>,
    ) -> Result<SeededGlobal, WorkspaceError> {
        let meta = M::meta();
        let kept: Vec<&'static str> = [meta.created_at(), meta.updated_at(), meta.deleted_at()]
            .into_iter()
            .flatten()
            .collect();
        let mut seeded = SeededGlobal::default();
        for mut item in items {
            item.set_library_owner(LibraryOwner::Global)?;
            let Some(value) = item.value(unique_by)? else {
                return Err(OrmError::Decode {
                    model: Cow::Borrowed(meta.name()),
                    column: unique_by.to_owned(),
                    expected: Cow::Borrowed("a column"),
                    message: format!("{} has no `{unique_by}` column to match global items by", meta.name()),
                }
                .into());
            };
            let existing = Self::global()
                .with_trashed()
                .where_(col(format!("{}.{unique_by}", meta.table())).eq(value))
                .first(exec)
                .await?;
            let Some(mut record) = existing else {
                M::create(exec, item).await?;
                seeded.created += 1;
                continue;
            };
            item.set_key(record.key());
            for column in &kept {
                if let Some(stored) = record.model().value(column)? {
                    item.set_value(column, stored)?;
                }
            }
            *record.model_mut() = item;
            if record.is_dirty() {
                record.save(exec).await?;
                seeded.updated += 1;
            } else {
                seeded.unchanged += 1;
            }
        }
        Ok(seeded)
    }

    /// Clones item `key` into workspace `workspace`'s library: a copy with
    /// a new key, the workspace as its owner and `cloned_from_id` set to
    /// `key`, prepared by [`Library::cloning`] and completed by
    /// [`Library::cloned`], in `uow`. The workspace must see the item (a
    /// global one, its organization's or its own; a 404 otherwise).
    /// [`LibraryItemCloned`] is emitted once `uow` commits.
    pub async fn clone_into(
        uow: &Uow,
        key: impl Into<M::Key>,
        workspace: Uuid,
    ) -> Result<Record<M>, WorkspaceError> {
        Self::clone_to(uow, key, LibraryOwner::Workspace(workspace)).await
    }

    /// Clones item `key` into the library of `owner`, who must see it: a
    /// workspace (see [`clone_into`](Self::clone_into)), an organization
    /// (a global item, or one of its own) or the global library (a global
    /// item). Inside a workspace request, cloning for another owner is
    /// refused unless it runs in [`unguarded`](super::unguarded).
    pub async fn clone_to(
        uow: &Uow,
        key: impl Into<M::Key>,
        owner: LibraryOwner,
    ) -> Result<Record<M>, WorkspaceError> {
        let viewer = match owner {
            LibraryOwner::Global => LibraryViewer::global(),
            LibraryOwner::Organization(id) => {
                Organization::find(uow, id)
                    .await?
                    .ok_or_else(|| WorkspaceError::OrganizationNotFound(id.to_string()))?;
                LibraryViewer::organization(id)
            }
            LibraryOwner::Workspace(id) => {
                let workspace = Workspace::find(uow, id)
                    .await?
                    .ok_or_else(|| WorkspaceError::WorkspaceNotFound(id.to_string()))?;
                LibraryViewer::from(workspace.model())
            }
        };
        let source = Self::find_visible_or_fail(uow, viewer, key).await?;
        let source_owner = source.model().library_owner()?;
        let mut copy = source.replicate();
        copy.model_mut().set_library_owner(owner)?;
        copy.model_mut()
            .set_value(M::CLONED_FROM_COLUMN, source.key().to_value())?;
        M::cloning(copy.model_mut(), source.model());
        if !copy.save(uow).await? {
            return Err(OrmError::Hook {
                model: Cow::Borrowed(M::meta().name()),
                message: "a hook cancelled saving the clone".to_owned(),
            }
            .into());
        }
        M::cloned(uow, &source, &copy).await?;
        emit_cloned(
            uow,
            LibraryItemCloned {
                library: M::meta().table().to_owned(),
                item_id: source.key().to_key_string(),
                clone_id: copy.key().to_key_string(),
                source: source_owner,
                owner,
                cloned_by: uow.actor(),
            },
        )
        .await;
        Ok(copy)
    }
}

impl<M> fmt::Debug for LibraryRepo<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LibraryRepo<{}>", std::any::type_name::<M>())
    }
}

/// Emits `event` once `uow` commits, when the app has an `EventBus`.
async fn emit_cloned(uow: &Uow, event: LibraryItemCloned) {
    let Some(app) = uow.db().app() else {
        return;
    };
    let Some(bus) = app.try_get::<EventBus>().await else {
        return;
    };
    uow.after_commit(move || {
        Box::pin(async move {
            if let Err(error) = bus.emit(event).await {
                tracing::warn!(target: "navree::workspace", %error, "a library event listener failed");
            }
        })
    });
}
