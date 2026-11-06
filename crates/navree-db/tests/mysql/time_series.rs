//! `prune_time_series` on a real MySQL server: a hypertable declared in a
//! migration is a regular table here, pruned with one `DELETE`.

use chrono::{DateTime, TimeZone, Utc};
use navree_db::{Executor, ExecutorExt, Statement, prune_time_series};
use navree_schema::Schema;

use crate::support::Scratch;

fn day(n: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, n, 12, 0, 0).unwrap()
}

#[tokio::test]
async fn old_rows_are_deleted_and_counted() {
    let Some(scratch) = Scratch::new("old_rows_are_deleted_and_counted").await else {
        return;
    };
    let db = scratch.db().await;
    let schema = Schema::new(&db);
    schema
        .create_hypertable("events", "occurred_at", |t| {
            t.uuid_primary();
            t.timestamp_tz("occurred_at");
            t.big_integer("at_ms");
            t.hypertable("occurred_at").chunk_interval("1 day");
        })
        .await
        .unwrap();
    assert!(!schema.is_hypertable("events").await.unwrap());
    for n in 1..=5 {
        db.execute(&Statement::portable(
            "INSERT INTO events (id, occurred_at, at_ms) VALUES (?, ?, ?)",
            [
                uuid::Uuid::now_v7().into(),
                day(n).into(),
                day(n).timestamp_millis().into(),
            ],
        ))
        .await
        .unwrap();
    }
    let deleted = prune_time_series(&db, "events", "occurred_at", day(3))
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    let tx = db.begin().await.unwrap();
    let deleted = prune_time_series(&tx, "events", "at_ms", day(4).timestamp_millis())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(deleted, 1);
    let left: i64 = db
        .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM events"))
        .await
        .unwrap();
    assert_eq!(left, 2);
}
