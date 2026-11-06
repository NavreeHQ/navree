//! SQLite, PostgreSQL and MySQL: the local-time SQL buckets rows into the
//! same local days as Rust, across a DST change and in half-hour zones, and
//! every zone name navree-time produces is one the server knows.
//!
//! Opt-in on the servers: build with `--features test-postgres,test-mysql` and set
//! `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL` (each test creates
//! a database of its own and drops it). Without them the test prints
//! `SKIPPED` and passes, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate, Utc};
use navree_db::{Backend, Executor, ExecutorExt, Statement};
use navree_time::{DayRange, LocalDay, Zone};

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{day, utc, zone};

/// Zones added to the tz database after the release PostgreSQL 17 servers
/// commonly ship with.
const RECENT_ZONES: &[&str] = &["America/Coyhaique"];

/// `events(id, created_at)` with the column type each server keeps instants
/// in, and `instants` inserted with ids from 0.
async fn events(db: &impl Executor, instants: &[DateTime<Utc>]) {
    let column = match db.backend() {
        Backend::Postgres => "TIMESTAMPTZ",
        Backend::MySql => "DATETIME(6)",
        Backend::Sqlite => "TEXT",
    };
    db.execute_script(&format!(
        "CREATE TABLE events (id INTEGER PRIMARY KEY, created_at {column} NOT NULL)"
    ))
    .await
    .unwrap();
    for (id, at) in instants.iter().enumerate() {
        db.execute(&Statement::portable(
            "INSERT INTO events (id, created_at) VALUES (?, ?)",
            [(id as i32).into(), (*at).into()],
        ))
        .await
        .unwrap();
    }
}

/// Rows per local day of `zone` between `from` and `to`, counted by the
/// database.
async fn counts_per_local_day(
    db: &impl Executor,
    zone: &Zone,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<(NaiveDate, i64)> {
    let sql = zone.sql(db.backend()).within(from, to);
    let local_day = sql.local_date(&sql.column("created_at"));
    // The expression appears in SELECT and GROUP BY: its values go with each.
    let statement = Statement::portable(
        format!(
            "SELECT {local_day} AS day, COUNT(*) AS n FROM events \
             WHERE created_at >= ? AND created_at < ? GROUP BY {local_day} ORDER BY 1",
            local_day = local_day.sql()
        ),
        local_day
            .values()
            .iter()
            .cloned()
            .chain([from.into(), to.into()])
            .chain(local_day.values().iter().cloned()),
    );
    db.fetch_all(&statement)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (
                row.get::<NaiveDate>("day").unwrap(),
                row.get::<i64>("n").unwrap(),
            )
        })
        .collect()
}

