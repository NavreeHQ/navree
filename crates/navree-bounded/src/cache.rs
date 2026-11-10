//! [`Cache`] and its [`CacheBuilder`].

use std::borrow::Borrow;
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::hash::{BuildHasher, Hash, RandomState};
use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use tokio::sync::watch;

use crate::clock::{Clock, NEVER, deadline_after, nanos};
use crate::shard::{Dead, FlightState, Lookup, Shard};

/// The least capacity the default shard count gives each shard: below it,
/// hash skew would make shards evict while the cache as a whole has room.
const MIN_SHARD_ENTRIES: usize = 256;

/// The most shards chosen by default.
const MAX_DEFAULT_SHARDS: usize = 64;

/// What [`Cache::compute`]'s closure decides for a key, given the entry it
/// found (a live one: an expired entry is treated as absent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compute<V> {
    /// Leave the entry as it is. A missing key stays missing.
    Keep,
    /// Remove the entry.
    Remove,
    /// Replace the value of a live entry and leave its lifetime alone (the
    /// TTL deadline stays; a read-style use refreshes the idle clock). A
    /// missing key is inserted with the cache's default TTL.
    Update(V),
    /// Store the value with a fresh lifetime: expiring `ttl` from now, or
    /// never for `None`, whatever the cache's default TTL is.
    Store(V, Option<Duration>),
}

/// What [`Cache::compute`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Computed {
    /// There was no live entry and one was inserted.
    Inserted,
    /// A live entry's value was replaced.
    Replaced,
    /// A live entry was removed.
    Removed,
    /// Nothing changed.
    Unchanged,
}

/// Settings for a [`Cache`]: the capacity is required, the rest is optional.
///
/// ```
/// use std::time::Duration;
/// use navree_bounded::Cache;
///
/// let cache: Cache<String, u32> = Cache::builder(10_000)
///     .time_to_live(Duration::from_secs(60))
///     .build();
/// assert_eq!(cache.capacity(), 10_000);
/// ```
pub struct CacheBuilder<K, V> {
    capacity: usize,
    shards: Option<usize>,
    ttl: Option<Duration>,
    tti: Option<Duration>,
    types: PhantomData<fn() -> (K, V)>,
}

impl<K, V> Clone for CacheBuilder<K, V> {
    fn clone(&self) -> Self {
        Self {
            capacity: self.capacity,
            shards: self.shards,
            ttl: self.ttl,
            tti: self.tti,
            types: PhantomData,
        }
    }
}

impl<K, V> fmt::Debug for CacheBuilder<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheBuilder")
            .field("capacity", &self.capacity)
            .field("shards", &self.shards)
            .field("ttl", &self.ttl)
            .field("tti", &self.tti)
            .finish()
    }
}

impl<K, V> CacheBuilder<K, V>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Settings for a cache that holds at most `max_entries` entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            capacity: max_entries,
            shards: None,
            ttl: None,
            tti: None,
            types: PhantomData,
        }
    }

    /// Entries expire this long after they were inserted or replaced. An
    /// entry given its own TTL ([`Cache::insert_with_ttl`]) ignores this.
    #[must_use]
    pub fn time_to_live(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Entries expire after this long without a read or a write. Combined
    /// with a TTL, an entry expires at whichever comes first.
    #[must_use]
    pub fn time_to_idle(mut self, tti: Duration) -> Self {
        self.tti = Some(tti);
        self
    }

    /// How many shards (independent locks) to split the cache into. The
    /// number is rounded down to a power of two and capped by the capacity.
    /// The default follows the processor count and keeps at least 256
    /// entries in each shard.
    #[must_use]
    pub fn shards(mut self, shards: usize) -> Self {
        self.shards = Some(shards);
        self
    }

    /// Builds the cache.
    pub fn build(self) -> Cache<K, V> {
        let shards = shard_count(self.capacity, self.shards);
        // Slab indices are `u32`.
        let per_shard_limit = u32::MAX as usize - 1;
        let capacity = self.capacity.min(per_shard_limit.saturating_mul(shards));
        let (each, extra) = (capacity / shards, capacity % shards);
        let tti = self.tti.map_or(NEVER, nanos);
        let cells = (0..shards)
            .map(|i| Cell {
                lock: RwLock::new(Shard::new(each + usize::from(i < extra), tti)),
                len: AtomicUsize::new(0),
            })
            .collect();
        Cache {
            inner: Arc::new(Inner {
                cells,
                mask: shards - 1,
                hasher: RandomState::new(),
                clock: Clock::new(),
                ttl: self.ttl,
                capacity,
            }),
        }
    }
}

