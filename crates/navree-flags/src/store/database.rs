//! [`DatabaseFlagStore`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::Utc;
use navree_db::{Db, Uow};
use navree_kernel::BoxFuture;
use navree_orm::{Executor, Model as _, OrmError, table};
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

/// Runs `$body` with `$exec` bound to what to run on: the unit of work, else
/// the `Db`, which reads in the task's open unit of work or on the pool and
/// writes where the ORM writes for any `Db`.
macro_rules! with_exec {
    ($self:ident, $uow:expr, |$exec:ident| $body:expr) => {{
        match $uow {
            Some($exec) => $body,
            None => {
                let $exec = &*$self.db;
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

async fn states_in<E: Executor + ?Sized>(exec: &E) -> Result<Vec<FlagState>, FlagsError> {
    feature_flag::Model::query()
        .order_by(feature_flag::Model::NAME)
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(to_state)
        .collect()
}

async fn state_in<E: Executor + ?Sized>(
    exec: &E,
    name: &str,
) -> Result<Option<FlagState>, FlagsError> {
    feature_flag::Model::query()
        .where_(feature_flag::Model::NAME.eq(name))
        .first(exec)
        .await?
        .map(|row| to_state(row.into_inner()))
        .transpose()
}

async fn save_state_in<E: Executor + ?Sized>(
    exec: &E,
    state: &FlagState,
) -> Result<(), FlagsError> {
    let rollout = state
        .rollout
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| FlagsError::Corrupt(error.to_string()))?;
    let row = feature_flag::Model {
        name: state.name.clone(),
        everyone: state.everyone,
        rollout,
        killed: state.killed,
        updated_at: state.updated_at.unwrap_or_else(Utc::now),
        updated_by: state.updated_by,
    };
    feature_flag::Model::query()
        .upsert(
            exec,
            [row],
            ["name"],
            ["everyone", "rollout", "killed", "updated_at", "updated_by"],
        )
        .await?;
    Ok(())
}

async fn overrides_of_in<E: Executor + ?Sized>(
    exec: &E,
    scope_type: ScopeType,
    scope_id: Uuid,
) -> Result<BTreeMap<String, bool>, FlagsError> {
    Ok(feature_flag_override::Model::query()
        .where_(feature_flag_override::Model::SCOPE_TYPE.eq(scope_type.as_str()))
        .where_(feature_flag_override::Model::SCOPE_ID.eq(scope_id))
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(|row| (row.flag, row.active))
        .collect())
}

async fn overrides_in<E: Executor + ?Sized>(
    exec: &E,
    flag: &str,
) -> Result<Vec<FlagOverride>, FlagsError> {
    feature_flag_override::Model::query()
        .where_(feature_flag_override::Model::FLAG.eq(flag))
        .order_by(feature_flag_override::Model::SCOPE_TYPE)
        .order_by(feature_flag_override::Model::SCOPE_ID)
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(to_override)
        .collect()
}

async fn override_counts_in<E: Executor + ?Sized>(
    exec: &E,
) -> Result<BTreeMap<String, u64>, FlagsError> {
    let rows = table("feature_flag_overrides")
        .select(["flag"])
        .select_raw("COUNT(flag) AS count", Vec::<i64>::new())
        .group_by(["flag"])
        .get(exec)
        .await?;
    let mut counts = BTreeMap::new();
    for row in rows {
        let flag: String = row.get("flag").map_err(OrmError::from)?;
        let count: i64 = row.get("count").map_err(OrmError::from)?;
        counts.insert(flag, u64::try_from(count).unwrap_or(0));
    }
    Ok(counts)
}

async fn set_override_in<E: Executor + ?Sized>(
    exec: &E,
    entry: &FlagOverride,
) -> Result<(), FlagsError> {
    let row = feature_flag_override::Model {
        flag: entry.flag.clone(),
        scope_type: entry.scope_type.as_str().to_owned(),
        scope_id: entry.scope_id,
        active: entry.active,
    };
    feature_flag_override::Model::query()
        .upsert(exec, [row], ["flag", "scope_type", "scope_id"], ["active"])
        .await?;
    Ok(())
}

async fn remove_override_in<E: Executor + ?Sized>(
    exec: &E,
    flag: &str,
    scope_type: ScopeType,
    scope_id: Uuid,
) -> Result<bool, FlagsError> {
    let deleted = feature_flag_override::Model::query()
        .where_(feature_flag_override::Model::FLAG.eq(flag))
        .where_(feature_flag_override::Model::SCOPE_TYPE.eq(scope_type.as_str()))
        .where_(feature_flag_override::Model::SCOPE_ID.eq(scope_id))
        .delete(exec)
        .await?;
    Ok(deleted > 0)
}

async fn purge_in<E: Executor + ?Sized>(exec: &E, flag: &str) -> Result<(), FlagsError> {
    feature_flag_override::Model::query()
        .where_(feature_flag_override::Model::FLAG.eq(flag))
        .delete(exec)
        .await?;
    feature_flag::Model::query()
        .where_(feature_flag::Model::NAME.eq(flag))
        .delete(exec)
        .await?;
    Ok(())
}

async fn names_in<E: Executor + ?Sized>(exec: &E) -> Result<BTreeSet<String>, FlagsError> {
    let mut names: BTreeSet<String> = feature_flag::Model::query()
        .pluck(exec, feature_flag::Model::NAME)
        .await?
        .into_iter()
        .collect();
    names.extend(
        feature_flag_override::Model::query()
            .distinct()
            .pluck(exec, feature_flag_override::Model::FLAG)
            .await?,
    );
    Ok(names)
}

impl FlagStore for DatabaseFlagStore {
    fn states<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<Vec<FlagState>, FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| states_in(exec).await) })
    }

    fn state<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<FlagState>, FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| state_in(exec, name).await) })
    }

    fn save_state<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        state: &'a FlagState,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| save_state_in(exec, state).await) })
    }

    fn overrides_of<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<BTreeMap<String, bool>, FlagsError>> {
        Box::pin(async move {
            with_exec!(self, uow, |exec| overrides_of_in(
                exec, scope_type, scope_id
            )
            .await)
        })
    }

    fn overrides<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FlagOverride>, FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| overrides_in(exec, flag).await) })
    }

    fn override_counts<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeMap<String, u64>, FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| override_counts_in(exec).await) })
    }

    fn set_override<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        entry: &'a FlagOverride,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| set_override_in(exec, entry).await) })
    }

    fn remove_override<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<bool, FlagsError>> {
        Box::pin(async move {
            with_exec!(self, uow, |exec| remove_override_in(
                exec, flag, scope_type, scope_id
            )
            .await)
        })
    }

    fn purge<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| purge_in(exec, flag).await) })
    }

    fn names<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeSet<String>, FlagsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| names_in(exec).await) })
    }
}