fn date(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

async fn groups_rows_by_local_day_across_a_dst_change(scratch: Scratch) {
    let db = scratch.db().await;
    let london = zone("Europe/London");
    let range = DayRange::new(day("2026-03-28"), day("2026-03-30")).unwrap();
    let (from, to) = range.utc_bounds(&london);
    events(
        &db,
        &[
            utc("2026-03-28T23:30:00Z"), // 28 March, 23:30 GMT
            utc("2026-03-29T00:30:00Z"), // 29 March, 00:30 GMT
            utc("2026-03-29T22:30:00Z"), // 29 March, 23:30 BST
            utc("2026-03-29T23:30:00Z"), // 30 March, 00:30 BST
            utc("2026-03-30T22:59:00Z"), // 30 March, 23:59 BST
        ],
    )
    .await;
    assert_eq!(
        counts_per_local_day(&db, &london, from, to).await,
        [
            (date("2026-03-28"), 1),
            (date("2026-03-29"), 2),
            (date("2026-03-30"), 2),
        ]
    );
}

async fn groups_rows_by_local_day_in_a_half_hour_zone(scratch: Scratch) {
    let db = scratch.db().await;
    let kolkata = zone("Asia/Kolkata");
    let range = DayRange::new(day("2026-10-26"), day("2026-10-27")).unwrap();
    let (from, to) = range.utc_bounds(&kolkata);
    events(
        &db,
        &[
            utc("2026-10-25T18:30:00Z"), // 26 October, 00:00 IST
            utc("2026-10-26T18:29:59Z"), // 26 October, 23:59:59 IST
            utc("2026-10-26T18:30:00Z"), // 27 October, 00:00 IST
        ],
    )
    .await;
    assert_eq!(
        counts_per_local_day(&db, &kolkata, from, to).await,
        [(date("2026-10-26"), 2), (date("2026-10-27"), 1)]
    );
}

/// Each local date the database computes names the same day as
/// `LocalDay::of`, in zones with and without DST.
async fn local_dates_agree_with_rust(scratch: Scratch) {
    let db = scratch.db().await;
    let instants = [
        utc("2026-03-29T00:30:00Z"),
        utc("2026-03-29T23:30:00Z"),
        utc("2026-10-25T00:30:00Z"),
        utc("2026-10-26T18:29:59Z"),
        utc("2026-10-26T18:30:00Z"),
        utc("2026-11-01T05:30:00Z"),
    ];
    events(&db, &instants).await;
    let (from, to) = (utc("2026-03-01T00:00:00Z"), utc("2026-12-01T00:00:00Z"));
    for name in [
        "Europe/London",
        "Asia/Kolkata",
        "America/New_York",
        "Asia/Calcutta",
    ] {
        let zone = Zone::parse(name).unwrap();
        // `within` covers the DST changes, so SQLite and MySQL switch
        // offsets where the zone does; PostgreSQL asks the server's zones.
        let sql = zone.sql(scratch.backend()).within(from, to);
        let local_date = sql.local_date(&sql.column("created_at"));
        let statement = Statement::portable(
            format!(
                "SELECT id, {} AS day FROM events ORDER BY id",
                local_date.sql()
            ),
            local_date.values().iter().cloned(),
        );
        for row in db.fetch_all(&statement).await.unwrap() {
            let id: i32 = row.get("id").unwrap();
            let at = instants[id as usize];
            assert_eq!(
                LocalDay::from(row.get::<NaiveDate>("day").unwrap()),
                LocalDay::of(at, &zone),
                "{name} {at} on {}",
                scratch.backend()
            );
        }
    }
}

/// PostgreSQL and MySQL (with its time zone tables) know every zone name
/// navree-time produces, and accept it in a query.
async fn the_server_knows_every_zone_name(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.backend();
    if backend == Backend::Sqlite {
        return; // SQLite has no zones: it adds offsets.
    }
    let catalog = match backend {
        Backend::Postgres => "SELECT name FROM pg_timezone_names",
        _ => "SELECT Name FROM mysql.time_zone_name",
    };
    let known: BTreeSet<String> = match db.fetch_column::<String>(&Statement::text(catalog)).await {
        Ok(names) => names.into_iter().collect(),
        Err(error) => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "the zone names on {backend}: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)"
            );
            println!("SKIPPED the zone names on {backend}: {error}");
            return;
        }
    };
    if known.is_empty() {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "the zone names on {backend}: no time zone tables (NAVREE_TEST_REQUIRE_SERVICES=1)"
        );
        println!("SKIPPED the zone names on {backend}: no time zone tables");
        return;
    }
    let missing: Vec<&str> = Zone::all()
        .map(|zone| zone.name())
        .filter(|name| !known.contains(*name) && !RECENT_ZONES.contains(name))
        .collect();
    assert!(missing.is_empty(), "{backend} does not know {missing:?}");
    for zone in Zone::all().filter(|z| known.contains(z.name())) {
        let sql = zone.sql(backend);
        let select = match backend {
            Backend::Postgres => format!("SELECT now() AT TIME ZONE {}", sql.literal()),
            _ => format!("SELECT CONVERT_TZ(NOW(), '+00:00', {})", sql.literal()),
        };
        db.fetch_all(&Statement::text(select))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", zone.name()));
    }
}

on_every_server!(
    "navree_time";
    groups_rows_by_local_day_across_a_dst_change,
    groups_rows_by_local_day_in_a_half_hour_zone,
    local_dates_agree_with_rust,
    the_server_knows_every_zone_name,
);
