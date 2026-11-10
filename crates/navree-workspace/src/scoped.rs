//! Reading across an organization's workspaces ([`OrgRepo`]) and keeping
//! members to their partitions ([`PartitionScoped`], [`PartitionRepo`]).

use std::fmt;
use std::marker::PhantomData;

use navree_orm::{Builder, Executor, Filter, OrmError, Record, Value, col};
use uuid::Uuid;

use crate::PartitionSelector;
use crate::isolation::{WorkspaceRepo, WorkspaceScoped, not_found};

/// A filter that matches nothing.
fn nothing() -> Filter {
    Filter::raw("1 = 0", Vec::<Value>::new())
}

/// Read-only queries across several workspaces of an organization (row
/// isolation): every query filters the model's workspace column by the
/// workspaces given, usually
/// [`CurrentOrganization::workspace_ids`](crate::CurrentOrganization::workspace_ids),
/// the ones the signed-in member reaches. An empty list matches nothing.
///
/// ```ignore
/// type Deals = OrgRepo<Deal>;
///
/// async fn report(org: CurrentOrganization, uow: Uow) -> Result<Json<Vec<Deal>>> {
///     Ok(Json(Deals::query(org.workspace_ids()).get(&uow).await?.into_models()))
/// }
/// ```
///
/// Writes go to one workspace at a time, through
/// [`WorkspaceRepo`](crate::WorkspaceRepo). Schema and database isolation
/// keep each workspace's rows apart physically, so `OrgRepo` reads row
/// isolation's shared tables only.
pub struct OrgRepo<M>(PhantomData<M>);

impl<M: WorkspaceScoped> OrgRepo<M> {
    /// `query` restricted to the rows of `workspaces`.
    pub fn scope(workspaces: &[Uuid], query: Builder<M>) -> Builder<M> {
        let query = query.without_workspace_scope();
        if workspaces.is_empty() {
            return query.where_(nothing());
        }
        query.where_(
            col(format!("{}.{}", M::meta().table(), M::WORKSPACE_COLUMN))
                .is_in(workspaces.iter().copied()),
        )
    }

    /// Every row of `workspaces` (`M::query()` scoped).
    pub fn query(workspaces: &[Uuid]) -> Builder<M> {
        Self::scope(workspaces, M::query())
    }

    /// The record with key `key` if it belongs to one of `workspaces`.
    pub async fn find<E: Executor + ?Sized>(
        exec: &E,
        workspaces: &[Uuid],
        key: impl Into<M::Key>,
    ) -> Result<Option<Record<M>>, OrmError> {
        Self::query(workspaces).where_key(key).first(exec).await
    }

    /// Like [`find`](OrgRepo::find), but [`OrmError::ModelNotFound`] (a
    /// 404) when no such record belongs to `workspaces`.
    pub async fn find_or_fail<E: Executor + ?Sized>(
        exec: &E,
        workspaces: &[Uuid],
        key: impl Into<M::Key>,
    ) -> Result<Record<M>, OrmError> {
        let key = key.into();
        Self::find(exec, workspaces, key.clone())
            .await?
            .ok_or_else(|| not_found::<M>(&key))
    }
}

impl<M> fmt::Debug for OrgRepo<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrgRepo<{}>", std::any::type_name::<M>())
    }
}

/// A workspace-scoped model whose rows also belong to a partition: its
/// table has a partition id column, `partition_id` unless
/// [`PARTITION_COLUMN`](PartitionScoped::PARTITION_COLUMN) says otherwise.
///
/// ```ignore
/// impl WorkspaceScoped for Contact {}
/// impl PartitionScoped for Contact {}
/// ```
pub trait PartitionScoped: WorkspaceScoped {
    /// The column holding the partition id.
    const PARTITION_COLUMN: &'static str = "partition_id";
}

/// [`WorkspaceRepo`] narrowed to the partitions a member sees: queries
/// match the workspace and, for a member limited to `Only` some
/// partitions, those partitions (rows without a partition are not visible
/// to them). Inserts set both columns; updates and deletes touch only rows
/// the selector sees, so anything else is [`OrmError::ModelNotFound`] (a
/// 404).
///
/// ```ignore
/// type Contacts = PartitionRepo<Contact>;
///
/// async fn index(scope: PartitionScope, uow: Uow) -> Result<Json<Vec<Contact>>> {
///     let rows = Contacts::query(scope.workspace_id(), scope.selector())
///         .get(&uow)
///         .await?;
///     Ok(Json(rows.into_models()))
/// }
/// ```
pub struct PartitionRepo<M>(PhantomData<M>);

