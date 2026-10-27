//! [`DatabaseSettingsStore`].

use std::sync::Arc;

use navree_db::{Db, Uow};
use navree_kernel::BoxFuture;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveValue, ColumnTrait, Condition, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect,
};
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

fn is_target(target: &ScopeTarget) -> Condition {
    Condition::all()
        .add(setting::Column::ScopeType.eq(target.kind.as_str()))
        .add(setting::Column::ScopeId.eq(target.stored_id()))
}

fn sorted(mut rows: Vec<StoredSetting>) -> Vec<StoredSetting> {
    rows.sort_by(|a, b| {
        (&a.key, a.scope.kind, a.scope.stored_id()).cmp(&(&b.key, b.scope.kind, b.scope.stored_id()))
    });
    rows
}

async fn load_in<C: ConnectionTrait>(
    conn: &C,
    key: &str,
    targets: &[ScopeTarget],
) -> Result<Vec<StoredSetting>, SettingsError> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let any = targets
        .iter()
        .fold(Condition::any(), |any, target| any.add(is_target(target)));
    setting::Entity::find()
        .filter(setting::Column::Key.eq(key))
        .filter(any)
        .all(conn)
        .await?
        .into_iter()
        .map(to_setting)
        .collect()
}

async fn save_in<C: ConnectionTrait>(
    conn: &C,
    setting: &StoredSetting,
) -> Result<(), SettingsError> {
    let row = setting::ActiveModel {
        key: ActiveValue::Set(setting.key.clone()),
        scope_type: ActiveValue::Set(setting.scope.kind.as_str().to_owned()),
        scope_id: ActiveValue::Set(setting.scope.stored_id()),
        value: ActiveValue::Set(Value::Object(setting.values.clone())),
        overridden_fields: ActiveValue::Set(Value::from(setting.overridden_fields())),
        updated_at: ActiveValue::Set(setting.updated_at),
        updated_by: ActiveValue::Set(setting.updated_by),
    };
    setting::Entity::insert(row)
        .on_conflict(
            OnConflict::columns([
                setting::Column::Key,
                setting::Column::ScopeType,
                setting::Column::ScopeId,
            ])
            .update_columns([
                setting::Column::Value,
                setting::Column::OverriddenFields,
                setting::Column::UpdatedAt,
                setting::Column::UpdatedBy,
            ])
            .to_owned(),
        )
        .exec_without_returning(conn)
        .await?;
    Ok(())
}

async fn delete_in<C: ConnectionTrait>(
    conn: &C,
    key: &str,
    target: ScopeTarget,
) -> Result<bool, SettingsError> {
    let result = setting::Entity::delete_many()
        .filter(setting::Column::Key.eq(key))
        .filter(is_target(&target))
        .exec(conn)
        .await?;
    Ok(result.rows_affected > 0)
}

async fn delete_scope_in<C: ConnectionTrait>(
    conn: &C,
    target: ScopeTarget,
) -> Result<Vec<String>, SettingsError> {
    let mut keys: Vec<String> = setting::Entity::find()
        .filter(is_target(&target))
        .all(conn)
        .await?
        .into_iter()
        .map(|row| row.key)
        .collect();
    if keys.is_empty() {
        return Ok(keys);
    }
    setting::Entity::delete_many()
        .filter(is_target(&target))
        .exec(conn)
        .await?;
    keys.sort();
    keys.dedup();
    Ok(keys)
}

async fn list_in<C: ConnectionTrait>(
    conn: &C,
    key: Option<&str>,
) -> Result<Vec<StoredSetting>, SettingsError> {
    let mut select = setting::Entity::find();
    if let Some(key) = key {
        select = select.filter(setting::Column::Key.eq(key));
    }
    let rows = select
        .all(conn)
        .await?
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
        Box::pin(async move { with_reader!(self, uow, |conn| load_in(conn, key, targets).await) })
    }

    fn load_for_update<'a>(
        &'a self,
        uow: &'a Uow,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Option<StoredSetting>, SettingsError>> {
        Box::pin(async move {
            let tx = uow.tx().await?;
            let mut select = setting::Entity::find()
                .filter(setting::Column::Key.eq(key))
                .filter(is_target(&target));
            if uow.lock_for_update() {
                select = select.lock_exclusive();
            }
            select.one(&*tx).await?.map(to_setting).transpose()
        })
    }

    fn save<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        setting: &'a StoredSetting,
    ) -> BoxFuture<'a, Result<(), SettingsError>> {
        Box::pin(async move { with_writer!(self, uow, |conn| save_in(conn, setting).await) })
    }

    fn delete<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<bool, SettingsError>> {
        Box::pin(async move { with_writer!(self, uow, |conn| delete_in(conn, key, target).await) })
    }

    fn delete_scope<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Vec<String>, SettingsError>> {
        Box::pin(
            async move { with_writer!(self, uow, |conn| delete_scope_in(conn, target).await) },
        )
    }

    fn list<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>> {
        Box::pin(async move { with_reader!(self, uow, |conn| list_in(conn, key).await) })
    }
}
