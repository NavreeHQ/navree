//! [`MemorySettingsStore`].

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use navree_db::Uow;
use navree_kernel::BoxFuture;
use uuid::Uuid;

use super::{SettingsStore, StoredSetting};
use crate::{ScopeKind, ScopeTarget, SettingsError};

/// The row key: settings key, level, stored id.
type RowKey = (String, ScopeKind, Uuid);

/// Stored values in process (`settings.store = "memory"`, and the
/// `SettingsFake`): lost on restart, and not transactional, so a write is
/// kept even when its unit of work rolls back.
#[derive(Debug, Default)]
pub struct MemorySettingsStore {
    rows: Mutex<BTreeMap<RowKey, StoredSetting>>,
}

impl MemorySettingsStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, BTreeMap<RowKey, StoredSetting>> {
        self.rows.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn row_key(key: &str, target: ScopeTarget) -> RowKey {
    (key.to_owned(), target.kind, target.stored_id())
}

impl SettingsStore for MemorySettingsStore {
    fn load<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        key: &'a str,
        targets: &'a [ScopeTarget],
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>> {
        let rows = self.rows();
        let found = targets
            .iter()
            .filter_map(|target| rows.get(&row_key(key, *target)).cloned())
            .collect();
        Box::pin(async move { Ok(found) })
    }

    fn save<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        setting: &'a StoredSetting,
    ) -> BoxFuture<'a, Result<(), SettingsError>> {
        self.rows()
            .insert(row_key(&setting.key, setting.scope), setting.clone());
        Box::pin(async { Ok(()) })
    }

    fn delete<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        key: &'a str,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<bool, SettingsError>> {
        let removed = self.rows().remove(&row_key(key, target)).is_some();
        Box::pin(async move { Ok(removed) })
    }

    fn delete_scope<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        target: ScopeTarget,
    ) -> BoxFuture<'a, Result<Vec<String>, SettingsError>> {
        let mut rows = self.rows();
        let mut keys = Vec::new();
        rows.retain(|(key, kind, id), _| {
            let hit = *kind == target.kind && *id == target.stored_id();
            if hit {
                keys.push(key.clone());
            }
            !hit
        });
        keys.sort();
        Box::pin(async move { Ok(keys) })
    }

    fn list<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        key: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<StoredSetting>, SettingsError>> {
        let rows: Vec<StoredSetting> = self
            .rows()
            .iter()
            .filter(|((row_key, _, _), _)| key.is_none_or(|key| key == row_key))
            .map(|(_, row)| row.clone())
            .collect();
        Box::pin(async move { Ok(rows) })
    }
}