impl<M: PartitionScoped> PartitionRepo<M> {
    /// `query` restricted to the rows of `workspace` that `partitions`
    /// sees.
    pub fn scope(workspace: Uuid, partitions: &PartitionSelector, query: Builder<M>) -> Builder<M> {
        let query = WorkspaceRepo::<M>::scope(workspace, query);
        match partitions.ids() {
            None => query,
            Some([]) => query.where_(nothing()),
            Some(ids) => query.where_(
                col(format!("{}.{}", M::meta().table(), M::PARTITION_COLUMN))
                    .is_in(ids.iter().copied()),
            ),
        }
    }

    /// Every row of `workspace` that `partitions` sees.
    pub fn query(workspace: Uuid, partitions: &PartitionSelector) -> Builder<M> {
        Self::scope(workspace, partitions, M::query())
    }

    /// The record with key `key`, if `partitions` sees it in `workspace`.
    pub async fn find<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        partitions: &PartitionSelector,
        key: impl Into<M::Key>,
    ) -> Result<Option<Record<M>>, OrmError> {
        Self::query(workspace, partitions)
            .where_key(key)
            .first(exec)
            .await
    }

    /// Like [`find`](PartitionRepo::find), but [`OrmError::ModelNotFound`]
    /// (a 404).
    pub async fn find_or_fail<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        partitions: &PartitionSelector,
        key: impl Into<M::Key>,
    ) -> Result<Record<M>, OrmError> {
        let key = key.into();
        Self::find(exec, workspace, partitions, key.clone())
            .await?
            .ok_or_else(|| not_found::<M>(&key))
    }

    /// Creates `model` in `workspace` and `partition` (both columns are
    /// set). Pass a partition the member sees, such as
    /// [`CurrentPartition::id`](crate::CurrentPartition::id).
    pub async fn insert<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        partition: Uuid,
        mut model: M,
    ) -> Result<Record<M>, OrmError> {
        model.set_value(M::PARTITION_COLUMN, Value::from(partition))?;
        WorkspaceRepo::<M>::insert(exec, workspace, model).await
    }

    /// Saves `record`, a record `partitions` sees in `workspace`
    /// ([`OrmError::ModelNotFound`] otherwise). When it moves the record to
    /// another partition, `partitions` must see that one too.
    pub async fn update<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        partitions: &PartitionSelector,
        record: &mut Record<M>,
    ) -> Result<(), OrmError> {
        let key = record.key();
        if !Self::query(workspace, partitions)
            .where_key(key.clone())
            .exists(exec)
            .await?
        {
            return Err(not_found::<M>(&key));
        }
        if !sees(partitions, record.model().value(M::PARTITION_COLUMN)?) {
            return Err(not_found::<M>(&key));
        }
        WorkspaceRepo::<M>::update(exec, workspace, record).await
    }

    /// Deletes `record`, a record `partitions` sees in `workspace`
    /// ([`OrmError::ModelNotFound`] otherwise).
    pub async fn delete<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        partitions: &PartitionSelector,
        record: Record<M>,
    ) -> Result<(), OrmError> {
        let key = record.key();
        if !sees(partitions, record.model().value(M::PARTITION_COLUMN)?)
            || !Self::query(workspace, partitions)
                .where_key(key.clone())
                .exists(exec)
                .await?
        {
            return Err(not_found::<M>(&key));
        }
        WorkspaceRepo::<M>::delete(exec, workspace, record).await
    }
}

impl<M> fmt::Debug for PartitionRepo<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PartitionRepo<{}>", std::any::type_name::<M>())
    }
}

/// Whether `partitions` sees a row whose partition column holds `value`
/// (`All` sees every row; `Only` the listed partitions, never a row
/// without one).
fn sees(partitions: &PartitionSelector, value: Option<Value>) -> bool {
    match partitions.ids() {
        None => true,
        Some(ids) => value.is_some_and(|value| ids.iter().any(|id| Value::from(*id) == value)),
    }
}
