//! The eviction policy: one shard compared, entry for entry, with a naive
//! SIEVE written the way the paper describes it (a vector, linear scans),
//! plus the properties the docs promise (scan resistance, the bound).

use navree_bounded::{Cache, Compute};
use proptest::prelude::*;

/// SIEVE on a `Vec`: index 0 is the tail (oldest), the last is the head.
struct Reference {
    capacity: usize,
    /// (key, value, visited)
    queue: Vec<(u8, u32, bool)>,
    /// The hand's position; `None` starts at the tail.
    hand: Option<usize>,
}

impl Reference {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            queue: Vec::new(),
            hand: None,
        }
    }

    fn position(&self, key: u8) -> Option<usize> {
        self.queue.iter().position(|(k, ..)| *k == key)
    }

    fn get(&mut self, key: u8) -> Option<u32> {
        let at = self.position(key)?;
        self.queue[at].2 = true;
        Some(self.queue[at].1)
    }

    fn insert(&mut self, key: u8, value: u32) {
        if let Some(at) = self.position(key) {
            self.queue[at].1 = value;
            self.queue[at].2 = true;
            return;
        }
        if self.capacity == 0 {
            return;
        }
        while self.queue.len() >= self.capacity {
            self.evict();
        }
        self.queue.push((key, value, false));
    }

    /// The paper's eviction: the hand walks from where it stopped (or the
    /// tail) toward the head, wrapping, clearing visited bits; the first
    /// unvisited entry goes and the hand moves to the next newer one.
    fn evict(&mut self) {
        let mut at = self.hand.unwrap_or(0);
        while self.queue[at].2 {
            self.queue[at].2 = false;
            at = if at + 1 < self.queue.len() { at + 1 } else { 0 };
        }
        self.queue.remove(at);
        self.hand = (at < self.queue.len()).then_some(at);
    }

    /// Removes the entry at `at`; a hand on it moves to the next newer one.
    fn remove_at(&mut self, at: usize) {
        self.queue.remove(at);
        self.hand = match self.hand {
            Some(hand) if hand == at => (at < self.queue.len()).then_some(at),
            Some(hand) if hand > at => Some(hand - 1),
            other => other,
        };
    }

    fn remove(&mut self, key: u8) -> Option<u32> {
        let at = self.position(key)?;
        let value = self.queue[at].1;
        self.remove_at(at);
        Some(value)
    }

    fn contains(&self, key: u8) -> bool {
        self.position(key).is_some()
    }
}

#[derive(Clone, Debug)]
enum Op {
    Insert(u8, u32),
    Get(u8),
    Remove(u8),
    Incr(u8),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        5 => (0..16u8, any::<u32>()).prop_map(|(k, v)| Op::Insert(k, v)),
        6 => (0..16u8).prop_map(Op::Get),
        1 => (0..16u8).prop_map(Op::Remove),
        2 => (0..16u8).prop_map(Op::Incr),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// With one shard the cache evicts exactly what the reference evicts.
    #[test]
    fn one_shard_evicts_exactly_what_sieve_evicts(
        capacity in 1..9usize,
        ops in proptest::collection::vec(op(), 1..200),
    ) {
        let cache: Cache<u8, u32> = Cache::builder(capacity).shards(1).build();
        let mut reference = Reference::new(capacity);
        for op in ops {
            match op {
                Op::Insert(key, value) => {
                    cache.insert(key, value);
                    reference.insert(key, value);
                }
                Op::Get(key) => {
                    prop_assert_eq!(cache.get(&key), reference.get(key), "get({})", key);
                }
                Op::Remove(key) => {
                    prop_assert_eq!(cache.remove(&key), reference.remove(key), "remove({})", key);
                }
                Op::Incr(key) => {
                    cache.compute(&key, |n| Compute::Update(n.map_or(1, |n| n.wrapping_add(1))));
                    let next = reference.queue.iter().find(|(k, ..)| *k == key).map_or(1, |e| e.1.wrapping_add(1));
                    reference.insert(key, next);
                }
            }
            prop_assert_eq!(cache.len(), reference.queue.len());
            for key in 0..16u8 {
                prop_assert_eq!(
                    cache.contains_key(&key),
                    reference.contains(key),
                    "contents differ at {} after {:?}", key, op
                );
            }
        }
    }
}

#[test]
fn the_reference_evicts_what_the_paper_says() {
    // The worked example: a, b, c, d (cap 4); read a and b; insert e. The
    // hand starts at the tail: a and b are visited (cleared, passed), c is
    // not: c goes.
    let mut reference = Reference::new(4);
    for key in 0..4 {
        reference.insert(key, 0);
    }
    reference.get(0);
    reference.get(1);
    reference.insert(4, 0);
    assert!(!reference.contains(2));
    assert!([0, 1, 3, 4].iter().all(|k| reference.contains(*k)));
    // The hand stayed at d: the next eviction takes d (unvisited), not a.
    reference.insert(5, 0);
    assert!(!reference.contains(3));
    assert!(reference.contains(0));
}

#[test]
fn the_cache_follows_the_same_worked_example() {
    let cache: Cache<u8, u8> = Cache::builder(4).shards(1).build();
    for key in 0..4 {
        cache.insert(key, 0);
    }
    cache.get(&0);
    cache.get(&1);
    cache.insert(4, 0);
    assert!(!cache.contains_key(&2));
    cache.insert(5, 0);
    assert!(!cache.contains_key(&3));
    assert!(cache.contains_key(&0) && cache.contains_key(&1));
}

