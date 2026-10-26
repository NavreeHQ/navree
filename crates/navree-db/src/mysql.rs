//! MySQL and MariaDB specifics: the session every pooled connection starts
//! with, the unit of work's session variables, and dropping every table.
//!
//! The statements are plain text so they are the same with the SQLx driver
//! (the `mysql` feature) and with a scripted connection in tests; only the
//! connection hook needs the driver.

/// Adds strict mode to the session's SQL mode (`database.mysql.strict =
/// true`). `NULLIF` keeps an empty server mode from leaving a leading comma.
pub(crate) const STRICT_SESSION: &str = "SET SESSION sql_mode = CONCAT_WS(',', \
     NULLIF(@@SESSION.sql_mode, ''), 'STRICT_TRANS_TABLES', 'NO_ZERO_IN_DATE', \
     'NO_ZERO_DATE', 'ERROR_FOR_DIVISION_BY_ZERO')";

/// Removes strict mode from the session's SQL mode (`database.mysql.strict
/// = false`). The server lists every mode once, so wrapping the list in
/// commas and replacing `,MODE,` with `,` removes exactly that mode.
pub(crate) const LENIENT_SESSION: &str = "SET SESSION sql_mode = TRIM(BOTH ',' FROM \
     REPLACE(REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), \
     ',STRICT_TRANS_TABLES,', ','), ',STRICT_ALL_TABLES,', ','))";

/// The statement a new connection runs for `database.mysql.strict`.
pub(crate) fn session_statement(strict: bool) -> &'static str {
    if strict {
        STRICT_SESSION
    } else {
        LENIENT_SESSION
    }
}

/// Sets the unit of work's actor and request id (`NULL` when absent) right
/// after `BEGIN`, overwriting whatever an earlier unit of work on the same
/// pooled connection left.
pub(crate) const SET_UOW_VARIABLES: &str = "SET @navree_actor = ?, @navree_request_id = ?";

/// Clears them right before `COMMIT` or `ROLLBACK`: user variables belong
/// to the connection, not to the transaction, and the connection goes back
/// to the pool.
pub(crate) const CLEAR_UOW_VARIABLES: &str = "SET @navree_actor = NULL, @navree_request_id = NULL";

/// The base tables of the current database (`migrate:fresh`). `CAST(.. AS
/// CHAR)` makes the name a text column whatever the server reports for
/// `information_schema`; `SYSTEM VERSIONED` is MariaDB's system-versioned
/// table.
pub(crate) const LIST_TABLES: &str = "SELECT CAST(table_name AS CHAR) AS name \
     FROM information_schema.tables \
     WHERE table_schema = DATABASE() AND table_type IN ('BASE TABLE', 'SYSTEM VERSIONED') \
     ORDER BY table_name";

/// Turns foreign key checks off for the session, so tables drop in any
/// order.
pub(crate) const FOREIGN_KEY_CHECKS_OFF: &str = "SET FOREIGN_KEY_CHECKS = 0";

/// Turns them back on before the connection returns to the pool.
pub(crate) const FOREIGN_KEY_CHECKS_ON: &str = "SET FOREIGN_KEY_CHECKS = 1";

/// `` `name` `` with embedded backticks doubled.
pub(crate) fn quote(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

/// One `DROP TABLE IF EXISTS` for every table in `tables` (all or none
/// are dropped).
pub(crate) fn drop_tables(tables: &[String]) -> String {
    let names: Vec<String> = tables.iter().map(|table| quote(table)).collect();
    format!("DROP TABLE IF EXISTS {}", names.join(", "))
}

/// Installs [`session_statement`] as SQLx's `after_connect` hook: it runs
/// once on every connection the pool opens.
#[cfg(feature = "mysql")]
pub(crate) fn configure(connect: &mut sea_orm::ConnectOptions, options: &crate::MySqlOptions) {
    let statement = session_statement(options.strict);
    connect.map_sqlx_mysql_pool_opts(move |pool| {
        pool.after_connect(move |conn, _meta| {
            Box::pin(async move {
                sea_orm::sqlx::raw_sql(statement).execute(conn).await?;
                Ok(())
            })
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_with_backticks() {
        assert_eq!(quote("users"), "`users`");
        assert_eq!(quote("we`ird"), "`we``ird`");
    }

    #[test]
    fn drops_every_table_in_one_statement() {
        assert_eq!(
            drop_tables(&["users".to_owned(), "role`s".to_owned()]),
            "DROP TABLE IF EXISTS `users`, `role``s`"
        );
    }

    #[test]
    fn strict_and_lenient_sessions() {
        assert_eq!(session_statement(true), STRICT_SESSION);
        assert_eq!(session_statement(false), LENIENT_SESSION);
        assert!(STRICT_SESSION.contains("'STRICT_TRANS_TABLES'"));
        assert!(LENIENT_SESSION.contains("',STRICT_ALL_TABLES,'"));
    }
}
