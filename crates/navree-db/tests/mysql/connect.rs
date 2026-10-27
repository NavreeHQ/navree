//! Connecting: `mysql://` and `mariadb://` URLs, the session every pooled
//! connection starts with (`database.mysql`: strict mode, an exact SQL
//! mode, the isolation level), and connection errors.

use navree_db::{Db, DbOptions, IsolationLevel, MySqlOptions};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::support::{Account, Scratch, count, is_mariadb, string, with_param};

/// The session's isolation level as the server names it
/// (`READ-COMMITTED`): `transaction_isolation` on MySQL 8 and MariaDB
/// 11.1+, `tx_isolation` before.
async fn isolation(conn: &impl ConnectionTrait) -> String {
    for variable in ["@@SESSION.transaction_isolation", "@@SESSION.tx_isolation"] {
        let sql = format!("SELECT CAST({variable} AS CHAR)");
        if let Ok(rows) = conn
            .query_all_raw(Statement::from_string(DbBackend::MySql, sql))
            .await
            && let Some(row) = rows.first()
        {
            return row.try_get_by_index::<String>(0).expect("text");
        }
    }
    panic!("the server reports no isolation level")
}

/// The SQL mode of `conn`'s session, as a sorted list.
async fn sql_mode(conn: &impl ConnectionTrait) -> Vec<String> {
    let mode = string(conn, "SELECT @@SESSION.sql_mode").await;
    let mut modes: Vec<String> = mode
        .split(',')
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
        .collect();
    modes.sort();
    modes
}

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
    assert_eq!(
        string(db.conn(), "SELECT @@SESSION.time_zone").await,
        "+00:00"
    );
    assert_eq!(
        string(db.conn(), "SELECT @@SESSION.character_set_client").await,
        "utf8mb4"
    );
    let mode = sql_mode(db.conn()).await;
    for expected in [
        "ONLY_FULL_GROUP_BY",
        "STRICT_TRANS_TABLES",
        "NO_ZERO_IN_DATE",
        "NO_ZERO_DATE",
        "ERROR_FOR_DIVISION_BY_ZERO",
        "PIPES_AS_CONCAT",
        "NO_ENGINE_SUBSTITUTION",
    ] {
        assert!(mode.iter().any(|m| m == expected), "{expected} in {mode:?}");
    }

    db.conn()
        .execute_unprepared("CREATE TABLE codes (code VARCHAR(3) NOT NULL, n INT NOT NULL)")
        .await
        .unwrap();
    let error = db
        .conn()
        .execute_unprepared("INSERT INTO codes (code, n) VALUES ('ABCDEF', 1)")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Data too long"), "{error}");

    // A column neither grouped by nor aggregated is refused, as on PostgreSQL.
    let error = db
        .conn()
        .execute_unprepared("SELECT code, n FROM codes GROUP BY code")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("only_full_group_by"), "{error}");
}

