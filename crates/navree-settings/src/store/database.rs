//! [`DatabaseSettingsStore`].

use std::sync::Arc;

use navree_db::{Db, Uow};
use navree_kernel::BoxFuture;
use navree_orm::{Executor, Filter, Model as _};
use serde_json::Value;

use super::{SettingsStore, StoredSetting};
use crate::entity::setting;
use crate::{ScopeKind, ScopeTarget, SettingsError};

/// Stored values in the `settings` table (`settings.store = "database"`,
/// the default).
#[derive(Clone, Debug)]
pub struct DatabaseSettingsStore {
    db: Arc<Db>,
}

impl DatabaseSettingsStore {
    /// A store on `db` (the `settings` migrations must have run).
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

fn to_setting(row: setting::Model) -> Result<StoredSetting, SettingsError> {
    let kind = row.scope_type.parse::<ScopeKind>().map_err(|error| {
        SettingsError::Corrupt(format!("a row of settings `{}`: {error}", row.key))
    })?;
    let values = match row.value {
        Value::Object(values) => values,
        other => {
            return Err(SettingsError::Corrupt(format!(
                "the value of settings `{}` at {} is not a JSON object: {other}",
                row.key, row.scope_type
            )));
        }
    };
    Ok(StoredSetting {
        key: row.key,
        scope: ScopeTarget::from_stored(kind, row.scope_id),
        values,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
    })
}

fn is_target(target: &ScopeTarget) -> Filter {
    Filter::all([
        setting::Model::SCOPE_TYPE.eq(target.kind.as_str()),
        setting::Model::SCOPE_ID.eq(target.stored_id()),
    ])
}

fn sorted(mut rows: Vec<StoredSetting>) -> Vec<StoredSetting> {
    rows.sort_by(|a, b| {
        (&a.key, a.scope.kind, a.scope.stored_id()).cmp(&(
            &b.key,
            b.scope.kind,
            b.scope.stored_id(),
        ))
    });
    rows
}

async fn load_in<E: Executor + ?Sized>(
    exec: &E,
    key: &str,
    targets: &[ScopeTarget],
) -> Result<Vec<StoredSetting>, SettingsError> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    setting::Model::query()
        .where_(setting::Model::KEY.eq(key))
        .where_(Filter::any(targets.iter().map(is_target)))
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(to_setting)
        .collect()
}

async fn save_in<E: Executor + ?Sized>(
    exec: &E,
    setting: &StoredSetting,
) -> Result<(), SettingsError> {
    let row = setting::Model {
        key: setting.key.clone(),
        scope_type: setting.scope.kind.as_str().to_owned(),
        scope_id: setting.scope.stored_id(),
        value: Value::Object(setting.values.clone()),
        overridden_fields: Value::from(setting.overridden_fields()),
        updated_at: setting.updated_at,
        updated_by: setting.updated_by,
    };
    setting::Model::query()
        .upsert(
            exec,
            [row],
            ["key", "scope_type", "scope_id"],
            ["value", "overridden_fields", "updated_at", "updated_by"],
        )
        .await?;
    Ok(())
}

async fn delete_in<E: Executor + ?Sized>(
    exec: &E,
    key: &str,
    target: ScopeTarget,
) -> Result<bool, SettingsError> {
    let deleted = setting::Model::query()
        .where_(setting::Model::KEY.eq(key))
        .where_(is_target(&target))
        .delete(exec)
        .await?;
    Ok(deleted > 0)
}

async fn delete_scope_in<E: Executor + ?Sized>(
    exec: &E,
    target: ScopeTarget,
) -> Result<Vec<String>, SettingsError> {
    let mut keys: Vec<String> = setting::Model::query()
        .where_(is_target(&target))
        .pluck(exec, setting::Model::KEY)
        .await?;
    if keys.is_empty() {
        return Ok(keys);
    }
    setting::Model::query()
        .where_(is_target(&target))
        .delete(exec)
        .await?;
    keys.sort();
    keys.dedup();
    Ok(keys)
}

async fn list_in<E: Executor + ?Sized>(
    exec: &E,
    key: Option<&str>,
) -> Result<Vec<StoredSetting>, SettingsError> {
    let rows = setting::Model::query()
        .when_some(key, |query, key| {
            query.where_(setting::Model::KEY.eq(key))
        })
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(to_setting)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(sorted(rows))
}

impl SettingsStore for DatabaseSettingsStore {
    fn load<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: &'a str,
        targets: &'a [ScopeTarget],
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| load_in(exec, key, targets).await) })
    }

    fn load_for_update<'a>(
        &'a self,
        uow: &'a Uow,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Option<StoredSetting>, SettingsError>> {
        Box::pin(async move {
            let mut select = setting::Model::query()
                .where_(setting::Model::KEY.eq(key))
                .where_(is_target(&target));
            if uow.lock_for_update() {
                select = select.lock_for_update();
            }
            select
                .first(uow)
                .await?
                .map(|row| to_setting(row.into_inner()))
                .transpose()
        })
    }

    fn save<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        setting: &'a StoredSetting,
    ) -> BoxFuture<'a, Result<(), SettingsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| save_in(exec, setting).await) })
    }

    fn delete<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<bool, SettingsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| delete_in(exec, key, target).await) })
    }

    fn delete_scope<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Vec<String>, SettingsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| delete_scope_in(exec, target).await) })
    }

    fn list<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>> {
        Box::pin(async move { with_exec!(self, uow, |exec| list_in(exec, key).await) })
    }
}
