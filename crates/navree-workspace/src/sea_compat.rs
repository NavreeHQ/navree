//! SeaORM compatibility layer. **Remove in step 8 of the data-layer move**
//! (docs/design, §4).
//!
//! navree-workspace itself runs on navree-orm models; nothing in its own
//! code uses this module. No crate of the workspace uses it either since
//! navree-search and the evidence app moved to models; what is left keeps
//! an app's remaining SeaORM entities classified and isolated, the way the
//! `workspace.*` conformity checks read them, until they are moved:
//!
//! - [`WorkspaceScoped`] for a SeaORM entity, and [`WorkspaceRepo`] over it
//!   (reads and writes in the unit of work's SeaORM transaction, writes
//!   through `navree_db::Repo`), the way the `workspace.repo` check names;
//! - [`ModuleBuilderSeaWorkspaceExt`], with `sea_workspace_scoped`,
//!   `sea_workspace_entity` and `sea_global_entity`, the registrations the
//!   `workspace.entities` check names;
//! - `?` from a SeaORM `DbErr` into [`WorkspaceError`].
//!
//! The organization and partition forms (`OrgRepo`, `PartitionScoped`,
//! `PartitionRepo`) had no user left and are gone; what remains is the
//! form the conformity checks name. (The SeaORM derive once kept on three
//! models and on [`LibraryScope`](crate::LibraryScope) for Helm's resources
//! is gone: Helm takes navree-orm models.)
//!
//! New code uses the model forms at the crate root:
//! [`crate::WorkspaceScoped`], [`crate::WorkspaceRepo`], [`crate::OrgRepo`],
//! [`crate::PartitionRepo`] and [`crate::ModuleBuilderWorkspaceExt`].

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use navree_access::Subject;
use navree_db::{DbError, Repo, Uow};
use navree_kernel::{BoxError, ModuleBuilder};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ColumnType, DbErr, EntityTrait, IntoActiveModel, Iterable,
    ModelTrait, PrimaryKeyToColumn, QueryFilter, Select, Value,
};
use serde::Serialize;
use uuid::Uuid;

use crate::WorkspaceError;
use crate::conformity::EntityClass;
use crate::current::ScopedSubject;

/// A SeaORM entity whose rows belong to a workspace (row isolation): its table
/// has a workspace id column, `workspace_id` unless
/// [`WORKSPACE_COLUMN`](WorkspaceScoped::WORKSPACE_COLUMN) says otherwise.
///
/// ```ignore
/// impl WorkspaceScoped for deal::Entity {}
/// ```
pub trait WorkspaceScoped: EntityTrait {
    /// The column holding the workspace id.
    const WORKSPACE_COLUMN: &'static str = "workspace_id";
}

/// The workspace column of `E`.
fn workspace_column<E: WorkspaceScoped>() -> Result<E::Column, DbErr> {
    E::Column::from_str(E::WORKSPACE_COLUMN).map_err(|_| {
        DbErr::Custom(format!(
            "`{}` has no column `{}`: set WorkspaceScoped::WORKSPACE_COLUMN",
            E::default().table_name(),
            E::WORKSPACE_COLUMN
        ))
    })
}

/// `navree_db::Repo` scoped to one workspace (row isolation), for SeaORM
/// entities: every read
/// filters by the entity's workspace column, inserts set it, and updates and
/// deletes only touch rows of that workspace, so a record of another
/// workspace is `RecordNotFound` (a 404 over HTTP). Writes go through
/// `Repo`, so change observers (the audit trail) see them.
///
/// ```ignore
/// type Deals = WorkspaceRepo<deal::Entity>;
///
/// async fn show(ws: CurrentWorkspace, uow: Uow, Path(id): Path<Uuid>) -> Result<Json<deal::Model>> {
///     Ok(Json(Deals::find_or_fail(&uow, ws.id(), id).await?))
/// }
///
/// async fn index(ws: CurrentWorkspace, uow: Uow, page: PageQuery) -> Result<Page<deal::Model>> {
///     Ok(paginate(Deals::select(ws.id()), &*uow.tx().await?, page).await?)
/// }
/// ```
pub struct WorkspaceRepo<E>(PhantomData<E>);

