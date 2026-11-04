//! A report's figures from the database agree with the same rows counted in memory,
//! on SQLite, PostgreSQL (with TimescaleDB's buckets and with bound comparisons),
//! MySQL and MongoDB, in zones with clock changes and half-hour offsets, at every
//! grain. Each store's test needs its URL in the environment
//! (`NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL`, `NAVREE_TEST_MONGODB_URL`) and
//! uses its own database, dropped afterwards.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::mongo::{MongoDb, MongoOptions};
use navree::orm::{Builder, Executor, Model, Value, col};
use navree::ui::charts::{Bucketing, Grain, Metric, Report, TimeColumn};
use navree::ui::dates::DateRange;

macro_rules! event {
    ($name:ident, $($attrs:tt)*) => {
        #[derive(Model, Clone, Debug, Default, serde::Serialize)]
        #[model($($attrs)*)]
        pub struct $name {
            pub id: i64,
            pub kind: String,
            pub amount: i64,
            pub at: DateTime<Utc>,
            pub at_s: i64,
        }
    };
}

event!(SqlEvent, table = "rp_events");
event!(MongoEvent, collection = "rp_events", driver = "mongodb");

const KINDS: [&str; 4] = ["opened", "clicked", "reported", "bounced"];

/// Rows from 2025-10-20 to 2026-11-10, closer together around 1 November 2026 (New
/// York's clock goes back that night) and on the very edges of periods.
fn rows() -> Vec<(i64, &'static str, i64, DateTime<Utc>)> {
    let mut out = Vec::new();
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let start = Utc.with_ymd_and_hms(2025, 10, 20, 0, 0, 0).unwrap();
    let mut at = start;
    let mut id = 0;
    while at < Utc.with_ymd_and_hms(2026, 11, 10, 0, 0, 0).unwrap() {
        id += 1;
        let r = next();
        out.push((id, KINDS[(r % 4) as usize], (r % 997) as i64 - 100, at));
        let dense = (Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).unwrap()
            ..Utc.with_ymd_and_hms(2026, 11, 4, 0, 0, 0).unwrap())
            .contains(&at);
        at += Duration::seconds(if dense {
            600 + (r % 1800) as i64
        } else {
            9_000 + (r % 40_000) as i64
        });
    }
    // The edges of periods: a local midnight to the second, and the second before it.
    for zone in ["America/New_York", "Asia/Kolkata", "UTC"] {
        let tz: Tz = zone.parse().unwrap();
        for day in [
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
        ] {
            let (midnight, _) = DateRange::single(day).utc_bounds(&tz);
            for at in [
                midnight,
                midnight - Duration::seconds(1),
                midnight + Duration::seconds(1),
            ] {
                id += 1;
                out.push((id, "edge", 7, at));
            }
        }
    }
    out
}

fn sql_rows() -> Vec<SqlEvent> {
    rows()
        .into_iter()
        .map(|(id, kind, amount, at)| SqlEvent {
            id,
            kind: kind.to_owned(),
            amount,
            at,
            at_s: at.timestamp(),
        })
        .collect()
}

fn mongo_rows() -> Vec<MongoEvent> {
    rows()
        .into_iter()
        .map(|(id, kind, amount, at)| MongoEvent {
            id,
            kind: kind.to_owned(),
            amount,
            at,
            at_s: at.timestamp(),
        })
        .collect()
}

/// The reports every store is asked: zones, windows and grains.
fn reports() -> Vec<(String, Report)> {
    let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
    let mut out = Vec::new();
    for zone in ["America/New_York", "Asia/Kolkata", "UTC"] {
        let tz: Tz = zone.parse().unwrap();
        for (range, grain) in [
            (
                DateRange::new(day(2026, 10, 31), day(2026, 11, 1)),
                Grain::Hour,
            ),
            (
                DateRange::new(day(2026, 10, 25), day(2026, 11, 8)),
                Grain::Day,
            ),
            (
                DateRange::new(day(2026, 8, 5), day(2026, 11, 8)),
                Grain::Week,
            ),
            (
                DateRange::new(day(2025, 11, 1), day(2026, 11, 8)),
                Grain::Month,
            ),
            (
                DateRange::new(day(2025, 11, 1), day(2026, 11, 8)),
                Grain::Quarter,
            ),
        ] {
            out.push((
                format!("{zone} {grain:?}"),
                Report::new(range, grain, &tz).zone_name(zone),
            ));
        }
    }
    out
}

