//! Watching statements: query logs, listeners, statement and transaction
//! hooks, slow units of work and the statistics of a unit of work.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_db::{Db, QueryLog, RepeatedQuery};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::support::{LogCapture, TempDb};

const INSERT: &str = "INSERT INTO people (name) VALUES (?)";

async fn people() -> (Db, TempDb) {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    db.conn()
        .execute_unprepared("CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .await
        .unwrap();
    (db, temp)
}

fn insert(name: &str) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, INSERT, [name.into()])
}

fn select() -> Statement {
    Statement::from_string(DbBackend::Sqlite, "SELECT name FROM people")
}

#[tokio::test]
async fn query_logs_capture_the_statements_of_a_scope() {
    let (db, _temp) = people().await;
    let (inner_count, queries) = QueryLog::capture(async {
        db.conn().execute_raw(insert("ada")).await.unwrap();
        let (_, inner) = QueryLog::capture(async {
            assert!(QueryLog::is_capturing());
            db.conn().query_all_raw(select()).await.unwrap()
        })
        .await;
        inner.len()
    })
    .await;
    assert_eq!(inner_count, 1);
    assert_eq!(queries.len(), 2, "the outer capture sees the inner one's statements");
    assert_eq!(queries[0].sql, INSERT);
    assert_eq!(queries[0].values, [sea_orm::Value::from("ada")]);
    assert_eq!(
        queries[0].raw_sql(),
        "INSERT INTO people (name) VALUES ('ada')"
    );
    assert_eq!(&*queries[0].connection, "default");
    assert_eq!(queries[0].backend, DbBackend::Sqlite);
    assert!(!queries[0].failed);
    assert_eq!(queries[1].sql, "SELECT name FROM people");
    assert!(!QueryLog::is_capturing());

    let (_, failed) = QueryLog::capture(async {
        let missing = Statement::from_string(DbBackend::Sqlite, "SELECT * FROM missing");
        db.conn().query_all_raw(missing).await.unwrap_err()
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

    db.conn().execute_raw(insert("ada")).await.unwrap();
    // Every handle on the pool shares the listeners.
    db.clone().conn().execute_raw(insert("grace")).await.unwrap();
    db.transaction(|uow| async move {
        uow.tx().await?.execute_raw(insert("edsger")).await?;
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
    db.run_before_executing("SELECT 1");
    assert_eq!(*executing.lock().unwrap(), ["SELECT 1"]);
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
        let tx = uow.tx().await.unwrap();
        for name in ["a", "b", "c"] {
            tx.execute_raw(insert(name)).await.unwrap();
        }
        tx.query_all_raw(select()).await.unwrap();
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
        uow.tx().await.unwrap().execute_raw(insert("d")).await.unwrap();
    })
    .await;
    assert_eq!(slow.lock().unwrap().len(), 2, "allowed again");
    uow.reset_total_query_duration();
    assert_eq!(uow.total_query_duration(), Duration::ZERO);
    assert!(uow.query_stats().statements >= 5, "the count stays");
    uow.commit().await.unwrap();

    // Statements outside the unit of work's scope are not its own.
    let other = db.uow(None, None).await;
    db.conn().execute_raw(insert("e")).await.unwrap();
    assert_eq!(other.query_stats().statements, 0);
}

#[tokio::test]
async fn a_finished_unit_of_work_logs_its_statements_and_repeats() {
    let (db, _temp) = people().await;
    let logs = LogCapture::default();
    let _guard = logs.install();
    let uow = db.uow(None, Some("req-9".into())).await;
    uow.scope(async {
        let tx = uow.tx().await.unwrap();
        for n in 0..10 {
            tx.execute_raw(insert(&format!("person {n}"))).await.unwrap();
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