/// How many shards a cache of `capacity` entries gets.
fn shard_count(capacity: usize, requested: Option<usize>) -> usize {
    let wanted = match requested {
        Some(shards) => shards,
        None => {
            let cpus = std::thread::available_parallelism().map_or(4, NonZeroUsize::get);
            cpus.saturating_mul(2)
                .next_power_of_two()
                .min(MAX_DEFAULT_SHARDS)
                .min(capacity / MIN_SHARD_ENTRIES)
        }
    };
    let wanted = wanted.clamp(1, capacity.max(1));
    1 << wanted.ilog2()
}

/// A bounded, sharded, in-process cache. See the [crate documentation](crate)
/// for the design.
///
/// Cloning is cheap: clones share the same entries.
pub struct Cache<K, V> {
    inner: Arc<Inner<K, V>>,
}

struct Inner<K, V> {
    cells: Box<[Cell<K, V>]>,
    mask: usize,
    hasher: RandomState,
    clock: Clock,
    ttl: Option<Duration>,
    capacity: usize,
}

/// A shard behind its lock, with its entry count readable without it.
/// Aligned so that neighbouring shards never share a cache line.
#[repr(align(64))]
struct Cell<K, V> {
    lock: RwLock<Shard<K, V>>,
    len: AtomicUsize,
}

impl<K, V> Clone for Cache<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, V> fmt::Debug for Cache<K, V>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cache")
            .field("len", &self.len())
            .field("capacity", &self.inner.capacity)
            .field("shards", &self.inner.cells.len())
            .finish()
    }
}

impl<K, V> Cell<K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    fn read(&self) -> RwLockReadGuard<'_, Shard<K, V>> {
        loop {
            if let Ok(guard) = self.lock.read() {
                return guard;
            }
            // A panic under the lock (a user's `Hash`, `Eq`, `Clone` or
            // `compute` closure) can leave the shard half updated: empty it,
            // which a cache can always afford. (The poisoned result, which
            // holds a read lock, is gone before this runs.)
            drop(self.write());
        }
    }

    fn write(&self) -> RwLockWriteGuard<'_, Shard<K, V>> {
        match self.lock.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.reset();
                self.len.store(0, Relaxed);
                self.lock.clear_poison();
                guard
            }
        }
    }
}

impl<K, V> Inner<K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    fn hash<Q: Hash + ?Sized>(&self, key: &Q) -> u64 {
        self.hasher.hash_one(key)
    }

    /// The shard of `hash`: bits that neither the table's bucket index (low)
    /// nor its control bytes (top seven) use.
    fn cell(&self, hash: u64) -> &Cell<K, V> {
        &self.cells[(hash >> 32) as usize & self.mask]
    }

    /// Runs `f` on the shard of `hash` under its write lock, with the time
    /// and a graveyard; what `f` removes is dropped after the unlock.
    fn write<R>(
        &self,
        hash: u64,
        f: impl FnOnce(&mut Shard<K, V>, &mut Vec<Dead<K, V>>, u64) -> R,
    ) -> R {
        let cell = self.cell(hash);
        let now = self.clock.now();
        let mut dead = Vec::new();
        let result = {
            let mut shard = cell.write();
            let result = f(&mut shard, &mut dead, now);
            cell.len.store(shard.len(), Relaxed);
            result
        };
        drop(dead);
        result
    }

    /// The deadline of an entry inserted now with the default TTL.
    fn default_deadline(&self, now: u64) -> u64 {
        deadline_after(now, self.ttl)
    }
}

