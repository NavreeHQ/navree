//! Atomic locks: [`Locks`], [`Lock`], [`LockGuard`], the [`LockStore`] trait
//! and the in-process [`MemoryLockStore`].

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navis_kernel::BoxFuture;
use tokio::time::Instant;

use crate::CacheError;
use crate::memory::deadline;

/// How often [`Lock::block`] retries.
const BLOCK_POLL: Duration = Duration::from_millis(50);

/// A lock backend: named locks held by an owner token until released or
/// expired.
///
/// Both operations must be atomic. [`MemoryLockStore`] is the built-in
/// store; a Redis or database store implements the same trait.
pub trait LockStore: Send + Sync + 'static {
    /// Takes `name` for `owner` if it is free (never taken, released or
    /// expired); `true` if taken. The lock expires after `ttl`.
    fn acquire<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
        ttl: Duration,
    ) -> BoxFuture<'a, Result<bool, CacheError>>;

    /// Frees `name` if `owner` holds it; `true` if it was released.
    fn release<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
    ) -> BoxFuture<'a, Result<bool, CacheError>>;

    /// Called when a [`LockGuard`] is dropped without an explicit release.
    /// The default spawns [`release`](LockStore::release) on the current
    /// Tokio runtime (if any); stores that can release synchronously should
    /// override it so the lock is free as soon as the guard is gone.
    fn release_on_drop(self: Arc<Self>, name: String, owner: String) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = self.release(&name, &owner).await {
                    tracing::warn!(lock = %name, error = %error, "could not release a dropped lock");
                }
            });
        }
    }
}

/// The application's atomic locks, provided (and exported globally) by
/// `CacheModule`; cheap to clone.
///
/// ```
/// use std::time::Duration;
/// use navis_cache::Locks;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navis_cache::CacheError> {
/// let locks = Locks::memory();
/// if let Some(_guard) = locks.lock("reports:nightly", Duration::from_secs(300)).acquire().await? {
///     // Only one holder at a time runs this; dropping the guard releases the lock.
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Locks {
    store: Arc<dyn LockStore>,
    prefix: String,
}

impl Locks {
    /// Locks in `store`; each lock is stored as `prefix + "lock:" + name`.
    pub fn new(store: Arc<dyn LockStore>, prefix: impl Into<String>) -> Self {
        Self {
            store,
            prefix: prefix.into(),
        }
    }

    /// Locks in a fresh [`MemoryLockStore`] (for tests and scripts).
    pub fn memory() -> Self {
        Self::new(Arc::new(MemoryLockStore::new()), "")
    }

    /// The underlying store.
    pub fn store(&self) -> &Arc<dyn LockStore> {
        &self.store
    }

    /// A handle to the lock `name` with a new random owner token. Nothing is
    /// taken until [`Lock::acquire`] or [`Lock::block`]; once taken, the lock
    /// expires after `ttl` even if never released.
    pub fn lock(&self, name: &str, ttl: Duration) -> Lock {
        Lock {
            store: self.store.clone(),
            name: name.to_owned(),
            key: format!("{}lock:{name}", self.prefix),
            owner: uuid::Uuid::new_v4().simple().to_string(),
            ttl,
        }
    }
}

impl fmt::Debug for Locks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Locks")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// A named lock and the owner token used to take and release it.
pub struct Lock {
    store: Arc<dyn LockStore>,
    name: String,
    key: String,
    owner: String,
    ttl: Duration,
}

