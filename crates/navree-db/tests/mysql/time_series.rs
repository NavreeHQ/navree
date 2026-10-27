//! `prune_time_series` on a real MySQL server: a hypertable declared in a
//! migration is a regular table here, pruned with one `DELETE`.

use chrono::{DateTime, TimeZone, Utc};
use navree_db::prune_time_series;
use navree_schema::Schema;
use sea_orm::ConnectionTrait;
use sea_orm::sea_query::{Alias, Query};

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
    let schema = Schema::new(db.conn());
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
        db.conn()
            .execute(
                Query::insert()
                    .into_table(Alias::new("events"))
                    .columns([
                        Alias::new("id"),
                        Alias::new("occurred_at"),
                        Alias::new("at_ms"),
                    ])
                    .values_panic([
                        // `uuid_primary()` is CHAR(36) on MySQL.
                        uuid::Uuid::now_v7().to_string().into(),
                        day(n).into(),
                        day(n).timestamp_millis().into(),
                    ]),
            )
            .await
            .unwrap();
    }
    let deleted = prune_time_series(db.conn(), "events", "occurred_at", day(3))
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    let deleted = prune_time_series(db.conn(), "events", "at_ms", day(4).timestamp_millis())
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    let left = db
        .conn()
        .query_all(
            Query::select()
                .column(Alias::new("at_ms"))
                .from(Alias::new("events")),
        )
        .await
        .unwrap()
        .len();
    assert_eq!(left, 2);
}
