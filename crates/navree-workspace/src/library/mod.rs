//! Shared libraries: content kept at three levels (the whole app's, an
//! organization's, a workspace's) that each workspace reads together and
//! clones to make its own.
//!
//! A library is a table of items (templates, courses, badges, policies)
//! whose rows carry a [`LibraryScope`] and the owner ids that go with it,
//! so each row has exactly one [`LibraryOwner`]:
//!
//! | `scope` | `organization_id` | `workspace_id` | Seen by |
//! |---|---|---|---|
//! | `global` | NULL | NULL | every workspace and organization |
//! | `organization` | the organization | NULL | the organization and its workspaces |
//! | `workspace` | NULL | the workspace | that workspace |
//!
//! A database `CHECK` constraint keeps the columns consistent
//! ([`LibraryColumns`] writes them in a migration). The model implements
//! [`Library`] and its module registers it with
//! [`m.library::<M>()`](crate::ModuleBuilderWorkspaceExt::library), which:
//!
//! - scopes every query of the model to what the current workspace sees
//!   (global items, its organization's, its own), or to the global items
//!   when no workspace is current ([`LibraryVisibility`]);
//! - refuses, inside a workspace, writes to items the workspace does not
//!   own (403, code `library_read_only`: clone the item first) and items it
//!   cannot see (404) ([`LibraryGuard`]);
//! - classifies the table for `navree check`.
//!
//! [`LibraryRepo`] reads one viewer's library
//! ([`visible_to`](LibraryRepo::visible_to)), one owner's items
//! ([`owned_by`](LibraryRepo::owned_by)), creates items at a level, places
//! the global items a seeder ships ([`seed_global`](LibraryRepo::seed_global))
//! and clones an item into a workspace
//! ([`clone_into`](LibraryRepo::clone_into)): the copy records
//! `cloned_from_id` and [`LibraryItemCloned`](crate::events::LibraryItemCloned)
//! is emitted once the unit of work commits.
//!
//! ```no_run
//! use navree_db::Uow;
//! use navree_orm::prelude::*;
//! use navree_workspace::library::{Library, LibraryRepo, LibraryScope};
//! use navree_workspace::{CurrentWorkspace, WorkspaceError};
//! use uuid::Uuid;
//!
//! #[derive(Model, Clone, Debug, Default, serde::Serialize)]
//! #[model(table = "templates", timestamps)]
//! pub struct Template {
//!     pub id: Uuid,
//!     pub scope: LibraryScope,
//!     pub organization_id: Option<Uuid>,
//!     pub workspace_id: Option<Uuid>,
//!     pub cloned_from_id: Option<Uuid>,
//!     pub name: String,
//!     pub created_at: chrono::DateTime<chrono::Utc>,
//!     pub updated_at: chrono::DateTime<chrono::Utc>,
//! }
//!
//! impl Library for Template {}
//!
//! async fn library(ws: &CurrentWorkspace, uow: &Uow) -> Result<Collection<Template>, WorkspaceError> {
//!     Ok(LibraryRepo::<Template>::visible_to(ws).order_by(Template::NAME).get(uow).await?)
//! }
//!
//! async fn use_template(ws: &CurrentWorkspace, uow: &Uow, id: Uuid) -> Result<Record<Template>, WorkspaceError> {
//!     LibraryRepo::<Template>::clone_into(uow, id, ws.id()).await
//! }
//! ```

mod columns;
mod guard;
mod repo;
mod visibility;

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use navree_db::Uow;
use navree_kernel::BoxFuture;
use navree_orm::{Field, FieldError, FieldKind, Model, OrmError, Record, Value};
use sea_orm::QueryResult;
use sea_orm::sea_query::StringLen;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use columns::LibraryColumns;
pub use guard::{LibraryGuard, unguarded};
pub use repo::{LibraryRepo, SeededGlobal};
pub use visibility::LibraryVisibility;

use crate::{CurrentOrganization, CurrentWorkspace, Workspace, WorkspaceError};