/// The same figures from memory: what the database must answer.
fn expected(report: &Report, metric: Metric, only: Option<&str>) -> Vec<Option<f64>> {
    let mut groups: Vec<Vec<f64>> = vec![Vec::new(); report.buckets().len()];
    let mut kinds: Vec<std::collections::BTreeSet<&str>> =
        vec![Default::default(); report.buckets().len()];
    for (_, kind, amount, at) in rows() {
        if only.is_some_and(|k| k != kind) {
            continue;
        }
        if let Some(i) = report.bucket_of(at) {
            groups[i].push(amount as f64);
            kinds[i].insert(kind);
        }
    }
    groups
        .iter()
        .zip(kinds)
        .map(|(values, kinds)| match metric {
            Metric::Count => Some(values.len() as f64),
            Metric::CountDistinct(_) => Some(kinds.len() as f64),
            Metric::Sum(_) => Some(values.iter().sum()),
            Metric::Average(_) => {
                (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
            }
            Metric::Min(_) => values.iter().copied().reduce(f64::min),
            Metric::Max(_) => values.iter().copied().reduce(f64::max),
            _ => unreachable!(),
        })
        .collect()
}

fn close(a: &[Option<f64>], b: &[Option<f64>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= 1e-6 * a.abs().max(1.0),
            (None, None) => true,
            _ => false,
        })
}

async fn check<M: Model, E: Executor + ?Sized>(
    store: &str,
    exec: &E,
    query: impl Fn() -> Builder<M>,
    bucketings: &[Bucketing],
    sql: bool,
) {
    let mut metrics = vec![
        Metric::Count,
        Metric::Sum("amount"),
        Metric::Average("amount"),
        Metric::Min("amount"),
        Metric::Max("amount"),
    ];
    if sql {
        metrics.push(Metric::CountDistinct("kind"));
    }
    for (name, report) in reports() {
        for bucketing in bucketings {
            for metric in &metrics {
                let found = report
                    .measure_with(query(), "at", *metric, *bucketing, exec)
                    .await
                    .unwrap_or_else(|e| panic!("{store} {name} {bucketing:?} {metric:?}: {e}"));
                let want = expected(&report, *metric, None);
                assert!(
                    close(&found, &want),
                    "{store} {name} {bucketing:?} {metric:?}:\n{found:?}\n{want:?}"
                );
            }
            // The app's own query is kept: a filtered count counts only its rows.
            let found = report
                .measure_with(
                    query().where_(col("kind").eq(Value::String("clicked".into()))),
                    "at",
                    Metric::Count,
                    *bucketing,
                    exec,
                )
                .await
                .unwrap();
            assert!(
                close(&found, &expected(&report, Metric::Count, Some("clicked"))),
                "{store} {name} scoped"
            );
        }
        if sql {
            // Seconds since 1970 in an integer column count the same.
            let found = report
                .measure(
                    query(),
                    TimeColumn::UnixSeconds("at_s"),
                    Metric::Count,
                    exec,
                )
                .await
                .unwrap();
            assert!(
                close(&found, &expected(&report, Metric::Count, None)),
                "{store} {name} seconds"
            );
        }
        // One series per kind, and Other for the rest.
        let by = report
            .measure_by(
                query(),
                "at",
                "kind",
                &["opened", "clicked"],
                Metric::Count,
                exec,
            )
            .await
            .unwrap();
        assert_eq!(by.len(), 3);
        assert!(
            close(&by[0], &expected(&report, Metric::Count, Some("opened"))),
            "{store} {name} by kind"
        );
        let total = expected(&report, Metric::Count, None);
        for i in 0..total.len() {
            let sum: f64 = by.iter().map(|s| s[i].unwrap_or(0.0)).sum();
            assert_eq!(
                Some(sum),
                total[i],
                "{store} {name}: the groups add up to the whole in period {i}"
            );
        }
    }
}

fn env_url(name: &str) -> Option<String> {
    let url = std::env::var(name).ok().filter(|url| !url.is_empty());
    if url.is_none() {
        eprintln!("skipped: {name} isn't set");
    }
    url
}

fn own_database(url: &str, name: &str) -> String {
    let (base, rest) = url.rsplit_once('/').expect("a URL with a database");
    let query = rest.split_once('?').map_or("", |(_, query)| query);
    if query.is_empty() {
        format!("{base}/{name}")
    } else {
        format!("{base}/{name}?{query}")
    }
}

async fn connect(url: String) -> Db {
    Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .expect("connects")
}

async fn seed(db: &Db, schema: &str) {
    let _ = db
        .pool()
        .execute_script("DROP TABLE IF EXISTS rp_events")
        .await;
    db.pool().execute_script(schema).await.expect("the table");
    for chunk in sql_rows().chunks(500) {
        SqlEvent::query()
            .insert(db, chunk.to_vec())
            .await
            .expect("the rows");
    }
}