impl<E> WorkspaceRepo<E>
where
    E: WorkspaceScoped,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    /// `select` restricted to the rows of `workspace`.
    pub fn scope(workspace: Uuid, select: Select<E>) -> Select<E> {
        match workspace_column::<E>() {
            Ok(column) => select.filter(column.eq(workspace)),
            // A misdeclared entity matches nothing rather than everything.
            Err(error) => {
                tracing::error!(target: "navree::workspace", %error, "cannot scope a query to a workspace");
                select.filter(sea_orm::sea_query::Expr::cust("1 = 0"))
            }
        }
    }

    /// Every row of `workspace` (`E::find()` scoped).
    pub fn select(workspace: Uuid) -> Select<E> {
        Self::scope(workspace, E::find())
    }

    /// The record of `workspace` with primary key `id` (single-column
    /// keys), read in the unit of work's transaction.
    pub async fn find(
        uow: &Uow,
        workspace: Uuid,
        id: impl Into<Value>,
    ) -> Result<Option<E::Model>, DbErr> {
        let select = Self::scope(workspace, by_id::<E>(id.into())?);
        select.one(&*uow.tx().await?).await
    }

    /// Like [`find`](WorkspaceRepo::find), but `RecordNotFound` (a 404) when
    /// `workspace` has no such record.
    pub async fn find_or_fail(
        uow: &Uow,
        workspace: Uuid,
        id: impl Into<Value>,
    ) -> Result<E::Model, DbErr> {
        Self::find(uow, workspace, id)
            .await?
            .ok_or_else(|| not_found::<E>())
    }

    /// Inserts `am` into `workspace` (its workspace column is set).
    pub async fn insert(
        uow: &Uow,
        workspace: Uuid,
        mut am: E::ActiveModel,
    ) -> Result<E::Model, DbErr> {
        am.set(workspace_column::<E>()?, workspace.into());
        Repo::<E>::insert(uow, am).await
    }

    /// Updates `am`, a record of `workspace` (`RecordNotFound` otherwise);
    /// its workspace column cannot be changed.
    pub async fn update(
        uow: &Uow,
        workspace: Uuid,
        mut am: E::ActiveModel,
    ) -> Result<E::Model, DbErr> {
        let column = workspace_column::<E>()?;
        let key = am.get_primary_key_value().ok_or(DbErr::PrimaryKeyNotSet {
            ctx: "WorkspaceRepo::update",
        })?;
        let mut keys = E::PrimaryKey::iter();
        let mut select = E::find();
        for value in key {
            let Some(pk) = keys.next() else {
                break;
            };
            select = select.filter(pk.into_column().eq(value));
        }
        let exists = Self::scope(workspace, select)
            .one(&*uow.tx().await?)
            .await?
            .is_some();
        if !exists {
            return Err(not_found::<E>());
        }
        am.set(column, workspace.into());
        Repo::<E>::update(uow, am).await
    }

    /// Deletes `model`, a record of `workspace` (`RecordNotFound` otherwise).
    pub async fn delete(uow: &Uow, workspace: Uuid, model: E::Model) -> Result<(), DbErr> {
        if model.get(workspace_column::<E>()?) != Value::from(workspace) {
            return Err(not_found::<E>());
        }
        Repo::<E>::delete(uow, model).await
    }
}

impl<E> fmt::Debug for WorkspaceRepo<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WorkspaceRepo<{}>", std::any::type_name::<E>())
    }
}

fn by_id<E: EntityTrait>(id: Value) -> Result<Select<E>, DbErr> {
    let mut columns = E::PrimaryKey::iter();
    match (columns.next(), columns.next()) {
        (Some(pk), None) => Ok(E::find().filter(pk.into_column().eq(id))),
        _ => Err(DbErr::KeyArityMismatch {
            expected: u8::try_from(E::PrimaryKey::iter().count()).unwrap_or(u8::MAX),
            received: 1,
        }),
    }
}

