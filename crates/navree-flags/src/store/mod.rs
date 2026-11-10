//! [`FlagStore`]: where flag state lives. [`DatabaseFlagStore`] keeps it in
//! `feature_flags` and `feature_flag_overrides`; [`MemoryFlagStore`] in
//! process (tests).

use std::collections::{BTreeMap, BTreeSet};

use navree_db::Uow;
use navree_kernel::BoxFuture;
use uuid::Uuid;

use crate::{FlagOverride, FlagState, FlagsError, ScopeType};

mod database;
mod memory;

pub use database::DatabaseFlagStore;
pub use memory::MemoryFlagStore;

/// Storage for flag state. The `Flags` service resolves through it (with
/// a cache in front) and changes it.
///
/// Every method takes the unit of work to run in: writes through `Some(uow)`
/// join its transaction (and commit or roll back with it), reads through it
/// see its uncommitted writes. With `None`, a database store reads with
/// `Db::reader` (the task's open transaction, else the pool) and writes on
/// the pool. A store that has no transactions ignores the unit of work.
pub trait FlagStore: Send + Sync + 'static {
    /// Every stored global state.
    fn states<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<Vec<FlagState>, FlagsError>>;

    /// The stored global state of flag `name`, if any.
    fn state<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<FlagState>, FlagsError>>;

    /// Stores `state` (insert or replace by name).
    fn save_state<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        state: &'a FlagState,
    ) -> BoxFuture<'a, Result<(), FlagsError>>;

    /// Every override of one user or workspace, by flag name.
    fn overrides_of<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<BTreeMap<String, bool>, FlagsError>>;

    /// Every override of flag `flag`, users first, then by id.
    fn overrides<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FlagOverride>, FlagsError>>;

    /// How many overrides each flag has (flags without any are left out).
    fn override_counts<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeMap<String, u64>, FlagsError>>;

    /// Stores an override (insert or replace by flag and scope).
    fn set_override<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        entry: &'a FlagOverride,
    ) -> BoxFuture<'a, Result<(), FlagsError>>;

    /// Removes an override; whether there was one.
    fn remove_override<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
        scope_type: ScopeType,
        scope_id: Uuid,
    ) -> BoxFuture<'a, Result<bool, FlagsError>>;

    /// Removes the global state and every override of flag `flag`.
    fn purge<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        flag: &'a str,
    ) -> BoxFuture<'a, Result<(), FlagsError>>;

    /// The name of every flag that has a global state or an override.
    fn names<'a>(
        &'a self,
        uow: Option<&'a Uow>,
    ) -> BoxFuture<'a, Result<BTreeSet<String>, FlagsError>>;
}
