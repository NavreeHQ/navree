//! [`DatabaseFlagStore`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::Utc;
use navree_db::{Db, Uow};
use navree_kernel::BoxFuture;
use sea_orm::sea_query::{Expr, Func, OnConflict};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use uuid::Uuid;

use super::FlagStore;
use crate::entity::{feature_flag, feature_flag_override};
use crate::{FlagOverride, FlagState, FlagsError, Rollout, ScopeType};

/// Flag state in `feature_flags` and `feature_flag_overrides`
/// (`flags.store = "database"`, the default).
#[derive(Clone, Debug)]
pub struct DatabaseFlagStore {
    db: Arc<Db>,
}

impl DatabaseFlagStore {
    /// A store on `db` (the `flags` migrations must have run).
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

/// Runs `$body` with `$conn` bound to a connection to read with: the unit
/// of work's transaction, else `Db::reader` (the task's open transaction,
/// else the pool).
macro_rules! with_reader {
    ($self:ident, $uow:expr, |$conn:ident| $body:expr) => {{
        match $uow {
            Some(uow) => {
                let tx = uow.tx().await?;
                let $conn = &*tx;
                $body
            }
            None => {
                let reader = $self.db.reader().await;
                let $conn = &reader;
                $body
            }
        }
    }};
}

/// Runs `$body` with `$conn` bound to a connection to write with: the unit
/// of work's transaction, else the pool.
macro_rules! with_writer {
    ($self:ident, $uow:expr, |$conn:ident| $body:expr) => {{
        match $uow {
            Some(uow) => {
                let tx = uow.tx().await?;
                let $conn = &*tx;
                $body
            }
            None => {
                let $conn = $self.db.conn();
                $body
            }
        }
    }};
}

fn to_state(row: feature_flag::Model) -> Result<FlagState, FlagsError> {
    let rollout = row
        .rollout
        .map(serde_json::from_value::<Rollout>)
        .transpose()
        .map_err(|error| {
            FlagsError::Corrupt(format!("the rollout of flag `{}`: {error}", row.name))
        })?;
    Ok(FlagState {
        name: row.name,
        everyone: row.everyone,
        rollout,
        killed: row.killed,
        updated_at: Some(row.updated_at),
        updated_by: row.updated_by,
    })
}

fn to_override(row: feature_flag_override::Model) -> Result<FlagOverride, FlagsError> {
    let scope_type = row.scope_type.parse::<ScopeType>().map_err(|error| {
        FlagsError::Corrupt(format!("an override of flag `{}`: {error}", row.flag))
    })?;
    Ok(FlagOverride {
        flag: row.flag,
        scope_type,
        scope_id: row.scope_id,
        active: row.active,
    })
}

async fn states_in<C: ConnectionTrait>(conn: &C) -> Result<Vec<FlagState>, FlagsError> {
    feature_flag::Entity::find()
        .order_by_asc(feature_flag::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(to_state)
        .collect()
}

async fn state_in<C: ConnectionTrait>(
    conn: &C,
    name: &str,
) -> Result<Option<FlagState>, FlagsError> {
    feature_flag::Entity::find_by_id(name.to_owned())
        .one(conn)
        .await?
        .map(to_state)
        .transpose()
}

async fn save_state_in<C: ConnectionTrait>(conn: &C, state: &FlagState) -> Result<(), FlagsError> {
    let rollout = state
        .rollout
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| FlagsError::Corrupt(error.to_string()))?;
    let row = feature_flag::ActiveModel {
        name: ActiveValue::Set(state.name.clone()),
        everyone: ActiveValue::Set(state.everyone),
        rollout: ActiveValue::Set(rollout),
        killed: ActiveValue::Set(state.killed),
        updated_at: ActiveValue::Set(state.updated_at.unwrap_or_else(Utc::now)),
        updated_by: ActiveValue::Set(state.updated_by),
    };
    feature_flag::Entity::insert(row)
        .on_conflict(
            OnConflict::column(feature_flag::Column::Name)
                .update_columns([
                    feature_flag::Column::Everyone,
                    feature_flag::Column::Rollout,
                    feature_flag::Column::Killed,
                    feature_flag::Column::UpdatedAt,
                    feature_flag::Column::UpdatedBy,
                ])
                .to_owned(),
        )
        .exec_without_returning(conn)
        .await?;
    Ok(())
}

async fn overrides_of_in<C: ConnectionTrait>(
    conn: &C,
    scope_type: ScopeType,
    scope_id: Uuid,
) -> Result<BTreeMap<String, bool>, FlagsError> {
    Ok(feature_flag_override::Entity::find()
        .filter(feature_flag_override::Column::ScopeType.eq(scope_type.as_str()))
        .filter(feature_flag_override::Column::ScopeId.eq(scope_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|row| (row.flag, row.active))
        .collect())
}

async fn overrides_in<C: ConnectionTrait>(
    conn: &C,
    flag: &str,
) -> Result<Vec<FlagOverride>, FlagsError> {
    feature_flag_override::Entity::find()
        .filter(feature_flag_override::Column::Flag.eq(flag))
        .order_by_asc(feature_flag_override::Column::ScopeType)
        .order_by_asc(feature_flag_override::Column::ScopeId)
        .all(conn)
        .await?
        .into_iter()
        .map(to_override)
        .collect()
}

async fn override_counts_in<C: ConnectionTrait>(
    conn: &C,
) -> Result<BTreeMap<String, u64>, FlagsError> {
    let rows: Vec<(String, i64)> = feature_flag_override::Entity::find()
        .select_only()
        .column(feature_flag_override::Column::Flag)
        .column_as(
            Expr::expr(Func::count(Expr::col(feature_flag_override::Column::Flag))),
            "count",
        )
        .group_by(feature_flag_override::Column::Flag)
        .into_tuple()
        .all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(flag, count)| (flag, u64::try_from(count).unwrap_or(0)))
        .collect())
}

