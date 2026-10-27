//! [`OrmError`]: what can go wrong in the ORM, and [`Result`].

use std::borrow::Cow;

use sea_orm::DbErr;

/// The result type of ORM operations.
pub type Result<T, E = OrmError> = std::result::Result<T, E>;

/// An ORM error.
///
/// Returned from an HTTP handler with `?`, an `OrmError` keeps its meaning:
/// a missing record ([`ModelNotFound`](OrmError::ModelNotFound)) answers 404
/// and a unique or foreign-key violation ([`Db`](OrmError::Db)) answers 409,
/// because both carry a [`DbErr`] in their source chain, which navree-db's
/// `DatabaseModule` maps. Anything else is a 500 whose details are logged
/// and never sent to the client.
///
/// ```
/// use navree_orm::OrmError;
///
/// let error = OrmError::not_found("Deal", vec!["42".to_owned()]);
/// assert_eq!(error.to_string(), "no Deal found for key 42");
/// assert!(error.is_not_found());
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OrmError {
    /// No record matched where one was required (`first_or_fail`,
    /// `find_or_fail`, `sole`, `refresh`).
    #[error("{}", not_found_message(model, keys))]
    ModelNotFound {
        /// The model, e.g. `Deal`.
        model: Cow<'static, str>,
        /// The keys looked for; empty when the query was not by key.
        keys: Vec<String>,
        /// `DbErr::RecordNotFound`, which navree-db maps to a 404.
        #[source]
        source: DbErr,
    },
    /// `sole` found more than one record.
    #[error("expected exactly one {model}, found more than one")]
    MultipleRecordsFound {
        /// The model.
        model: Cow<'static, str>,
    },
    /// The database refused a statement (or the connection failed).
    #[error("database error: {0}")]
    Db(#[from] DbErr),
    /// A column could not be read as the type of its field.
    #[error("cannot read column `{column}` of {model} as {expected}: {message}")]
    Decode {
        /// The model (or the table, for untyped rows).
        model: Cow<'static, str>,
        /// The column.
        column: String,
        /// The Rust type the column was read as.
        expected: Cow<'static, str>,
        /// What went wrong.
        message: String,
    },
    /// A cast could not convert a value between its field and its column.
    #[error("cannot cast column `{column}` of {model}: {message}")]
    Cast {
        /// The model.
        model: Cow<'static, str>,
        /// The column.
        column: String,
        /// What went wrong.
        message: String,
    },
    /// The query is invalid: an unknown operator, a malformed JSON path, a
    /// raw fragment with the wrong number of bindings, ...
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    /// The database or driver cannot do this.
    #[error("{feature} is not supported on {driver}")]
    Unsupported {
        /// What was asked for.
        feature: Cow<'static, str>,
        /// The database or driver, e.g. `SQLite`.
        driver: Cow<'static, str>,
    },
    /// The operation needs a record that is stored in the database.
    #[error("cannot {action} a {model} that has not been saved")]
    NotPersisted {
        /// The model.
        model: Cow<'static, str>,
        /// The operation, e.g. `increment`.
        action: &'static str,
    },
    /// A record whose key the application assigns (`key_type = "manual"`)
    /// was saved without one.
    #[error(
        "cannot insert a {model} without a key: set `{column}` first (its key type is manual, so no key is generated)"
    )]
    MissingKey {
        /// The model.
        model: Cow<'static, str>,
        /// The key column.
        column: &'static str,
    },
    /// A lifecycle hook failed (or could not be resolved from the app).
    #[error("a {model} hook failed: {message}")]
    Hook {
        /// The model.
        model: Cow<'static, str>,
        /// What went wrong.
        message: String,
    },
    /// A document driver failed.
    #[error("{driver}: {message}")]
    Driver {
        /// The driver, e.g. `mongodb`.
        driver: Cow<'static, str>,
        /// What went wrong.
        message: String,
    },
    /// An operation could not run on the connection it names
    /// (`on_connection`, `#[model(connection = "..")]`): the app configures
    /// no such connection, or the executor is a unit of work of another
    /// one.
    #[error("connection `{connection}`: {message}")]
    Connection {
        /// The connection the operation names.
        connection: Cow<'static, str>,
        /// What went wrong.
        message: String,
    },
    /// The database stopped a statement that ran longer than the query's
    /// `timeout`.
    #[error("the statement ran longer than its timeout of {timeout:?} and was stopped")]
    Timeout {
        /// The query's timeout.
        timeout: std::time::Duration,
        /// The database's error.
        #[source]
        source: DbErr,
    },
}

