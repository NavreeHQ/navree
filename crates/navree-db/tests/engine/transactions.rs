//! Transactions and savepoints on navree-db's own connections.

use futures_util::StreamExt;
use navree_db::{
    Backend, DbOptions, ErrorKind, Executor, ExecutorExt, IsolationLevel, Statement, TxOptions,
};

use crate::support::{Scratch, script, serial};

async fn notes_table(db: &impl Executor, backend: Backend) {
    script(
        db,
        &format!(
            "CREATE TABLE notes (id {serial}, body VARCHAR(100) NOT NULL UNIQUE)",
            serial = serial(backend)
        ),
    )
    .await;
}

fn add(body: &str) -> Statement {
    Statement::portable("INSERT INTO notes (body) VALUES (?)", [body.into()])
}

async fn bodies(db: &impl Executor) -> Vec<String> {
    db.fetch_column(&Statement::text("SELECT body FROM notes ORDER BY body"))
        .await
        .unwrap()
}

/// Commit keeps, rollback discards, dropping rolls back.
pub async fn commit_rollback_and_drop(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    notes_table(&db, backend).await;

    let tx = db.begin().await.unwrap();
    assert!(!tx.is_savepoint());
    tx.execute(&add("kept")).await.unwrap();
    // Inside: visible. Outside: not yet (another connection).
    assert_eq!(bodies(&tx).await, ["kept"]);
    if backend != Backend::Sqlite {
        assert!(bodies(&db).await.is_empty(), "uncommitted rows stay inside");
    }
    tx.commit().await.unwrap();
    assert_eq!(bodies(&db).await, ["kept"]);

    let tx = db.begin().await.unwrap();
    tx.execute(&add("discarded")).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(bodies(&db).await, ["kept"]);

    {
        let tx = db.begin().await.unwrap();
        tx.execute(&add("dropped")).await.unwrap();
        // Dropped without commit: rolled back.
    }
    assert_eq!(bodies(&db).await, ["kept"]);

    // Every connection went back to the pool usable, outside a transaction.
    for _ in 0..db.pool().unwrap().stats().size + 1 {
        let tx = db.begin().await.unwrap();
        assert_eq!(bodies(&tx).await, ["kept"]);
        tx.rollback().await.unwrap();
    }
}

/// Savepoints commit (release) and roll back on their own; a failed
/// statement inside a savepoint leaves the transaction usable on
/// PostgreSQL once the savepoint rolls back.
pub async fn savepoints_nest(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    notes_table(&db, backend).await;

    let tx = db.begin().await.unwrap();
    tx.execute(&add("outer")).await.unwrap();

    let inner = tx.savepoint().await.unwrap();
    assert!(inner.is_savepoint());
    assert_eq!(inner.depth(), 2);
    inner.execute(&add("rolled back")).await.unwrap();
    inner.rollback().await.unwrap();

    let inner = tx.savepoint().await.unwrap();
    inner.execute(&add("released")).await.unwrap();
    let deeper = inner.savepoint().await.unwrap();
    deeper.execute(&add("deepest")).await.unwrap();
    // The parent cannot finish while a savepoint inside it is open.
    let refused = tx.savepoint().await.unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::Unsupported);
    deeper.commit().await.unwrap();
    inner.commit().await.unwrap();

    // A statement that fails inside a savepoint.
    let inner = tx.savepoint().await.unwrap();
    let duplicate = inner.execute(&add("outer")).await.unwrap_err();
    assert!(duplicate.is_unique_violation());
    inner.rollback().await.unwrap();
    // The transaction carries on (PostgreSQL included).
    tx.execute(&add("after the failure")).await.unwrap();

    // A dropped savepoint rolls back to where it began.
    {
        let inner = tx.savepoint().await.unwrap();
        inner.execute(&add("dropped savepoint")).await.unwrap();
    }
    tx.execute(&add("after the drop")).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        bodies(&db).await,
        [
            "after the drop",
            "after the failure",
            "deepest",
            "outer",
            "released"
        ]
    );
}

