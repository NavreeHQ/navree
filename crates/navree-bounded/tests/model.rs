//! Model-based tests: a single-threaded sequence of operations, with time
//! advanced on Tokio's paused clock, against a plain `HashMap` that knows
//! the rules of contents and expiry but has no capacity.
//!
//! Two regimes:
//!
//! - **No eviction** (room for every key): the cache and the model must agree
//!   exactly, on contents, values, `compute` outcomes and `len` after a sweep.
//! - **Eviction** (a few entries, many keys): the cache's contents must be a
//!   subset of the model's, with the latest values; the bound must hold after
//!   every operation; a key that was just written must be present; and an
//!   operation may drop at most one live key, and only by inserting a new one.

use std::collections::HashMap;
use std::time::Duration;

use navree_bounded::{Cache, Compute, Computed};
use proptest::prelude::*;

const KEYS: u8 = 24;

#[derive(Clone, Debug)]
enum Op {
    Insert(u8, u32),
    /// `None`: never expires; `Some(0)`: expired on arrival.
    InsertTtl(u8, u32, Option<u16>),
    Get(u8),
    Remove(u8),
    Add(u8, u32, Option<u16>),
    Incr(u8),
    Advance(u16),
    Clear,
    Sweep,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        5 => (0..KEYS, any::<u32>()).prop_map(|(k, v)| Op::Insert(k, v)),
        3 => (0..KEYS, any::<u32>(), proptest::option::of(0..150u16))
            .prop_map(|(k, v, ttl)| Op::InsertTtl(k, v, ttl)),
        5 => (0..KEYS).prop_map(Op::Get),
        2 => (0..KEYS).prop_map(Op::Remove),
        3 => (0..KEYS, any::<u32>(), proptest::option::of(0..150u16))
            .prop_map(|(k, v, ttl)| Op::Add(k, v, ttl)),
        3 => (0..KEYS).prop_map(Op::Incr),
        4 => (0..120u16).prop_map(Op::Advance),
        1 => Just(Op::Clear),
        2 => Just(Op::Sweep),
    ]
}

#[derive(Clone, Copy, Debug)]
struct Config {
    capacity: usize,
    shards: usize,
    ttl: Option<u16>,
    tti: Option<u16>,
}

fn config(capacity: impl Strategy<Value = usize>, shards: [usize; 3]) -> impl Strategy<Value = Config> {
    (
        capacity,
        proptest::sample::select(shards.to_vec()),
        proptest::option::of(1..200u16),
        proptest::option::of(1..200u16),
    )
        .prop_map(|(capacity, shards, ttl, tti)| Config {
            capacity,
            shards,
            ttl,
            tti,
        })
}

#[derive(Clone, Debug)]
struct Entry {
    value: u32,
    deadline: Option<u64>,
    accessed: u64,
}

/// The rules, with no capacity. Times are milliseconds.
struct Model {
    now: u64,
    ttl: Option<u64>,
    tti: Option<u64>,
    entries: HashMap<u8, Entry>,
}

impl Model {
    fn expired(&self, entry: &Entry) -> bool {
        entry.deadline.is_some_and(|d| self.now >= d)
            || self.tti.is_some_and(|t| self.now >= entry.accessed + t)
    }

    fn live(&self, key: u8) -> Option<&Entry> {
        self.entries.get(&key).filter(|e| !self.expired(e))
    }

    /// A read: returns the value and counts as a use.
    fn get(&mut self, key: u8) -> Option<u32> {
        if self.live(key).is_none() {
            self.entries.remove(&key);
            return None;
        }
        let now = self.now;
        let entry = self.entries.get_mut(&key)?;
        entry.accessed = now;
        Some(entry.value)
    }

    fn store(&mut self, key: u8, value: u32, ttl: Option<u64>) {
        match ttl {
            Some(0) => {
                self.entries.remove(&key);
            }
            ttl => {
                let deadline = ttl.map(|t| self.now + t);
                let accessed = self.now;
                self.entries.insert(
                    key,
                    Entry {
                        value,
                        deadline,
                        accessed,
                    },
                );
            }
        }
    }
}

/// Whether the operation inserts a key the cache may not hold (and so may
/// evict one to make room).
fn may_insert(op: &Op) -> bool {
    matches!(
        op,
        Op::Insert(..) | Op::InsertTtl(..) | Op::Add(..) | Op::Incr(..)
    )
}