/// Entries that keep being read survive a long scan of keys read once; the
/// scan's own keys are the ones that go.
#[test]
fn a_scan_does_not_push_out_entries_that_are_read() {
    let cache: Cache<u32, u32> = Cache::builder(100).shards(1).build();
    let hot: Vec<u32> = (0..40).collect();
    for key in &hot {
        cache.insert(*key, *key);
    }
    for scan in 1_000..6_000u32 {
        cache.insert(scan, scan);
        // The application reads its hot keys between scan inserts.
        let key = hot[(scan as usize) % hot.len()];
        assert_eq!(
            cache.get(&key),
            Some(key),
            "hot key {key} lost at scan {scan}"
        );
    }
    for key in &hot {
        assert!(cache.contains_key(key), "hot key {key} lost");
    }
    assert_eq!(cache.len(), 100);
}

/// A key written once and never read leaves before a key that was read.
#[test]
fn unread_entries_go_before_read_ones() {
    let cache: Cache<u8, u8> = Cache::builder(3).shards(1).build();
    cache.insert(1, 1);
    cache.insert(2, 2);
    cache.insert(3, 3);
    cache.get(&1);
    cache.insert(4, 4);
    assert!(cache.contains_key(&1), "the read key stays");
    assert!(!cache.contains_key(&2), "the oldest unread key goes");
    assert!(cache.contains_key(&3) && cache.contains_key(&4));
}

#[test]
fn replacing_a_value_counts_as_a_use() {
    let cache: Cache<u8, u8> = Cache::builder(2).shards(1).build();
    cache.insert(1, 1);
    cache.insert(2, 2);
    cache.insert(1, 10);
    cache.insert(3, 3);
    assert_eq!(cache.get(&1), Some(10));
    assert!(!cache.contains_key(&2));
}

#[test]
fn the_capacity_is_split_over_shards_without_losing_any() {
    for capacity in [0usize, 1, 2, 3, 7, 8, 100, 1_000, 10_000] {
        for shards in [1usize, 2, 3, 4, 8, 64] {
            let cache: Cache<u32, u32> = Cache::builder(capacity).shards(shards).build();
            assert_eq!(cache.capacity(), capacity);
            assert!(cache.shard_count().is_power_of_two());
            assert!(cache.shard_count() <= shards.max(1));
            assert!(cache.shard_count() <= capacity.max(1));
            for key in 0..(capacity as u32 * 4 + 16) {
                cache.insert(key, key);
                assert!(
                    cache.len() <= capacity,
                    "len {} over {capacity}",
                    cache.len()
                );
            }
        }
    }
}

#[test]
fn a_cache_of_zero_entries_stores_nothing_but_loads() {
    let cache: Cache<u8, u8> = Cache::new(0);
    cache.insert(1, 1);
    assert_eq!(cache.get(&1), None);
    assert!(cache.is_empty());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let value = runtime.block_on(cache.get_with(1, async { 5 }));
    assert_eq!(value, 5);
    assert!(cache.is_empty());
}

#[test]
fn the_default_shard_count_keeps_shards_roomy() {
    let small: Cache<u8, u8> = Cache::new(100);
    assert_eq!(small.shard_count(), 1, "a small cache is one shard");
    let large: Cache<u8, u8> = Cache::new(1_000_000);
    assert!(large.shard_count() >= 1);
    assert!(large.capacity() / large.shard_count() >= 256);
}

#[test]
fn remove_and_invalidate_return_and_forget() {
    let cache: Cache<String, u32> = Cache::new(10);
    cache.insert("a".into(), 1);
    assert_eq!(cache.remove("a"), Some(1));
    assert_eq!(cache.remove("a"), None);
    cache.insert("b".into(), 2);
    cache.invalidate("b");
    assert_eq!(cache.get("b"), None);
    cache.insert("c".into(), 3);
    cache.invalidate_all();
    assert!(cache.is_empty());
}

#[test]
fn compute_decides_each_outcome() {
    use navree_bounded::Computed;
    let cache: Cache<String, u32> = Cache::new(10);
    assert_eq!(cache.compute("a", |_| Compute::Keep), Computed::Unchanged);
    assert_eq!(cache.compute("a", |_| Compute::Remove), Computed::Unchanged);
    assert_eq!(
        cache.compute("a", |current| {
            assert!(current.is_none());
            Compute::Store(1, None)
        }),
        Computed::Inserted
    );
    assert_eq!(
        cache.compute("a", |current| {
            assert_eq!(current, Some(&1));
            Compute::Update(2)
        }),
        Computed::Replaced
    );
    assert_eq!(cache.get("a"), Some(2));
    assert_eq!(cache.compute("a", |_| Compute::Keep), Computed::Unchanged);
    assert_eq!(cache.compute("a", |_| Compute::Remove), Computed::Removed);
    assert_eq!(cache.get("a"), None);
}

#[test]
fn a_cache_is_shared_by_its_clones_and_debug_prints() {
    let cache: Cache<u8, u8> = Cache::new(10);
    let clone = cache.clone();
    clone.insert(1, 1);
    assert_eq!(cache.get(&1), Some(1));
    let printed = format!("{cache:?}");
    assert!(
        printed.contains("len: 1") && printed.contains("capacity: 10"),
        "{printed}"
    );
}
