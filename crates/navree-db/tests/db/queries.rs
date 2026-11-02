//! Watching statements: query logs, listeners, statement and transaction
//! hooks, slow units of work and the statistics of a unit of work.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_db::{Backend, Db, Executor, QueryLog, RepeatedQuery, Statement, Value};

use crate::support::{LogCapture, TempDb};

const INSERT: &str = "INSERT INTO people (name) VALUES (?)";

async fn people() -> (Db, TempDb) {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    db.pool()
        .execute_script("CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .await
        .unwrap();
    (db, temp)
}

fn insert(name: &str) -> Statement {
    Statement::portable(INSERT, [name.into()])
}

fn select() -> Statement {
    Statement::text("SELECT name FROM people")
}

#[tokio::test]
async fn query_logs_capture_the_statements_of_a_scope() {
    let (db, _temp) = people().await;
    let (inner_count, queries) = QueryLog::capture(async {
        db.pool().execute(&insert("ada")).await.unwrap();
        let (_, inner) = QueryLog::capture(async {
            assert!(QueryLog::is_capturing());
            db.pool().fetch_all(&select()).await.unwrap()
        })
        .await;
        inner.len()
    })
    .await;
    assert_eq!(inner_count, 1);
    assert_eq!(
        queries.len(),
        2,
        "the outer capture sees the inner one's statements"
    );
    assert_eq!(queries[0].sql, INSERT);
    assert_eq!(queries[0].values, [Value::from("ada")]);
    assert_eq!(
        queries[0].raw_sql(),
        "INSERT INTO people (name) VALUES ('ada')"
    );
    assert_eq!(&*queries[0].connection, "default");
    assert_eq!(queries[0].backend, Backend::Sqlite);
    assert!(!queries[0].failed);
    assert_eq!(queries[1].sql, "SELECT name FROM people");
    assert!(!QueryLog::is_capturing());

    let (_, failed) = QueryLog::capture(async {
        let missing = Statement::text("SELECT * FROM missing");
        db.pool().fetch_all(&missing).await.unwrap_err()
    })
    .await;
    assert!(failed[0].failed);
}

#[tokio::test]
async fn listeners_and_hooks_see_statements_and_transactions() {
    let (db, _temp) = people().await;
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = seen.clone();
    db.on_query(move |query| sink.lock().unwrap().push(query.raw_sql()));
    let transactions = Arc::new(AtomicUsize::new(0));
    let counter = transactions.clone();
    db.before_starting_transaction(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let executing = Arc::new(Mutex::new(Vec::<String>::new()));
    let hook = executing.clone();
    db.before_executing(move |sql| hook.lock().unwrap().push(sql.to_owned()));

    db.execute(&insert("ada")).await.unwrap();
    // Every handle on the pool shares the listeners.
    db.clone().execute(&insert("grace")).await.unwrap();
    db.transaction(|uow| async move {
        uow.execute(&insert("edsger")).await?;
        Ok(())
    })
    .await
    .unwrap();
    let inserts: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|sql| sql.starts_with("INSERT"))
        .cloned()
        .collect();
    assert_eq!(
        inserts,
        [
            "INSERT INTO people (name) VALUES ('ada')",
            "INSERT INTO people (name) VALUES ('grace')",
            "INSERT INTO people (name) VALUES ('edsger')",
        ],
        "statements in transactions too"
    );
    assert_eq!(transactions.load(Ordering::SeqCst), 1);
    // Statement hooks run for code that announces its statements.
    let announced: Vec<String> = executing
        .lock()
        .unwrap()
        .iter()
        .filter(|sql| *sql == "SELECT 1")
        .cloned()
        .collect();
    assert!(announced.is_empty());
    db.run_before_executing("SELECT 1");
    assert!(
        executing
            .lock()
            .unwrap()
            .iter()
            .any(|sql| sql == "SELECT 1")
    );
    db.run_before_starting_transaction();
    assert_eq!(transactions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn units_of_work_count_their_statements() {
    let (db, _temp) = people().await;
    let slow = Arc::new(Mutex::new(Vec::<(Option<String>, Duration)>::new()));
    let sink = slow.clone();
    db.when_querying_for_longer_than(Duration::ZERO, move |uow, total| {
        sink.lock()
            .unwrap()
            .push((uow.request_id().map(str::to_owned), total));
    });
    let uow = db.uow(None, Some("req-1".into())).await;
    uow.scope(async {
        for name in ["a", "b", "c"] {
            uow.execute(&insert(name)).await.unwrap();
        }
        uow.fetch_all(&select()).await.unwrap();
    })
    .await;
    let stats = uow.query_stats();
    assert!(stats.statements >= 4, "{stats:?}");
    assert_eq!(
        stats.repeated,
        [RepeatedQuery {
            sql: INSERT.into(),
            count: 3
        }]
    );
    assert!(stats.total > Duration::ZERO);
    assert_eq!(uow.total_query_duration(), stats.total);
    {
        let slow = slow.lock().unwrap();
        assert_eq!(slow.len(), 1, "once per unit of work");
        assert_eq!(slow[0].0.as_deref(), Some("req-1"));
    }
    uow.allow_duration_handlers_again();
    uow.scope(async {
        uow.execute(&insert("d")).await.unwrap();
    })
    .await;
    assert_eq!(slow.lock().unwrap().len(), 2, "allowed again");
    uow.reset_total_query_duration();
    assert_eq!(uow.total_query_duration(), Duration::ZERO);
    assert!(uow.query_stats().statements >= 5, "the count stays");
    uow.commit().await.unwrap();

    // Statements outside the unit of work's scope are not its own.
    let other = db.uow(None, None).await;
    db.pool().execute(&insert("e")).await.unwrap();
    assert_eq!(other.query_stats().statements, 0);
}

#[tokio::test]
async fn a_finished_unit_of_work_logs_its_statements_and_repeats() {
    let (db, _temp) = people().await;
    let logs = LogCapture::default();
    let _guard = logs.install();
    let uow = db.uow(None, Some("req-9".into())).await;
    uow.scope(async {
        for n in 0..10 {
            uow.execute(&insert(&format!("person {n}"))).await.unwrap();
        }
    })
    .await;
    uow.commit().await.unwrap();
    let text = logs.text();
    assert!(text.contains("unit of work queries"), "logs: {text}");
    assert!(
        text.contains("the same statement ran 10 times in one unit of work"),
        "logs: {text}"
    );
    assert!(text.contains("req-9"), "logs: {text}");
    assert!(!text.contains("person 3"), "values are not logged: {text}");
}

/// A pool of one connection, every statement logged as (sql, time, failed).
async fn one_connection() -> (Db, Arc<Mutex<Vec<(String, Duration, bool)>>>, TempDb) {
    let temp = TempDb::new();
    let db = Db::connect(&navree_db::DbOptions {
        url: temp.url.clone(),
        max_connections: 1,
        min_connections: 0,
        connect_timeout_secs: 1,
        ..navree_db::DbOptions::default()
    })
    .await
    .unwrap();
    let log: Arc<Mutex<Vec<(String, Duration, bool)>>> = Arc::default();
    let seen = log.clone();
    db.on_query(move |query| {
        seen.lock()
            .unwrap()
            .push((query.sql.clone(), query.duration, query.failed));
    });
    (db, log, temp)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_statement_that_gets_no_connection_is_reported_as_failed() {
    let (db, log, _temp) = one_connection().await;
    let tx = db.begin().await.unwrap();
    let error = db
        .fetch_all(&Statement::text("SELECT 3 AS timed_out"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    let reported = log.lock().unwrap().clone();
    let this = reported
        .iter()
        .find(|(sql, _, _)| sql.contains("timed_out"))
        .unwrap_or_else(|| panic!("reported: {reported:?}"));
    assert!(this.2, "failed: {this:?}");
    assert!(
        this.1 < Duration::from_millis(500),
        "it never ran: {this:?}"
    );
    tx.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_reports_the_time_it_ran_not_the_wait_for_a_connection() {
    use futures_util::StreamExt as _;
    let (db, log, _temp) = one_connection().await;
    let tx = db.begin().await.unwrap();
    let reader = db.clone();
    let waiter = tokio::spawn(async move {
        let mut rows = reader
            .fetch(&Statement::text("SELECT 2 AS streamed"))
            .await
            .unwrap();
        while rows.next().await.is_some() {}
    });
    tokio::time::sleep(Duration::from_millis(700)).await;
    tx.rollback().await.unwrap();
    waiter.await.unwrap();
    let reported = log.lock().unwrap().clone();
    let this = reported
        .iter()
        .find(|(sql, _, _)| sql.contains("streamed"))
        .unwrap_or_else(|| panic!("reported: {reported:?}"));
    assert!(this.1 < Duration::from_millis(500), "{this:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wait_for_a_connection_is_reported_beside_the_time_a_statement_ran() {
    use futures_util::StreamExt as _;
    let temp = TempDb::new();
    let db = Db::connect(&navree_db::DbOptions {
        url: temp.url.clone(),
        max_connections: 1,
        min_connections: 0,
        connect_timeout_secs: 5,
        ..navree_db::DbOptions::default()
    })
    .await
    .unwrap();
    let log: Arc<Mutex<Vec<(String, Duration, Duration)>>> = Arc::default();
    let seen = log.clone();
    db.on_query(move |query| {
        seen.lock()
            .unwrap()
            .push((query.sql.clone(), query.duration, query.wait));
    });
    let find = |needle: &str| {
        log.lock()
            .unwrap()
            .iter()
            .find(|(sql, _, _)| sql.contains(needle))
            .cloned()
            .unwrap_or_else(|| panic!("{needle} is not reported"))
    };
    let tx = db.begin().await.unwrap();
    let (one, two, three) = (db.clone(), db.clone(), db.clone());
    let fetched = tokio::spawn(async move {
        one.fetch_all(&Statement::text("SELECT 1 AS fetched"))
            .await
            .unwrap();
    });
    let streamed = tokio::spawn(async move {
        let mut rows = two
            .fetch(&Statement::text("SELECT 2 AS streamed"))
            .await
            .unwrap();
        while rows.next().await.is_some() {}
    });
    let begun = tokio::spawn(async move {
        three.begin().await.unwrap().rollback().await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(db.pool_stats().waiting, 3, "{:?}", db.pool_stats());
    assert_eq!(db.pool_stats().idle, 0);
    tx.rollback().await.unwrap();
    fetched.await.unwrap();
    streamed.await.unwrap();
    begun.await.unwrap();
    assert_eq!(db.pool_stats().waiting, 0);
    for needle in ["fetched", "streamed"] {
        let (_, ran, wait) = find(needle);
        assert!(
            wait >= Duration::from_millis(300),
            "{needle}: waited {wait:?}"
        );
        assert!(ran < wait, "{needle}: ran {ran:?}, waited {wait:?}");
    }
}