fn not_found<E: EntityTrait>() -> DbErr {
    DbErr::RecordNotFound(format!("{} (in this workspace)", E::default().table_name()))
}

/// The SeaORM forms of the module's registrations
/// ([`ModuleBuilderWorkspaceExt`](crate::ModuleBuilderWorkspaceExt) takes
/// navree-orm models).
pub trait ModuleBuilderSeaWorkspaceExt {
    /// [`workspace_scoped`](crate::ModuleBuilderWorkspaceExt::workspace_scoped)
    /// for SeaORM entity `E`.
    fn sea_workspace_scoped<E>(&mut self) -> &mut Self
    where
        E: WorkspaceScoped + Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync;

    /// [`workspace_entity`](crate::ModuleBuilderWorkspaceExt::workspace_entity)
    /// for SeaORM entity `E`.
    fn sea_workspace_entity<E: WorkspaceScoped>(&mut self) -> &mut Self;

    /// [`global_entity`](crate::ModuleBuilderWorkspaceExt::global_entity)
    /// for SeaORM entity `E`.
    fn sea_global_entity<E: EntityTrait>(&mut self) -> &mut Self;
}

impl ModuleBuilderSeaWorkspaceExt for ModuleBuilder {
    fn sea_workspace_scoped<E>(&mut self) -> &mut Self
    where
        E: WorkspaceScoped + Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
    {
        self.contribute(ScopedSubject {
            subject: E::SUBJECT,
            belongs: |uow, workspace, id| Box::pin(belongs::<E>(uow, workspace, id)),
        })
        .sea_workspace_entity::<E>()
    }

    fn sea_workspace_entity<E: WorkspaceScoped>(&mut self) -> &mut Self {
        self.contribute(EntityClass::workspace_table(
            E::default().table_name(),
            E::WORKSPACE_COLUMN,
        ))
    }

    fn sea_global_entity<E: EntityTrait>(&mut self) -> &mut Self {
        self.contribute(EntityClass::global(E::default().table_name()))
    }
}

/// Whether the record of `E` with primary key `id` belongs to `workspace`
/// (read in `uow`'s transaction).
async fn belongs<E>(uow: Uow, workspace: Uuid, id: String) -> Result<bool, BoxError>
where
    E: WorkspaceScoped,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    let Some(pk) = E::PrimaryKey::iter().next() else {
        return Ok(false);
    };
    let column = pk.into_column();
    let Some(value) = key_value(column.def().get_column_type(), &id) else {
        return Ok(false);
    };
    let select = WorkspaceRepo::<E>::scope(workspace, E::find().filter(column.eq(value)));
    Ok(select.one(&*uow.tx().await?).await?.is_some())
}

/// A path parameter as a value of the key column's type (`None` when it
/// cannot be one).
fn key_value(column_type: &ColumnType, id: &str) -> Option<Value> {
    match column_type {
        ColumnType::Uuid => Uuid::from_str(id).ok().map(Value::from),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::BigInteger => id.parse::<i64>().ok().map(Value::from),
        ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned
        | ColumnType::BigUnsigned => id.parse::<u64>().ok().map(Value::from),
        _ => Some(Value::from(id.to_owned())),
    }
}

/// `?` from SeaORM's error, for the crates that still speak it.
impl From<DbErr> for WorkspaceError {
    fn from(error: DbErr) -> Self {
        WorkspaceError::Db(DbError::from(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_take_the_column_type() {
        let id = Uuid::now_v7();
        assert_eq!(
            key_value(&ColumnType::Uuid, &id.to_string()),
            Some(Value::from(id))
        );
        assert_eq!(key_value(&ColumnType::Uuid, "nope"), None);
        assert_eq!(
            key_value(&ColumnType::BigInteger, "42"),
            Some(Value::from(42_i64))
        );
        assert_eq!(key_value(&ColumnType::Integer, "x"), None);
        assert_eq!(
            key_value(&ColumnType::Text, "abc"),
            Some(Value::from("abc".to_owned()))
        );
    }
}