fn run(config: Config, evicting: bool, ops: Vec<Op>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("a runtime");
    runtime.block_on(async move {
        let mut builder = Cache::builder(config.capacity).shards(config.shards);
        if let Some(ms) = config.ttl {
            builder = builder.time_to_live(Duration::from_millis(u64::from(ms)));
        }
        if let Some(ms) = config.tti {
            builder = builder.time_to_idle(Duration::from_millis(u64::from(ms)));
        }
        let cache: Cache<u8, u32> = builder.build();
        let mut model = Model {
            now: 0,
            ttl: config.ttl.map(u64::from),
            tti: config.tti.map(u64::from),
            entries: HashMap::new(),
        };
        assert!(cache.capacity() == config.capacity);

        for op in ops {
            // Apply the operation to both and compare what it returns.
            let mut inserted = None;
            match &op {
                Op::Insert(key, value) => {
                    cache.insert(*key, *value);
                    model.store(*key, *value, model.ttl);
                    inserted = Some(*key);
                }
                Op::InsertTtl(key, value, ttl) => {
                    cache.insert_with_ttl(
                        *key,
                        *value,
                        ttl.map(|ms| Duration::from_millis(u64::from(ms))),
                    );
                    model.store(*key, *value, ttl.map(u64::from).or(None));
                    // `None` is "never", not "the default".
                    if ttl.is_none() {
                        model.entries.entry(*key).and_modify(|e| e.deadline = None);
                    }
                    if ttl != &Some(0) {
                        inserted = Some(*key);
                    }
                }
                Op::Get(key) => {
                    let expected = model.get(*key);
                    let actual = cache.get(key);
                    if evicting {
                        if actual.is_some() {
                            assert_eq!(actual, expected, "get({key})");
                        }
                    } else {
                        assert_eq!(actual, expected, "get({key})");
                    }
                }
                Op::Remove(key) => {
                    let expected = model.live(*key).map(|e| e.value);
                    model.entries.remove(key);
                    let actual = cache.remove(key);
                    if evicting {
                        if actual.is_some() {
                            assert_eq!(actual, expected, "remove({key})");
                        }
                    } else {
                        assert_eq!(actual, expected, "remove({key})");
                    }
                }
                Op::Add(key, value, ttl) => {
                    let was_live = model.live(*key).is_some();
                    let outcome = cache.compute(key, |existing| match existing {
                        Some(_) => Compute::Keep,
                        None => Compute::Store(
                            *value,
                            ttl.map(|ms| Duration::from_millis(u64::from(ms))),
                        ),
                    });
                    if !evicting {
                        let expected = if was_live {
                            Computed::Unchanged
                        } else {
                            Computed::Inserted
                        };
                        assert_eq!(outcome, expected, "add({key})");
                    }
                    if outcome == Computed::Inserted && !was_live {
                        model.store(*key, *value, ttl.map(u64::from));
                        if ttl.is_none() {
                            model.entries.entry(*key).and_modify(|e| e.deadline = None);
                        }
                        if ttl != &Some(0) {
                            inserted = Some(*key);
                        }
                    } else if was_live {
                        // `Keep` still counts as a use.
                        let now = model.now;
                        if let Some(entry) = model.entries.get_mut(key) {
                            entry.accessed = now;
                        }
                    }
                }
                Op::Incr(key) => {
                    let existing = model.live(*key).map(|e| e.value);
                    let outcome = cache.compute(key, |current| {
                        Compute::Update(current.map_or(1, |n| n.wrapping_add(1)))
                    });
                    match existing {
                        Some(value) => {
                            assert_eq!(outcome, Computed::Replaced, "incr({key})");
                            let now = model.now;
                            let entry = model.entries.get_mut(key).expect("live in the model");
                            entry.value = value.wrapping_add(1);
                            entry.accessed = now;
                        }
                        None => {
                            // A missing key is inserted with the default TTL.
                            assert_eq!(outcome, Computed::Inserted, "incr({key})");
                            model.store(*key, 1, model.ttl);
                            inserted = Some(*key);
                        }
                    }
                }
                Op::Advance(ms) => {
                    tokio::time::advance(Duration::from_millis(u64::from(*ms))).await;
                    model.now += u64::from(*ms);
                }
                Op::Clear => {
                    cache.invalidate_all();
                    model.entries.clear();
                    assert!(cache.is_empty());
                }
                Op::Sweep => {
                    cache.sweep(usize::MAX);
                    let resident = (0..KEYS).filter(|k| cache.contains_key(k)).count();
                    assert_eq!(cache.len(), resident, "after a full sweep only live entries remain");
                }
            }

            // The bound holds after every operation.
            assert!(cache.len() <= config.capacity, "len {} over capacity", cache.len());

            // Compare contents (without touching anything).
            let mut dropped = 0;
            for key in 0..KEYS {
                let in_cache = cache.contains_key(&key);
                let in_model = model.live(key).is_some();
                if evicting {
                    if in_cache {
                        assert!(in_model, "{key} is cached but the model has it expired or absent");
                    } else if in_model {
                        dropped += 1;
                        model.entries.remove(&key);
                    }
                } else {
                    assert_eq!(in_cache, in_model, "contents differ at key {key} after {op:?}");
                }
            }
            if evicting {
                assert!(
                    dropped == 0 || may_insert(&op),
                    "{op:?} dropped {dropped} live keys without inserting"
                );
                assert!(dropped <= 1, "{op:?} dropped {dropped} live keys");
                if let Some(key) = inserted {
                    assert!(cache.contains_key(&key), "the key just written is gone");
                }
            }
            // Compare values (reads are uses on both sides).
            for key in 0..KEYS {
                if model.live(key).is_some() {
                    assert_eq!(cache.get(&key), model.get(key), "value of {key} after {op:?}");
                }
            }
        }
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Room for every key, in one shard or several: exact agreement.
    #[test]
    fn matches_the_model_when_nothing_is_evicted(
        config in config(Just(512usize), [1, 2, 4]),
        ops in proptest::collection::vec(op(), 1..150),
    ) {
        run(config, false, ops);
    }

    /// A few entries for many keys: the bound, and what may be dropped.
    #[test]
    fn stays_within_the_bound_and_inside_the_model_when_evicting(
        config in config(2..9usize, [1, 2, 2]),
        ops in proptest::collection::vec(op(), 1..150),
    ) {
        run(config, true, ops);
    }
}
