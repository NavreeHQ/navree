//! One shard: a slab of entries linked into the SIEVE queue, a hash table of
//! slab indices, and the shard's in-flight loads.
//!
//! Nothing here locks: the cache wraps each shard in a `RwLock`. Methods that
//! only read take `&self` (so many readers share the lock); they touch an
//! entry through atomics. Methods that change the shard take `&mut self` and
//! push whatever they remove into a graveyard, so the cache drops keys and
//! values (arbitrary user code) after it has released the lock.

use std::any::Any;
use std::borrow::Borrow;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

use hashbrown::HashTable;
use tokio::sync::watch;

use crate::clock::{Clock, NEVER, deadline_after};
use crate::{Compute, Computed};

#[cfg(test)]
mod tests;

/// "No slot": the end of the queue and of the free list.
const NIL: u32 = u32::MAX;

/// How many slots each insertion of a new key sweeps for expired entries.
pub(crate) const SWEEP_PER_INSERT: usize = 4;

/// What a flight tells its waiters: still loading (`None`), the loaded
/// value, or the loader's error (type-erased; waiters downcast it).
pub(crate) type FlightState<V> = Option<Result<V, Arc<dyn Any + Send + Sync>>>;

/// A load in progress for one key.
pub(crate) struct Flight<K, V> {
    hash: u64,
    id: u64,
    key: K,
    rx: watch::Receiver<FlightState<V>>,
}

/// An entry in the slab and in the SIEVE queue.
pub(crate) struct Node<K, V> {
    key: K,
    value: V,
    hash: u64,
    /// When the entry stops being visible (`NEVER`: no TTL).
    deadline: u64,
    /// The last read or write, for the idle limit.
    accessed: AtomicU64,
    /// SIEVE's visited bit: set by a read, cleared by the eviction hand.
    visited: AtomicBool,
    /// Toward the head of the queue (inserted later).
    newer: u32,
    /// Toward the tail of the queue (inserted earlier).
    older: u32,
}

impl<K, V> Node<K, V> {
    fn is_expired(&self, now: u64, tti: u64) -> bool {
        now >= self.deadline || now >= self.accessed.load(Relaxed).saturating_add(tti)
    }

    /// Records a read. The writes are skipped when they would change
    /// nothing, so a hot entry's cache line is not bounced between cores.
    fn touch(&self, now: u64, tti: u64) {
        if !self.visited.load(Relaxed) {
            self.visited.store(true, Relaxed);
        }
        if tti != NEVER && self.accessed.load(Relaxed) < now {
            self.accessed.fetch_max(now, Relaxed);
        }
    }
}

pub(crate) enum Slot<K, V> {
    Occupied(Node<K, V>),
    /// A free slot, pointing at the next one.
    Vacant(u32),
}

/// What a shard removed or replaced, to be dropped outside its lock.
#[allow(
    dead_code,
    reason = "held only to be dropped after the lock is released"
)]
pub(crate) enum Dead<K, V> {
    Node(Node<K, V>),
    Value(V),
    Key(K),
    Slots(Vec<Slot<K, V>>),
    Flights(HashTable<Flight<K, V>>),
}

/// The result of a read-locked lookup.
pub(crate) enum Lookup<V> {
    Hit(V),
    Miss,
    /// Found, but past its deadline: the caller removes it under the write lock.
    Expired,
}

pub(crate) struct Shard<K, V> {
    /// Slab indices, hashed by their entry's key hash.
    table: HashTable<u32>,
    slots: Vec<Slot<K, V>>,
    /// The first free slot.
    free: u32,
    /// The newest entry: new keys are linked in here.
    head: u32,
    /// The oldest entry: the eviction hand starts here.
    tail: u32,
    /// The next eviction candidate (`NIL`: start again at the tail).
    hand: u32,
    len: usize,
    /// How many entries have a deadline (the sweep skips shards with none).
    expiring: usize,
    capacity: usize,
    /// The idle limit in nanoseconds (`NEVER`: none).
    tti: u64,
    /// Where the next sweep resumes.
    cursor: usize,
    flights: HashTable<Flight<K, V>>,
    next_flight: u64,
}

impl<K, V> Shard<K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    pub(crate) fn new(capacity: usize, tti: u64) -> Self {
        Self {
            table: HashTable::new(),
            slots: Vec::new(),
            free: NIL,
            head: NIL,
            tail: NIL,
            hand: NIL,
            len: 0,
            expiring: 0,
            capacity,
            tti,
            cursor: 0,
            flights: HashTable::new(),
            next_flight: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Drops everything and starts empty (a shard whose lock was poisoned
    /// cannot be trusted).
    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.capacity, self.tti);
    }

