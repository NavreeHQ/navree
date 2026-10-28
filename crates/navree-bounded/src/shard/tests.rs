//! Tests of a single shard against random operations, with the structure's
//! invariants checked after every one.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};

use proptest::prelude::*;

use super::*;

impl<K: Hash + Eq, V: Clone> Shard<K, V> {
    /// Panics unless the slab, the queue, the free list and the table agree
    /// with each other.
    fn check(&self) {
        let mut seen = 0;
        let (mut at, mut newer) = (self.head, NIL);
        while at != NIL {
            let node = self.node(at);
            assert_eq!(node.newer, newer, "newer link of slot {at}");
            assert!(
                self.table.find(node.hash, |&index| index == at).is_some(),
                "slot {at} is not in the table"
            );
            newer = at;
            at = node.older;
            seen += 1;
            assert!(seen <= self.slots.len(), "the queue loops");
        }
        assert_eq!(newer, self.tail, "the tail is the last of the queue");
        assert_eq!(seen, self.len, "queue length");
        assert_eq!(self.table.len(), self.len, "table length");
        let occupied = self
            .slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Occupied(_)))
            .count();
        assert_eq!(occupied, self.len, "occupied slots");
        let (mut free, mut at) = (0, self.free);
        while at != NIL {
            match &self.slots[at as usize] {
                Slot::Vacant(next) => at = *next,
                Slot::Occupied(_) => panic!("occupied slot {at} on the free list"),
            }
            free += 1;
            assert!(free <= self.slots.len(), "the free list loops");
        }
        assert_eq!(free + occupied, self.slots.len(), "free plus occupied");
        let expiring = self
            .slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Occupied(n) if n.deadline != NEVER))
            .count();
        assert_eq!(expiring, self.expiring, "expiring count");
        assert!(self.len <= self.capacity, "the bound");
        if self.hand != NIL {
            assert!(matches!(self.slots[self.hand as usize], Slot::Occupied(_)));
        }
        assert_eq!(self.len == 0, self.head == NIL && self.tail == NIL);
    }
}

#[derive(Clone, Debug)]
enum Op {
    Insert(u8, u32, Option<u8>),
    Get(u8),
    Take(u8),
    Add(u8, u32),
    Incr(u8),
    Advance(u8),
    Sweep(u8),
    Clear,
    Flight(u8),
    EndFlight(u8, u32),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..24u8, any::<u32>(), proptest::option::of(1..40u8))
            .prop_map(|(k, v, ttl)| Op::Insert(k, v, ttl)),
        4 => (0..24u8).prop_map(Op::Get),
        2 => (0..24u8).prop_map(Op::Take),
        2 => (0..24u8, any::<u32>()).prop_map(|(k, v)| Op::Add(k, v)),
        2 => (0..24u8).prop_map(Op::Incr),
        2 => (1..30u8).prop_map(Op::Advance),
        1 => (0..12u8).prop_map(Op::Sweep),
        1 => Just(Op::Clear),
        1 => (0..24u8).prop_map(Op::Flight),
        1 => (0..24u8, any::<u32>()).prop_map(|(k, v)| Op::EndFlight(k, v)),
    ]
}

/// Random operations at a synthetic time, the shard's invariants checked
/// after every one, and every value read back equal to the last one written
/// to its key.
fn run(capacity: usize, tti: Option<u8>, ops: Vec<Op>) {
    let hasher = RandomState::new();
    let hash = |key: &u8| hasher.hash_one(key);
    let tti = tti.map_or(NEVER, u64::from);
    let mut shard: Shard<u8, u32> = Shard::new(capacity, tti);
    let mut written: HashMap<u8, u32> = HashMap::new();
    let mut flights: HashMap<u8, u64> = HashMap::new();
    let mut now = 1;
    let mut dead = Vec::new();
    for op in ops {
        match op {
            Op::Insert(key, value, ttl) => {
                let deadline = ttl.map_or(NEVER, |ttl| now + u64::from(ttl));
                shard.insert(hash(&key), key, value, deadline, now, &mut dead);
                written.insert(key, value);
                flights.remove(&key);
            }
            Op::Get(key) => {
                if let Some(value) = shard.get_mut(hash(&key), &key, now, &mut dead) {
                    assert_eq!(Some(&value), written.get(&key), "stale value for {key}");
                }
            }
            Op::Take(key) => {
                if let Some(value) = shard.take(hash(&key), &key, now, &mut dead) {
                    assert_eq!(Some(&value), written.get(&key), "stale value for {key}");
                }
                written.remove(&key);
                flights.remove(&key);
            }
            Op::Add(key, value) => {
                let outcome = shard.compute(
                    hash(&key),
                    &key,
                    now,
                    NEVER,
                    |existing| match existing {
                        Some(_) => Compute::Keep,
                        None => Compute::Store(value, None),
                    },
                    &mut dead,
                );
                if outcome == Computed::Inserted {
                    written.insert(key, value);
                    flights.remove(&key);
                }
            }
            Op::Incr(key) => {
                let mut next = None;
                let outcome = shard.compute(
                    hash(&key),
                    &key,
                    now,
                    NEVER,
                    |existing| {
                        let value = existing.map_or(1, |n| n.wrapping_add(1));
                        next = Some(value);
                        Compute::Update(value)
                    },
                    &mut dead,
                );
                assert!(matches!(outcome, Computed::Inserted | Computed::Replaced));
                written.insert(key, next.expect("the closure ran"));
                flights.remove(&key);
            }
            Op::Advance(by) => now += u64::from(by),
            Op::Sweep(budget) => {
                shard.sweep(now, usize::from(budget), &mut dead);
            }
            Op::Clear => {
                shard.clear(&mut dead);
                written.clear();
                flights.clear();
            }
            Op::Flight(key) => {
                if shard.flight(hash(&key), &key).is_none() {
                    let (_tx, rx) = watch::channel(None);
                    let id = shard.start_flight(hash(&key), key, rx);
                    flights.insert(key, id);
                }
            }
            Op::EndFlight(key, value) => {
                if let Some(id) = flights.remove(&key) {
                    shard.finish_flight(hash(&key), id, value, NEVER, now, &mut dead);
                    if shard.get_mut(hash(&key), &key, now, &mut dead) == Some(value) {
                        written.insert(key, value);
                    }
                }
            }
        }
        dead.clear();
        shard.check();
        assert_eq!(shard.flights.len(), flights.len(), "flights");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn invariants_hold_under_random_operations(
        capacity in 0..10usize,
        tti in proptest::option::of(5..60u8),
        ops in proptest::collection::vec(op(), 1..120),
    ) {
        run(capacity, tti, ops);
    }
}

#[test]
fn a_zero_capacity_shard_stores_nothing() {
    let mut shard: Shard<u8, u8> = Shard::new(0, NEVER);
    let mut dead = Vec::new();
    shard.insert(1, 1, 1, NEVER, 0, &mut dead);
    assert_eq!(shard.len(), 0);
    shard.check();
}