impl<K, V> Cache<K, V>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// A cache that holds at most `max_entries` entries, with no expiry.
    pub fn new(max_entries: usize) -> Self {
        CacheBuilder::new(max_entries).build()
    }

    /// Settings for a cache that holds at most `max_entries` entries.
    pub fn builder(max_entries: usize) -> CacheBuilder<K, V> {
        CacheBuilder::new(max_entries)
    }

    /// The most entries the cache holds; never exceeded, not even for an
    /// instant.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }

    /// How many shards the cache has.
    pub fn shard_count(&self) -> usize {
        self.inner.cells.len()
    }

    /// How many entries the cache holds, expired ones that no read, write or
    /// [`sweep`](Cache::sweep) has removed yet included.
    pub fn len(&self) -> usize {
        self.inner
            .cells
            .iter()
            .map(|cell| cell.len.load(Relaxed))
            .sum()
    }

    /// Whether the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The value under `key`, if there is a live one. A hit marks the entry
    /// as used, so eviction passes it over once and the idle clock restarts.
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let hash = self.inner.hash(key);
        let lookup = self
            .inner
            .cell(hash)
            .read()
            .get(hash, key, &self.inner.clock);
        match lookup {
            Lookup::Hit(value) => Some(value),
            Lookup::Miss => None,
            Lookup::Expired => {
                self.inner
                    .write(hash, |shard, dead, now| shard.reap(hash, key, now, dead));
                None
            }
        }
    }

    /// Whether `key` has a live entry. Unlike [`get`](Cache::get) it does not
    /// count as a use.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let hash = self.inner.hash(key);
        self.inner
            .cell(hash)
            .read()
            .contains(hash, key, &self.inner.clock)
    }

    /// Stores `value` under `key` with the cache's default TTL, replacing
    /// any entry. If the cache is full, an entry is evicted first.
    pub fn insert(&self, key: K, value: V) {
        let hash = self.inner.hash(&key);
        self.inner.write(hash, |shard, dead, now| {
            let deadline = self.inner.default_deadline(now);
            shard.insert(hash, key, value, deadline, now, dead);
        });
    }

    /// Stores `value` under `key` with its own lifetime: expiring `ttl` from
    /// now, or never for `None`, whatever the cache's default TTL is. A TTL
    /// of zero stores nothing (and removes what was there).
    pub fn insert_with_ttl(&self, key: K, value: V, ttl: Option<Duration>) {
        let hash = self.inner.hash(&key);
        self.inner.write(hash, |shard, dead, now| {
            let deadline = deadline_after(now, ttl);
            shard.insert(hash, key, value, deadline, now, dead);
        });
    }

    /// Removes `key` and returns its value if there was a live one.
    pub fn remove<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let hash = self.inner.hash(key);
        self.inner
            .write(hash, |shard, dead, now| shard.take(hash, key, now, dead))
    }

    /// Removes `key`. A load in flight for it will not store its result.
    pub fn invalidate<Q>(&self, key: &Q)
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        drop(self.remove(key));
    }

    /// Removes every entry. Loads in flight will not store their results.
    pub fn invalidate_all(&self) {
        for cell in &self.inner.cells {
            let mut dead = Vec::new();
            {
                let mut shard = cell.write();
                shard.clear(&mut dead);
                cell.len.store(0, Relaxed);
            }
            drop(dead);
        }
    }

    /// Reads and changes the entry under `key` in one atomic step: `f` sees
    /// the live value (or `None`) and decides with a [`Compute`]. No other
    /// operation on the key's shard runs meanwhile, so keep `f` short and do
    /// not touch this cache from it.
    ///
    /// ```
    /// use navree_bounded::{Cache, Compute};
    ///
    /// let counters: Cache<String, u64> = Cache::new(100);
    /// for _ in 0..3 {
    ///     counters.compute("hits", |current| Compute::Update(current.map_or(1, |n| n + 1)));
    /// }
    /// assert_eq!(counters.get("hits"), Some(3));
    /// ```
    pub fn compute<Q, F>(&self, key: &Q, f: F) -> Computed
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
        F: FnOnce(Option<&V>) -> Compute<V>,
    {
        let hash = self.inner.hash(key);
        self.inner.write(hash, |shard, dead, now| {
            let default_deadline = self.inner.default_deadline(now);
            shard.compute(hash, key, now, default_deadline, f, dead)
        })
    }

    /// Looks at up to `budget` entries in each shard (from where the last
    /// sweep stopped) and removes the expired ones; returns how many it
    /// removed. Reads remove the expired entries they meet and every new
    /// key's insertion sweeps a few slots, so this is for a periodic task of
    /// an app that wants memory back during a quiet spell.
    pub fn sweep(&self, budget: usize) -> usize {
        let now = self.inner.clock.now();
        let mut removed = 0;
        for cell in &self.inner.cells {
            let mut dead = Vec::new();
            {
                let mut shard = cell.write();
                removed += shard.sweep(now, budget, &mut dead);
                cell.len.store(shard.len(), Relaxed);
            }
            drop(dead);
        }
        removed
    }

    /// The value under `key`, loaded with `init` on a miss. Callers that
    /// miss on the same key at the same time share one `init`: the first
    /// runs it, the rest wait for its value.
    ///
    /// See [`try_get_with`](Cache::try_get_with) for cancellation and panics.
    pub async fn get_with<F>(&self, key: K, init: F) -> V
    where
        F: Future<Output = V>,
    {
        match self
            .try_get_with(key, async { Ok::<V, Infallible>(init.await) })
            .await
        {
            Ok(value) => value,
            Err(never) => match *never {},
        }
    }

    /// The value under `key`, loaded with `init` on a miss, where loading can
    /// fail.
    ///
    /// - Callers that miss on the same key at the same time share one `init`:
    ///   the first runs it, the rest wait. All of them get its value, or all
    ///   get the same error (an `Arc`, so it need not be `Clone`).
    /// - An error is not cached: the next call runs `init` again.
    /// - If the caller running `init` is cancelled (its future dropped) or
    ///   `init` panics, the key is left as it was and one of the waiting
    ///   callers runs its own `init`. Nothing waits forever.
    /// - [`insert`](Cache::insert), [`remove`](Cache::remove),
    ///   [`compute`](Cache::compute) and the invalidations abandon a load in
    ///   flight for their key: its result goes to its own callers but is not
    ///   stored, so a stale load never overwrites a newer write. A call after
    ///   that starts a new load.
    /// - A value that is already expired on arrival (a zero TTL) is returned
    ///   but not stored.
    pub async fn try_get_with<F, E>(&self, key: K, init: F) -> Result<V, Arc<E>>
    where
        F: Future<Output = Result<V, E>>,
        E: Send + Sync + 'static,
    {
        if let Some(value) = self.get(&key) {
            return Ok(value);
        }
        let hash = self.inner.hash(&key);
        let mut key = key;
        loop {
            match self.begin(hash, key) {
                Step::Hit(value) => return Ok(value),
                Step::Lead(lead) => {
                    return match init.await {
                        Ok(value) => {
                            lead.succeed(value.clone());
                            Ok(value)
                        }
                        Err(error) => {
                            let error = Arc::new(error);
                            lead.fail(Arc::clone(&error));
                            Err(error)
                        }
                    };
                }
                Step::Wait(waiting, mut rx) => {
                    key = waiting;
                    if let Ok(state) = rx.wait_for(Option::is_some).await {
                        match state.as_ref() {
                            Some(Ok(value)) => return Ok(value.clone()),
                            Some(Err(error)) => {
                                if let Ok(error) = Arc::clone(error).downcast::<E>() {
                                    return Err(error);
                                }
                            }
                            None => {}
                        }
                    }
                    // The load was abandoned (or failed with another error
                    // type): look again; this caller may lead the next one.
                }
            }
        }
    }

    /// Under the shard's lock: a live value, the load already in flight for
    /// `key`, or the lead of a new one.
    fn begin(&self, hash: u64, key: K) -> Step<'_, K, V> {
        self.inner.write(hash, |shard, dead, now| {
            if let Some(value) = shard.get_mut(hash, &key, now, dead) {
                return Step::Hit(value);
            }
            if let Some(rx) = shard.flight(hash, &key) {
                return Step::Wait(key, rx);
            }
            let (tx, rx) = watch::channel(None);
            let id = shard.start_flight(hash, key, rx);
            Step::Lead(Lead {
                inner: &self.inner,
                hash,
                id,
                tx: Some(tx),
            })
        })
    }
}

