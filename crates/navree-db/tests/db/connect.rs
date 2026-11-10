//! Options and connecting.

use navree_db::{Backend, Db, DbOptions, Executor, ExecutorExt, MySqlOptions, Statement};

use crate::support::TempDb;

#[test]
fn default_options() {
    let options = DbOptions::default();
    assert_eq!(options.url, "sqlite://storage/app.db?mode=rwc");
    assert_eq!(options.max_connections, 10);
    assert_eq!(options.slow_query_ms, 200);
    assert!(!options.migrate_on_boot);
    assert!(!options.log_queries);
    assert!(!options.test_before_acquire);
    assert_eq!(options.idle_timeout_secs, 600);
    assert_eq!(options.max_lifetime_secs, 1800);
}

/// A pool whose connections are pinged at every checkout and never idle
/// out still serves statements (the settings themselves are checked by the
/// driver's unit tests).
#[tokio::test]
async fn pool_checks_and_lifetimes_follow_the_options() {
    let temp = TempDb::new();
    let db = Db::connect(&DbOptions {
        test_before_acquire: true,
        idle_timeout_secs: 0,
        max_lifetime_secs: 60,
        ..temp.options()
    })
    .await
    .expect("connects");
    let one: i64 = db
        .pool()
        .fetch_scalar(&Statement::text("SELECT 1"))
        .await
        .expect("query");
    assert_eq!(one, 1);
    db.ping().await.expect("pings");
    let stats = db.pool_stats();
    assert_eq!(stats.max, 10);
    assert!(stats.size >= 1, "{stats:?}");
}

#[test]
fn pool_settings_come_from_the_database_section() {
    let options: DbOptions = serde_json::from_value(serde_json::json!({
        "test_before_acquire": true,
        "idle_timeout_secs": 30,
        "max_lifetime_secs": 0,
    }))
    .expect("valid section");
    assert!(options.test_before_acquire);
    assert_eq!(options.idle_timeout_secs, 30);
    assert_eq!(options.max_lifetime_secs, 0);
    let debug = format!("{options:?}");
    assert!(debug.contains("test_before_acquire: true"), "{debug}");
}

async fn pragma(db: &Db, name: &str) -> String {
    let row = db
        .pool()
        .fetch_one(&Statement::text(format!("PRAGMA {name}")))
        .await
        .expect("pragma query");
    row.get::<String>(0)
        .or_else(|_| row.get::<i64>(0).map(|v| v.to_string()))
        .expect("pragma value")
}

#[tokio::test]
async fn sqlite_connection_uses_wal_busy_timeout_and_foreign_keys() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.expect("connects");
    assert_eq!(db.backend(), Backend::Sqlite);
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
    db.pool()
        .fetch_optional(&Statement::portable(
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

/// A MySQL or MariaDB URL without the `mysql` feature names the feature;
/// with it, the error is the driver's. The password never shows.
#[tokio::test]
async fn mysql_urls_name_the_feature_they_need() {
    let connect = |url: &str| {
        let options = DbOptions {
            url: url.to_owned(),
            connect_timeout_secs: 1,
            ..DbOptions::default()
        };
        async move { Db::connect(&options).await.unwrap_err().to_string() }
    };
    let mysql = "mysql://navree:hunter2@127.0.0.1:1/app";
    let mariadb = "mariadb://navree:hunter2@127.0.0.1:1/app";
    // Concurrently: with the driver, each waits for the pool's timeout.
    let (mysql_error, mariadb_error) = tokio::join!(connect(mysql), connect(mariadb));
    for (url, error) in [(mysql, mysql_error), (mariadb, mariadb_error)] {
        assert!(
            error.contains(&format!(
                "cannot connect to {}",
                url.replace("hunter2", "***")
            )),
            "{error}"
        );
        assert!(!error.contains("hunter2"), "{error}");
        assert_eq!(
            error.contains("(enable the `mysql` feature of navree-db)"),
            !cfg!(feature = "mysql"),
            "{error}"
        );
    }
}

/// Options built in code (`DatabaseModule::for_root`, `Db::connect`) are
/// checked like a configuration section is, before anything connects: an
/// SQL mode that is not a mode name never reaches the server.
#[tokio::test]
async fn invalid_options_fail_before_connecting() {
    let options = DbOptions {
        url: "mysql://navree:hunter2@127.0.0.1:1/app".to_owned(),
        connect_timeout_secs: 1,
        mysql: MySqlOptions {
            modes: Some(vec![
                "STRICT_ALL_TABLES".to_owned(),
                "NO_ZERO_DATE'; DROP TABLE users; --".to_owned(),
            ]),
            ..MySqlOptions::default()
        },
        ..DbOptions::default()
    };
    let error = Db::connect(&options).await.unwrap_err().to_string();
    assert!(
        error.ends_with(
            "invalid database options: mysql.modes: `NO_ZERO_DATE'; DROP TABLE users; --` \
             is not an SQL mode name (letters, digits and underscores, e.g. STRICT_ALL_TABLES)"
        ),
        "{error}"
    );

    let temp = TempDb::new();
    let error = Db::connect(&DbOptions {
        max_connections: 0,
        ..temp.options()
    })
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.ends_with("invalid database options: max_connections must be at least 1"),
        "{error}"
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
    db.pool()
        .fetch_optional(&Statement::text("SELECT 42"))
        .await
        .expect("query runs");
    let text = logs.text();
    assert!(text.contains("DEBUG"), "logs: {text}");
    assert!(text.contains("SELECT 42"), "logs: {text}");
    assert!(!text.contains("slow query"), "logs: {text}");
}