/// The level a library item belongs to: the `scope` column.
///
/// Stored as `global`, `organization` or `workspace`. A navree-orm model
/// field of this type needs no cast, and a SeaORM entity takes it as a
/// column too (it is an active enum), so one struct can derive both. It
/// defaults to `Workspace`: an item created inside a workspace request
/// belongs to that workspace.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
    sea_orm::EnumIter,
    sea_orm::DeriveActiveEnum,
)]
#[serde(rename_all = "snake_case")]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
pub enum LibraryScope {
    /// The app's own items, seen by everyone (seeders and Helm place them).
    #[sea_orm(string_value = "global")]
    Global,
    /// An organization's items, seen by the organization and its
    /// workspaces.
    #[sea_orm(string_value = "organization")]
    Organization,
    /// A workspace's own items (its clones among them).
    #[default]
    #[sea_orm(string_value = "workspace")]
    Workspace,
}

impl LibraryScope {
    /// Every level, from the widest to the narrowest.
    pub const ALL: [LibraryScope; 3] = [
        LibraryScope::Global,
        LibraryScope::Organization,
        LibraryScope::Workspace,
    ];

    /// `global`, `organization` or `workspace`.
    pub const fn as_str(self) -> &'static str {
        match self {
            LibraryScope::Global => "global",
            LibraryScope::Organization => "organization",
            LibraryScope::Workspace => "workspace",
        }
    }
}

impl fmt::Display for LibraryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LibraryScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "global" => Ok(LibraryScope::Global),
            "organization" => Ok(LibraryScope::Organization),
            "workspace" => Ok(LibraryScope::Workspace),
            other => Err(format!(
                "unknown library scope `{other}`: expected `global`, `organization` or `workspace`"
            )),
        }
    }
}

/// A `scope` column as a navree-orm field: text in the database.
impl Field for LibraryScope {
    const KIND: FieldKind = FieldKind::Text;

    fn to_value(&self) -> Value {
        Value::String(Some(self.as_str().to_owned()))
    }

    fn null() -> Value {
        <String as Field>::null()
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        String::from_sql(row, column)?
            .parse()
            .map_err(FieldError::Invalid)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        String::from_value(value)?
            .parse()
            .map_err(FieldError::Invalid)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("LibraryScope")
    }
}

/// Who owns a library item: the app (`Global`), an organization or a
/// workspace. Serialized as `{"scope": "global"}` or `{"scope":
/// "workspace", "id": ".."}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "scope", content = "id", rename_all = "snake_case")]
pub enum LibraryOwner {
    /// The app: a global item.
    Global,
    /// An organization.
    Organization(Uuid),
    /// A workspace.
    Workspace(Uuid),
}

impl LibraryOwner {
    /// The owner's level.
    pub const fn scope(&self) -> LibraryScope {
        match self {
            LibraryOwner::Global => LibraryScope::Global,
            LibraryOwner::Organization(_) => LibraryScope::Organization,
            LibraryOwner::Workspace(_) => LibraryScope::Workspace,
        }
    }

    /// The organization, for an organization's item.
    pub const fn organization_id(&self) -> Option<Uuid> {
        match self {
            LibraryOwner::Organization(id) => Some(*id),
            _ => None,
        }
    }

    /// The workspace, for a workspace's item.
    pub const fn workspace_id(&self) -> Option<Uuid> {
        match self {
            LibraryOwner::Workspace(id) => Some(*id),
            _ => None,
        }
    }

