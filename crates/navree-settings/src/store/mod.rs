//! [`SettingsStore`]: where stored values live. [`DatabaseSettingsStore`]
//! keeps them in the `settings` table; [`MemorySettingsStore`] in process
//! (tests).

use chrono::{DateTime, Utc};
use navree_db::Uow;
use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{ScopeTarget, SettingsError};

mod database;
mod memory;

pub use database::DatabaseSettingsStore;
pub use memory::MemorySettingsStore;

/// The values stored for one settings key at one level: one row of
/// `settings`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredSetting {
    /// The settings key.
    pub key: String,
    /// The level.
    pub scope: ScopeTarget,
    /// The fields set at the level.
    pub values: Map<String, Value>,
    /// When they last changed.
    pub updated_at: DateTime<Utc>,
    /// Who changed them, when a user did.
    pub updated_by: Option<Uuid>,
}

impl StoredSetting {
    /// The fields set at the level, sorted.
    pub fn overridden_fields(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }
}

/// Storage for stored values. The `Settings` service resolves through it
/// (with a cache in front) and changes it.
///
/// Every method takes the unit of work to run in: writes through `Some(uow)`
/// join its transaction (and commit or roll back with it), reads through it
/// see its uncommitted writes. With `None`, a database store reads with
/// `Db::reader` (the task's open transaction, else the pool) and writes on
/// the pool. A store without transactions ignores the unit of work.
pub trait SettingsStore: Send + Sync + 'static {
    /// The rows of `key` at `targets` (those that exist, in any order).
    fn load<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: &'a str,
        targets: &'a [ScopeTarget],
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>>;

    /// The row of `key` at `target`, locked in `uow` until it finishes
    /// where the database supports row locks (`Uow::lock_for_update`), so
    /// a read-modify-write does not lose a concurrent change. By default,
    /// [`load`](SettingsStore::load).
    fn load_for_update<'a>(
        &'a self,
        uow: &'a Uow,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Option<StoredSetting>, SettingsError>> {
        Box::pin(async move {
            Ok(self
                .load(Some(uow), key, std::slice::from_ref(&target))
                .await?
                .into_iter()
                .next())
        })
    }

    /// Stores `setting` (insert, or replace by key and level).
    fn save<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        setting: &'a StoredSetting,
    ) -> BoxFuture<'a, Result<(), SettingsError>>;

    /// Removes the row of `key` at `target`; whether there was one.
    fn delete<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<bool, SettingsError>>;

    /// Removes every row stored at `target` (a deleted workspace or
    /// organization); the keys it held, sorted.
    fn delete_scope<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Vec<String>, SettingsError>>;

    /// Every row, or every row of `key`, ordered by key, then level
    /// (global first), then id.
    fn list<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        key: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>>;
}
