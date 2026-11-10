//! `prune_time_series` on SQLite: one `DELETE` of the rows older than the
//! cutoff (on TimescaleDB it drops whole chunks first; see
//! `tests/postgres_time_series.rs`).

use chrono::{DateTime, TimeZone, Utc};
use navree_db::{Db, Executor, ExecutorExt, Statement, TimePoint, prune_time_series};
use navree_schema::Schema;

use crate::support::TempDb;

fn day(n: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, n, 12, 0, 0).unwrap()
}

/// `events(kind, occurred_at)`, a hypertable on TimescaleDB, with one
/// event a day from October 1st to 5th.
async fn events(db: &Db) {
    Schema::new(db)
        .create_hypertable("events", "occurred_at", |t| {
            t.uuid_primary();
            t.string("kind");
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").chunk_interval("1 day");
        })
        .await
        .unwrap();
    for n in 1..=5 {
        db.execute(&Statement::portable(
            "INSERT INTO events (id, kind, occurred_at) VALUES (?, ?, ?)",
            [
                uuid::Uuid::now_v7().into(),
                format!("day {n}").into(),
                day(n).into(),
            ],
        ))
        .await
        .unwrap();
    }
}

/// The `kind` of every event left, oldest first.
async fn kinds(db: &Db) -> Vec<String> {
    db.fetch_column(&Statement::text(
        "SELECT kind FROM events ORDER BY occurred_at",
    ))
    .await
    .unwrap()
}

#[tokio::test]
async fn deletes_the_rows_before_the_cutoff_and_counts_them() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    events(&db).await;
    let deleted = prune_time_series(&db, "events", "occurred_at", day(3))
        .await
        .unwrap();
    assert_eq!(deleted, 2, "October 1st and 2nd are before the 3rd at noon");
    assert_eq!(kinds(&db).await, ["day 3", "day 4", "day 5"]);
    // Nothing older is left: pruning again deletes nothing.
    let again = prune_time_series(&db, "events", "occurred_at", day(3))
        .await
        .unwrap();
    assert_eq!(again, 0);
    // A cutoff before every row deletes nothing either.
    let none = prune_time_series(db.pool(), "events", "occurred_at", TimePoint::At(day(1)))
        .await
        .unwrap();
    assert_eq!(none, 0);
}

#[tokio::test]
async fn integer_time_columns_take_integer_cutoffs() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    db.execute_script(
        "CREATE TABLE ticks (id INTEGER PRIMARY KEY, at_ms BIGINT NOT NULL); \
         INSERT INTO ticks (at_ms) VALUES (1000), (2000), (3000), (4000)",
    )
    .await
    .unwrap();
    let deleted = prune_time_series(&db, "ticks", "at_ms", 3000_i64)
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    let left: Vec<i64> = db
        .fetch_column(&Statement::text("SELECT at_ms FROM ticks ORDER BY at_ms"))
        .await
        .unwrap();
    assert_eq!(left, [3000, 4000]);
}

#[tokio::test]
async fn inside_a_transaction_too() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    events(&db).await;
    let deleted = db
        .transaction(
            |uow| async move { prune_time_series(&uow, "events", "occurred_at", day(5)).await },
        )
        .await
        .unwrap();
    assert_eq!(deleted, 4);
    assert_eq!(kinds(&db).await, ["day 5"]);

    let tx = db.begin().await.unwrap();
    let deleted = prune_time_series(&tx, "events", "occurred_at", day(6))
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    tx.rollback().await.unwrap();
    assert_eq!(
        kinds(&db).await,
        ["day 5"],
        "rolled back with the transaction"
    );
}

#[tokio::test]
async fn a_missing_table_is_the_database_error() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    let error = prune_time_series(&db, "nothing_here", "at", day(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no such table"), "{error}");
}