    /// The owner the three columns describe, or why they contradict each
    /// other (what the `CHECK` constraint refuses).
    ///
    /// ```
    /// use navree_workspace::library::{LibraryOwner, LibraryScope};
    /// use uuid::Uuid;
    ///
    /// let acme = Uuid::now_v7();
    /// assert_eq!(
    ///     LibraryOwner::from_columns(LibraryScope::Workspace, None, Some(acme)),
    ///     Ok(LibraryOwner::Workspace(acme))
    /// );
    /// assert!(LibraryOwner::from_columns(LibraryScope::Global, None, Some(acme)).is_err());
    /// ```
    pub fn from_columns(
        scope: LibraryScope,
        organization_id: Option<Uuid>,
        workspace_id: Option<Uuid>,
    ) -> Result<LibraryOwner, String> {
        match (scope, organization_id, workspace_id) {
            (LibraryScope::Global, None, None) => Ok(LibraryOwner::Global),
            (LibraryScope::Organization, Some(org), None) => Ok(LibraryOwner::Organization(org)),
            (LibraryScope::Workspace, None, Some(ws)) => Ok(LibraryOwner::Workspace(ws)),
            (LibraryScope::Global, _, _) => {
                Err("a global item has no organization_id and no workspace_id".to_owned())
            }
            (LibraryScope::Organization, None, _) => {
                Err("an organization's item needs an organization_id".to_owned())
            }
            (LibraryScope::Organization, Some(_), Some(_)) => {
                Err("an organization's item has no workspace_id".to_owned())
            }
            (LibraryScope::Workspace, _, None) => {
                Err("a workspace's item needs a workspace_id".to_owned())
            }
            (LibraryScope::Workspace, Some(_), Some(_)) => {
                Err("a workspace's item has no organization_id".to_owned())
            }
        }
    }
}

impl fmt::Display for LibraryOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryOwner::Global => f.write_str("global"),
            LibraryOwner::Organization(id) => write!(f, "organization {id}"),
            LibraryOwner::Workspace(id) => write!(f, "workspace {id}"),
        }
    }
}

/// Whose view of a library: a workspace (with its organization, if any),
/// an organization, or nobody in particular (the global items only).
///
/// A viewer sees the global items, its organization's and its
/// workspace's. Build one from a [`CurrentWorkspace`], a [`Workspace`] or a
/// [`CurrentOrganization`] (`&ws` converts), or with the constructors.
///
/// ```
/// use navree_workspace::library::{LibraryOwner, LibraryViewer};
/// use uuid::Uuid;
///
/// let (group, acme, other) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
/// let viewer = LibraryViewer::workspace(acme).in_organization(Some(group));
/// assert!(viewer.sees(&LibraryOwner::Global));
/// assert!(viewer.sees(&LibraryOwner::Organization(group)));
/// assert!(viewer.sees(&LibraryOwner::Workspace(acme)));
/// assert!(!viewer.sees(&LibraryOwner::Workspace(other)));
/// assert_eq!(viewer.owner(), LibraryOwner::Workspace(acme));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LibraryViewer {
    organization: Option<Uuid>,
    workspace: Option<Uuid>,
}

impl LibraryViewer {
    /// Nobody in particular: the global items only (a public catalog, a
    /// seeder, the admin panel's global library).
    pub const fn global() -> Self {
        LibraryViewer {
            organization: None,
            workspace: None,
        }
    }

    /// Organization `id`: the global items and the organization's.
    pub const fn organization(id: Uuid) -> Self {
        LibraryViewer {
            organization: Some(id),
            workspace: None,
        }
    }

    /// Workspace `id`, not in an organization: the global items and the
    /// workspace's. Add its organization with
    /// [`in_organization`](Self::in_organization).
    pub const fn workspace(id: Uuid) -> Self {
        LibraryViewer {
            organization: None,
            workspace: Some(id),
        }
    }

    /// The viewer, seeing organization `organization`'s items too.
    pub const fn in_organization(mut self, organization: Option<Uuid>) -> Self {
        self.organization = organization;
        self
    }

    /// The workspace, if the viewer is one.
    pub const fn workspace_id(&self) -> Option<Uuid> {
        self.workspace
    }

    /// The organization whose items the viewer sees, if any.
    pub const fn organization_id(&self) -> Option<Uuid> {
        self.organization
    }

    /// The level the viewer adds and changes items at: its workspace, else
    /// its organization, else the global library.
    pub const fn owner(&self) -> LibraryOwner {
        match (self.workspace, self.organization) {
            (Some(ws), _) => LibraryOwner::Workspace(ws),
            (None, Some(org)) => LibraryOwner::Organization(org),
            (None, None) => LibraryOwner::Global,
        }
    }

    /// Whether the viewer sees the items of `owner`.
    pub fn sees(&self, owner: &LibraryOwner) -> bool {
        match owner {
            LibraryOwner::Global => true,
            LibraryOwner::Organization(org) => self.organization == Some(*org),
            LibraryOwner::Workspace(ws) => self.workspace == Some(*ws),
        }
    }
}