/// `modes` is the session's whole SQL mode, whatever `strict` says.
#[tokio::test]
async fn explicit_modes_replace_the_sql_mode() {
    let Some(scratch) = Scratch::new("explicit_modes_replace_the_sql_mode").await else {
        return;
    };
    let db = scratch
        .db_with(|options| {
            options.mysql = MySqlOptions {
                strict: true,
                modes: Some(vec![
                    "no_engine_substitution".to_owned(),
                    "ANSI_QUOTES".to_owned(),
                ]),
                isolation_level: None,
            }
        })
        .await;
    assert_eq!(
        sql_mode(db.conn()).await,
        ["ANSI_QUOTES", "NO_ENGINE_SUBSTITUTION"]
    );
    // No strict mode, though `strict` is true: a long value is cut.
    db.conn()
        .execute_unprepared(r#"CREATE TABLE "codes" ("code" VARCHAR(3) NOT NULL)"#)
        .await
        .expect("ANSI_QUOTES: double quotes name identifiers");
    db.conn()
        .execute_unprepared(r#"INSERT INTO "codes" ("code") VALUES ('ABCDEF')"#)
        .await
        .expect("cut, not refused");
    assert_eq!(string(db.conn(), "SELECT code FROM codes").await, "ABC");

    let cleared = scratch
        .db_with(|options| options.mysql.modes = Some(Vec::new()))
        .await;
    assert!(sql_mode(cleared.conn()).await.is_empty());
}

/// Every pooled connection runs at `isolation_level`: under READ COMMITTED
/// a unit of work's second read sees a row committed in between; under the
/// server's default REPEATABLE READ it keeps its first snapshot.
#[tokio::test]
async fn the_isolation_level_applies_to_every_connection() {
    let Some(scratch) = Scratch::new("the_isolation_level_applies_to_every_connection").await
    else {
        return;
    };
    let committed = scratch
        .db_with(|options| {
            options.max_connections = 2;
            options.mysql.isolation_level = Some(IsolationLevel::ReadCommitted);
        })
        .await;
    let repeatable = scratch.db().await;
    committed
        .conn()
        .execute_unprepared("CREATE TABLE seen (id INT PRIMARY KEY)")
        .await
        .unwrap();

    let mut held = Vec::new();
    for _ in 0..2 {
        held.push(committed.conn().begin().await.unwrap());
    }
    for tx in &held {
        assert_eq!(isolation(tx).await, "READ-COMMITTED");
    }
    drop(held);
    assert_eq!(isolation(repeatable.conn()).await, "REPEATABLE-READ");

    for (db, sees_the_new_row) in [(&committed, true), (&repeatable, false)] {
        let uow = db.uow(None, None).await;
        let tx = uow.tx().await.unwrap();
        let before = count(&*tx, "seen").await;
        repeatable
            .conn()
            .execute_unprepared(&format!("INSERT INTO seen (id) VALUES ({})", before + 1))
            .await
            .unwrap();
        let after = count(&*tx, "seen").await;
        drop(tx);
        uow.commit().await.unwrap();
        assert_eq!(
            after == before + 1,
            sees_the_new_row,
            "{before} then {after}"
        );
    }
}

/// `strict = false` turns strict mode off whatever the server's default,
/// so a value too long for its column is cut instead of refused.
#[tokio::test]
async fn strict_false_turns_strict_mode_off() {
    let Some(scratch) = Scratch::new("strict_false_turns_strict_mode_off").await else {
        return;
    };
    let db = scratch
        .db_with(|options| {
            options.mysql = MySqlOptions {
                strict: false,
                ..MySqlOptions::default()
            }
        })
        .await;
    let mode = sql_mode(db.conn()).await;
    assert!(
        !mode.iter().any(|m| m.starts_with("STRICT_")),
        "no strict mode in {mode:?}"
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

/// The value column of a `SHOW ... LIKE 'name'` statement, when the server
/// has that variable.
async fn show(conn: &impl ConnectionTrait, statement: &str) -> Option<String> {
    let rows = conn
        .query_all_raw(Statement::from_string(DbBackend::MySql, statement))
        .await
        .ok()?;
    rows.first()?.try_get_by_index::<String>(1).ok()
}

/// The default `ssl-mode` (PREFERRED) uses TLS when the server offers it.
#[tokio::test]
async fn connections_use_tls_when_the_server_offers_it() {
    let Some(scratch) = Scratch::new("connections_use_tls_when_the_server_offers_it").await else {
        return;
    };
    let db = scratch.db().await;
    // `have_ssl` on MariaDB (and MySQL before 8.4), a loaded certificate on MySQL 8.
    let offered = show(db.conn(), "SHOW GLOBAL VARIABLES LIKE 'have_ssl'")
        .await
        .as_deref()
        == Some("YES")
        || show(db.conn(), "SHOW GLOBAL STATUS LIKE 'Current_tls_cert'")
            .await
            .is_some_and(|cert| !cert.is_empty());
    let session = show(db.conn(), "SHOW SESSION STATUS LIKE 'Ssl_version'")
        .await
        .unwrap_or_default();
    if offered {
        assert!(session.starts_with("TLS"), "a TLS session: {session:?}");
    } else {
        eprintln!("the server offers no TLS; nothing to check");
    }
}

/// A `caching_sha2_password` account (MySQL 8's default) signs in with its
/// password over TLS, or encrypted with the server's RSA key on a connection
/// without TLS, which needs the `mysql-rsa` feature. Without it, the
/// connection error says how to fix it. The account is new, so the server
/// has no cached hash of its password and asks for the password itself.
#[tokio::test]
async fn caching_sha2_password_accounts_without_tls() {
    let Some(scratch) = Scratch::new("caching_sha2_password_accounts_without_tls").await else {
        return;
    };
    if is_mariadb(scratch.db().await.conn()).await {
        eprintln!("MariaDB has no caching_sha2_password; nothing to check");
        return;
    }
    let account = Account::create(&scratch, "caching_sha2_password").await;
    let plain = DbOptions {
        url: with_param(&account.url, "ssl-mode", "DISABLED"),
        connect_timeout_secs: 5,
        ..DbOptions::default()
    };
    let connected = Db::connect(&plain).await;

    #[cfg(feature = "mysql-rsa")]
    {
        let db = connected.expect("signs in with the RSA exchange");
        assert_eq!(string(db.conn(), "SELECT DATABASE()").await, scratch.name);
    }
    #[cfg(not(feature = "mysql-rsa"))]
    {
        let error = connected.expect_err("no TLS, no RSA").to_string();
        assert!(
            error.contains("this caching_sha2_password account needs TLS or the RSA exchange"),
            "{error}"
        );
        assert!(
            error.contains("enable the `mysql-rsa` feature of navree"),
            "{error}"
        );
        assert!(!error.contains(&account.password), "{error}");
    }

    // Over TLS, the default, the same account signs in either way.
    let db = Db::connect(&DbOptions {
        url: account.url.clone(),
        ..DbOptions::default()
    })
    .await
    .expect("signs in over TLS");
    assert_eq!(string(db.conn(), "SELECT DATABASE()").await, scratch.name);
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