#[tokio::test]
async fn sqlite_counts_like_memory() {
    let dir = std::env::temp_dir().join(format!("ui-reports-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = connect(format!("sqlite://{}?mode=rwc", dir.join("r.db").display())).await;
    seed(
        &db,
        "CREATE TABLE rp_events (id BIGINT PRIMARY KEY, kind TEXT NOT NULL, amount BIGINT NOT NULL, at TEXT NOT NULL, at_s BIGINT NOT NULL)",
    )
    .await;
    check(
        "sqlite",
        &db,
        SqlEvent::query,
        &[Bucketing::Auto, Bucketing::Exact],
        true,
    )
    .await;
    // TimescaleDB's buckets are PostgreSQL's: asking for them elsewhere says so.
    let report = &reports()[0].1;
    let error = report
        .measure_with(
            SqlEvent::query(),
            "at",
            Metric::Count,
            Bucketing::TimeBucket,
            &db,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("TimescaleDB"), "{error}");
    // A column name that isn't a plain name never reaches the SQL.
    let error = report
        .measure(
            SqlEvent::query(),
            "at; DROP TABLE rp_events",
            Metric::Count,
            &db,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("plain names"), "{error}");
    db.close().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn postgres_counts_like_memory_with_and_without_timescaledb() {
    let Some(url) = env_url("NAVREE_TEST_POSTGRES_URL") else {
        return;
    };
    let admin = connect(url.clone()).await;
    let _ = admin
        .pool()
        .execute_script("DROP DATABASE IF EXISTS ui_reports WITH (FORCE)")
        .await;
    admin
        .pool()
        .execute_script("CREATE DATABASE ui_reports")
        .await
        .unwrap();
    {
        let db = connect(own_database(&url, "ui_reports")).await;
        let timescale = db
            .pool()
            .execute_script("CREATE EXTENSION IF NOT EXISTS timescaledb")
            .await
            .is_ok();
        seed(
            &db,
            "CREATE TABLE rp_events (id BIGINT PRIMARY KEY, kind VARCHAR(20) NOT NULL, amount BIGINT NOT NULL, at TIMESTAMPTZ NOT NULL, at_s BIGINT NOT NULL)",
        )
        .await;
        let mut bucketings = vec![Bucketing::Exact, Bucketing::Auto];
        if timescale {
            bucketings.push(Bucketing::TimeBucket);
        } else {
            eprintln!("TimescaleDB is not installable here: bound comparisons only");
        }
        check("postgres", &db, SqlEvent::query, &bucketings, true).await;
        db.close().await.unwrap();
    }
    admin
        .pool()
        .execute_script("DROP DATABASE ui_reports WITH (FORCE)")
        .await
        .unwrap();
}

#[tokio::test]
async fn mysql_counts_like_memory() {
    let Some(url) = env_url("NAVREE_TEST_MYSQL_URL") else {
        return;
    };
    let admin = connect(url.clone()).await;
    let _ = admin
        .pool()
        .execute_script("DROP DATABASE IF EXISTS ui_reports")
        .await;
    admin
        .pool()
        .execute_script("CREATE DATABASE ui_reports CHARACTER SET utf8mb4")
        .await
        .unwrap();
    {
        let db = connect(own_database(&url, "ui_reports")).await;
        seed(
            &db,
            "CREATE TABLE rp_events (id BIGINT PRIMARY KEY, kind VARCHAR(20) NOT NULL, amount BIGINT NOT NULL, at DATETIME(6) NOT NULL, at_s BIGINT NOT NULL)",
        )
        .await;
        check(
            "mysql",
            &db,
            SqlEvent::query,
            &[Bucketing::Exact, Bucketing::Auto],
            true,
        )
        .await;
        db.close().await.unwrap();
    }
    admin
        .pool()
        .execute_script("DROP DATABASE ui_reports")
        .await
        .unwrap();
}

#[tokio::test]
async fn mongodb_counts_like_memory() {
    let Some(url) = env_url("NAVREE_TEST_MONGODB_URL") else {
        return;
    };
    let mongo = MongoDb::connect(&MongoOptions::new(own_database(&url, "ui_reports")))
        .await
        .unwrap();
    MongoEvent::query().delete(&mongo).await.unwrap();
    for chunk in mongo_rows().chunks(500) {
        MongoEvent::query()
            .insert(&mongo, chunk.to_vec())
            .await
            .unwrap();
    }
    check(
        "mongodb",
        &mongo,
        MongoEvent::query,
        &[Bucketing::Auto],
        false,
    )
    .await;
    mongo.database().drop().await.unwrap();
}
