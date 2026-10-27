//! Reading across an organization's workspaces ([`OrgRepo`]) and keeping
//! members to their partitions ([`PartitionScoped`], [`PartitionRepo`]).

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use navree_db::Uow;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DbErr, IntoActiveModel, Iterable, ModelTrait,
    PrimaryKeyToColumn, QueryFilter, Select, Value,
};
use serde::Serialize;
use uuid::Uuid;

use crate::PartitionSelector;
use crate::isolation::{WorkspaceRepo, WorkspaceScoped, by_id, not_found, workspace_column};

/// Read-only queries across several workspaces of an organization (row
/// isolation): every read filters the entity's workspace column by the
/// workspaces given, usually
/// [`CurrentOrganization::workspace_ids`](crate::CurrentOrganization::workspace_ids),
/// the ones the signed-in member reaches. An empty list matches nothing.
///
/// ```ignore
/// type Deals = OrgRepo<deal::Entity>;
///
/// async fn report(org: CurrentOrganization, uow: Uow) -> Result<Json<Vec<deal::Model>>> {
///     Ok(Json(Deals::select(org.workspace_ids()).all(&*uow.tx().await?).await?))
/// }
/// ```
///
/// Writes go to one workspace at a time, through
/// [`WorkspaceRepo`](crate::WorkspaceRepo). Schema and database isolation
/// keep each workspace's rows apart physically, so `OrgRepo` reads row
/// isolation's shared tables only.
pub struct OrgRepo<E>(PhantomData<E>);

impl<E> OrgRepo<E>
where
    E: WorkspaceScoped,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    /// `select` restricted to the rows of `workspaces`.
    pub fn scope(workspaces: &[Uuid], select: Select<E>) -> Select<E> {
        match workspace_column::<E>() {
            Ok(_) if workspaces.is_empty() => select.filter(Expr::cust("1 = 0")),
            Ok(column) => select.filter(column.is_in(workspaces.iter().copied())),
            Err(error) => {
                tracing::error!(target: "navree::workspace", %error, "cannot scope a query to an organization");
                select.filter(Expr::cust("1 = 0"))
            }
        }
    }

    /// Every row of `workspaces` (`E::find()` scoped).
    pub fn select(workspaces: &[Uuid]) -> Select<E> {
        Self::scope(workspaces, E::find())
    }

    /// The record with primary key `id` (single-column keys) if it belongs
    /// to one of `workspaces`, read in the unit of work's transaction.
    pub async fn find(
        uow: &Uow,
        workspaces: &[Uuid],
        id: impl Into<Value>,
    ) -> Result<Option<E::Model>, DbErr> {
        let select = Self::scope(workspaces, by_id::<E>(id.into())?);
        select.one(&*uow.tx().await?).await
    }

    /// Like [`find`](OrgRepo::find), but `RecordNotFound` (a 404) when no
    /// such record belongs to `workspaces`.
    pub async fn find_or_fail(
        uow: &Uow,
        workspaces: &[Uuid],
        id: impl Into<Value>,
    ) -> Result<E::Model, DbErr> {
        Self::find(uow, workspaces, id)
            .await?
            .ok_or_else(|| not_found::<E>())
    }
}

impl<E> fmt::Debug for OrgRepo<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrgRepo<{}>", std::any::type_name::<E>())
    }
}

/// A workspace-scoped entity whose rows also belong to a partition: its
/// table has a partition id column, `partition_id` unless
/// [`PARTITION_COLUMN`](PartitionScoped::PARTITION_COLUMN) says otherwise.
///
/// ```ignore
/// impl WorkspaceScoped for contact::Entity {}
/// impl PartitionScoped for contact::Entity {}
/// ```
pub trait PartitionScoped: WorkspaceScoped {
    /// The column holding the partition id.
    const PARTITION_COLUMN: &'static str = "partition_id";
}

/// The partition column of `E`.
fn partition_column<E: PartitionScoped>() -> Result<E::Column, DbErr> {
    E::Column::from_str(E::PARTITION_COLUMN).map_err(|_| {
        DbErr::Custom(format!(
            "`{}` has no column `{}`: set PartitionScoped::PARTITION_COLUMN",
            E::default().table_name(),
            E::PARTITION_COLUMN
        ))
    })
}

/// [`WorkspaceRepo`] narrowed to the partitions a member sees: reads match
/// the workspace and, for a member limited to `Only` some partitions,
/// those partitions (rows without a partition are not visible to them).
/// Inserts set both columns; updates and deletes touch only rows the
/// selector sees, so anything else is `RecordNotFound` (a 404).
///
/// ```ignore
/// type Contacts = PartitionRepo<contact::Entity>;
///
/// async fn index(scope: PartitionScope, uow: Uow) -> Result<Json<Vec<contact::Model>>> {
///     let rows = Contacts::select(scope.workspace_id(), scope.selector())
///         .all(&*uow.tx().await?)
///         .await?;
///     Ok(Json(rows))
/// }
/// ```
pub struct PartitionRepo<E>(PhantomData<E>);

