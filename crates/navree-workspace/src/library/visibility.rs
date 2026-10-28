//! [`LibraryVisibility`], the global scope of library models, and the
//! filters it and [`LibraryRepo`](super::LibraryRepo) build.

use navree_orm::{Builder, Column, Filter, GlobalScope, Model as _, Untyped, Value, col};
use uuid::Uuid;

use super::{Library, LibraryOwner, LibraryScope, LibraryViewer};
use crate::entity::Workspace;

/// The global scope of library models
/// ([`m.library::<M>()`](crate::ModuleBuilderWorkspaceExt::library)
/// installs it): every query sees what the current workspace sees, the
/// global items, its organization's and its own; with no current workspace
/// (a command, a seeder, an organization or admin request) it sees the
/// global items only.
///
/// The current workspace is navree-orm's
/// ([`navree_orm::workspace::current`]): the one a workspace request
/// entered on its unit of work, or the one a task runs with. Its
/// organization is read in the same statement (a subquery on
/// `workspaces`). [`LibraryRepo`](super::LibraryRepo)'s queries replace
/// the scope with the viewer or owner they name; remove it yourself with
/// `query.without_global_scope::<LibraryVisibility>()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LibraryVisibility;

impl<M: Library> GlobalScope<M> for LibraryVisibility {
    fn apply(&self, query: Builder<M>) -> Builder<M> {
        match navree_orm::workspace::current() {
            Some(workspace) => query.where_(current_workspace_filter::<M>(workspace)),
            None => query.where_(owner_filter::<M>(&LibraryOwner::Global)),
        }
    }
}

/// Column `name` of `M`'s table, qualified.
fn column<M: Library>(name: &'static str) -> Column<Untyped, Value> {
    col(format!("{}.{name}", M::meta().table()))
}

/// The rows of `owner`: its scope and its id.
pub(crate) fn owner_filter<M: Library>(owner: &LibraryOwner) -> Filter {
    let scope = column::<M>(M::SCOPE_COLUMN).eq(owner.scope().as_str());
    match owner {
        LibraryOwner::Global => scope,
        LibraryOwner::Organization(id) => scope.and(column::<M>(M::ORGANIZATION_COLUMN).eq(*id)),
        LibraryOwner::Workspace(id) => scope.and(column::<M>(M::WORKSPACE_COLUMN).eq(*id)),
    }
}

/// The rows `viewer` sees.
pub(crate) fn viewer_filter<M: Library>(viewer: &LibraryViewer) -> Filter {
    let mut owners = vec![LibraryOwner::Global];
    owners.extend(viewer.organization_id().map(LibraryOwner::Organization));
    owners.extend(viewer.workspace_id().map(LibraryOwner::Workspace));
    Filter::any(owners.iter().map(owner_filter::<M>))
}

/// The rows workspace `workspace` sees, its organization read from
/// `workspaces` in the same statement.
fn current_workspace_filter<M: Library>(workspace: Uuid) -> Filter {
    let organization = Workspace::query()
        .select([Workspace::ORGANIZATION_ID])
        .where_(Workspace::ID.eq(workspace));
    Filter::any([
        owner_filter::<M>(&LibraryOwner::Global),
        column::<M>(M::SCOPE_COLUMN)
            .eq(LibraryScope::Organization.as_str())
            .and(column::<M>(M::ORGANIZATION_COLUMN).in_subquery(organization)),
        owner_filter::<M>(&LibraryOwner::Workspace(workspace)),
    ])
}
