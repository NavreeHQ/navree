//! Values bound and read back on every backend: every kind, typed NULLs,
//! typed reads and the dynamic `Row::value`.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use navree_db::{Backend, Executor, ExecutorExt, FromValue, Kind, Row, Statement, Value};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::support::{Scratch, script, serial};

/// The table every kind lands in, with each backend's column types.
fn create_table(backend: Backend) -> String {
    let (json, uuid, text, bytes, timestamp, datetime, boolean, double, decimal, time) =
        match backend {
            Backend::Postgres => (
                "JSONB",
                "UUID",
                "TEXT",
                "BYTEA",
                "TIMESTAMPTZ",
                "TIMESTAMP",
                "BOOLEAN",
                "DOUBLE PRECISION",
                "NUMERIC(12, 4)",
                "TIME",
            ),
            Backend::MySql => (
                "JSON",
                "BINARY(16)",
                "TEXT",
                "BLOB",
                "DATETIME(6)",
                "DATETIME(6)",
                "BOOLEAN",
                "DOUBLE",
                "DECIMAL(12, 4)",
                "TIME(6)",
            ),
            // Exact decimals are text on SQLite (a NUMERIC column would turn
            // them into floats).
            Backend::Sqlite => (
                "TEXT", "BLOB", "TEXT", "BLOB", "TEXT", "TEXT", "BOOLEAN", "REAL", "TEXT", "TEXT",
            ),
        };
    format!(
        "CREATE TABLE kinds (
            id {serial},
            b {boolean} NULL,
            small SMALLINT NULL,
            medium INTEGER NULL,
            big BIGINT NULL,
            approx {double} NULL,
            exact {decimal} NULL,
            words {text} NULL,
            bytes {bytes} NULL,
            doc {json} NULL,
            id_v7 {uuid} NULL,
            day DATE NULL,
            clock {time} NULL,
            local_at {datetime} NULL,
            at_utc {timestamp} NULL
        )",
        serial = serial(backend)
    )
}

const COLUMNS: &str =
    "b, small, medium, big, approx, exact, words, bytes, doc, id_v7, day, clock, local_at, at_utc";

fn sample() -> Vec<Value> {
    vec![
        Value::Bool(true),
        Value::I16(-12),
        Value::I32(70_000),
        Value::I64(9_000_000_000),
        Value::F64(2.5),
        Value::Decimal(Decimal::new(12_345_678, 4)),
        Value::String("Grüße, \"world\"; it's ✓".to_owned()),
        Value::Bytes(vec![0, 1, 254, 255]),
        Value::Json(serde_json::json!({"stage": "won", "tags": ["a", "b"], "n": 3})),
        Value::Uuid(Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_0042)),
        Value::Date(NaiveDate::from_ymd_opt(2026, 10, 28).unwrap()),
        Value::Time(NaiveTime::from_hms_micro_opt(9, 30, 15, 250_000).unwrap()),
        Value::DateTime(
            NaiveDateTime::parse_from_str("2026-10-28 09:30:15.123456", "%Y-%m-%d %H:%M:%S%.f")
                .unwrap(),
        ),
        Value::Timestamp(Utc.with_ymd_and_hms(2026, 10, 28, 9, 30, 15).unwrap()),
    ]
}

fn nulls() -> Vec<Value> {
    [
        Kind::Bool,
        Kind::I16,
        Kind::I32,
        Kind::I64,
        Kind::F64,
        Kind::Decimal,
        Kind::String,
        Kind::Bytes,
        Kind::Json,
        Kind::Uuid,
        Kind::Date,
        Kind::Time,
        Kind::DateTime,
        Kind::Timestamp,
    ]
    .into_iter()
    .map(Value::Null)
    .collect()
}

fn insert(values: Vec<Value>) -> Statement {
    let placeholders = vec!["?"; values.len()].join(", ");
    Statement::portable(
        format!("INSERT INTO kinds ({COLUMNS}) VALUES ({placeholders})"),
        values,
    )
}

/// Every kind goes in and comes back as the same Rust value.
pub async fn every_kind_round_trips(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    script(&db, &create_table(backend)).await;
    let done = db.execute(&insert(sample())).await.unwrap();
    assert_eq!(done.rows_affected, 1);
    db.execute(&insert(nulls())).await.unwrap();

    let rows = db
        .fetch_all(&Statement::text(format!(
            "SELECT {COLUMNS} FROM kinds ORDER BY id"
        )))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let row = &rows[0];
    assert!(row.get::<bool>("b").unwrap());
    assert_eq!(row.get::<i16>("small").unwrap(), -12);
    assert_eq!(row.get::<i64>("small").unwrap(), -12, "any width that fits");
    assert_eq!(row.get::<i32>("medium").unwrap(), 70_000);
    assert_eq!(row.get::<i64>("big").unwrap(), 9_000_000_000);
    assert!((row.get::<f64>("approx").unwrap() - 2.5).abs() < f64::EPSILON);
    assert_eq!(
        row.get::<Decimal>("exact").unwrap(),
        Decimal::new(12_345_678, 4)
    );
    assert_eq!(
        row.get::<String>("words").unwrap(),
        "Grüße, \"world\"; it's ✓"
    );
    assert_eq!(row.get::<Vec<u8>>("bytes").unwrap(), vec![0, 1, 254, 255]);
    assert_eq!(
        row.get::<serde_json::Value>("doc").unwrap(),
        serde_json::json!({"stage": "won", "tags": ["a", "b"], "n": 3})
    );
    assert_eq!(
        row.get::<Uuid>("id_v7").unwrap(),
        Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_0042)
    );
    assert_eq!(
        row.get::<NaiveDate>("day").unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 28).unwrap()
    );
    assert_eq!(
        row.get::<NaiveTime>("clock").unwrap(),
        NaiveTime::from_hms_micro_opt(9, 30, 15, 250_000).unwrap(),
        "{backend}"
    );
    assert_eq!(
        row.get::<NaiveDateTime>("local_at").unwrap().to_string(),
        "2026-10-28 09:30:15.123456"
    );
    assert_eq!(
        row.get::<DateTime<Utc>>("at_utc").unwrap(),
        Utc.with_ymd_and_hms(2026, 10, 28, 9, 30, 15).unwrap()
    );

    let row = &rows[1];
    for column in COLUMNS.split(", ") {
        assert!(
            row.value(column).unwrap().is_null(),
            "{column} on {backend}"
        );
        assert_eq!(row.get::<Option<String>>(column).unwrap(), None);
    }
    assert!(
        row.get::<i64>("big").is_err(),
        "NULL reads only as an Option"
    );
}

