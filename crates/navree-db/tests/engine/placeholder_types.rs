//! One statement run again with a value of another type in a placeholder:
//! an integer, then a float; text, then an integer; a typed `NULL`, then a
//! value of another kind. Every run must answer as if it were the first,
//! on a lone connection, across pooled connections, inside transactions
//! and savepoints, and for every kind of value.
//!
//! A database that types a prepared statement's parameters (PostgreSQL)
//! or its result columns (MySQL) when it first runs must not apply those
//! types to a value of another type: the bytes of a float read as an
//! integer are another number, not an error.

use futures_util::StreamExt;
use navree_db::{Backend, Db, DbOptions, Executor, Kind, Statement, Value};

use crate::support::{Scratch, script};

/// Three measures: `(1, 1.0, 'a')`, `(2, 2.5, '7')`, `(3, 4.0, '12345678')`.
async fn measures(db: &impl Executor) {
    script(
        db,
        "CREATE TABLE measures (id BIGINT PRIMARY KEY, amount DOUBLE PRECISION NOT NULL, code VARCHAR(10) NOT NULL);
         INSERT INTO measures (id, amount, code) VALUES (1, 1.0, 'a'), (2, 2.5, '7'), (3, 4.0, '12345678')",
    )
    .await;
}

/// A pool of `connections` on the scratch database.
async fn pool_of(scratch: &Scratch, connections: u32) -> Db {
    Db::connect(&DbOptions {
        max_connections: connections,
        min_connections: 0,
        ..scratch.options.clone()
    })
    .await
    .expect("connects to the scratch database")
}

/// The ids `sql` (one portable placeholder) returns with `value` bound, or
/// the error, as text.
async fn ids(db: &impl Executor, sql: &str, value: Value) -> Result<Vec<i64>, String> {
    let rows = db
        .fetch_all(&Statement::portable(sql, [value]))
        .await
        .map_err(|error| error.to_string())?;
    rows.iter()
        .map(|row| row.get::<i64>("id").map_err(|error| error.to_string()))
        .collect()
}

/// Every run's outcome against what it should be, reported together.
#[derive(Default)]
struct Checks(Vec<String>);

impl Checks {
    fn expect(&mut self, case: &str, got: Result<Vec<i64>, String>, want: &[i64]) {
        if got.as_deref() != Ok(want) {
            self.0.push(format!("{case}: got {got:?}, want {want:?}"));
        }
    }

