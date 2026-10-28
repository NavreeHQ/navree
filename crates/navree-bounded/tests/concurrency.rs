//! Many tasks on a multi-threaded runtime: the bound always holds, no write
//! is lost, computes are atomic, a wave of callers on one key runs one load,
//! and cancelled or panicking loaders never wedge a key.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::SeqCst};
use std::time::{Duration, Instant};

use navree_bounded::{Cache, Compute};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};

/// xorshift64: enough randomness to scatter keys, no dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn below(&mut self, n: u64) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x % n
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn the_bound_holds_while_many_tasks_write() {
    const CAPACITY: usize = 200;
    let cache: Cache<u64, u64> = Cache::builder(CAPACITY).shards(8).build();
    let highest = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    // A watcher that does nothing but read the size.
    let watcher = {
        let (cache, highest, stop) = (cache.clone(), highest.clone(), stop.clone());
        tokio::spawn(async move {
            while !stop.load(SeqCst) {
                highest.fetch_max(cache.len(), SeqCst);
                tokio::task::yield_now().await;
            }
        })
    };

    let mut writers = JoinSet::new();
    for task in 0..16u64 {
        let (cache, highest) = (cache.clone(), highest.clone());
        writers.spawn(async move {
            let mut rng = Rng::new(task + 1);
            for i in 0..20_000u64 {
                let key = rng.below(2_000);
                match rng.below(10) {
                    0..=5 => cache.insert(key, key * 2),
                    6..=8 => {
                        if let Some(value) = cache.get(&key) {
                            assert_eq!(value, key * 2, "a torn or foreign value for {key}");
                        }
                    }
                    _ => {
                        cache.remove(&key);
                    }
                }
                highest.fetch_max(cache.len(), SeqCst);
                if i.is_multiple_of(256) {
                    tokio::task::yield_now().await;
                }
            }
        });
    }
    while let Some(done) = writers.join_next().await {
        done.expect("a writer panicked");
    }
    stop.store(true, SeqCst);
    watcher.await.expect("the watcher panicked");

    assert!(
        highest.load(SeqCst) <= CAPACITY,
        "the cache reached {} entries, over its capacity of {CAPACITY}",
        highest.load(SeqCst)
    );
    assert!(cache.len() <= CAPACITY);
    assert!(
        cache.len() > CAPACITY / 2,
        "the cache stays mostly full: {}",
        cache.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn no_insert_is_lost_when_there_is_room() {
    const TASKS: u64 = 16;
    const EACH: u64 = 5_000;
    let cache: Cache<u64, u64> = Cache::new(TASKS as usize * EACH as usize * 5);
    let mut writers = JoinSet::new();
    for task in 0..TASKS {
        let cache = cache.clone();
        writers.spawn(async move {
            for i in 0..EACH {
                cache.insert(task * EACH + i, task * EACH + i + 1);
                if i.is_multiple_of(512) {
                    tokio::task::yield_now().await;
                }
            }
        });
    }
    while let Some(done) = writers.join_next().await {
        done.expect("a writer panicked");
    }
    assert_eq!(cache.len() as u64, TASKS * EACH);
    for key in 0..TASKS * EACH {
        assert_eq!(
            cache.get(&key),
            Some(key + 1),
            "the insert of {key} was lost"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn computes_on_one_key_never_lose_an_update() {
    const TASKS: usize = 16;
    const EACH: usize = 5_000;
    const KEYS: usize = 8;
    let cache: Cache<usize, usize> = Cache::new(100);
    let mut workers = JoinSet::new();
    for task in 0..TASKS {
        let cache = cache.clone();
        workers.spawn(async move {
            for i in 0..EACH {
                let key = (task + i) % KEYS;
                cache.compute(&key, |n| Compute::Update(n.map_or(1, |n| n + 1)));
                if i.is_multiple_of(512) {
                    tokio::task::yield_now().await;
                }
            }
        });
    }
    while let Some(done) = workers.join_next().await {
        done.expect("a worker panicked");
    }
    let total: usize = (0..KEYS).map(|key| cache.get(&key).unwrap_or(0)).sum();
    assert_eq!(
        total,
        TASKS * EACH,
        "every increment was applied exactly once"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_wave_of_callers_on_one_key_runs_one_load() {
    const KEYS: u32 = 64;
    const CALLERS: u32 = 16;
    const WAVES: u32 = 10;
    let cache: Cache<u32, u32> = Cache::new(1_000);
    let loads: Arc<Vec<AtomicU32>> = Arc::new((0..KEYS).map(|_| AtomicU32::new(0)).collect());

    for wave in 0..WAVES {
        let mut callers = JoinSet::new();
        for key in 0..KEYS {
            for _ in 0..CALLERS {
                let (cache, loads) = (cache.clone(), loads.clone());
                callers.spawn(async move {
                    let value = cache
                        .get_with(key, async {
                            loads[key as usize].fetch_add(1, SeqCst);
                            // Long enough for the other callers to arrive.
                            sleep(ms(2)).await;
                            key * 1_000 + wave
                        })
                        .await;
                    (key, value)
                });
            }
        }
        while let Some(done) = callers.join_next().await {
            let (key, value) = done.expect("a caller panicked");
            assert_eq!(value, key * 1_000 + wave, "wave {wave}, key {key}");
        }
        for key in 0..KEYS {
            assert_eq!(
                loads[key as usize].load(SeqCst),
                wave + 1,
                "key {key}: exactly one load in each of {} waves",
                wave + 1
            );
        }
        // The next wave starts cold.
        cache.invalidate_all();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_failing_wave_runs_one_load_and_every_caller_sees_the_error() {
    const CALLERS: usize = 64;
    let cache: Cache<u32, u32> = Cache::new(10);
    let loads = Arc::new(AtomicU32::new(0));
    let mut callers = JoinSet::new();
    for _ in 0..CALLERS {
        let (cache, loads) = (cache.clone(), loads.clone());
        callers.spawn(async move {
            cache
                .try_get_with(1, async {
                    loads.fetch_add(1, SeqCst);
                    sleep(ms(20)).await;
                    Err::<u32, String>("unavailable".to_owned())
                })
                .await
        });
    }
    let mut errors = Vec::new();
    while let Some(done) = callers.join_next().await {
        errors.push(done.expect("a caller panicked").unwrap_err());
    }
    assert_eq!(errors.len(), CALLERS);
    assert_eq!(loads.load(SeqCst), 1);
    assert!(errors.iter().all(|e| Arc::ptr_eq(e, &errors[0])));
    assert!(cache.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn cancelled_loaders_never_wedge_a_key() {
    const KEYS: u32 = 16;
    let cache: Cache<u32, u32> = Cache::builder(1_000).shards(4).build();
    let started = Arc::new(AtomicUsize::new(0));

    for round in 0..20u32 {
        let mut doomed = Vec::new();
        let mut survivors = Vec::new();
        for key in 0..KEYS {
            for caller in 0..8u32 {
                let (cache, started) = (cache.clone(), started.clone());
                let handle = tokio::spawn(async move {
                    cache
                        .get_with(key, async {
                            started.fetch_add(1, SeqCst);
                            sleep(ms(5)).await;
                            key * 100 + round
                        })
                        .await
                });
                // Every third caller is cancelled while it loads or waits.
                if caller.is_multiple_of(3) {
                    doomed.push(handle);
                } else {
                    survivors.push((key, handle));
                }
            }
        }
        sleep(ms(1)).await;
        for handle in &doomed {
            handle.abort();
        }
        for (key, handle) in survivors {
            let value = timeout(Duration::from_secs(10), handle)
                .await
                .expect("a caller waited forever on a cancelled load")
                .expect("a surviving caller was cancelled");
            assert_eq!(value, key * 100 + round);
        }
        for handle in doomed {
            let _ = handle.await;
        }
        cache.invalidate_all();
    }
    assert!(started.load(SeqCst) >= 20 * KEYS as usize);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn panicking_loaders_never_wedge_a_key() {
    const KEYS: u32 = 8;
    const CALLERS: u32 = 16;
    let cache: Cache<u32, u32> = Cache::builder(1_000).shards(4).build();
    let attempts = Arc::new(AtomicU32::new(0));

    for round in 0..10u32 {
        let mut callers = Vec::new();
        for key in 0..KEYS {
            for _ in 0..CALLERS {
                let (cache, attempts) = (cache.clone(), attempts.clone());
                callers.push(tokio::spawn(async move {
                    cache
                        .get_with(key, async {
                            let attempt = attempts.fetch_add(1, SeqCst);
                            sleep(ms(1)).await;
                            if attempt.is_multiple_of(3) {
                                panic!("loader {attempt} exploded");
                            }
                            key + round
                        })
                        .await
                }));
            }
        }
        let (mut loaded, mut panicked) = (0, 0);
        for handle in callers {
            match timeout(Duration::from_secs(10), handle)
                .await
                .expect("a caller waited forever on a panicked load")
            {
                Ok(value) => {
                    assert!(value >= round, "a value from nowhere: {value}");
                    loaded += 1;
                }
                Err(error) => {
                    assert!(error.is_panic());
                    panicked += 1;
                }
            }
        }
        assert!(
            loaded > 0 && panicked > 0,
            "loaded {loaded}, panicked {panicked}"
        );
        // Every key can still be loaded.
        for key in 0..KEYS {
            let value = timeout(
                Duration::from_secs(5),
                cache.get_with(key, async move { key + round }),
            )
            .await
            .expect("the key is wedged");
            assert!(value >= round);
        }
        cache.invalidate_all();
    }
}

/// Everything at once, with real expiry, for a second: no deadlock, no panic,
/// the bound holds, and what a read returns is what some write stored.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_mixed_storm_with_expiry_keeps_the_bound() {
    const CAPACITY: usize = 256;
    let cache: Cache<u32, u32> = Cache::builder(CAPACITY)
        .shards(8)
        .time_to_live(ms(20))
        .time_to_idle(ms(10))
        .build();
    let deadline = Instant::now() + Duration::from_millis(1_000);
    let mut workers = JoinSet::new();
    for task in 0..12u64 {
        let cache = cache.clone();
        workers.spawn(async move {
            let mut rng = Rng::new(task + 100);
            let mut i = 0u32;
            while Instant::now() < deadline {
                let key = rng.below(1_000) as u32;
                match rng.below(12) {
                    0..=2 => cache.insert(key, key + 1),
                    3 => cache.insert_with_ttl(key, key + 1, Some(ms(rng.below(30)))),
                    4..=6 => {
                        if let Some(value) = cache.get(&key) {
                            assert_eq!(value, key + 1);
                        }
                    }
                    7 => {
                        cache.remove(&key);
                    }
                    8 => {
                        cache.compute(&key, |_| Compute::Store(key + 1, Some(ms(15))));
                    }
                    9 => {
                        let value = cache.get_with(key, async move { key + 1 }).await;
                        assert_eq!(value, key + 1);
                    }
                    10 => {
                        cache.sweep(16);
                    }
                    _ => {
                        if rng.below(200) == 0 {
                            cache.invalidate_all();
                        }
                    }
                }
                assert!(cache.len() <= CAPACITY);
                i += 1;
                if i.is_multiple_of(128) {
                    tokio::task::yield_now().await;
                }
            }
        });
    }
    while let Some(done) = workers.join_next().await {
        done.expect("a worker panicked");
    }
    assert!(cache.len() <= CAPACITY);
}
