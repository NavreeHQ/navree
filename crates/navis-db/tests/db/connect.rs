//! Options and connecting.

use navis_db::{Db, DbOptions};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::support::TempDb;

#[test]
fn default_options() {
    let options = DbOptions::default();
    assert_eq!(options.url, "sqlite://storage/app.db?mode=rwc");
    assert_eq!(options.max_connections, 10);
    assert_eq!(options.slow_query_ms, 200);
    assert!(!options.migrate_on_boot);
    assert!(!options.log_queries);
}

async fn pragma(db: &Db, name: &str) -> String {
    let row = db
        .conn()
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("PRAGMA {name}"),
        ))
        .await
        .expect("pragma query")
        .expect("pragma row");
    row.try_get_by_index::<String>(0)
        .or_else(|_| row.try_get_by_index::<i64>(0).map(|v| v.to_string()))
        .expect("pragma value")
}

#[tokio::test]
async fn sqlite_connection_uses_wal_busy_timeout_and_foreign_keys() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.expect("connects");
    assert_eq!(db.backend(), DbBackend::Sqlite);
    assert_eq!(pragma(&db, "journal_mode").await, "wal");
    assert_eq!(pragma(&db, "foreign_keys").await, "1");
    assert_eq!(pragma(&db, "busy_timeout").await, "5000");
}

#[tokio::test]
async fn sqlite_parent_directory_is_created() {
    let temp = TempDb::new();
    let path = temp.dir.path().join("nested").join("dir").join("app.db");
    let options = DbOptions {
        url: format!("sqlite://{}?mode=rwc", path.display()),
        ..DbOptions::default()
    };
    Db::connect(&options).await.expect("connects");
    assert!(path.exists(), "database file created at {}", path.display());
}

#[tokio::test]
async fn slow_queries_are_logged_at_warn_without_bound_values() {
    let temp = TempDb::new();
    let options = DbOptions {
        slow_query_ms: 1,
        ..temp.options()
    };
    let db = Db::connect(&options).await.expect("connects");
    let logs = crate::support::LogCapture::default();
    let _guard = logs.install();
    db.conn()
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 400000) \
             SELECT count(*) FROM c WHERE ? <> 'hunter2'",
            ["secret-binding".into()],
        ))
        .await
        .expect("query runs");
    let text = logs.text();
    assert!(text.contains("WARN"), "logs: {text}");
    assert!(text.contains("slow query"), "logs: {text}");
    assert!(text.contains("WITH RECURSIVE"), "logs: {text}");
    assert!(
        !text.contains("secret-binding"),
        "bound values are not logged: {text}"
    );
}

#[tokio::test]
async fn log_queries_logs_every_statement_at_debug() {
    let temp = TempDb::new();
    let options = DbOptions {
        log_queries: true,
        slow_query_ms: 0,
        ..temp.options()
    };
    let db = Db::connect(&options).await.expect("connects");
    let logs = crate::support::LogCapture::default();
    let _guard = logs.install();
    db.conn()
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT 42"))
        .await
        .expect("query runs");
    let text = logs.text();
    assert!(text.contains("DEBUG"), "logs: {text}");
    assert!(text.contains("SELECT 42"), "logs: {text}");
    assert!(!text.contains("slow query"), "logs: {text}");
}