/// What a caller of a load does next.
enum Step<'a, K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    /// The value is there.
    Hit(V),
    /// Another caller is loading it.
    Wait(K, watch::Receiver<FlightState<V>>),
    /// This caller loads it.
    Lead(Lead<'a, K, V>),
}

/// The caller running a load. Dropping it without finishing (the caller was
/// cancelled, or the load panicked) takes the load out of the shard and
/// closes the channel, which sends the waiting callers around again.
struct Lead<'a, K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    inner: &'a Inner<K, V>,
    hash: u64,
    id: u64,
    tx: Option<watch::Sender<FlightState<V>>>,
}

impl<K, V> Lead<'_, K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    /// Stores the value (unless the load was abandoned meanwhile), then
    /// hands it to the waiters.
    fn succeed(mut self, value: V) {
        self.inner.write(self.hash, |shard, dead, now| {
            let deadline = self.inner.default_deadline(now);
            shard.finish_flight(self.hash, self.id, value.clone(), deadline, now, dead);
        });
        if let Some(tx) = self.tx.take()
            && tx.receiver_count() > 0
        {
            tx.send_replace(Some(Ok(value)));
        }
    }

    /// Takes the load out of the shard and hands the error to the waiters.
    fn fail<E: Send + Sync + 'static>(mut self, error: Arc<E>) {
        self.end();
        if let Some(tx) = self.tx.take() {
            tx.send_replace(Some(Err(error)));
        }
    }

    fn end(&self) {
        self.inner.write(self.hash, |shard, dead, _| {
            if let Some(key) = shard.end_flight(self.hash, self.id) {
                dead.push(Dead::Key(key));
            }
        });
    }
}

impl<K, V> Drop for Lead<'_, K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    fn drop(&mut self) {
        // Still armed: abandoned. The flight goes first, then the sender
        // (a field, dropped after this body), so a waiter that wakes finds
        // no stale flight.
        if self.tx.is_some() {
            self.end();
        }
    }
}