/// `Row::value` reads each column by its type; `to_json` gives the row.
pub async fn rows_read_dynamically(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    script(&db, &create_table(backend)).await;
    db.execute(&insert(sample())).await.unwrap();
    let row = db
        .fetch_one(&Statement::text(format!("SELECT {COLUMNS} FROM kinds")))
        .await
        .unwrap();
    assert_eq!(row.backend(), Some(backend));
    assert_eq!(row.len(), 14);
    assert_eq!(row.columns()[0], "b");
    match backend {
        Backend::Postgres => {
            assert_eq!(row.value("b").unwrap(), Value::Bool(true));
            assert_eq!(row.value("medium").unwrap(), Value::I32(70_000));
            assert_eq!(
                row.value("exact").unwrap(),
                Value::Decimal(Decimal::new(12_345_678, 4))
            );
            assert!(matches!(row.value("id_v7").unwrap(), Value::Uuid(_)));
            assert!(matches!(row.value("at_utc").unwrap(), Value::Timestamp(_)));
            assert!(matches!(row.value("doc").unwrap(), Value::Json(_)));
        }
        Backend::MySql => {
            assert_eq!(row.value("b").unwrap(), Value::I8(1));
            assert_eq!(row.value("medium").unwrap(), Value::I32(70_000));
            assert_eq!(
                row.value("exact").unwrap(),
                Value::Decimal(Decimal::new(12_345_678, 4))
            );
            assert!(matches!(row.value("id_v7").unwrap(), Value::Bytes(ref b) if b.len() == 16));
            assert!(matches!(row.value("doc").unwrap(), Value::Json(_)));
        }
        Backend::Sqlite => {
            assert_eq!(row.value("b").unwrap(), Value::I64(1));
            assert_eq!(row.value("medium").unwrap(), Value::I64(70_000));
            assert_eq!(
                row.value("exact").unwrap(),
                Value::String("1234.5678".to_owned())
            );
            assert!(matches!(row.value("id_v7").unwrap(), Value::Bytes(ref b) if b.len() == 16));
        }
    }
    let json = row.to_json().unwrap();
    assert_eq!(json["words"], serde_json::json!("Grüße, \"world\"; it's ✓"));
    assert_eq!(json["big"], serde_json::json!(9_000_000_000_i64));

    // Typed reads through ExecutorExt.
    let count: i64 = db
        .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM kinds"))
        .await
        .unwrap();
    assert_eq!(count, 1);
    let texts: Vec<String> = db
        .fetch_column(&Statement::text("SELECT words FROM kinds"))
        .await
        .unwrap();
    assert_eq!(texts, ["Grüße, \"world\"; it's ✓"]);
}

/// Expressions come back in each backend's types, and read as the Rust type
/// asked for: aggregates, booleans, date-times and JSON from text.
pub async fn expressions_read_tolerantly(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    script(&db, &create_table(backend)).await;
    for n in 1..=3_i64 {
        db.execute(&Statement::portable(
            "INSERT INTO kinds (big, exact) VALUES (?, ?)",
            [n.into(), Decimal::new(n * 150, 2).into()],
        ))
        .await
        .unwrap();
    }
    let row = db
        .fetch_one(&Statement::text(
            "SELECT COUNT(*) AS n, SUM(big) AS total, AVG(big) AS mean, MAX(exact) AS top, 1 = 1 AS yes FROM kinds",
        ))
        .await
        .unwrap();
    assert_eq!(row.get::<i64>("n").unwrap(), 3);
    assert_eq!(
        row.get::<i64>("total").unwrap(),
        6,
        "SUM is DECIMAL on MySQL and NUMERIC on PostgreSQL"
    );
    assert!((row.get::<f64>("mean").unwrap() - 2.0).abs() < 1e-9);
    assert_eq!(
        row.get::<Decimal>("top").unwrap().normalize(),
        Decimal::new(45, 1)
    );
    assert!(row.get::<bool>("yes").unwrap());

    // Bound values come back in the expression's type.
    let echoed = db
        .fetch_one(&Statement::portable(
            "SELECT ? AS a, ? AS b, ? AS c",
            [Value::I64(42), Value::from("x"), Value::Bool(false)],
        ))
        .await
        .unwrap();
    assert_eq!(echoed.get::<i64>("a").unwrap(), 42);
    assert_eq!(echoed.get::<String>("b").unwrap(), "x");
    assert!(!echoed.get::<bool>("c").unwrap());
}

/// A row built by hand reads like a database's (decoders shared).
pub fn hand_rows_share_the_decoders() {
    let row = Row::from_values([("at", Value::from("2026-10-28 09:30:00"))]);
    assert_eq!(
        row.get::<DateTime<FixedOffset>>("at").unwrap().to_rfc3339(),
        "2026-10-28T09:30:00+00:00"
    );
    assert!(<i64 as FromValue>::from_value(Value::Null(Kind::I64)).is_err());
}
