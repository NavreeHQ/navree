//! PostgreSQL 17: every zone name navree-time produces is one the server
//! knows, and `AT TIME ZONE` buckets rows into the same local days as Rust.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` (any database; the tests only create a
//! temporary table). Without the variable each test prints `SKIPPED` and
//! passes, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.
#![cfg(feature = "postgres")]

use std::collections::BTreeSet;

use navree_time::{LocalDay, Zone};
use sea_orm::sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DbBackend, FromQueryResult, Statement,
};

use crate::support::utc;

/// Zones added to the tz database after the release PostgreSQL 17 servers
/// commonly ship with.
const RECENT_ZONES: &[&str] = &["America/Coyhaique"];

async fn connect(test: &str) -> Option<DatabaseConnection> {
    match std::env::var("NAVREE_TEST_POSTGRES_URL") {
        Ok(url) => Some(Database::connect(url).await.expect("connect to PostgreSQL")),
        Err(_) => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_REQUIRE_SERVICES=1 but NAVREE_TEST_POSTGRES_URL is not set"
            );
            println!("SKIPPED {test}: set NAVREE_TEST_POSTGRES_URL");
            None
        }
    }
}

#[derive(FromQueryResult)]
struct Name {
    name: String,
}

#[tokio::test]
async fn postgres_knows_every_zone_name() {
    let Some(db) = connect("postgres_knows_every_zone_name").await else {
        return;
    };
    let known: BTreeSet<String> = Name::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        "SELECT name FROM pg_timezone_names",
    ))
    .all(&db)
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.name)
    .collect();
    let missing: Vec<&str> = Zone::all()
        .map(|zone| zone.name())
        .filter(|name| !known.contains(*name) && !RECENT_ZONES.contains(name))
        .collect();
    assert!(missing.is_empty(), "PostgreSQL does not know {missing:?}");
    // Each one also works in a query.
    for zone in Zone::all().filter(|z| known.contains(z.name())) {
        db.execute_unprepared(&format!(
            "SELECT now() AT TIME ZONE {}",
            zone.sql(DbBackend::Postgres).literal()
        ))
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", zone.name()));
    }
}

#[derive(Debug, PartialEq, FromQueryResult)]
struct LocalDate {
    id: i32,
    day: chrono::NaiveDate,
}

#[tokio::test]
async fn at_time_zone_agrees_with_local_days() {
    let Some(db) = connect("at_time_zone_agrees_with_local_days").await else {
        return;
    };
    db.execute_unprepared(
        "CREATE TEMPORARY TABLE navree_time_events (id INTEGER PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL)",
    )
    .await
    .unwrap();
    let instants = [
        utc("2026-03-29T00:30:00Z"),
        utc("2026-03-29T23:30:00Z"),
        utc("2026-10-25T00:30:00Z"),
        utc("2026-10-26T18:29:59Z"),
        utc("2026-10-26T18:30:00Z"),
        utc("2026-11-01T05:30:00Z"),
    ];
    for (id, at) in instants.iter().enumerate() {
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO navree_time_events (id, created_at) VALUES ($1, $2)",
            [(id as i32).into(), (*at).into()],
        ))
        .await
        .unwrap();
    }
    for name in ["Europe/London", "Asia/Kolkata", "America/New_York", "Asia/Calcutta"] {
        let zone = Zone::parse(name).unwrap();
        let query = Query::select()
            .column("id")
            .expr_as(
                zone.sql(DbBackend::Postgres).local_date(Expr::col("created_at")),
                Alias::new("day"),
            )
            .from("navree_time_events")
            .order_by("id", sea_orm::sea_query::Order::Asc)
            .to_owned();
        let (text, values) = query.build(PostgresQueryBuilder);
        let rows = LocalDate::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            text,
            values,
        ))
        .all(&db)
        .await
        .unwrap();
        for row in rows {
            let at = instants[row.id as usize];
            assert_eq!(
                LocalDay::from(row.day),
                LocalDay::of(at, &zone),
                "{name} {at}"
            );
        }
    }
}
