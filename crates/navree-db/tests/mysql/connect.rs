//! Connecting: `mysql://` and `mariadb://` URLs, the session every pooled
//! connection starts with, and connection errors.

use navree_db::{Db, DbOptions, MySqlOptions};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::support::{Scratch, string};

#[tokio::test]
async fn connects_with_mysql_and_mariadb_urls() {
    let Some(scratch) = Scratch::new("connects_with_mysql_and_mariadb_urls").await else {
        return;
    };
    for url in [scratch.url.clone(), scratch.mariadb_url()] {
        let db = Db::connect(&DbOptions {
            url: url.clone(),
            ..DbOptions::default()
        })
        .await
        .unwrap_or_else(|error| panic!("{url}: {error}"));
        assert_eq!(db.backend(), DbBackend::MySql, "{url}");
        db.conn().ping().await.expect("pings");
        assert_eq!(string(db.conn(), "SELECT DATABASE()").await, scratch.name);
    }
}

/// UTC and utf8mb4 come from SQLx; strict mode from `database.mysql`.
#[tokio::test]
async fn sessions_are_utc_utf8mb4_and_strict() {
    let Some(scratch) = Scratch::new("sessions_are_utc_utf8mb4_and_strict").await else {
        return;
    };
    let db = scratch.db().await;
    assert_eq!(string(db.conn(), "SELECT @@SESSION.time_zone").await, "+00:00");
    assert_eq!(
        string(db.conn(), "SELECT @@SESSION.character_set_client").await,
        "utf8mb4"
    );
    let mode = string(db.conn(), "SELECT @@SESSION.sql_mode").await;
    for expected in [
        "STRICT_TRANS_TABLES",
        "NO_ZERO_IN_DATE",
        "NO_ZERO_DATE",
        "ERROR_FOR_DIVISION_BY_ZERO",
        "PIPES_AS_CONCAT",
        "NO_ENGINE_SUBSTITUTION",
    ] {
        assert!(mode.split(',').any(|m| m == expected), "{expected} in {mode}");
    }

    db.conn()
        .execute_unprepared("CREATE TABLE codes (code VARCHAR(3) NOT NULL)")
        .await
        .unwrap();
    let error = db
        .conn()
        .execute_unprepared("INSERT INTO codes (code) VALUES ('ABCDEF')")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Data too long"), "{error}");
}

/// `strict = false` turns strict mode off whatever the server's default,
/// so a value too long for its column is cut instead of refused.
#[tokio::test]
async fn strict_false_turns_strict_mode_off() {
    let Some(scratch) = Scratch::new("strict_false_turns_strict_mode_off").await else {
        return;
    };
    let db = scratch
        .db_with(|options| options.mysql = MySqlOptions { strict: false })
        .await;
    let mode = string(db.conn(), "SELECT @@SESSION.sql_mode").await;
    assert!(
        !mode.split(',').any(|m| m.starts_with("STRICT_")),
        "no strict mode in {mode}"
    );
    db.conn()
        .execute_unprepared("CREATE TABLE codes (code VARCHAR(3) NOT NULL)")
        .await
        .unwrap();
    db.conn()
        .execute_unprepared("INSERT INTO codes (code) VALUES ('ABCDEF')")
        .await
        .expect("cut, not refused");
    assert_eq!(string(db.conn(), "SELECT code FROM codes").await, "ABC");
}

/// The session is set when SQLx opens a connection, so every pooled
/// connection has it, not only the first.
#[tokio::test]
async fn every_pooled_connection_starts_with_the_session() {
    let Some(scratch) = Scratch::new("every_pooled_connection_starts_with_the_session").await
    else {
        return;
    };
    let db = scratch
        .db_with(|options| {
            options.max_connections = 3;
            options.mysql.strict = false;
        })
        .await;
    // Three open transactions hold three different connections.
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(db.conn().begin().await.expect("begins"));
    }
    let mut ids = Vec::new();
    for tx in &held {
        let mode = string(tx, "SELECT @@SESSION.sql_mode").await;
        assert!(!mode.contains("STRICT_TRANS_TABLES"), "{mode}");
        ids.push(string(tx, "SELECT CAST(CONNECTION_ID() AS CHAR)").await);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "three connections: {ids:?}");
}

#[tokio::test]
async fn connection_errors_name_the_server_without_the_password() {
    let Some(scratch) =
        Scratch::new("connection_errors_name_the_server_without_the_password").await
    else {
        return;
    };
    for url in [scratch.url.clone(), scratch.mariadb_url()] {
        let (scheme, rest) = url.split_once("://").unwrap();
        let (_, host_and_db) = rest.split_once('@').unwrap();
        let wrong = format!("{scheme}://navree_nobody:not-the-password-42@{host_and_db}");
        let error = Db::connect(&DbOptions {
            url: wrong,
            connect_timeout_secs: 2,
            ..DbOptions::default()
        })
        .await
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(&format!("{scheme}://navree_nobody:***@{host_and_db}")),
            "{error}"
        );
        assert!(!error.contains("not-the-password-42"), "{error}");
        assert!(!error.contains("enable the `mysql` feature"), "{error}");
    }
}

/// Raw statements use `?` placeholders on MySQL.
#[tokio::test]
async fn raw_statements_bind_values() {
    let Some(scratch) = Scratch::new("raw_statements_bind_values").await else {
        return;
    };
    let db = scratch.db().await;
    let row = db
        .conn()
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::MySql,
            "SELECT CONCAT(?, '-', ?) AS joined",
            ["a".into(), 42i32.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "joined").unwrap(), "a-42");
}
