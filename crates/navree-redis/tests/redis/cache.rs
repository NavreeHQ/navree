//! The cache and lock stores on a real Redis server (`NAVREE_TEST_REDIS_URL`):
//! the same behavior as navree-cache's memory stores.

use std::sync::Arc;
use std::time::Duration;

use navree_cache::{Cache, CacheError, CacheModule, CacheOptions, CacheStoreKind, Locks, Ttl};
use navree_kernel::Navree;
use navree_redis::{RedisCacheStore, RedisLockStore, RedisModule, redis};

use crate::support::TestRedis;

const MINUTE: Duration = Duration::from_secs(60);

fn cache(t: &TestRedis) -> Cache {
    let store = RedisCacheStore::new(t.redis.connection().clone(), &t.prefix);
    Cache::new(Arc::new(store), "app:", Duration::from_secs(3600))
}

fn locks(t: &TestRedis) -> Locks {
    Locks::new(
        Arc::new(RedisLockStore::new(t.redis.connection().clone(), &t.prefix)),
        "app:",
    )
}

#[tokio::test]
async fn values_are_stored_with_their_ttl() {
    let Some(t) = TestRedis::connect("values_are_stored_with_their_ttl").await else {
        return;
    };
    let cache = cache(&t);
    cache.put("greeting", &"hello", MINUTE).await.unwrap();
    assert_eq!(
        cache.get::<String>("greeting").await.unwrap().as_deref(),
        Some("hello")
    );
    assert!(cache.has("greeting").await.unwrap());

    let key = format!("{}cache:app:greeting", t.prefix);
    let ttl: i64 = redis::cmd("PTTL")
        .arg(&key)
        .query_async(&mut t.redis.manager())
        .await
        .unwrap();
    assert!((59_000..=60_000).contains(&ttl), "PTTL {ttl}");

    cache.forever("setting", &4).await.unwrap();
    let ttl: i64 = redis::cmd("PTTL")
        .arg(format!("{}cache:app:setting", t.prefix))
        .query_async(&mut t.redis.manager())
        .await
        .unwrap();
    assert_eq!(ttl, -1, "forever has no expiry");

    cache
        .put("short", &1, Duration::from_millis(100))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(cache.get::<u32>("short").await.unwrap(), None, "expired");

    assert_eq!(
        cache.pull::<String>("greeting").await.unwrap().as_deref(),
        Some("hello")
    );
    assert!(!cache.has("greeting").await.unwrap());
    assert!(!cache.forget("greeting").await.unwrap());
    assert!(cache.forget("setting").await.unwrap());
    t.cleanup().await;
}

#[tokio::test]
async fn add_stores_only_once() {
    let Some(t) = TestRedis::connect("add_stores_only_once").await else {
        return;
    };
    let cache = cache(&t);
    assert!(cache.add("once", &1, MINUTE).await.unwrap());
    assert!(!cache.add("once", &2, MINUTE).await.unwrap());
    assert_eq!(cache.get::<u32>("once").await.unwrap(), Some(1));
    t.cleanup().await;
}