async fn set_override_in<C: ConnectionTrait>(
    conn: &C,
    entry: &FlagOverride,
) -> Result<(), FlagsError> {
    let row = feature_flag_override::ActiveModel {
        flag: ActiveValue::Set(entry.flag.clone()),
        scope_type: ActiveValue::Set(entry.scope_type.as_str().to_owned()),
        scope_id: ActiveValue::Set(entry.scope_id),
        active: ActiveValue::Set(entry.active),
    };
    feature_flag_override::Entity::insert(row)
        .on_conflict(
            OnConflict::columns([
                feature_flag_override::Column::Flag,
                feature_flag_override::Column::ScopeType,
                feature_flag_override::Column::ScopeId,
            ])
            .update_column(feature_flag_override::Column::Active)
            .to_owned(),
        )
        .exec_without_returning(conn)
        .await?;
    Ok(())
}

async fn remove_override_in<C: ConnectionTrait>(
    conn: &C,
    flag: &str,
    scope_type: ScopeType,
    scope_id: Uuid,
) -> Result<bool, FlagsError> {
    let result = feature_flag_override::Entity::delete_many()
        .filter(feature_flag_override::Column::Flag.eq(flag))
        .filter(feature_flag_override::Column::ScopeType.eq(scope_type.as_str()))
        .filter(feature_flag_override::Column::ScopeId.eq(scope_id))
        .exec(conn)
        .await?;
    Ok(result.rows_affected > 0)
}

async fn purge_in<C: ConnectionTrait>(conn: &C, flag: &str) -> Result<(), FlagsError> {
    feature_flag_override::Entity::delete_many()
        .filter(feature_flag_override::Column::Flag.eq(flag))
        .exec(conn)
        .await?;
    feature_flag::Entity::delete_by_id(flag.to_owned())
        .exec(conn)
        .await?;
    Ok(())
}

async fn names_in<C: ConnectionTrait>(conn: &C) -> Result<BTreeSet<String>, FlagsError> {
    let mut names: BTreeSet<String> = feature_flag::Entity::find()
        .select_only()
        .column(feature_flag::Column::Name)
        .into_tuple::<String>()
        .all(conn)
        .await?
        .into_iter()
        .collect();
    names.extend(
        feature_flag_override::Entity::find()
            .select_only()
            .column(feature_flag_override::Column::Flag)
            .distinct()
            .into_tuple::<String>()
            .all(conn)
            .await?,
    );
    Ok(names)
}

impl FlagStore for DatabaseFlagStore {
    fn states<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<Vec<FlagState>, FlagsError>> {
        Box::pin(async move { with_reader!(self, uow, |conn| states_in(conn).await) })
    }

    fn state<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<FlagState>, FlagsError>> {
        Box::pin(async move { with_reader!(self, uow, |conn| state_in(conn, name).await) })
    }

    fn save_state<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        state: &'a FlagState,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        Box::pin(async move { with_writer!(self, uow, |conn| save_state_in(conn, state).await) })
    }

    fn overrides_of<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<BTreeMap<String, bool>, FlagsError>> {
        Box::pin(async move {
            with_reader!(self, uow, |conn| overrides_of_in(
                conn, scope_type, scope_id
            )
            .await)
        })
    }

    fn overrides<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FlagOverride>, FlagsError>> {
        Box::pin(async move { with_reader!(self, uow, |conn| overrides_in(conn, flag).await) })
    }

    fn override_counts<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeMap<String, u64>, FlagsError>> {
        Box::pin(async move { with_reader!(self, uow, |conn| override_counts_in(conn).await) })
    }

    fn set_override<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        entry: &'a FlagOverride,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        Box::pin(async move { with_writer!(self, uow, |conn| set_override_in(conn, entry).await) })
    }

    fn remove_override<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<bool, FlagsError>> {
        Box::pin(async move {
            with_writer!(self, uow, |conn| remove_override_in(
                conn, flag, scope_type, scope_id
            )
            .await)
        })
    }

    fn purge<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        Box::pin(async move { with_writer!(self, uow, |conn| purge_in(conn, flag).await) })
    }

    fn names<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeSet<String>, FlagsError>> {
        Box::pin(async move { with_reader!(self, uow, |conn| names_in(conn).await) })
    }
}
