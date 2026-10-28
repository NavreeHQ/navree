//! # navree-bounded
//!
//! The bounded in-process cache behind Navree's framework caches: the
//! ability cache of navree-access, the memory store of navree-cache, the
//! domain and token lookups of navree-domains and navree-oauth, and the
//! metric results of navree-helm. It is also exported as `navree::bounded`
//! for an app's own caches.
//!
//! ```
//! use std::time::Duration;
//! use navree_bounded::Cache;
//!
//! # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(async {
//! let cache: Cache<String, u64> = Cache::builder(10_000)
//!     .time_to_live(Duration::from_secs(60))
//!     .build();
//!
//! cache.insert("answer".to_owned(), 42);
//! assert_eq!(cache.get("answer"), Some(42));
//!
//! // Concurrent misses on one key share one load; errors are not cached.
//! let loaded = cache
//!     .try_get_with("fresh".to_owned(), async { Ok::<_, std::io::Error>(7) })
//!     .await
//!     .unwrap();
//! assert_eq!(loaded, 7);
//! # });
//! ```
//!
//! ## What it guarantees
//!
//! - **A strict bound.** [`Cache::capacity`] is split over the shards, and each
//!   shard enforces its share before it links in a new entry, under its own
//!   lock. The sum can therefore never exceed the capacity, not even for an
//!   instant, however many threads write.
//! - **Sharded locks, no lock across an `await`.** Keys hash (with the
//!   standard library's randomly seeded SipHash, so a client cannot choose
//!   colliding keys) to a shard, and each shard sits behind its own
//!   `RwLock`. Reads share a lock; writes take it exclusively for a few
//!   steps. Nothing in a shard waits on anything but its lock.
//! - **O(1) operations.** A shard is a hash table of slab indices plus an
//!   intrusive doubly linked queue of the slab's entries. Lookups, inserts,
//!   removals and the queue's relinking cost a constant number of steps.
//!   Eviction is amortized O(1) (see below).
//! - **No unsafe code in this crate** (`#![forbid(unsafe_code)]`). The table
//!   is `hashbrown`'s `HashTable`, the implementation under the standard
//!   library's `HashMap`; used raw, it holds slab indices, so each key is
//!   stored once, in the slab.
//! - **Entries are dropped outside the lock.** Whatever a write removes or
//!   replaces is dropped after the shard's lock is released, so a slow
//!   `Drop` never holds up other callers.
//!
//! ## Eviction: SIEVE
//!
//! When a shard is full and a new key arrives, it evicts with
//! [SIEVE](https://www.usenix.org/conference/nsdi24/presentation/zhang-yazhuo)
//! (Zhang et al., NSDI 2024):
//!
//! - New entries are linked in at the head of the queue, unvisited.
//! - A read only sets the entry's *visited* flag. It does not move the entry.
//! - The *hand* walks from the tail toward the head, clearing visited flags
//!   as it passes, and evicts the first entry that is unvisited (or
//!   expired, which is evicted whatever its flag). It stays where it
//!   stopped for the next eviction.
//!
//! Why this and not LRU or a frequency sketch:
//!
//! - **Reads stay shared.** LRU must move an entry to the front on every
//!   hit, which is a write under an exclusive lock; the hot path of a
//!   request-path cache would serialize on it. SIEVE's hit writes one
//!   atomic flag (and only if it is not already set), so hits run in
//!   parallel under the shard's read lock.
//! - **It resists scans.** A burst of keys that are each used once (a crawl,
//!   a scan, a one-off report) enters at the head and is evicted by the hand
//!   before it reaches the entries that were read again. LRU would let the
//!   burst push the hot set out.
//! - **It is small enough to verify.** The policy is thirty lines; the
//!   tests compare it, entry for entry, with a naive reference
//!   implementation.
//! - **Amortized, not worst-case, O(1).** One eviction can pass many visited
//!   entries, but each pass clears a flag that a hit set, so the total work
//!   is bounded by the number of hits.
//!
//! Capacity is divided evenly over the shards, so a cache that is exactly
//! full of keys that hash unevenly can evict a little early: the default
//! shard count keeps at least 256 entries in a shard to keep that skew
//! small, and [`CacheBuilder::shards`] overrides it.
//!
//! ## Expiry
//!
//! - **TTL** ([`CacheBuilder::time_to_live`], or per entry with
//!   [`Cache::insert_with_ttl`]): an entry is gone `ttl` after it was
//!   inserted or replaced.
//! - **TTI** ([`CacheBuilder::time_to_idle`]): an entry is gone after that
//!   long without a read or a write. With both, the sooner one wins.
//! - **Checked on read.** A read that finds an entry past its deadline
//!   treats it as absent and removes it. Nothing expired is ever returned.
//! - **A bounded sweep.** Every insertion of a new key looks at four slots of
//!   its shard, from where the last sweep stopped, and removes the expired
//!   entries among them; the eviction hand takes an expired entry before
//!   any live one; and [`Cache::sweep`] does the same for a given number of
//!   slots per shard, for an app that wants its memory back during a quiet
//!   spell. There is no background thread.
//! - **The clock is Tokio's**, so `tokio::time::pause` and `advance` drive
//!   expiry in tests. A cache reads the clock only when something can
//!   expire (an entry with a deadline, an idle limit, or any write).
//!
//! ## Single-flight loading
//!
//! [`Cache::get_with`] and [`Cache::try_get_with`] run an `async` load on a
//! miss. Callers that miss on one key at the same time share one load: the
//! first runs it, the others wait for its value. A failed load is not
//! cached; every caller that shared it gets the same `Arc` of its error, and
//! the next call loads again. If the caller running the load is cancelled or
//! the load panics, the key is untouched and one of the waiting callers
//! runs its own load: nothing is poisoned and nothing waits forever.
//! A write or invalidation of the key while it loads means the load's
//! result is delivered to its callers but not stored.
//!
//! ## What is deliberately missing
//!
//! No iteration, no eviction listener and no weights: none of the framework's
//! caches uses them, and each would add a way to run user code or observe
//! the table while a lock is held. Values are cloned out ([`Clone`]), so
//! store an `Arc` for anything large.
//!
//! A panic under a shard's lock (a key's `Hash` or `Eq`, a value's `Clone`, or
//! a [`Cache::compute`] closure) poisons the lock; the next caller empties
//! that shard and carries on.
#![forbid(unsafe_code)]

mod cache;
mod clock;
mod shard;

pub use cache::{Cache, CacheBuilder, Compute, Computed};