impl Lock {
    /// Uses `owner` as the owner token, e.g. one handed over by another
    /// process with [`LockGuard::into_owner`], so [`release`](Lock::release)
    /// can free that holder's lock.
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = owner.into();
        self
    }

    /// The lock name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The owner token.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Takes the lock if it is free; `None` if someone else holds it.
    pub async fn acquire(&self) -> Result<Option<LockGuard>, CacheError> {
        if self.store.acquire(&self.key, &self.owner, self.ttl).await? {
            Ok(Some(LockGuard {
                store: self.store.clone(),
                name: self.name.clone(),
                key: self.key.clone(),
                owner: self.owner.clone(),
                armed: true,
            }))
        } else {
            Ok(None)
        }
    }

    /// Waits up to `timeout` for the lock, retrying every 50 ms; fails with
    /// [`CacheError::LockTimeout`] if it stays taken.
    pub async fn block(&self, timeout: Duration) -> Result<LockGuard, CacheError> {
        let started = Instant::now();
        let deadline = started.checked_add(timeout);
        loop {
            if let Some(guard) = self.acquire().await? {
                return Ok(guard);
            }
            let now = Instant::now();
            let wait = match deadline {
                Some(deadline) if now >= deadline => {
                    return Err(CacheError::LockTimeout {
                        name: self.name.clone(),
                        waited: now - started,
                    });
                }
                Some(deadline) => BLOCK_POLL.min(deadline - now),
                None => BLOCK_POLL,
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// Frees the lock if this owner token holds it; `true` if it was released.
    pub async fn release(&self) -> Result<bool, CacheError> {
        self.store.release(&self.key, &self.owner).await
    }
}

impl fmt::Debug for Lock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The owner token is a capability (it can release the lock): not shown.
        f.debug_struct("Lock")
            .field("name", &self.name)
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

/// A held lock. Dropping it releases the lock (if this owner still holds it).
#[must_use = "the lock is released as soon as the guard is dropped"]
pub struct LockGuard {
    store: Arc<dyn LockStore>,
    name: String,
    key: String,
    owner: String,
    armed: bool,
}

impl LockGuard {
    /// The lock name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The owner token.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Releases the lock now; `false` if it had already expired and been
    /// taken by someone else (or released).
    pub async fn release(mut self) -> Result<bool, CacheError> {
        self.armed = false;
        self.store.release(&self.key, &self.owner).await
    }

    /// Keeps the lock held (until its TTL or a release with this token) and
    /// returns the owner token, to hand the lock over to another task or
    /// process (`locks.lock(name, ttl).with_owner(token).release()`).
    pub fn into_owner(mut self) -> String {
        self.armed = false;
        std::mem::take(&mut self.owner)
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if self.armed {
            self.store.clone().release_on_drop(
                std::mem::take(&mut self.key),
                std::mem::take(&mut self.owner),
            );
        }
    }
}

impl fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LockGuard")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// An in-process [`LockStore`]. Expiry follows Tokio's clock (so
/// `tokio::time::pause` controls it in tests) and a dropped guard frees its
/// lock synchronously.
#[derive(Default)]
pub struct MemoryLockStore {
    table: Mutex<LockTable>,
}

#[derive(Default)]
struct LockTable {
    held: HashMap<String, Held>,
    /// Expired entries are swept when the table reaches this size.
    sweep_at: usize,
}

struct Held {
    owner: String,
    expires_at: Option<Instant>,
}

impl Held {
    fn is_live(&self, now: Instant) -> bool {
        self.expires_at.is_none_or(|at| now < at)
    }
}

impl MemoryLockStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn table(&self) -> std::sync::MutexGuard<'_, LockTable> {
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn try_acquire(&self, name: &str, owner: &str, ttl: Duration) -> bool {
        let now = Instant::now();
        let mut table = self.table();
        if table.held.get(name).is_some_and(|held| held.is_live(now)) {
            return false;
        }
        table.held.insert(
            name.to_owned(),
            Held {
                owner: owner.to_owned(),
                expires_at: deadline(Some(ttl)),
            },
        );
        if table.held.len() >= table.sweep_at {
            table.held.retain(|_, held| held.is_live(now));
            table.sweep_at = (table.held.len() * 2).max(64);
        }
        true
    }

    fn release_now(&self, name: &str, owner: &str) -> bool {
        let now = Instant::now();
        let mut table = self.table();
        match table.held.get(name) {
            Some(held) if held.owner == owner => {
                let live = held.is_live(now);
                table.held.remove(name);
                live
            }
            _ => false,
        }
    }
}

impl fmt::Debug for MemoryLockStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryLockStore")
            .field("held", &self.table().held.len())
            .finish()
    }
}

impl LockStore for MemoryLockStore {
    fn acquire<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
        ttl: Duration,
    ) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move { Ok(self.try_acquire(name, owner, ttl)) })
    }

    fn release<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
    ) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move { Ok(self.release_now(name, owner)) })
    }

    fn release_on_drop(self: Arc<Self>, name: String, owner: String) {
        self.release_now(&name, &owner);
    }
}