#[tokio::test]
async fn counters_increment_atomically_and_keep_their_ttl() {
    let Some(t) = TestRedis::connect("counters_increment_atomically_and_keep_their_ttl").await
    else {
        return;
    };
    let cache = cache(&t);
    assert_eq!(
        cache.increment("visits", 1).await.unwrap(),
        1,
        "missing is 0"
    );
    assert_eq!(cache.increment("visits", 5).await.unwrap(), 6);
    assert_eq!(cache.decrement("visits", 2).await.unwrap(), 4);
    assert_eq!(cache.get::<i64>("visits").await.unwrap(), Some(4));

    cache.add("window", &0, MINUTE).await.unwrap();
    assert_eq!(cache.increment("window", 1).await.unwrap(), 1);
    let ttl: i64 = redis::cmd("PTTL")
        .arg(format!("{}cache:app:window", t.prefix))
        .query_async(&mut t.redis.manager())
        .await
        .unwrap();
    assert!(ttl > 0, "the counter kept its TTL ({ttl})");

    cache.put("name", &"ada", MINUTE).await.unwrap();
    assert!(matches!(
        cache.increment("name", 1).await,
        Err(CacheError::NotAnInteger { .. })
    ));
    cache.put("big", &i64::MAX, MINUTE).await.unwrap();
    assert!(matches!(
        cache.increment("big", 1).await,
        Err(CacheError::Overflow { .. })
    ));
    assert_eq!(cache.get::<i64>("big").await.unwrap(), Some(i64::MAX));

    // Concurrent increments are never lost.
    let tasks: Vec<_> = (0..20)
        .map(|_| {
            let cache = cache.clone();
            tokio::spawn(async move { cache.increment("shared", 1).await.unwrap() })
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(cache.get::<i64>("shared").await.unwrap(), Some(20));
    t.cleanup().await;
}

#[tokio::test]
async fn remember_tags_and_flexible_work_on_redis() {
    let Some(t) = TestRedis::connect("remember_tags_and_flexible_work_on_redis").await else {
        return;
    };
    let cache = cache(&t);
    let first: u32 = cache
        .remember("totals", MINUTE, || async { Ok(7) })
        .await
        .unwrap();
    let second: u32 = cache
        .remember("totals", MINUTE, || async { Ok(8) })
        .await
        .unwrap();
    assert_eq!((first, second), (7, 7));

    let deals = cache.tags(&["deals", "company:1"]);
    deals.put("board", &"cards", MINUTE).await.unwrap();
    assert_eq!(
        deals.get::<String>("board").await.unwrap().as_deref(),
        Some("cards")
    );
    cache.tags(&["company:1"]).flush().await.unwrap();
    assert_eq!(
        deals.get::<String>("board").await.unwrap(),
        None,
        "flushed by tag"
    );

    let rate: f64 = cache
        .flexible("fx", MINUTE, MINUTE, || async { Ok(1.25) })
        .await
        .unwrap();
    assert!((rate - 1.25).abs() < f64::EPSILON);
    t.cleanup().await;
}

#[tokio::test]
async fn flush_clears_the_cache_and_nothing_else() {
    let Some(t) = TestRedis::connect("flush_clears_the_cache_and_nothing_else").await else {
        return;
    };
    let cache = cache(&t);
    for i in 0..30 {
        cache.put(&format!("k{i}"), &i, Ttl::Default).await.unwrap();
    }
    let other = format!("{}queue:pending:default", t.prefix);
    let _: i64 = redis::cmd("ZADD")
        .arg(&other)
        .arg(1)
        .arg("job")
        .query_async(&mut t.redis.manager())
        .await
        .unwrap();
    let held = locks(&t).lock("nightly", MINUTE).acquire().await.unwrap();
    assert!(held.is_some());

    cache.flush().await.unwrap();
    assert_eq!(cache.get::<u32>("k1").await.unwrap(), None);
    let keys = t.keys().await;
    assert!(keys.contains(&other), "{keys:?}");
    assert!(
        keys.iter().any(|k| k.contains("locks:app:lock:nightly")),
        "locks survive a cache flush: {keys:?}"
    );
    assert!(!keys.iter().any(|k| k.contains(":cache:")), "{keys:?}");
    drop(held);
    t.cleanup().await;
}

#[tokio::test]
async fn locks_are_shared_and_owned() {
    let Some(t) = TestRedis::connect("locks_are_shared_and_owned").await else {
        return;
    };
    // Two stores on the same server: two processes.
    let (here, there) = (locks(&t), locks(&t));
    let guard = here
        .lock("import", MINUTE)
        .acquire()
        .await
        .unwrap()
        .expect("free");
    assert!(
        there
            .lock("import", MINUTE)
            .acquire()
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !there.lock("import", MINUTE).release().await.unwrap(),
        "another owner cannot release it"
    );

    let owner = guard.into_owner();
    assert!(
        there
            .lock("import", MINUTE)
            .acquire()
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        there
            .lock("import", MINUTE)
            .with_owner(owner)
            .release()
            .await
            .unwrap()
    );
    let guard = there.lock("import", MINUTE).acquire().await.unwrap();
    assert!(guard.is_some(), "free again");
    assert!(guard.unwrap().release().await.unwrap());

    // Expiry frees a lock nobody released.
    let short = here
        .lock("short", Duration::from_millis(100))
        .acquire()
        .await
        .unwrap()
        .unwrap();
    let _ = short.into_owner();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(
        there
            .lock("short", MINUTE)
            .acquire()
            .await
            .unwrap()
            .is_some()
    );

    // `block` waits for the holder.
    let held = here
        .lock("report", Duration::from_millis(200))
        .acquire()
        .await
        .unwrap()
        .unwrap();
    let _ = held.into_owner();
    let waited = there
        .lock("report", MINUTE)
        .block(Duration::from_secs(2))
        .await
        .unwrap();
    assert!(waited.release().await.unwrap());
    t.cleanup().await;
}

#[tokio::test]
async fn cache_store_redis_runs_through_the_modules() {
    let Some(t) = TestRedis::connect("cache_store_redis_runs_through_the_modules").await else {
        return;
    };
    let boot = || {
        Navree::app()
            .module(RedisModule::for_root(t.options()))
            .module(CacheModule::for_root(CacheOptions {
                store: CacheStoreKind::Redis,
                prefix: "crm:".into(),
                ..CacheOptions::default()
            }))
            .boot()
    };
    let (first, second) = (boot().await.unwrap(), boot().await.unwrap());
    let a = first.ctx().get::<Cache>().await.unwrap();
    let b = second.ctx().get::<Cache>().await.unwrap();
    a.put("shared", &"across processes", MINUTE).await.unwrap();
    assert_eq!(
        b.get::<String>("shared").await.unwrap().as_deref(),
        Some("across processes")
    );
    second.run_command("cache:clear", Vec::new()).await.unwrap();
    assert_eq!(
        a.get::<String>("shared").await.unwrap(),
        None,
        "cleared for every process"
    );
    t.cleanup().await;
}