impl OrmError {
    /// A [`ModelNotFound`](OrmError::ModelNotFound) error for `model` and the
    /// keys looked for (none when the query was not by key).
    pub fn not_found(model: impl Into<Cow<'static, str>>, keys: Vec<String>) -> Self {
        let model = model.into();
        let message = not_found_message(&model, &keys);
        OrmError::ModelNotFound {
            model,
            keys,
            source: DbErr::RecordNotFound(message),
        }
    }

    /// Whether this is a [`ModelNotFound`](OrmError::ModelNotFound) error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, OrmError::ModelNotFound { .. })
    }

    /// Whether the database refused a statement because it violates a
    /// unique constraint.
    pub fn is_unique_violation(&self) -> bool {
        matches!(
            self,
            OrmError::Db(error)
                if matches!(error.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_)))
        )
    }

    /// Whether the database stopped a statement for running longer than
    /// its query's `timeout`.
    pub fn is_timeout(&self) -> bool {
        matches!(self, OrmError::Timeout { .. })
    }

    /// Whether the connection to the database was lost under the
    /// statement: the server went away or ended the session, the network
    /// dropped it, or the pool was closed. The ORM reads again once, on
    /// another pooled connection, when a read outside a transaction loses
    /// its connection; inside a transaction the error stands (the
    /// transaction went with its connection), and so it does for writes
    /// (the write may have run before the connection dropped).
    ///
    /// ```
    /// use navree_orm::OrmError;
    /// use sea_orm::DbErr;
    ///
    /// let gone = OrmError::from(DbErr::Custom("2006 (HY000): MySQL server has gone away".into()));
    /// assert!(gone.is_lost_connection());
    /// assert!(!OrmError::from(DbErr::Custom("syntax error".into())).is_lost_connection());
    /// ```
    pub fn is_lost_connection(&self) -> bool {
        match self {
            OrmError::Db(error) => lost_connection(error),
            _ => false,
        }
    }

    /// `self` as a [`Timeout`](OrmError::Timeout) of `timeout` when the
    /// database stopped the statement for its time: PostgreSQL's
    /// `statement_timeout` (SQLSTATE 57014), MySQL's `MAX_EXECUTION_TIME`
    /// (3024), MariaDB's `max_statement_time` (1969).
    pub(crate) fn timed_out(self, timeout: std::time::Duration) -> Self {
        match self {
            OrmError::Db(source) if is_timeout_message(&source.to_string()) => {
                OrmError::Timeout { timeout, source }
            }
            other => other,
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        OrmError::InvalidQuery(message.into())
    }

    pub(crate) fn unsupported(
        feature: impl Into<Cow<'static, str>>,
        driver: impl Into<Cow<'static, str>>,
    ) -> Self {
        OrmError::Unsupported {
            feature: feature.into(),
            driver: driver.into(),
        }
    }
}