/// A finished transaction refuses statements; options begin read-only,
/// isolated and (SQLite) immediate transactions.
pub async fn options_and_finished_transactions(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    notes_table(&db, backend).await;

    let tx = db.begin().await.unwrap();
    let savepoint = tx.savepoint().await.unwrap();
    tx.rollback().await.unwrap();
    let done = savepoint.execute(&add("x")).await.unwrap_err();
    assert_eq!(done.kind(), ErrorKind::TransactionDone, "{done:?}");

    match backend {
        Backend::Postgres | Backend::MySql => {
            let read_only = db.begin_with(TxOptions::new().read_only()).await.unwrap();
            let refused = read_only.execute(&add("nope")).await.unwrap_err();
            assert_eq!(refused.kind(), ErrorKind::Database, "{refused:?}");
            drop(read_only);

            let (serializable, logged) = navree_db::QueryLog::capture(
                db.begin_with(TxOptions::new().isolation(IsolationLevel::Serializable)),
            )
            .await;
            let serializable = serializable.unwrap();
            let sql: Vec<&str> = logged.iter().map(|query| query.sql.as_str()).collect();
            if backend == Backend::Postgres {
                assert_eq!(sql, ["BEGIN ISOLATION LEVEL SERIALIZABLE"]);
                let level: String = serializable
                    .fetch_scalar(&Statement::text("SHOW transaction_isolation"))
                    .await
                    .unwrap();
                assert_eq!(level, "serializable");
            } else {
                // MySQL sets the next transaction's level before it begins
                // (`@@transaction_isolation` shows the session's).
                assert_eq!(
                    sql,
                    ["SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", "BEGIN"]
                );
            }
            serializable.commit().await.unwrap();
        }
        Backend::Sqlite => {
            // `BEGIN IMMEDIATE` takes the write lock at once: a second one
            // waits (here: up to a short busy timeout, then fails).
            let impatient = navree_db::Db::connect(&DbOptions {
                sqlite: navree_db::SqliteOptions {
                    busy_timeout_ms: 100,
                    ..navree_db::SqliteOptions::default()
                },
                ..scratch.options.clone()
            })
            .await
            .unwrap();
            let first = db.begin_with(TxOptions::new().immediate()).await.unwrap();
            let second = impatient
                .begin_with(TxOptions::new().immediate())
                .await
                .unwrap_err();
            assert_eq!(second.kind(), ErrorKind::LockUnavailable, "{second:?}");
            first.commit().await.unwrap();
            impatient
                .begin_with(TxOptions::new().immediate())
                .await
                .unwrap()
                .commit()
                .await
                .unwrap();
        }
    }
}

/// Rows stream from a pool and from a transaction; a stream dropped half
/// way leaves the transaction usable.
pub async fn rows_stream(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    notes_table(&db, backend).await;
    let tx = db.begin().await.unwrap();
    for n in 0..500 {
        tx.execute(&add(&format!("note {n:04}"))).await.unwrap();
    }
    tx.commit().await.unwrap();

    let mut stream = db
        .fetch(&Statement::text("SELECT body FROM notes ORDER BY body"))
        .await
        .unwrap();
    let mut count = 0;
    while let Some(row) = stream.next().await {
        let body: String = row.unwrap().get(0).unwrap();
        assert_eq!(body, format!("note {count:04}"));
        count += 1;
    }
    assert_eq!(count, 500);

    let tx = db.begin().await.unwrap();
    {
        let mut stream = tx
            .fetch(&Statement::text("SELECT body FROM notes ORDER BY body"))
            .await
            .unwrap();
        for _ in 0..10 {
            stream.next().await.unwrap().unwrap();
        }
        // Dropped with rows left.
    }
    tx.execute(&add("after the stream")).await.unwrap();
    let total: i64 = tx
        .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM notes"))
        .await
        .unwrap();
    assert_eq!(total, 501);
    tx.commit().await.unwrap();
}
