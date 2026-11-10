//! # navree-cache
//!
//! `CacheModule`: the application cache and atomic locks.
//!
//! - [`Cache`] stores JSON-encoded values under a key prefix with a TTL per
//!   entry ([`Ttl`]): `get`, `put`, `add` (only if absent), `forget`, `has`,
//!   `pull`, counters (`increment`/`decrement`), [`remember`](Cache::remember)
//!   (compute on a miss, one computation per key at a time) and
//!   [`flexible`](Cache::flexible) (stale-while-revalidate, refreshed in the
//!   background).
//! - [`Cache::tags`] gives a [`TaggedCache`] whose entries are flushed
//!   together, via tag version keys.
//! - [`Locks`] hands out named [`Lock`]s with owner tokens; a [`LockGuard`]
//!   releases its lock when dropped, and every lock expires after its TTL.
//! - Backends are trait objects: [`CacheStore`] (bytes with TTLs, atomic
//!   `add` and `increment`) and [`LockStore`]. The built-in
//!   [`MemoryStore`] (bounded: at most [`DEFAULT_MAX_ENTRIES`] entries unless
//!   built with [`MemoryStore::with_capacity`]) and [`MemoryLockStore`] keep
//!   data in process and follow Tokio's clock, so `tokio::time::pause`
//!   drives expiry in tests.
//!   `store = "redis"` ([`CacheStoreKind::Redis`]) uses the stores
//!   navree-redis's `RedisModule` registers under the name `redis`; any
//!   other store implements the same traits.
//! - [`CacheModule`] is global: `for_root(CacheOptions)` or `from_config()`
//!   (section `cache`) provides and exports [`Cache`], [`Locks`],
//!   `dyn CacheStore` and `dyn LockStore`, registers a `cache` health check
//!   (`dyn navree_contracts::HealthCheck`) and the `cache:clear` command.
//!
//! ```no_run
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! use navree_cache::{Cache, CacheModule, CacheOptions};
//! use navree_kernel::{Factory, Navree, Module, ModuleBuilder, ProviderKey};
//!
//! struct Dashboard {
//!     cache: Arc<Cache>,
//! }
//!
//! impl Dashboard {
//!     async fn total_deals(&self) -> Result<u64, navree_cache::CacheError> {
//!         self.cache
//!             .remember("dashboard:total_deals", Duration::from_secs(60), || async {
//!                 Ok(42) // run the expensive query here
//!             })
//!             .await
//!     }
//! }
//!
//! struct DashboardModule;
//!
//! impl Module for DashboardModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         // No import needed: CacheModule is global.
//!         m.factory(
//!             Factory::<Dashboard>::new(|r| async move {
//!                 Ok(Arc::new(Dashboard { cache: r.get::<Cache>().await? }))
//!             })
//!             .deps(vec![ProviderKey::of::<Cache>()]),
//!         );
//!     }
//! }
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     Navree::app()
//!         .module(CacheModule::for_root(CacheOptions::default()))
//!         .module(DashboardModule)
//!         .run()
//! }
//! ```
//!
//! Keys starting with `navree:` (and the suffixes `#navree:fresh` and
//! `#navree:refresh`) are used internally by tags, `flexible` and the health
//! check. Errors ([`CacheError`]) and logs name keys, never cached values.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod cache;
mod error;
mod flight;
mod lock;
mod memory;
mod module;
mod store;
mod tags;

pub use cache::{Cache, DEFAULT_TTL, Ttl};
pub use error::CacheError;
pub use lock::{Lock, LockGuard, LockStore, Locks, MemoryLockStore};
pub use memory::{DEFAULT_MAX_ENTRIES, MemoryStore};
pub use module::{CacheModule, CacheOptions, CacheStoreKind};
pub use store::CacheStore;
pub use tags::TaggedCache;