    fn finish(self) {
        assert!(
            self.0.is_empty(),
            "{} wrong:\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// `amount > ?` with an integer, then a float, and back.
const ABOVE: &str = "SELECT id FROM measures WHERE amount > ? ORDER BY id";
/// `amount >= ?` with a float, then an integer.
const AT_LEAST: &str = "SELECT id FROM measures WHERE amount >= ? ORDER BY id";
/// `amount < ?`.
const BELOW: &str = "SELECT id FROM measures WHERE amount < ? ORDER BY id";

/// `code` equal to the text of the bound value (text or a number).
fn code_is(backend: Backend, suffix: &str) -> String {
    let text = match backend {
        Backend::MySql => "CONCAT('', ?)",
        _ => "('' || ?)",
    };
    format!("SELECT id FROM measures WHERE code = {text}{suffix} ORDER BY id")
}

/// One connection, so every run reuses whatever the first prepared.
pub async fn changing_value_types_on_one_connection(scratch: Scratch) {
    let backend = scratch.server.backend();
    let db = pool_of(&scratch, 1).await;
    measures(&db).await;
    let mut checks = Checks::default();

    // An integer, then a float, then an integer again.
    checks.expect("I64 1", ids(&db, ABOVE, 1_i64.into()).await, &[2, 3]);
    let got = ids(&db, ABOVE, 3.5_f64.into()).await;
    checks.expect("F64 3.5 after I64", got, &[3]);
    checks.expect("I64 2", ids(&db, ABOVE, 2_i64.into()).await, &[2, 3]);
    // A float first, then integers and a 32-bit float.
    checks.expect("F64 2.5", ids(&db, AT_LEAST, 2.5_f64.into()).await, &[2, 3]);
    let got = ids(&db, AT_LEAST, 3_i64.into()).await;
    checks.expect("I64 3 after F64", got, &[3]);
    let got = ids(&db, AT_LEAST, 2.5_f32.into()).await;
    checks.expect("F32 2.5 after I64", got, &[2, 3]);
    let got = ids(&db, AT_LEAST, 4_i32.into()).await;
    checks.expect("I32 4 after F32", got, &[3]);
    let got = ids(&db, AT_LEAST, Value::Decimal("2.5".parse().unwrap())).await;
    checks.expect("Decimal 2.5 after I32", got, &[2, 3]);

    // Text, then an integer.
    let text_first = code_is(backend, "");
    checks.expect("text 'a'", ids(&db, &text_first, "a".into()).await, &[1]);
    let got = ids(&db, &text_first, 7_i64.into()).await;
    checks.expect("I64 7 after text", got, &[2]);
    // An integer, then text (one of eight bytes, the size of a BIGINT).
    let integer_first = code_is(backend, " AND id > 0");
    checks.expect("I64 7", ids(&db, &integer_first, 7_i64.into()).await, &[2]);
    let got = ids(&db, &integer_first, "12345678".into()).await;
    checks.expect("8-byte text after I64", got, &[3]);
    let got = ids(&db, &integer_first, "a".into()).await;
    checks.expect("1-byte text after I64", got, &[1]);

    // An unsigned integer that fits a BIGINT, then one that does not
    // (PostgreSQL binds it as NUMERIC; SQLite has no such integer), then a
    // small one again.
    if backend != Backend::Sqlite {
        checks.expect("U64 2", ids(&db, BELOW, 2_u64.into()).await, &[1]);
        let got = ids(&db, BELOW, u64::MAX.into()).await;
        checks.expect("U64 MAX after U64 2", got, &[1, 2, 3]);
        let got = ids(&db, BELOW, 3_u64.into()).await;
        checks.expect("U64 3 after U64 MAX", got, &[1, 2]);
    }
    checks.finish();
}

/// A typed `NULL` first, then a value of another kind.
pub async fn typed_nulls_then_other_kinds(scratch: Scratch) {
    let backend = scratch.server.backend();
    let db = pool_of(&scratch, 1).await;
    measures(&db).await;
    let mut checks = Checks::default();

    let above_or_zero = "SELECT id FROM measures WHERE amount > COALESCE(?, 0) ORDER BY id";
    let got = ids(&db, above_or_zero, Value::Null(Kind::I64)).await;
    checks.expect("I64 NULL", got, &[1, 2, 3]);
    let got = ids(&db, above_or_zero, 2.0_f64.into()).await;
    checks.expect("F64 2.0 after I64 NULL", got, &[2, 3]);

    let float_null = "SELECT id FROM measures WHERE amount > COALESCE(?, 0) AND id > 0 ORDER BY id";
    let got = ids(&db, float_null, Value::Null(Kind::F64)).await;
    checks.expect("F64 NULL", got, &[1, 2, 3]);
    let got = ids(&db, float_null, 2_i64.into()).await;
    checks.expect("I64 2 after F64 NULL", got, &[2, 3]);

    let text = match backend {
        Backend::MySql => "COALESCE(CONCAT('', ?), 'a')",
        _ => "COALESCE('' || ?, 'a')",
    };
    let text_null = format!("SELECT id FROM measures WHERE code = {text} ORDER BY id");
    let got = ids(&db, &text_null, Value::Null(Kind::String)).await;
    checks.expect("text NULL", got, &[1]);
    let got = ids(&db, &text_null, 7_i64.into()).await;
    checks.expect("I64 7 after text NULL", got, &[2]);
    let got = ids(&db, &text_null, Value::Null(Kind::Uuid)).await;
    checks.expect("UUID NULL after I64", got, &[1]);

    checks.finish();
}

/// Every pooled connection, transactions, savepoints and streamed rows.
pub async fn changing_value_types_across_connections_and_transactions(scratch: Scratch) {
    let connections = 3;
    let db = pool_of(&scratch, connections).await;
    measures(&db).await;
    let mut checks = Checks::default();

    // Each pooled connection runs the statement with an integer (three
    // transactions at once hold three connections).
    let mut warm = Vec::new();
    for _ in 0..connections {
        let tx = db.begin().await.unwrap();
        checks.expect(
            "I64 1 to warm",
            ids(&tx, ABOVE, 1_i64.into()).await,
            &[2, 3],
        );
        warm.push(tx);
    }
    for tx in warm {
        tx.commit().await.unwrap();
    }
    // Then the pool runs it with a float, on whichever connection.
    for run in 0..connections * 2 {
        let got = ids(&db, ABOVE, 3.5_f64.into()).await;
        checks.expect(&format!("pool: F64 3.5 after I64 (run {run})"), got, &[3]);
    }
    let concurrent = futures_util::future::join_all(
        (0..connections * 2).map(|_| ids(&db, ABOVE, 2.0_f64.into())),
    )
    .await;
    for (run, got) in concurrent.into_iter().enumerate() {
        checks.expect(&format!("pool: concurrent F64 2.0 ({run})"), got, &[2, 3]);
    }

    // Inside a transaction.
    let tx = db.begin().await.unwrap();
    let got = ids(&tx, AT_LEAST, 2.5_f64.into()).await;
    checks.expect("transaction: F64 2.5", got, &[2, 3]);
    let got = ids(&tx, AT_LEAST, 3_i64.into()).await;
    checks.expect("transaction: I64 3 after F64", got, &[3]);
    // In a savepoint, then in the transaction after it is released.
    let savepoint = tx.savepoint().await.unwrap();
    let got = ids(&savepoint, BELOW, 2_i64.into()).await;
    checks.expect("savepoint: I64 2", got, &[1]);
    let got = ids(&savepoint, BELOW, 3.5_f64.into()).await;
    checks.expect("savepoint: F64 3.5 after I64", got, &[1, 2]);
    savepoint.commit().await.unwrap();
    let got = ids(&tx, BELOW, Value::Null(Kind::I64)).await;
    checks.expect("transaction: I64 NULL after F64", got, &[]);
    let got = ids(&tx, BELOW, 2.6_f64.into()).await;
    checks.expect("transaction: F64 2.6 after I64 NULL", got, &[1, 2]);
    // Rows streamed from the transaction's connection.
    checks.expect(
        "transaction stream: I64 4 after F64",
        streamed(&tx, BELOW, 4_i64.into()).await,
        &[1, 2],
    );
    tx.commit().await.unwrap();

    // Streamed from the pool after the transaction's float.
    for run in 0..connections {
        checks.expect(
            &format!("pool stream: F64 2.5 after I64 ({run})"),
            streamed(&db, AT_LEAST, 2.5_f64.into()).await,
            &[2, 3],
        );
    }
    checks.finish();
}

/// The ids `sql` streams with `value` bound.
async fn streamed(db: &impl Executor, sql: &str, value: Value) -> Result<Vec<i64>, String> {
    let statement = Statement::portable(sql, [value]);
    let mut rows = db
        .fetch(&statement)
        .await
        .map_err(|error| error.to_string())?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await {
        let row = row.map_err(|error| error.to_string())?;
        ids.push(row.get::<i64>("id").map_err(|error| error.to_string())?);
    }
    Ok(ids)
}

/// One value of every kind, and a `NULL` of every kind.
fn every_kind() -> Vec<Value> {
    use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};

    let date = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
    let time = NaiveTime::from_hms_opt(13, 45, 30).unwrap();
    let mut values = vec![
        Value::Bool(true),
        Value::I8(-8),
        Value::I16(1_600),
        Value::I32(320_000),
        Value::I64(6_400_000_000),
        Value::U8(8),
        Value::U16(16_000),
        Value::U32(3_200_000_000),
        Value::U64(64),
        Value::U64(u64::MAX),
        Value::F32(0.5),
        Value::F64(2.25),
        Value::Decimal("12.34".parse().unwrap()),
        Value::String("12345678".into()),
        Value::Bytes(b"\x01\x02\x03\x04\x05\x06\x07\x08".to_vec()),
        Value::Json(serde_json::json!({"n": 1})),
        Value::Uuid(uuid::Uuid::from_u128(
            0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
        )),
        Value::Date(date),
        Value::Time(time),
        Value::DateTime(date.and_time(time)),
        Value::Timestamp(Utc.from_utc_datetime(&date.and_time(time))),
        Value::TimestampTz(
            FixedOffset::east_opt(2 * 3600)
                .unwrap()
                .from_local_datetime(&date.and_time(time))
                .unwrap(),
        ),
        Value::Enum {
            type_name: "mood".into(),
            value: "happy".into(),
        },
        Value::Vector(vec![1.0, 2.0]),
    ];
    let kinds = [
        Kind::Bool,
        Kind::I8,
        Kind::I16,
        Kind::I32,
        Kind::I64,
        Kind::U8,
        Kind::U16,
        Kind::U32,
        Kind::U64,
        Kind::F32,
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
        Kind::TimestampTz,
        Kind::Enum,
        Kind::Vector,
    ];
    values.extend(kinds.into_iter().map(Value::Null));
    values
}

/// What one run of `sql` returned: the value of its one column, or that
/// it failed.
///
/// On MySQL a `NULL` is any `NULL`: the kind it reads back as is the
/// result column's type, which MySQL derives for `SELECT ?` from the
/// values a statement saw (text for a statement first run with a `NULL`,
/// `BIGINT` for one that ran with an integer of the same type before).
/// The value is `NULL` either way; keying statements by which values are
/// `NULL` too would multiply the statements of every insert.
async fn outcome(db: &impl Executor, sql: &str, value: &Value) -> String {
    match db
        .fetch_one(&Statement::portable(sql, [value.clone()]))
        .await
    {
        Ok(row) => match row.value(0) {
            Ok(Value::Null(_)) if db.backend() == Backend::MySql => "Ok(NULL)".to_owned(),
            read => format!("{read:?}"),
        },
        Err(error) => format!("error {:?} {}", error.kind(), error.code().unwrap_or("")),
    }
}

/// For each of `sqls`: every value after every other value on one
/// connection answers as its first run did.
async fn every_kind_after_every_other_on(db: &Db, values: &[Value], sqls: &[String]) {
    for sql in sqls {
        // The first run of each value: a statement text of its own.
        let mut first = Vec::new();
        for (at, value) in values.iter().enumerate() {
            first.push(outcome(db, &format!("{sql} -- first run {at}\n"), value).await);
        }
        let mut wrong = Vec::new();
        // Values first, then the NULLs; and the other way round, in a
        // statement of its own (ending with a line comment).
        let reversed = format!("{sql} -- NULLs first\n");
        for (text, order) in [(sql, false), (&reversed, true)] {
            let mut earlier_ones: Vec<&Value> = values.iter().collect();
            if order {
                earlier_ones.reverse();
            }
            for earlier in earlier_ones {
                for (at, value) in values.iter().enumerate() {
                    let _ = outcome(db, text, earlier).await;
                    let again = outcome(db, text, value).await;
                    if again != first[at] {
                        wrong.push(format!(
                            "{value:?} after {earlier:?}: {again}, first run: {}",
                            first[at]
                        ));
                    }
                }
            }
        }
        let silent = wrong
            .iter()
            .filter(|line| !line.contains(": error"))
            .count();
        assert!(
            wrong.is_empty(),
            "`{sql}`: {} of {} runs differ from a first run ({silent} with a wrong value, the others with an error); the first 40, then those of values:\n{}\n{}",
            wrong.len(),
            2 * values.len() * values.len(),
            wrong[..wrong.len().min(40)].join("\n"),
            wrong
                .iter()
                .filter(|line| !line.starts_with("Null("))
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// Every kind of value after every other in one placeholder, read back
/// as itself and through a cast to text.
pub async fn every_kind_after_every_other(scratch: Scratch) {
    let backend = scratch.server.backend();
    let db = pool_of(&scratch, 1).await;
    let mut values = every_kind();
    // pgvector's `vector` on PostgreSQL when the server has the extension
    // (bytes elsewhere).
    if backend == Backend::Postgres
        && db
            .execute_script("CREATE EXTENSION IF NOT EXISTS vector")
            .await
            .is_err()
    {
        values.retain(|value| !matches!(value, Value::Vector(_) | Value::Null(Kind::Vector)));
    }
    let cast = match backend {
        Backend::MySql => "SELECT CAST(? AS CHAR) AS v",
        _ => "SELECT CAST(? AS TEXT) AS v",
    };
    let sqls = [cast.to_owned(), "SELECT ? AS v".to_owned()];
    every_kind_after_every_other_on(&db, &values, &sqls).await;
}

/// Query logs, listeners and errors show the SQL as written; the server
/// sees it followed by the types its values bind (PostgreSQL and MySQL),
/// placed before a trailing `;`.
pub async fn logs_show_the_sql_as_written(scratch: Scratch) {
    let backend = scratch.server.backend();
    let db = pool_of(&scratch, 1).await;
    measures(&db).await;
    let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let listener = heard.clone();
    db.on_query(move |query| listener.lock().unwrap().push(query.sql.clone()));

    let written = match backend {
        Backend::Postgres => {
            "SELECT query AS seen FROM pg_stat_activity WHERE pid = pg_backend_pid() AND ? > 0;"
        }
        Backend::MySql => {
            "SELECT INFO AS seen FROM information_schema.PROCESSLIST WHERE ID = CONNECTION_ID() AND ? > 0;"
        }
        Backend::Sqlite => "SELECT 'not kept' AS seen WHERE ? > 0;",
    };
    let statement = Statement::portable(written, [1_i64.into()]);
    let ((), logged) = navree_db::QueryLog::capture(async {
        let row = db.fetch_one(&statement).await.unwrap();
        let seen: String = row.get("seen").unwrap();
        let expected = match backend {
            Backend::Postgres => format!(
                "{} /* params: INT8 */;",
                written.trim_end_matches(';').replace("? > 0", "$1 > 0")
            ),
            // MySQL shows a prepared statement with its values in place,
            // without the `;`.
            Backend::MySql => format!(
                "{} /* params: BIGINT */",
                written.trim_end_matches(';').replace("? > 0", "1 > 0")
            ),
            Backend::Sqlite => "not kept".to_owned(),
        };
        assert_eq!(seen, expected, "what the server sees");
    })
    .await;
    let as_run = statement.for_backend(backend).unwrap().sql().to_owned();
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].sql, as_run, "the query log");
    assert_eq!(
        heard.lock().unwrap().as_slice(),
        [as_run.clone()],
        "listeners"
    );

    // A failure names the SQL as written too.
    let missing = Statement::portable("SELECT id FROM nowhere WHERE id = ?", [1_i64.into()]);
    let ((), logged) = navree_db::QueryLog::capture(async {
        assert!(db.fetch_all(&missing).await.is_err());
    })
    .await;
    assert!(logged[0].failed);
    assert!(!logged[0].sql.contains("params:"), "{}", logged[0].sql);

    // Statements without values, and a NULL, run as written and keyed.
    let rows = ids(
        &db,
        "SELECT id FROM measures WHERE id = COALESCE(?, 2);",
        Value::Null(Kind::I64),
    )
    .await;
    assert_eq!(rows, Ok(vec![2]));
    let count = db
        .fetch_all(&Statement::text("SELECT id FROM measures ORDER BY id;\n"))
        .await
        .unwrap();
    assert_eq!(count.len(), 3);
}

/// How long repeated statements with stable value types take: a query
/// with two placeholders, one with eight, and an insert, each run
/// 2,000 times on one connection (the median of three rounds, in
/// microseconds per statement). Printed, not asserted:
/// `cargo test -p navree-db --features postgres,mysql --test engine -- --ignored stable_types_timing --nocapture`.
pub async fn stable_types_timing(scratch: Scratch) {
    let backend = scratch.server.backend();
    let db = pool_of(&scratch, 1).await;
    measures(&db).await;
    script(
        &db,
        "CREATE TABLE timed (id BIGINT NOT NULL, amount DOUBLE PRECISION NOT NULL, code VARCHAR(10) NOT NULL)",
    )
    .await;
    let runs = 2_000_u32;
    let two = Statement::portable(
        "SELECT id FROM measures WHERE amount > ? AND code <> ? ORDER BY id",
        [1.5_f64.into(), "zz".into()],
    );
    let eight = Statement::portable(
        "SELECT id FROM measures WHERE id IN (?, ?, ?, ?) AND amount BETWEEN ? AND ? AND code <> ? AND code <> ? ORDER BY id",
        [
            1_i64.into(),
            2_i64.into(),
            3_i64.into(),
            4_i64.into(),
            0.5_f64.into(),
            9.5_f64.into(),
            "x".into(),
            "y".into(),
        ],
    );
    let insert = Statement::portable(
        "INSERT INTO timed (id, amount, code) VALUES (?, ?, ?)",
        [7_i64.into(), 2.5_f64.into(), "ins".into()],
    );
    for (name, statement, read) in [
        ("2 params", &two, true),
        ("8 params", &eight, true),
        ("insert", &insert, false),
    ] {
        let mut rounds = Vec::new();
        for _ in 0..3 {
            let started = std::time::Instant::now();
            for _ in 0..runs {
                if read {
                    db.fetch_all(statement).await.unwrap();
                } else {
                    db.execute(statement).await.unwrap();
                }
            }
            rounds.push(started.elapsed().as_secs_f64() * 1e6 / f64::from(runs));
        }
        rounds.sort_by(f64::total_cmp);
        eprintln!(
            "stable_types_timing {backend:?} {name}: {:.1} us per statement (rounds {:.1} / {:.1} / {:.1})",
            rounds[1], rounds[0], rounds[1], rounds[2]
        );
    }
}
