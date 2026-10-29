//! `prune_time_series` on a real PostgreSQL server with TimescaleDB: whole
//! chunks dropped, the straddling chunk's old rows deleted, the count
//! exact; regular tables and other columns pruned row by row.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a URL of a throwaway server whose user may
//! create databases, e.g.
//! `postgres://postgres:secret@127.0.0.1:55432/postgres`. Each test creates
//! its own database and drops it at the end.
#![cfg(feature = "postgres")]

use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use navree_db::{Db, DbOptions, TimePoint, prune_time_series};
use navree_schema::Schema;
use sea_orm::sea_query::{Alias, Query};
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own on the test server.
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new() -> Option<Scratch> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!(
            "navree_time_series_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let conn = Database::connect(&admin).await.unwrap();
        conn.execute_unprepared(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .await
            .unwrap();
        conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        conn.close().await.unwrap();
        let (base, _) = admin.rsplit_once('/').unwrap();
        let url = format!("{base}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn db(&self) -> Db {
        Db::connect(&DbOptions {
            url: self.url.clone(),
            ..DbOptions::default()
        })
        .await
        .unwrap()
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        conn.execute_unprepared(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
        conn.close().await.unwrap();
    }
}

/// The single number `sql` returns.
async fn number(db: &Db, sql: &str) -> i64 {
    let row = db
        .conn()
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .unwrap();
    row.try_get_by_index::<i64>(0).unwrap()
}

/// Noon UTC on October `n`, 2026.
fn day(n: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, n, 12, 0, 0).unwrap()
}

async fn insert_event(db: &Db, table: &str, at: DateTime<Utc>) {
    db.conn()
        .execute(
            Query::insert()
                .into_table(Alias::new(table))
                .columns([Alias::new("id"), Alias::new("occurred_at")])
                .values_panic([uuid::Uuid::now_v7().into(), at.into()]),
        )
        .await
        .unwrap();
}

/// Whether the server has TimescaleDB (installed when it may be).
async fn timescaledb(db: &Db) -> bool {
    let schema = Schema::new(&db);
    let target = schema.target().await.unwrap();
    if target.timescaledb().is_some() {
        return true;
    }
    if target.timescaledb_installable().is_some() {
        db.conn()
            .execute_unprepared("CREATE EXTENSION IF NOT EXISTS timescaledb")
            .await
            .unwrap();
        return true;
    }
    eprintln!("TimescaleDB is not available on this server: skipping");
    false
}

#[tokio::test]
async fn hypertables_lose_whole_chunks() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let db = scratch.db().await;
    if !timescaledb(&db).await {
        scratch.drop().await;
        return;
    }
    Schema::new(&db)
        .create_hypertable("events", "occurred_at", |t| {
            t.uuid_primary();
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").chunk_interval("1 day");
        })
        .await
        .unwrap();
    // One event a day from October 1st to 10th at noon, plus one on the
    // 5th at 20:00: ten daily chunks.
    for n in 1..=10 {
        insert_event(&db, "events", day(n)).await;
    }
    insert_event(&db, "events", day(5) + TimeDelta::hours(8)).await;
    let chunks = || number(&db, "SELECT count(*) FROM show_chunks('events')");
    assert_eq!(chunks().await, 10);

    // October 5th at 18:00: the chunks of the 1st to the 4th go whole, the
    // 5th's chunk straddles the cutoff and loses its noon event.
    let cutoff = day(5) + TimeDelta::hours(6);
    let deleted = db
        .transaction(|uow| async move {
            let tx = uow.tx().await?;
            prune_time_series(&*tx, "events", "occurred_at", cutoff).await
        })
        .await
        .unwrap();
    assert_eq!(deleted, 5, "the 1st to the 5th at noon");
    assert_eq!(chunks().await, 6, "the chunks of the 5th to the 10th");
    assert_eq!(
        number(
            &db,
            "SELECT count(*) FROM events WHERE occurred_at < '2026-10-05 18:00:00+00'"
        )
        .await,
        0
    );
    assert_eq!(number(&db, "SELECT count(*) FROM events").await, 6);

    // A qualified name works the same.
    let deleted = prune_time_series(db.conn(), "public.events", "occurred_at", day(8))
        .await
        .unwrap();
    assert_eq!(deleted, 3, "the 5th at 20:00, the 6th and the 7th");
    assert_eq!(chunks().await, 3);

    // Nothing older: nothing happens.
    let none = prune_time_series(db.conn(), "events", "occurred_at", day(8))
        .await
        .unwrap();
    assert_eq!(none, 0);
    assert_eq!(chunks().await, 3);
    scratch.drop().await;
}

#[tokio::test]
async fn integer_time_columns_lose_whole_chunks_too() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let db = scratch.db().await;
    if !timescaledb(&db).await {
        scratch.drop().await;
        return;
    }
    // Unix milliseconds, in chunks of a day.
    Schema::new(&db)
        .create_hypertable("ticks", "at_ms", |t| {
            t.uuid_primary();
            t.big_integer("at_ms");
            t.hypertable("at_ms").chunk_interval("86400000");
        })
        .await
        .unwrap();
    for n in 1..=6 {
        db.conn()
            .execute(
                Query::insert()
                    .into_table(Alias::new("ticks"))
                    .columns([Alias::new("id"), Alias::new("at_ms")])
                    .values_panic([
                        uuid::Uuid::now_v7().into(),
                        day(n).timestamp_millis().into(),
                    ]),
            )
            .await
            .unwrap();
    }
    let chunks = || number(&db, "SELECT count(*) FROM show_chunks('ticks')");
    assert_eq!(chunks().await, 6);
    let deleted = prune_time_series(db.conn(), "ticks", "at_ms", day(4).timestamp_millis())
        .await
        .unwrap();
    assert_eq!(deleted, 3);
    assert_eq!(
        chunks().await,
        3,
        "the chunks of the 1st to the 3rd went; the 4th's straddles the cutoff and stays"
    );

    // A moment for an integer column is refused before anything runs (the
    // database would only say `operator does not exist: bigint < timestamp
    // with time zone`).
    let error = prune_time_series(db.conn(), "ticks", "at_ms", TimePoint::At(day(6)))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("its time column `at_ms` is `bigint`; pass an integer (TimePoint::Integer)"),
        "{error}"
    );
    assert_eq!(number(&db, "SELECT count(*) FROM ticks").await, 3);
    scratch.drop().await;
}

