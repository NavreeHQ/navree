//! [`MemoryFlagStore`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use navree_db::Uow;
use navree_kernel::BoxFuture;
use uuid::Uuid;

use super::FlagStore;
use crate::{FlagOverride, FlagState, FlagsError, ScopeType};

/// Flag state in process (`flags.store = "memory"`): for tests and
/// single-process tools. It has no transactions, so writes apply at once,
/// whatever the unit of work does later.
#[derive(Debug, Default)]
pub struct MemoryFlagStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    states: BTreeMap<String, FlagState>,
    /// (flag, scope type, scope id) → active.
    overrides: BTreeMap<(String, ScopeType, Uuid), bool>,
}

impl MemoryFlagStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, Result<T, FlagsError>> {
    Box::pin(async move { Ok(value) })
}

impl FlagStore for MemoryFlagStore {
    fn states<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<Vec<FlagState>, FlagsError>> {
        ready(self.lock().states.values().cloned().collect())
    }

    fn state<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<FlagState>, FlagsError>> {
        ready(self.lock().states.get(name).cloned())
    }

    fn save_state<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        state: &'a FlagState,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        self.lock()
            .states
            .insert(state.name.clone(), state.clone());
        ready(())
    }

    fn overrides_of<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<BTreeMap<String, bool>, FlagsError>> {
        let found = self
            .lock()
            .overrides
            .iter()
            .filter(|((_, kind, id), _)| *kind == scope_type && *id == scope_id)
            .map(|((flag, _, _), active)| (flag.clone(), *active))
            .collect();
        ready(found)
    }

    fn overrides<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FlagOverride>, FlagsError>> {
        let mut found: Vec<FlagOverride> = self
            .lock()
            .overrides
            .iter()
            .filter(|((name, _, _), _)| name == flag)
            .map(|((name, scope_type, scope_id), active)| FlagOverride {
                flag: name.clone(),
                scope_type: *scope_type,
                scope_id: *scope_id,
                active: *active,
            })
            .collect();
        found.sort_by_key(|entry| (entry.scope_type, entry.scope_id));
        ready(found)
    }

    fn override_counts<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeMap<String, u64>, FlagsError>> {
        let mut counts = BTreeMap::new();
        for (flag, _, _) in self.lock().overrides.keys() {
            *counts.entry(flag.clone()).or_insert(0) += 1;
        }
        ready(counts)
    }

    fn set_override<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        entry: &'a FlagOverride,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        self.lock().overrides.insert(
            (entry.flag.clone(), entry.scope_type, entry.scope_id),
            entry.active,
        );
        ready(())
    }

    fn remove_override<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        flag: &'a str,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<bool, FlagsError>> {
        let removed = self
            .lock()
            .overrides
            .remove(&(flag.to_owned(), scope_type, scope_id))
            .is_some();
        ready(removed)
    }

    fn purge<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<(), FlagsError>> {
        let mut inner = self.lock();
        inner.states.remove(flag);
        inner.overrides.retain(|(name, _, _), _| name != flag);
        ready(())
    }

    fn names<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeSet<String>, FlagsError>> {
        let inner = self.lock();
        let names = inner
            .states
            .keys()
            .cloned()
            .chain(inner.overrides.keys().map(|(flag, _, _)| flag.clone()))
            .collect();
        ready(names)
    }
}
