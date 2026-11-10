//! MySQL and MariaDB specifics: the session every pooled connection starts
//! with, the unit of work's session variables, and dropping every table.
//!
//! The statements are plain text so they are the same with the SQLx driver
//! (the `mysql` feature) and with a scripted connection in tests; the
//! connection hook that runs them is navree-db's MySQL driver's
//! (`engine::driver::mysql`).

#[cfg(any(feature = "mysql", test))]
use crate::MySqlOptions;

/// Adds strict mode to the session's SQL mode (`database.mysql.strict =
/// true`). `NULLIF` keeps an empty server mode from leaving a leading comma;
/// a mode the server already has is listed twice, which MySQL accepts.
#[cfg(any(feature = "mysql", test))]
const STRICT_SESSION: &str = "SET SESSION sql_mode = CONCAT_WS(',', \
     NULLIF(@@SESSION.sql_mode, ''), 'ONLY_FULL_GROUP_BY', 'STRICT_TRANS_TABLES', \
     'NO_ZERO_IN_DATE', 'NO_ZERO_DATE', 'ERROR_FOR_DIVISION_BY_ZERO')";

/// Removes strict mode from the session's SQL mode (`database.mysql.strict
/// = false`). The server lists every mode once, so wrapping the list in
/// commas and replacing `,MODE,` with `,` removes exactly that mode.
#[cfg(any(feature = "mysql", test))]
const LENIENT_SESSION: &str = "SET SESSION sql_mode = TRIM(BOTH ',' FROM \
     REPLACE(REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), \
     ',STRICT_TRANS_TABLES,', ','), ',STRICT_ALL_TABLES,', ','))";

/// Makes a `TIMESTAMP` column without a default have none, as on MySQL 8:
/// MariaDB before 10.10 otherwise gives the first one of a table
/// `DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP` and the others a
/// zero default, which strict mode refuses.
#[cfg(any(feature = "mysql", test))]
const EXPLICIT_TIMESTAMP_DEFAULTS: &str = "SET SESSION explicit_defaults_for_timestamp = ON";

/// The statements a new connection runs for `database.mysql`, in order:
/// the SQL mode (`modes` exactly, else strict or lenient), explicit
/// `TIMESTAMP` defaults, then the isolation level when one is set.
///
/// `Db::connect` has checked that every entry of `modes` is a mode name
/// (letters, digits and underscores); the list is still written as an
/// escaped string literal, upper-cased.
#[cfg(any(feature = "mysql", test))]
pub(crate) fn session_statements(options: &MySqlOptions) -> Vec<String> {
    let mode = match &options.modes {
        Some(modes) => {
            let modes: Vec<String> = modes.iter().map(|m| m.to_ascii_uppercase()).collect();
            format!(
                "SET SESSION sql_mode = {}",
                string_literal(&modes.join(","))
            )
        }
        None if options.strict => STRICT_SESSION.to_owned(),
        None => LENIENT_SESSION.to_owned(),
    };
    let mut statements = vec![mode, EXPLICIT_TIMESTAMP_DEFAULTS.to_owned()];
    if let Some(level) = options.isolation_level {
        statements.push(format!(
            "SET SESSION TRANSACTION ISOLATION LEVEL {}",
            level.as_sql()
        ));
    }
    statements
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

/// `'text'`, with embedded single quotes doubled. Only checked mode names
/// reach it, which hold no backslash (whose meaning in a literal depends on
/// the server's `NO_BACKSLASH_ESCAPES`), so doubling quotes is all the
/// escaping it needs.
#[cfg(any(feature = "mysql", test))]
fn string_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IsolationLevel;

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

    fn options(f: impl FnOnce(&mut MySqlOptions)) -> MySqlOptions {
        let mut options = MySqlOptions::default();
        f(&mut options);
        options
    }

    #[test]
    fn strict_sessions_add_the_strict_modes_to_the_servers() {
        assert_eq!(
            session_statements(&MySqlOptions::default()),
            [
                "SET SESSION sql_mode = CONCAT_WS(',', NULLIF(@@SESSION.sql_mode, ''), \
                 'ONLY_FULL_GROUP_BY', 'STRICT_TRANS_TABLES', 'NO_ZERO_IN_DATE', \
                 'NO_ZERO_DATE', 'ERROR_FOR_DIVISION_BY_ZERO')",
                EXPLICIT_TIMESTAMP_DEFAULTS,
            ]
        );
    }

    #[test]
    fn lenient_sessions_remove_the_strict_modes() {
        assert_eq!(
            session_statements(&options(|o| o.strict = false)),
            [
                "SET SESSION sql_mode = TRIM(BOTH ',' FROM \
                 REPLACE(REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), \
                 ',STRICT_TRANS_TABLES,', ','), ',STRICT_ALL_TABLES,', ','))",
                EXPLICIT_TIMESTAMP_DEFAULTS,
            ]
        );
    }

    /// `modes` replaces the mode whatever `strict` says; names are
    /// upper-cased.
    #[test]
    fn explicit_modes_replace_the_sql_mode() {
        for strict in [true, false] {
            let session = session_statements(&options(|o| {
                o.strict = strict;
                o.modes = Some(vec![
                    "strict_all_tables".to_owned(),
                    "NO_ENGINE_SUBSTITUTION".to_owned(),
                ]);
            }));
            assert_eq!(
                session,
                [
                    "SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION'",
                    EXPLICIT_TIMESTAMP_DEFAULTS,
                ]
            );
        }
        assert_eq!(
            session_statements(&options(|o| o.modes = Some(Vec::new())))[0],
            "SET SESSION sql_mode = ''"
        );
        // Checked names hold no quote; one would still stay inside the literal.
        assert_eq!(
            session_statements(&options(|o| o.modes = Some(vec!["a'b".to_owned()])))[0],
            "SET SESSION sql_mode = 'A''B'"
        );
    }

    #[test]
    fn the_isolation_level_follows_the_sql_mode() {
        let session = session_statements(&options(|o| {
            o.modes = Some(vec!["ANSI".to_owned()]);
            o.isolation_level = Some(IsolationLevel::ReadCommitted);
        }));
        assert_eq!(
            session,
            [
                "SET SESSION sql_mode = 'ANSI'",
                EXPLICIT_TIMESTAMP_DEFAULTS,
                "SET SESSION TRANSACTION ISOLATION LEVEL READ COMMITTED",
            ]
        );
        for (level, sql) in [
            (IsolationLevel::ReadUncommitted, "READ UNCOMMITTED"),
            (IsolationLevel::RepeatableRead, "REPEATABLE READ"),
            (IsolationLevel::Serializable, "SERIALIZABLE"),
        ] {
            let session = session_statements(&options(|o| o.isolation_level = Some(level)));
            assert_eq!(
                session[2],
                format!("SET SESSION TRANSACTION ISOLATION LEVEL {sql}")
            );
        }
    }
}
