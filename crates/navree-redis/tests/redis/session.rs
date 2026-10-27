//! The session store on a real Redis server (`NAVREE_TEST_REDIS_URL`).

use std::sync::Arc;

use chrono::Duration as Span;
use navree_contracts::{FixedClock, SessionRecord, SessionStore};
use navree_redis::{RedisSessionStore, redis};
use serde_json::json;

use crate::support::TestRedis;

fn record(clock: &FixedClock, minutes: i64) -> SessionRecord {
    let mut record = SessionRecord::default();
    record.data.insert("user".to_owned(), json!("ada"));
    record.expires_at = Some(navree_contracts::Clock::now(clock) + Span::minutes(minutes));
    record
}

#[tokio::test]
async fn records_are_saved_loaded_and_deleted() {
    let Some(t) = TestRedis::connect("records_are_saved_loaded_and_deleted").await else {
        return;
    };
    let clock = Arc::new(FixedClock::new("2026-10-27T09:00:00Z".parse().unwrap()));
    let store =
        RedisSessionStore::new(t.redis.connection().clone(), &t.prefix).with_clock(clock.clone());

    store.save("s1", record(&clock, 120)).await.unwrap();
    let loaded = store.load("s1").await.unwrap().expect("saved");
    assert_eq!(loaded.data["user"], "ada");
    let ttl: i64 = redis::cmd("PTTL")
        .arg(store.redis_key("s1"))
        .query_async(&mut t.redis.manager())
        .await
        .unwrap();
    assert!(
        (119 * 60_000..=120 * 60_000).contains(&ttl),
        "Redis expires the record with the session: {ttl}"
    );

    // Past its expiry (by the app's clock) a record is never loaded.
    clock.advance(std::time::Duration::from_secs(121 * 60));
    assert!(store.load("s1").await.unwrap().is_none());
    assert!(t.keys().await.is_empty(), "and it is removed");

    // An already expired record is not stored at all.
    store.save("s2", record(&clock, -1)).await.unwrap();
    assert!(store.load("s2").await.unwrap().is_none());

    store.save("s3", SessionRecord::default()).await.unwrap();
    assert!(store.load("s3").await.unwrap().is_some(), "no expiry");
    store.delete("s3").await.unwrap();
    assert!(store.load("s3").await.unwrap().is_none());
    t.cleanup().await;
}