impl From<&Workspace> for LibraryViewer {
    fn from(workspace: &Workspace) -> Self {
        LibraryViewer::workspace(workspace.id).in_organization(workspace.organization_id)
    }
}

impl From<&CurrentWorkspace> for LibraryViewer {
    fn from(workspace: &CurrentWorkspace) -> Self {
        LibraryViewer::from(workspace.workspace())
    }
}

impl From<&CurrentOrganization> for LibraryViewer {
    fn from(organization: &CurrentOrganization) -> Self {
        LibraryViewer::organization(organization.id())
    }
}

/// A navree-orm model whose rows are library items: its table has the
/// owner columns ([`LibraryScope`] in `scope`, `organization_id`,
/// `workspace_id`) and `cloned_from_id`, under these names unless the
/// constants say otherwise. Register it with
/// [`m.library::<M>()`](crate::ModuleBuilderWorkspaceExt::library).
///
/// The two hooks shape clones ([`LibraryRepo::clone_into`]):
///
/// ```no_run
/// # use navree_db::Uow;
/// # use navree_kernel::BoxFuture;
/// # use navree_orm::prelude::*;
/// # use navree_workspace::library::{Library, LibraryScope};
/// # use uuid::Uuid;
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// # #[model(table = "courses")]
/// # pub struct Course { pub id: Uuid, pub scope: LibraryScope, pub organization_id: Option<Uuid>,
/// #     pub workspace_id: Option<Uuid>, pub cloned_from_id: Option<Uuid>, pub title: String, pub published: bool }
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// # #[model(table = "lessons")]
/// # pub struct Lesson { pub id: Uuid, pub course_id: Uuid, pub title: String }
/// impl Library for Course {
///     // Before the copy is saved: a draft until the workspace publishes it.
///     fn cloning(&mut self, _source: &Self) {
///         self.published = false;
///     }
///
///     // After it is saved, in the same unit of work: the lessons come too.
///     fn cloned<'a>(uow: &'a Uow, source: &'a Record<Self>, copy: &'a Record<Self>) -> BoxFuture<'a, navree_orm::Result<()>> {
///         Box::pin(async move {
///             for lesson in Lesson::query().where_(Lesson::COURSE_ID.eq(source.id)).get(uow).await? {
///                 let mut lesson = lesson.replicate();
///                 lesson.course_id = copy.id;
///                 lesson.save(uow).await?;
///             }
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait Library: Model {
    /// The column holding the [`LibraryScope`].
    const SCOPE_COLUMN: &'static str = "scope";
    /// The column holding an organization's id (organization items).
    const ORGANIZATION_COLUMN: &'static str = "organization_id";
    /// The column holding a workspace's id (workspace items).
    const WORKSPACE_COLUMN: &'static str = "workspace_id";
    /// The column holding the key of the item a clone was made from.
    const CLONED_FROM_COLUMN: &'static str = "cloned_from_id";

    /// Prepares a copy before it is saved: `self` is the copy (the item's
    /// values, its new owner and `cloned_from_id` set), `source` the item.
    /// Rename it, reset counters, mark it a draft. Nothing by default.
    fn cloning(&mut self, source: &Self) {
        let _ = source;
    }

    /// Runs after the copy is saved, in the same unit of work: copy what
    /// belongs to the item (a course's lessons, a template's attachments).
    /// An error rolls the clone back with the unit of work. Nothing by
    /// default.
    fn cloned<'a>(
        uow: &'a Uow,
        source: &'a Record<Self>,
        copy: &'a Record<Self>,
    ) -> BoxFuture<'a, navree_orm::Result<()>> {
        let _ = (uow, source, copy);
        Box::pin(async { Ok(()) })
    }

    /// The item's owner, read from its columns. An error when they
    /// contradict each other (a 400 on `scope`) or the model lacks one.
    fn library_owner(&self) -> navree_orm::Result<LibraryOwner> {
        let (scope, organization, workspace) = owner_columns(self)?;
        LibraryOwner::from_columns(scope, organization, workspace).map_err(|message| {
            OrmError::refused(
                Self::meta().name(),
                WorkspaceError::invalid("scope", message),
            )
        })
    }

    /// Sets the item's owner columns.
    fn set_library_owner(&mut self, owner: LibraryOwner) -> navree_orm::Result<()> {
        self.set_value(Self::SCOPE_COLUMN, owner.scope().to_value())?;
        self.set_value(
            Self::ORGANIZATION_COLUMN,
            <Option<Uuid> as Field>::to_value(&owner.organization_id()),
        )?;
        self.set_value(
            Self::WORKSPACE_COLUMN,
            <Option<Uuid> as Field>::to_value(&owner.workspace_id()),
        )
    }

    /// The key of the item this one was cloned from, if it is a clone.
    fn cloned_from(&self) -> navree_orm::Result<Option<Self::Key>> {
        let value = column_value(self, Self::CLONED_FROM_COLUMN)?;
        <Option<Self::Key> as Field>::from_value(value).map_err(|error| OrmError::Decode {
            model: Cow::Borrowed(Self::meta().name()),
            column: Self::CLONED_FROM_COLUMN.to_owned(),
            expected: <Option<Self::Key> as Field>::type_name(),
            message: error.to_string(),
        })
    }
}

