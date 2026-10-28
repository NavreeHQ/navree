//! A timed mixed read/write benchmark on 8 threads, for spotting a slowdown
//! after a change to the cache:
//!
//! ```text
//! cargo run --release -p navree-bounded --example throughput
//! ```
//!
//! Each workload warms the cache, then runs for a second. Reads that miss
//! insert, like a read-through cache. The numbers are for comparing one
//! build with another on one machine, not across machines.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

use navree_bounded::Cache;

const THREADS: usize = 8;
const RUN: Duration = Duration::from_secs(1);

struct Workload {
    name: &'static str,
    keys: usize,
    capacity: usize,
    read_percent: u64,
    /// Whether 80% of the accesses go to 20% of the keys.
    skewed: bool,
    ttl: Option<Duration>,
}

/// xorshift64: enough randomness to scatter keys.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

fn run(workload: &Workload) -> f64 {
    let keys: Arc<Vec<String>> = Arc::new(
        (0..workload.keys)
            .map(|i| format!("tenant-{i:07}.customers.example.com"))
            .collect(),
    );
    let values: Arc<Vec<Arc<[u8; 64]>>> =
        Arc::new((0..workload.keys).map(|_| Arc::new([7u8; 64])).collect());
    let mut builder = Cache::builder(workload.capacity);
    if let Some(ttl) = workload.ttl {
        builder = builder.time_to_live(ttl);
    }
    let cache: Cache<String, Arc<[u8; 64]>> = builder.build();
    for i in 0..workload.capacity.min(workload.keys) {
        cache.insert(keys[i].clone(), values[i].clone());
    }

    let stop = AtomicBool::new(false);
    let operations: u64 = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..THREADS)
            .map(|thread| {
                let (cache, keys, values, stop) = (&cache, &keys, &values, &stop);
                scope.spawn(move || {
                    let mut rng = Rng((thread as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
                    let mut operations = 0u64;
                    while !stop.load(Relaxed) {
                        for _ in 0..512 {
                            let r = rng.next();
                            let i = if workload.skewed && r % 100 < 80 {
                                (r >> 8) as usize % (workload.keys / 5).max(1)
                            } else {
                                (r >> 8) as usize % workload.keys
                            };
                            if rng.next() % 100 < workload.read_percent {
                                if cache.get(&keys[i]).is_none() {
                                    cache.insert(keys[i].clone(), values[i].clone());
                                }
                            } else {
                                cache.insert(keys[i].clone(), values[i].clone());
                            }
                            operations += 1;
                        }
                    }
                    operations
                })
            })
            .collect();
        std::thread::sleep(RUN);
        stop.store(true, Relaxed);
        workers
            .into_iter()
            .map(|worker| worker.join().expect("a worker panicked"))
            .sum()
    });
    operations as f64 / RUN.as_secs_f64()
}

fn main() {
    let workloads = [
        Workload {
            name: "read-heavy 95/5, everything fits, TTL 60s",
            keys: 10_000,
            capacity: 10_000,
            read_percent: 95,
            skewed: false,
            ttl: Some(Duration::from_secs(60)),
        },
        Workload {
            name: "read-heavy 99/1, skewed, 200k keys in 100k, TTL 60s",
            keys: 200_000,
            capacity: 100_000,
            read_percent: 99,
            skewed: true,
            ttl: Some(Duration::from_secs(60)),
        },
        Workload {
            name: "mixed 80/20, skewed, 20k keys in 10k, TTL 60s",
            keys: 20_000,
            capacity: 10_000,
            read_percent: 80,
            skewed: true,
            ttl: Some(Duration::from_secs(60)),
        },
        Workload {
            name: "write-heavy 50/50, uniform, 100k keys in 10k",
            keys: 100_000,
            capacity: 10_000,
            read_percent: 50,
            skewed: false,
            ttl: None,
        },
    ];
    println!("{THREADS} threads, {RUN:?} per workload");
    for workload in &workloads {
        println!("{:<56} {:>8.2} Mops/s", workload.name, run(workload) / 1e6);
    }
}