#[tokio::test]
async fn regular_tables_and_other_columns_are_pruned_row_by_row() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let db = scratch.db().await;
    let has_timescaledb = timescaledb(&db).await;
    let schema = Schema::new(&db);
    schema
        .create("notes", |t| {
            t.uuid_primary();
            t.timestamp_tz("occurred_at");
        })
        .await
        .unwrap();
    for n in 1..=4 {
        insert_event(&db, "notes", day(n)).await;
    }
    let deleted = prune_time_series(db.conn(), "notes", "occurred_at", day(3))
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(number(&db, "SELECT count(*) FROM notes").await, 2);

    if has_timescaledb {
        // Pruned by a column that is not its time dimension, a hypertable
        // keeps every chunk: only the matching rows go.
        schema
            .create_hypertable("visits", "occurred_at", |t| {
                t.uuid_primary();
                t.timestamp_tz("occurred_at");
                t.timestamp_tz("seen_at");
                t.hypertable("occurred_at").chunk_interval("1 day");
            })
            .await
            .unwrap();
        for n in 1..=3 {
            db.conn()
                .execute(
                    Query::insert()
                        .into_table(Alias::new("visits"))
                        .columns([
                            Alias::new("id"),
                            Alias::new("occurred_at"),
                            Alias::new("seen_at"),
                        ])
                        .values_panic([uuid::Uuid::now_v7().into(), day(n).into(), day(1).into()]),
                )
                .await
                .unwrap();
        }
        let deleted = prune_time_series(db.conn(), "visits", "seen_at", day(2))
            .await
            .unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(
            number(&db, "SELECT count(*) FROM show_chunks('visits')").await,
            3,
            "rows deleted, chunks kept"
        );
    }
    scratch.drop().await;
}

/// `prune_time_series_on`, the same pruning on navree-db's executors: a
/// transaction, then the `Db`; whole chunks on hypertables (by a moment or
/// by an integer), the cutoff's kind checked, a regular table row by row.
#[tokio::test]
async fn the_executor_variant_prunes_like_the_connection_one() {
    use navree_db::prune_time_series_on;

    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let db = scratch.db().await;
    if !timescaledb(&db).await {
        scratch.drop().await;
        return;
    }
    let schema = Schema::new(&db);
    schema
        .create_hypertable("events", "occurred_at", |t| {
            t.uuid_primary();
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").chunk_interval("1 day");
        })
        .await
        .unwrap();
    schema
        .create_hypertable("ticks", "at_ms", |t| {
            t.uuid_primary();
            t.big_integer("at_ms");
            t.hypertable("at_ms").chunk_interval("86400000");
        })
        .await
        .unwrap();
    schema
        .create("notes", |t| {
            t.uuid_primary();
            t.timestamp_tz("occurred_at");
        })
        .await
        .unwrap();
    for n in 1..=10 {
        insert_event(&db, "events", day(n)).await;
        insert_event(&db, "notes", day(n)).await;
        db.conn()
            .execute(
                Query::insert()
                    .into_table(Alias::new("ticks"))
                    .columns([Alias::new("id"), Alias::new("at_ms")])
                    .values_panic([
                        uuid::Uuid::now_v7().into(),
                        day(n).timestamp_millis().into(),
                    ]),
            )
            .await
            .unwrap();
    }
    let chunks = |table: &'static str| {
        let db = db.clone();
        async move { number(&db, &format!("SELECT count(*) FROM show_chunks('{table}')")).await }
    };
    assert_eq!(chunks("events").await, 10);

    // In a transaction: the count, the dropped chunks and the delete commit
    // together.
    let tx = db.begin().await.unwrap();
    let deleted = prune_time_series_on(&tx, "events", "occurred_at", day(4))
        .await
        .unwrap();
    assert_eq!(deleted, 3);
    tx.commit().await.unwrap();
    assert_eq!(chunks("events").await, 7);

    // A rolled-back prune leaves everything.
    let tx = db.begin().await.unwrap();
    prune_time_series_on(&tx, "events", "occurred_at", day(8))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(chunks("events").await, 7);

    // The `Db`, a qualified name, nothing older.
    let deleted = prune_time_series_on(&db, "public.events", "occurred_at", day(6))
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(
        prune_time_series_on(&db, "events", "occurred_at", day(6))
            .await
            .unwrap(),
        0
    );
    assert_eq!(chunks("events").await, 5);

    // Integer time columns, and a cutoff of the wrong kind refused.
    let deleted = prune_time_series_on(&db, "ticks", "at_ms", day(4).timestamp_millis())
        .await
        .unwrap();
    assert_eq!(deleted, 3);
    assert_eq!(chunks("ticks").await, 7);
    let error = prune_time_series_on(&db, "ticks", "at_ms", TimePoint::At(day(6)))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("its time column `at_ms` is `bigint`; pass an integer (TimePoint::Integer)"),
        "{error}"
    );

    // A regular table is pruned row by row.
    let deleted = prune_time_series_on(&db, "notes", "occurred_at", day(3))
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    scratch.drop().await;
}