/// Whether `error` says the connection under a statement was lost.
pub(crate) fn lost_connection(error: &DbErr) -> bool {
    if matches!(
        error,
        DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed)
    ) {
        return true;
    }
    let message = error.to_string().to_ascii_lowercase();
    [
        // MySQL 2006 and 2013.
        "server has gone away",
        "lost connection to",
        // PostgreSQL 57P01 (pg_terminate_backend, shutdown) and a backend
        // that died.
        "terminating connection",
        "server closed the connection unexpectedly",
        "ssl connection has been closed unexpectedly",
        "no connection to the server",
        // The socket.
        "error communicating with database",
        "connection reset",
        "reset by peer",
        "broken pipe",
        "connection refused",
        "unexpected eof",
        "communication link failure",
        "decryption failed or bad record mac",
        // The pool.
        "attempted to acquire a connection on a closed pool",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Whether a database error message says a statement ran out of time.
fn is_timeout_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        // PostgreSQL, SQLSTATE 57014.
        "canceling statement due to statement timeout",
        // MySQL 3024.
        "maximum statement execution time exceeded",
        // MariaDB 1969.
        "max_statement_time exceeded",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn not_found_message(model: &str, keys: &[String]) -> String {
    match keys {
        [] => format!("no {model} found"),
        [key] => format!("no {model} found for key {key}"),
        keys => format!("no {model} found for keys {}", keys.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn not_found_carries_a_record_not_found_source() {
        let error = OrmError::not_found("Deal", vec!["1".into(), "2".into()]);
        assert_eq!(error.to_string(), "no Deal found for keys 1, 2");
        let source = error.source().expect("a source");
        assert!(matches!(
            source.downcast_ref::<DbErr>(),
            Some(DbErr::RecordNotFound(message)) if message == "no Deal found for keys 1, 2"
        ));
        assert_eq!(
            OrmError::not_found("Deal", Vec::new()).to_string(),
            "no Deal found"
        );
    }

    #[test]
    fn timeouts_are_recognised_by_the_database_message() {
        let timeout = std::time::Duration::from_millis(100);
        let postgres = OrmError::from(DbErr::Custom(
            "error returned from database: canceling statement due to statement timeout".into(),
        ))
        .timed_out(timeout);
        assert!(postgres.is_timeout());
        assert_eq!(
            postgres.to_string(),
            "the statement ran longer than its timeout of 100ms and was stopped"
        );
        assert!(postgres.source().unwrap().downcast_ref::<DbErr>().is_some());
        let mysql = OrmError::from(DbErr::Custom(
            "3024 (HY000): Query execution was interrupted, maximum statement execution time exceeded".into(),
        ))
        .timed_out(timeout);
        assert!(mysql.is_timeout());
        let mariadb = OrmError::from(DbErr::Custom(
            "1969 (70100): Query execution was interrupted (max_statement_time exceeded)".into(),
        ))
        .timed_out(timeout);
        assert!(mariadb.is_timeout());
        let other = OrmError::from(DbErr::Custom("syntax error".into())).timed_out(timeout);
        assert!(!other.is_timeout());
    }

    #[test]
    fn lost_connections_are_recognised() {
        for message in [
            "2006 (HY000): MySQL server has gone away",
            "2013 (HY000): Lost connection to MySQL server during query",
            "error returned from database: terminating connection due to administrator command",
            "error communicating with database: An existing connection was forcibly closed by the remote host. (os error 10054)",
            "error communicating with database: Connection reset by peer (os error 104)",
            "server closed the connection unexpectedly",
            "Broken pipe (os error 32)",
        ] {
            let error = OrmError::from(DbErr::Custom(message.into()));
            assert!(error.is_lost_connection(), "{message}");
        }
        assert!(
            OrmError::from(DbErr::ConnectionAcquire(
                sea_orm::ConnAcquireErr::ConnectionClosed
            ))
            .is_lost_connection()
        );
        for message in [
            "UNIQUE constraint failed: tags.name",
            "Connection pool timed out",
        ] {
            assert!(
                !OrmError::from(DbErr::Custom(message.into())).is_lost_connection(),
                "{message}"
            );
        }
        assert!(
            !OrmError::from(DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout))
                .is_lost_connection(),
            "an exhausted pool is not a lost connection"
        );
        assert!(!OrmError::invalid("x").is_lost_connection());
    }

    #[test]
    fn database_errors_expose_the_db_err_as_their_source() {
        let error = OrmError::from(DbErr::Custom("boom".into()));
        assert!(error.source().unwrap().downcast_ref::<DbErr>().is_some());
        assert!(!error.is_unique_violation());
        assert!(!error.is_not_found());
    }
}