impl<E> PartitionRepo<E>
where
    E: PartitionScoped,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    /// `select` restricted to the rows of `workspace` that `partitions`
    /// sees.
    pub fn scope(workspace: Uuid, partitions: &PartitionSelector, select: Select<E>) -> Select<E> {
        let select = WorkspaceRepo::<E>::scope(workspace, select);
        let Some(ids) = partitions.ids() else {
            return select;
        };
        match partition_column::<E>() {
            Ok(_) if ids.is_empty() => select.filter(Expr::cust("1 = 0")),
            Ok(column) => select.filter(column.is_in(ids.iter().copied())),
            Err(error) => {
                tracing::error!(target: "navree::workspace", %error, "cannot scope a query to partitions");
                select.filter(Expr::cust("1 = 0"))
            }
        }
    }

    /// Every row of `workspace` that `partitions` sees.
    pub fn select(workspace: Uuid, partitions: &PartitionSelector) -> Select<E> {
        Self::scope(workspace, partitions, E::find())
    }

    /// The record with primary key `id` (single-column keys), if
    /// `partitions` sees it in `workspace`.
    pub async fn find(
        uow: &Uow,
        workspace: Uuid,
        partitions: &PartitionSelector,
        id: impl Into<Value>,
    ) -> Result<Option<E::Model>, DbErr> {
        let select = Self::scope(workspace, partitions, by_id::<E>(id.into())?);
        select.one(&*uow.tx().await?).await
    }

    /// Like [`find`](PartitionRepo::find), but `RecordNotFound` (a 404).
    pub async fn find_or_fail(
        uow: &Uow,
        workspace: Uuid,
        partitions: &PartitionSelector,
        id: impl Into<Value>,
    ) -> Result<E::Model, DbErr> {
        Self::find(uow, workspace, partitions, id)
            .await?
            .ok_or_else(|| not_found::<E>())
    }

    /// Inserts `am` into `workspace` and `partition` (both columns are
    /// set). Pass a partition the member sees, such as
    /// [`CurrentPartition::id`](crate::CurrentPartition::id).
    pub async fn insert(
        uow: &Uow,
        workspace: Uuid,
        partition: Uuid,
        mut am: E::ActiveModel,
    ) -> Result<E::Model, DbErr> {
        am.set(partition_column::<E>()?, partition.into());
        WorkspaceRepo::<E>::insert(uow, workspace, am).await
    }

    /// Updates `am`, a record `partitions` sees in `workspace`
    /// (`RecordNotFound` otherwise). When `am` moves the record to another
    /// partition, `partitions` must see that one too.
    pub async fn update(
        uow: &Uow,
        workspace: Uuid,
        partitions: &PartitionSelector,
        am: E::ActiveModel,
    ) -> Result<E::Model, DbErr> {
        let column = partition_column::<E>()?;
        let key = am.get_primary_key_value().ok_or(DbErr::PrimaryKeyNotSet {
            ctx: "PartitionRepo::update",
        })?;
        let mut select = E::find();
        let mut keys = E::PrimaryKey::iter();
        for value in key {
            let Some(pk) = keys.next() else {
                break;
            };
            select = select.filter(pk.into_column().eq(value));
        }
        let exists = Self::scope(workspace, partitions, select)
            .one(&*uow.tx().await?)
            .await?
            .is_some();
        if !exists {
            return Err(not_found::<E>());
        }
        if let Some(target) = am.get(column).into_value()
            && !sees(partitions, &target)
        {
            return Err(not_found::<E>());
        }
        WorkspaceRepo::<E>::update(uow, workspace, am).await
    }

    /// Deletes `model`, a record `partitions` sees in `workspace`
    /// (`RecordNotFound` otherwise).
    pub async fn delete(
        uow: &Uow,
        workspace: Uuid,
        partitions: &PartitionSelector,
        model: E::Model,
    ) -> Result<(), DbErr> {
        if !sees(partitions, &model.get(partition_column::<E>()?)) {
            return Err(not_found::<E>());
        }
        WorkspaceRepo::<E>::delete(uow, workspace, model).await
    }
}

impl<E> fmt::Debug for PartitionRepo<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PartitionRepo<{}>", std::any::type_name::<E>())
    }
}

/// Whether `partitions` sees a row whose partition column holds `value`
/// (`All` sees every row; `Only` the listed partitions, never a row
/// without one).
fn sees(partitions: &PartitionSelector, value: &Value) -> bool {
    match partitions.ids() {
        None => true,
        Some(ids) => ids.iter().any(|id| Value::from(*id) == *value),
    }
}
