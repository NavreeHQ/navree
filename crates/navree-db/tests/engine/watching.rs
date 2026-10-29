//! Statements reach the query logs, listeners, hooks and unit-of-work
//! statistics of their connection; connections start with the
//! session of `database.*`.

use std::sync::{Arc, Mutex};

use navree_db::{Backend, Db, DbOptions, Executor, ExecutorExt, QueryLog, Statement};

use crate::support::{Scratch, script, serial};

/// Query logs, listeners, before-executing and before-transaction hooks,
/// and the statistics of the unit of work the statements run for.
pub async fn statements_are_watched(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    script(
        &db,
        &format!(
            "CREATE TABLE marks (id {serial}, label VARCHAR(20) NOT NULL)",
            serial = serial(backend)
        ),
    )
    .await;

    let heard = Arc::new(Mutex::new(Vec::new()));
    let listener = heard.clone();
    db.on_query(move |query| {
        listener
            .lock()
            .unwrap()
            .push((query.sql.clone(), query.failed))
    });
    let announced = Arc::new(Mutex::new(Vec::new()));
    let hook = announced.clone();
    db.before_executing(move |sql| hook.lock().unwrap().push(sql.to_owned()));
    let begun = Arc::new(Mutex::new(0));
    let counter = begun.clone();
    db.before_starting_transaction(move || *counter.lock().unwrap() += 1);

    let insert = Statement::portable("INSERT INTO marks (label) VALUES (?)", ["first".into()]);
    let ((), logged) = QueryLog::capture(async {
        db.execute(&insert).await.unwrap();
        let _ = db
            .execute(&Statement::text("SELECT nonsense FROM nowhere"))
            .await;
    })
    .await;
    assert_eq!(logged.len(), 2, "{logged:?}");
    assert!(logged[0].sql.starts_with("INSERT INTO marks"));
    assert_eq!(
        logged[0].values.len(),
        1,
        "the values of the captured statement"
    );
    assert!(!logged[0].failed);
    assert!(logged[1].failed);
    assert_eq!(logged[0].connection.as_ref(), "default");
    assert!(
        logged[0].raw_sql().contains("'first'"),
        "{}",
        logged[0].raw_sql()
    );
    assert_eq!(heard.lock().unwrap().len(), 2);
    assert_eq!(announced.lock().unwrap().len(), 2);

    let uow = db.uow(None, None).await;
    uow.scope(async {
        for label in ["a", "b", "c"] {
            uow.execute(&Statement::portable(
                "INSERT INTO marks (label) VALUES (?)",
                [label.into()],
            ))
            .await
            .unwrap();
        }
    })
    .await;
    let stats = uow.query_stats();
    assert!(stats.statements >= 3, "{stats:?}");
    assert!(
        stats
            .repeated
            .iter()
            .any(|repeated| repeated.sql.starts_with("INSERT INTO marks") && repeated.count == 3),
        "{stats:?}"
    );
    uow.commit().await.unwrap();
    assert_eq!(*begun.lock().unwrap(), 1, "one transaction began");
}

/// Every pooled connection starts with the session `database.*` asks
/// for: SQLite's pragmas, PostgreSQL's settings, MySQL's SQL mode.
pub async fn sessions_follow_the_options(scratch: Scratch) {
    let backend = scratch.server.backend();
    let mut options: DbOptions = scratch.options.clone();
    match backend {
        Backend::Sqlite => {
            options.sqlite.busy_timeout_ms = 1234;
            options
                .sqlite
                .pragmas
                .insert("cache_size".into(), "-4000".into());
        }
        Backend::Postgres => {
            options.postgres.application_name = Some("navree-engine".into());
            options.postgres.timezone = Some("Europe/Berlin".into());
            options.postgres.synchronous_commit = Some("off".into());
        }
        Backend::MySql => {
            options.mysql.strict = true;
            options.mysql.isolation_level = Some(navree_db::IsolationLevel::ReadCommitted);
        }
    }
    let db = Db::connect(&options).await.unwrap();
    let pool = db.pool();
    let text = |sql: &'static str| {
        let pool = pool.clone();
        async move {
            pool.fetch_scalar::<String>(&Statement::text(sql))
                .await
                .unwrap()
        }
    };
    match backend {
        Backend::Sqlite => {
            assert_eq!(text("PRAGMA journal_mode").await, "wal");
            assert_eq!(text("PRAGMA busy_timeout").await, "1234");
            assert_eq!(text("PRAGMA foreign_keys").await, "1");
            assert_eq!(text("PRAGMA cache_size").await, "-4000");
        }
        Backend::Postgres => {
            assert_eq!(
                text("SELECT current_setting('application_name')").await,
                "navree-engine"
            );
            assert_eq!(
                text("SELECT current_setting('TimeZone')").await,
                "Europe/Berlin"
            );
            assert_eq!(
                text("SELECT current_setting('synchronous_commit')").await,
                "off"
            );
            assert!(
                text("SELECT current_setting('search_path')")
                    .await
                    .starts_with("engine_")
            );
        }
        Backend::MySql => {
            let mode = text("SELECT @@SESSION.sql_mode").await;
            assert!(mode.contains("STRICT_TRANS_TABLES"), "{mode}");
            assert_eq!(
                text("SELECT @@SESSION.transaction_isolation").await,
                "READ-COMMITTED"
            );
            assert_eq!(text("SELECT @@SESSION.time_zone").await, "+00:00");
        }
    }
    let stats = db.pool_stats();
    assert!(
        stats.size >= 1 && stats.max == options.max_connections,
        "{stats:?}"
    );
    db.ping().await.unwrap();
    db.close().await.unwrap();
    assert!(pool.is_closed());
    let closed = pool
        .fetch_all(&Statement::text("SELECT 1"))
        .await
        .unwrap_err();
    assert_eq!(
        closed.kind(),
        navree_db::ErrorKind::PoolClosed,
        "{closed:?}"
    );
}