/// The value of `column` of `model`, or an error naming the constant to
/// set when the model has no such column.
fn column_value<M: Library>(model: &M, column: &'static str) -> navree_orm::Result<Value> {
    model.value(column)?.ok_or_else(|| OrmError::Decode {
        model: Cow::Borrowed(M::meta().name()),
        column: column.to_owned(),
        expected: Cow::Borrowed("a library column"),
        message: format!(
            "{} has no `{column}` column: add it (LibraryColumns) or point the Library constant at the column that holds it",
            M::meta().name()
        ),
    })
}

/// The scope and owner ids of `model`, as stored (the nil UUID reads as
/// none).
pub(crate) fn owner_columns<M: Library>(
    model: &M,
) -> navree_orm::Result<(LibraryScope, Option<Uuid>, Option<Uuid>)> {
    let decode =
        |column: &'static str, expected: &'static str, error: FieldError| OrmError::Decode {
            model: Cow::Borrowed(M::meta().name()),
            column: column.to_owned(),
            expected: Cow::Borrowed(expected),
            message: error.to_string(),
        };
    let scope = LibraryScope::from_value(column_value(model, M::SCOPE_COLUMN)?)
        .map_err(|e| decode(M::SCOPE_COLUMN, "a library scope", e))?;
    let organization =
        <Option<Uuid> as Field>::from_value(column_value(model, M::ORGANIZATION_COLUMN)?)
            .map_err(|e| decode(M::ORGANIZATION_COLUMN, "an organization id (Uuid)", e))?;
    let workspace = <Option<Uuid> as Field>::from_value(column_value(model, M::WORKSPACE_COLUMN)?)
        .map_err(|e| decode(M::WORKSPACE_COLUMN, "a workspace id (Uuid)", e))?;
    Ok((
        scope,
        organization.filter(|id| !id.is_nil()),
        workspace.filter(|id| !id.is_nil()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_parse_and_print_their_stored_names() {
        for scope in LibraryScope::ALL {
            assert_eq!(scope.as_str().parse::<LibraryScope>(), Ok(scope));
            assert_eq!(scope.to_string(), scope.as_str());
            assert_eq!(
                serde_json::to_value(scope).unwrap(),
                serde_json::json!(scope.as_str())
            );
            assert_eq!(LibraryScope::from_value(Field::to_value(&scope)), Ok(scope));
        }
        assert_eq!(LibraryScope::default(), LibraryScope::Workspace);
        assert!(
            "company"
                .parse::<LibraryScope>()
                .unwrap_err()
                .contains("expected `global`, `organization` or `workspace`")
        );
        assert!(matches!(
            LibraryScope::from_value(Value::String(Some("shared".into()))),
            Err(FieldError::Invalid(_))
        ));
        assert_eq!(
            LibraryScope::from_value(Value::String(None)),
            Err(FieldError::Null)
        );
        // A SeaORM column too, stored under the same names.
        use sea_orm::{ActiveEnum, Iterable};
        assert_eq!(LibraryScope::iter().collect::<Vec<_>>(), LibraryScope::ALL);
        for scope in LibraryScope::ALL {
            assert_eq!(ActiveEnum::to_value(&scope), scope.as_str());
            assert_eq!(
                <LibraryScope as ActiveEnum>::try_from_value(&scope.as_str().to_owned()).unwrap(),
                scope
            );
        }
    }

    #[test]
    fn owners_come_from_consistent_columns_only() {
        let (org, ws) = (Uuid::now_v7(), Uuid::now_v7());
        use LibraryScope::*;
        assert_eq!(
            LibraryOwner::from_columns(Global, None, None),
            Ok(LibraryOwner::Global)
        );
        assert_eq!(
            LibraryOwner::from_columns(Organization, Some(org), None),
            Ok(LibraryOwner::Organization(org))
        );
        assert_eq!(
            LibraryOwner::from_columns(Workspace, None, Some(ws)),
            Ok(LibraryOwner::Workspace(ws))
        );
        for (scope, organization, workspace, message) in [
            (
                Global,
                Some(org),
                None,
                "a global item has no organization_id",
            ),
            (
                Global,
                None,
                Some(ws),
                "a global item has no organization_id",
            ),
            (Organization, None, None, "needs an organization_id"),
            (
                Organization,
                Some(org),
                Some(ws),
                "an organization's item has no workspace_id",
            ),
            (Workspace, None, None, "needs a workspace_id"),
            (
                Workspace,
                Some(org),
                Some(ws),
                "a workspace's item has no organization_id",
            ),
        ] {
            let error = LibraryOwner::from_columns(scope, organization, workspace).unwrap_err();
            assert!(error.contains(message), "{scope}: {error}");
        }
        let owner = LibraryOwner::Organization(org);
        assert_eq!(owner.scope(), Organization);
        assert_eq!(owner.organization_id(), Some(org));
        assert_eq!(owner.workspace_id(), None);
        assert_eq!(owner.to_string(), format!("organization {org}"));
        assert_eq!(
            serde_json::to_value(LibraryOwner::Workspace(ws)).unwrap(),
            serde_json::json!({"scope": "workspace", "id": ws})
        );
        assert_eq!(
            serde_json::to_value(LibraryOwner::Global).unwrap(),
            serde_json::json!({"scope": "global"})
        );
    }

    #[test]
    fn viewers_see_the_global_items_their_organization_s_and_their_own() {
        let (group, other_group, acme, globex) = (
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
        );
        let nobody = LibraryViewer::global();
        assert!(nobody.sees(&LibraryOwner::Global));
        assert!(!nobody.sees(&LibraryOwner::Organization(group)));
        assert!(!nobody.sees(&LibraryOwner::Workspace(acme)));
        assert_eq!(nobody.owner(), LibraryOwner::Global);

        let org = LibraryViewer::organization(group);
        assert!(org.sees(&LibraryOwner::Organization(group)));
        assert!(!org.sees(&LibraryOwner::Organization(other_group)));
        assert!(!org.sees(&LibraryOwner::Workspace(acme)));
        assert_eq!(org.owner(), LibraryOwner::Organization(group));

        let standalone = LibraryViewer::workspace(globex);
        assert!(standalone.sees(&LibraryOwner::Global));
        assert!(!standalone.sees(&LibraryOwner::Organization(group)));
        assert!(standalone.sees(&LibraryOwner::Workspace(globex)));
        assert!(!standalone.sees(&LibraryOwner::Workspace(acme)));
        assert_eq!(standalone.organization_id(), None);

        let member = LibraryViewer::workspace(acme).in_organization(Some(group));
        assert_eq!(member.workspace_id(), Some(acme));
        assert_eq!(member.organization_id(), Some(group));
        assert!(member.sees(&LibraryOwner::Organization(group)));
        assert_eq!(member.owner(), LibraryOwner::Workspace(acme));
    }
}