    fn node(&self, index: u32) -> &Node<K, V> {
        match &self.slots[index as usize] {
            Slot::Occupied(node) => node,
            Slot::Vacant(_) => unreachable!("the queue and the table only hold occupied slots"),
        }
    }

    fn node_mut(&mut self, index: u32) -> &mut Node<K, V> {
        match &mut self.slots[index as usize] {
            Slot::Occupied(node) => node,
            Slot::Vacant(_) => unreachable!("the queue and the table only hold occupied slots"),
        }
    }

    fn find<Q>(&self, hash: u64, key: &Q) -> Option<u32>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.table
            .find(hash, |&index| match &self.slots[index as usize] {
                Slot::Occupied(node) => node.hash == hash && node.key.borrow() == key,
                Slot::Vacant(_) => false,
            })
            .copied()
    }

    // ---- reads (shared lock) ----

    /// The live value under `key`, marking the entry as used.
    pub(crate) fn get<Q>(&self, hash: u64, key: &Q, clock: &Clock) -> Lookup<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let Some(index) = self.find(hash, key) else {
            return Lookup::Miss;
        };
        let node = self.node(index);
        if node.deadline != NEVER || self.tti != NEVER {
            let now = clock.now();
            if node.is_expired(now, self.tti) {
                return Lookup::Expired;
            }
            node.touch(now, self.tti);
        } else if !node.visited.load(Relaxed) {
            node.visited.store(true, Relaxed);
        }
        Lookup::Hit(node.value.clone())
    }

    /// Whether `key` has a live entry, without marking it as used.
    pub(crate) fn contains<Q>(&self, hash: u64, key: &Q, clock: &Clock) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.find(hash, key).is_some_and(|index| {
            let node = self.node(index);
            node.deadline == NEVER && self.tti == NEVER || !node.is_expired(clock.now(), self.tti)
        })
    }

    // ---- writes (exclusive lock) ----

    /// Removes `key` if it is expired (a reader found it so).
    pub(crate) fn reap<Q>(&mut self, hash: u64, key: &Q, now: u64, dead: &mut Vec<Dead<K, V>>)
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(index) = self.find(hash, key)
            && self.node(index).is_expired(now, self.tti)
        {
            dead.push(Dead::Node(self.remove_at(index)));
        }
    }

    /// Removes `key` and returns its value if it was live.
    pub(crate) fn take<Q>(
        &mut self,
        hash: u64,
        key: &Q,
        now: u64,
        dead: &mut Vec<Dead<K, V>>,
    ) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.cancel_flight(hash, key, dead);
        let index = self.find(hash, key)?;
        let node = self.remove_at(index);
        if node.is_expired(now, self.tti) {
            dead.push(Dead::Node(node));
            return None;
        }
        let Node { key, value, .. } = node;
        dead.push(Dead::Key(key));
        Some(value)
    }

    /// Stores `value` under `key` with its deadline, replacing any entry
    /// (and abandoning any load in flight: its result would be older).
    pub(crate) fn insert(
        &mut self,
        hash: u64,
        key: K,
        value: V,
        deadline: u64,
        now: u64,
        dead: &mut Vec<Dead<K, V>>,
    ) {
        self.cancel_flight(hash, &key, dead);
        self.store(hash, key, value, deadline, now, dead);
    }

    /// Reads, inserts, replaces or removes `key` as `f` decides, atomically.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute<Q, F>(
        &mut self,
        hash: u64,
        key: &Q,
        now: u64,
        default_deadline: u64,
        f: F,
        dead: &mut Vec<Dead<K, V>>,
    ) -> Computed
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
        F: FnOnce(Option<&V>) -> Compute<V>,
    {
        let mut found = self.find(hash, key);
        if let Some(index) = found
            && self.node(index).is_expired(now, self.tti)
        {
            dead.push(Dead::Node(self.remove_at(index)));
            found = None;
        }
        let op = f(found.map(|index| &self.node(index).value));
        match (op, found) {
            (Compute::Keep, Some(index)) => {
                self.node(index).touch(now, self.tti);
                Computed::Unchanged
            }
            (Compute::Keep, None) => Computed::Unchanged,
            (Compute::Remove, Some(index)) => {
                self.cancel_flight(hash, key, dead);
                dead.push(Dead::Node(self.remove_at(index)));
                Computed::Removed
            }
            (Compute::Remove, None) => Computed::Unchanged,
            (Compute::Update(value), Some(index)) => {
                self.cancel_flight(hash, key, dead);
                self.replace(index, value, None, now, dead);
                Computed::Replaced
            }
            (Compute::Update(value), None) => {
                self.cancel_flight(hash, key, dead);
                self.store(hash, key.to_owned(), value, default_deadline, now, dead);
                Computed::Inserted
            }
            (Compute::Store(value, ttl), Some(index)) => {
                self.cancel_flight(hash, key, dead);
                let deadline = deadline_after(now, ttl);
                self.replace(index, value, Some(deadline), now, dead);
                Computed::Replaced
            }
            (Compute::Store(value, ttl), None) => {
                self.cancel_flight(hash, key, dead);
                let deadline = deadline_after(now, ttl);
                self.store(hash, key.to_owned(), value, deadline, now, dead);
                Computed::Inserted
            }
        }
    }

    /// Removes every entry and flight.
    pub(crate) fn clear(&mut self, dead: &mut Vec<Dead<K, V>>) {
        dead.push(Dead::Slots(std::mem::take(&mut self.slots)));
        dead.push(Dead::Flights(std::mem::take(&mut self.flights)));
        self.table = HashTable::new();
        self.free = NIL;
        self.head = NIL;
        self.tail = NIL;
        self.hand = NIL;
        self.len = 0;
        self.expiring = 0;
        self.cursor = 0;
    }

    /// Looks at up to `budget` slots from where the last sweep stopped and
    /// removes the expired entries among them (one pass at most).
    pub(crate) fn sweep(&mut self, now: u64, budget: usize, dead: &mut Vec<Dead<K, V>>) -> usize {
        if self.expiring == 0 && self.tti == NEVER {
            return 0;
        }
        let slots = self.slots.len();
        let mut removed = 0;
        for _ in 0..budget.min(slots) {
            if self.cursor >= slots {
                self.cursor = 0;
            }
            let index = self.cursor as u32;
            self.cursor += 1;
            if let Slot::Occupied(node) = &self.slots[index as usize]
                && node.is_expired(now, self.tti)
            {
                dead.push(Dead::Node(self.remove_at(index)));
                removed += 1;
            }
        }
        removed
    }

    // ---- the slab and the queue ----

    /// Inserts a key the shard does not hold, or replaces the entry it holds.
    fn store(
        &mut self,
        hash: u64,
        key: K,
        value: V,
        deadline: u64,
        now: u64,
        dead: &mut Vec<Dead<K, V>>,
    ) {
        if deadline <= now {
            // Expired on arrival: nothing to keep, and the old entry is stale.
            if let Some(index) = self.find(hash, &key) {
                dead.push(Dead::Node(self.remove_at(index)));
            }
            dead.push(Dead::Key(key));
            dead.push(Dead::Value(value));
            return;
        }
        if let Some(index) = self.find(hash, &key) {
            dead.push(Dead::Key(key));
            self.replace(index, value, Some(deadline), now, dead);
            return;
        }
        if self.capacity == 0 {
            dead.push(Dead::Key(key));
            dead.push(Dead::Value(value));
            return;
        }
        self.sweep(now, SWEEP_PER_INSERT, dead);
        while self.len >= self.capacity {
            match self.evict(now) {
                Some(node) => dead.push(Dead::Node(node)),
                None => break,
            }
        }
        self.link_new(Node {
            key,
            value,
            hash,
            deadline,
            accessed: AtomicU64::new(now),
            visited: AtomicBool::new(false),
            newer: NIL,
            older: NIL,
        });
    }

    /// Replaces the value of the entry at `index` (and its deadline, if
    /// given). A write counts as a use.
    fn replace(
        &mut self,
        index: u32,
        value: V,
        deadline: Option<u64>,
        now: u64,
        dead: &mut Vec<Dead<K, V>>,
    ) {
        let node = self.node_mut(index);
        let old = std::mem::replace(&mut node.value, value);
        *node.visited.get_mut() = true;
        *node.accessed.get_mut() = now;
        let before = node.deadline;
        if let Some(deadline) = deadline {
            node.deadline = deadline;
            match (before == NEVER, deadline == NEVER) {
                (true, false) => self.expiring += 1,
                (false, true) => self.expiring -= 1,
                _ => {}
            }
        }
        dead.push(Dead::Value(old));
    }

    /// Takes a slot for `node`, links it in at the head and indexes it.
    fn link_new(&mut self, mut node: Node<K, V>) {
        let hash = node.hash;
        if node.deadline != NEVER {
            self.expiring += 1;
        }
        node.older = self.head;
        let index = if self.free != NIL {
            let index = self.free;
            self.free = match &self.slots[index as usize] {
                Slot::Vacant(next) => *next,
                Slot::Occupied(_) => unreachable!("the free list only holds vacant slots"),
            };
            self.slots[index as usize] = Slot::Occupied(node);
            index
        } else {
            self.slots.push(Slot::Occupied(node));
            u32::try_from(self.slots.len() - 1).expect("a shard holds fewer than 2^32 entries")
        };
        if self.head != NIL {
            let head = self.head;
            self.node_mut(head).newer = index;
        } else {
            self.tail = index;
        }
        self.head = index;
        self.len += 1;
        let slots = &self.slots;
        self.table
            .insert_unique(hash, index, |&at| match &slots[at as usize] {
                Slot::Occupied(node) => node.hash,
                Slot::Vacant(_) => 0,
            });
    }

    /// Unindexes, unlinks and frees the entry at `index`.
    fn remove_at(&mut self, index: u32) -> Node<K, V> {
        let hash = self.node(index).hash;
        if let Ok(entry) = self.table.find_entry(hash, |&at| at == index) {
            entry.remove();
        }
        let slot = std::mem::replace(&mut self.slots[index as usize], Slot::Vacant(self.free));
        self.free = index;
        let Slot::Occupied(node) = slot else {
            unreachable!("only occupied slots are removed")
        };
        if node.newer != NIL {
            self.node_mut(node.newer).older = node.older;
        } else {
            self.head = node.older;
        }
        if node.older != NIL {
            self.node_mut(node.older).newer = node.newer;
        } else {
            self.tail = node.newer;
        }
        if self.hand == index {
            self.hand = node.newer;
        }
        if node.deadline != NEVER {
            self.expiring -= 1;
        }
        self.len -= 1;
        node
    }

    /// SIEVE: moves the hand from the tail toward the head, clearing visited
    /// bits, and evicts the first entry that is expired or was not visited
    /// since the hand last passed. The hand stays where it stopped.
    fn evict(&mut self, now: u64) -> Option<Node<K, V>> {
        if self.len == 0 {
            return None;
        }
        let mut at = if self.hand != NIL {
            self.hand
        } else {
            self.tail
        };
        let tti = self.tti;
        loop {
            let node = self.node_mut(at);
            let victim = node.is_expired(now, tti) || !std::mem::take(node.visited.get_mut());
            let newer = node.newer;
            if victim {
                self.hand = newer;
                return Some(self.remove_at(at));
            }
            at = if newer != NIL { newer } else { self.tail };
        }
    }

    // ---- single-flight ----

    /// The channel of the load in flight for `key`, if any.
    pub(crate) fn flight<Q>(&self, hash: u64, key: &Q) -> Option<watch::Receiver<FlightState<V>>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.flights
            .find(hash, |flight| {
                flight.hash == hash && flight.key.borrow() == key
            })
            .map(|flight| flight.rx.clone())
    }

    /// Registers a load for `key` (which has none) and returns its id.
    pub(crate) fn start_flight(
        &mut self,
        hash: u64,
        key: K,
        rx: watch::Receiver<FlightState<V>>,
    ) -> u64 {
        let id = self.next_flight;
        self.next_flight += 1;
        self.flights
            .insert_unique(hash, Flight { hash, id, key, rx }, |flight| flight.hash);
        id
    }

    /// Ends the load `id` and gives back its key, unless the load was
    /// abandoned meanwhile (the key was written or invalidated).
    pub(crate) fn end_flight(&mut self, hash: u64, id: u64) -> Option<K> {
        match self
            .flights
            .find_entry(hash, |flight| flight.hash == hash && flight.id == id)
        {
            Ok(entry) => Some(entry.remove().0.key),
            Err(_) => None,
        }
    }

    /// Abandons the load in flight for `key`: its result is no longer stored.
    fn cancel_flight<Q>(&mut self, hash: u64, key: &Q, dead: &mut Vec<Dead<K, V>>)
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if self.flights.is_empty() {
            return;
        }
        if let Ok(entry) = self.flights.find_entry(hash, |flight| {
            flight.hash == hash && flight.key.borrow() == key
        }) {
            dead.push(Dead::Key(entry.remove().0.key));
        }
    }

    /// For a load about to start: the live value under `key` (marked as
    /// used), removing the entry if it is expired.
    pub(crate) fn get_mut<Q>(
        &mut self,
        hash: u64,
        key: &Q,
        now: u64,
        dead: &mut Vec<Dead<K, V>>,
    ) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let index = self.find(hash, key)?;
        let node = self.node(index);
        if node.is_expired(now, self.tti) {
            dead.push(Dead::Node(self.remove_at(index)));
            return None;
        }
        node.touch(now, self.tti);
        Some(node.value.clone())
    }

    /// Stores the result of a finished load under its key, with the cache's
    /// default lifetime.
    pub(crate) fn finish_flight(
        &mut self,
        hash: u64,
        id: u64,
        value: V,
        deadline: u64,
        now: u64,
        dead: &mut Vec<Dead<K, V>>,
    ) {
        if let Some(key) = self.end_flight(hash, id) {
            self.store(hash, key, value, deadline, now, dead);
        }
    }
}
