//! [`DbError`]: what went wrong talking to the database, classified.

use std::fmt;
use std::sync::Arc;

use crate::engine::Backend;

/// What kind of failure a [`DbError`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A statement that had to return a row returned none
    /// ([`Executor::fetch_one`](crate::Executor::fetch_one)).
    NotFound,
    /// A unique or primary-key constraint refused the write.
    UniqueViolation,
    /// A foreign-key constraint refused the write.
    ForeignKeyViolation,
    /// A `NOT NULL` constraint refused the write.
    NotNullViolation,
    /// A check (or exclusion) constraint refused the write.
    CheckViolation,
    /// The server picked this transaction as a deadlock victim and rolled
    /// it back (PostgreSQL `40P01`, MySQL 1213).
    Deadlock,
    /// A serializable transaction could not be ordered (PostgreSQL `40001`):
    /// run it again.
    SerializationFailure,
    /// A lock could not be taken: `NOWAIT`, a lock wait timeout (MySQL
    /// 1205), or SQLite's `database is locked` after its busy timeout.
    LockUnavailable,
    /// The server stopped a statement that ran past its time limit
    /// (PostgreSQL `statement_timeout`, MySQL `MAX_EXECUTION_TIME`).
    Timeout,
    /// The connection broke (the server went away, the network failed).
    ConnectionLost,
    /// No pooled connection became free in time.
    PoolTimeout,
    /// The pool is closed.
    PoolClosed,
    /// The database could not be connected to.
    Connect,
    /// A column could not be read as the type asked for, or does not exist.
    Decode,
    /// The statement or its values are wrong before anything runs
    /// (placeholder count, a statement for another backend, a value the
    /// backend cannot bind).
    InvalidStatement,
    /// Something this backend or build cannot do (a driver that is not
    /// compiled in, savepoints out of order).
    Unsupported,
    /// The transaction or unit of work is no longer usable (committed,
    /// rolled back, or broken by an earlier failure).
    TransactionDone,
    /// Any other error the database returned.
    Database,
    /// Anything else.
    Other,
}

/// What went wrong talking to the database.
///
/// [`kind`](DbError::kind) classifies it the same way on every backend (a
/// unique violation is [`ErrorKind::UniqueViolation`] whether PostgreSQL
/// said `23505`, MySQL 1062 or SQLite `2067`); [`code`](DbError::code) and
/// [`constraint`](DbError::constraint) carry the server's own details. The
/// message never contains bound values.
#[derive(Clone)]
pub struct DbError {
    kind: ErrorKind,
    message: String,
    code: Option<String>,
    constraint: Option<String>,
    source: Option<Arc<sqlx::Error>>,
}

impl DbError {
    /// An error of `kind` with `message`.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> DbError {
        DbError {
            kind,
            message: message.into(),
            code: None,
            constraint: None,
            source: None,
        }
    }

    /// A statement that is wrong before it runs.
    pub fn invalid(message: impl Into<String>) -> DbError {
        DbError::new(ErrorKind::InvalidStatement, message)
    }

    /// Something unsupported.
    pub fn unsupported(message: impl Into<String>) -> DbError {
        DbError::new(ErrorKind::Unsupported, message)
    }

    /// A statement that returned no row where one was needed.
    pub fn not_found(message: impl Into<String>) -> DbError {
        DbError::new(ErrorKind::NotFound, message)
    }

    /// A transaction or unit of work that is finished.
    pub fn done(message: impl Into<String>) -> DbError {
        DbError::new(ErrorKind::TransactionDone, message)
    }

    /// A driver that is not compiled in (connecting adds which feature to
    /// enable).
    pub(crate) fn driver_missing(backend: Backend) -> DbError {
        DbError::unsupported(format!("the {backend} driver is not compiled in"))
    }

    /// The driver's error, classified for `backend`.
    pub fn from_sqlx(error: sqlx::Error, backend: Option<Backend>) -> DbError {
        DbError::from_shared(Arc::new(error), backend)
    }

    /// A driver error shared with another error type, classified for
    /// `backend`.
    pub(crate) fn from_shared(error: Arc<sqlx::Error>, backend: Option<Backend>) -> DbError {
        let (kind, code, constraint) = classify(&error, backend);
        DbError {
            kind,
            message: error.to_string(),
            code,
            constraint,
            source: Some(error),
        }
    }

    /// The same error, of `kind`.
    pub(crate) fn with_kind(mut self, kind: ErrorKind) -> DbError {
        self.kind = kind;
        self
    }

    /// The same error, its message replaced.
    pub(crate) fn with_message(mut self, message: impl Into<String>) -> DbError {
        self.message = message.into();
        self
    }

    /// What kind of failure it is.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The server's error code: a SQLSTATE on PostgreSQL (`23505`), the
    /// error number on MySQL (`1062`), the extended result code on SQLite
    /// (`2067`).
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// The constraint the server named, when it names one.
    pub fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }

    /// The driver's error, when it came from the driver.
    pub fn sqlx_error(&self) -> Option<&Arc<sqlx::Error>> {
        self.source.as_ref()
    }

    /// A statement returned no row where one was needed.
    pub fn is_not_found(&self) -> bool {
        self.kind == ErrorKind::NotFound
    }

    /// A unique or primary-key constraint refused the write.
    pub fn is_unique_violation(&self) -> bool {
        self.kind == ErrorKind::UniqueViolation
    }

    /// A foreign-key constraint refused the write.
    pub fn is_foreign_key_violation(&self) -> bool {
        self.kind == ErrorKind::ForeignKeyViolation
    }

    /// The connection broke: a read outside a transaction may run again on
    /// another connection.
    pub fn is_connection_lost(&self) -> bool {
        self.kind == ErrorKind::ConnectionLost
    }

    /// Running the transaction again may succeed: a deadlock, a
    /// serialization failure, or a lock that was not free.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Deadlock | ErrorKind::SerializationFailure | ErrorKind::LockUnavailable
        )
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::InvalidStatement => write!(f, "invalid statement: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl fmt::Debug for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("code", &self.code)
            .field("constraint", &self.constraint)
            .finish()
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

/// The kind, code and constraint of a driver error.
fn classify(
    error: &sqlx::Error,
    backend: Option<Backend>,
) -> (ErrorKind, Option<String>, Option<String>) {
    use sqlx::Error as E;
    let kind = match error {
        E::RowNotFound => ErrorKind::NotFound,
        E::PoolTimedOut => ErrorKind::PoolTimeout,
        E::PoolClosed => ErrorKind::PoolClosed,
        E::Io(_) | E::Protocol(_) | E::WorkerCrashed => ErrorKind::ConnectionLost,
        E::Tls(_) | E::Configuration(_) => ErrorKind::Connect,
        E::TypeNotFound { .. }
        | E::ColumnIndexOutOfBounds { .. }
        | E::ColumnNotFound(_)
        | E::ColumnDecode { .. }
        | E::Decode(_) => ErrorKind::Decode,
        E::Encode(_) | E::InvalidArgument(_) => ErrorKind::InvalidStatement,
        E::InvalidSavePointStatement | E::BeginFailed => ErrorKind::Unsupported,
        E::Database(database) => {
            let backend = backend.or_else(|| backend_of(database.as_ref()));
            let code = server_code(database.as_ref());
            let constraint = database.constraint().map(str::to_owned);
            let kind = database_kind(database.as_ref(), code.as_deref(), backend);
            return (kind, code, constraint);
        }
        _ => ErrorKind::Other,
    };
    (kind, None, None)
}

/// The backend whose driver made `error`.
fn backend_of(error: &dyn sqlx::error::DatabaseError) -> Option<Backend> {
    #[cfg(feature = "postgres")]
    if error
        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
        .is_some()
    {
        return Some(Backend::Postgres);
    }
    #[cfg(feature = "mysql")]
    if error
        .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
        .is_some()
    {
        return Some(Backend::MySql);
    }
    #[cfg(feature = "sqlite")]
    if error
        .try_downcast_ref::<sqlx::sqlite::SqliteError>()
        .is_some()
    {
        return Some(Backend::Sqlite);
    }
    let _ = error;
    None
}

/// The server's code of `error`: the SQLSTATE on PostgreSQL, the error
/// number on MySQL (its SQLSTATEs are too coarse: a deadlock and a
/// serialization failure share `40001`), the extended result code on
/// SQLite.
fn server_code(error: &dyn sqlx::error::DatabaseError) -> Option<String> {
    #[cfg(feature = "mysql")]
    if let Some(mysql) = error.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>() {
        return Some(mysql.number().to_string());
    }
    error.code().map(|code| code.into_owned())
}

/// The kind of an error the server returned.
fn database_kind(
    error: &dyn sqlx::error::DatabaseError,
    code: Option<&str>,
    backend: Option<Backend>,
) -> ErrorKind {
    use sqlx::error::ErrorKind as Driver;
    match error.kind() {
        Driver::UniqueViolation => return ErrorKind::UniqueViolation,
        Driver::ForeignKeyViolation => return ErrorKind::ForeignKeyViolation,
        Driver::NotNullViolation => return ErrorKind::NotNullViolation,
        Driver::CheckViolation | Driver::ExclusionViolation => return ErrorKind::CheckViolation,
        _ => {}
    }
    let (Some(code), Some(backend)) = (code, backend) else {
        return ErrorKind::Database;
    };
    match (backend, code) {
        (Backend::Postgres, "40P01") => ErrorKind::Deadlock,
        (Backend::Postgres, "40001") => ErrorKind::SerializationFailure,
        (Backend::Postgres, "55P03") => ErrorKind::LockUnavailable,
        (Backend::Postgres, "57014") => ErrorKind::Timeout,
        (Backend::Postgres, "25P02") => ErrorKind::TransactionDone,
        (Backend::Postgres, "57P01" | "57P02" | "57P03") => ErrorKind::ConnectionLost,
        (Backend::Postgres, code) if code.starts_with("08") => ErrorKind::ConnectionLost,
        // ER_DUP_ENTRY and its relatives, in case the driver did not say.
        (Backend::MySql, "1062" | "1586" | "1859") => ErrorKind::UniqueViolation,
        (Backend::MySql, "1451" | "1452" | "1216" | "1217") => ErrorKind::ForeignKeyViolation,
        (Backend::MySql, "1048") => ErrorKind::NotNullViolation,
        (Backend::MySql, "3819") => ErrorKind::CheckViolation,
        (Backend::MySql, "1213") => ErrorKind::Deadlock,
        (Backend::MySql, "1205" | "3572") => ErrorKind::LockUnavailable,
        (Backend::MySql, "3024" | "1969") => ErrorKind::Timeout,
        (Backend::MySql, "2006" | "2013" | "1053" | "1927") => ErrorKind::ConnectionLost,
        // SQLITE_BUSY and SQLITE_LOCKED with their extended codes.
        (Backend::Sqlite, code) => match code.parse::<u32>().map(|code| code & 0xff) {
            Ok(5 | 6) => ErrorKind::LockUnavailable,
            _ => ErrorKind::Database,
        },
        _ => ErrorKind::Database,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_errors_are_classified() {
        assert_eq!(
            DbError::from_sqlx(sqlx::Error::RowNotFound, None).kind(),
            ErrorKind::NotFound
        );
        assert_eq!(
            DbError::from_sqlx(sqlx::Error::PoolTimedOut, None).kind(),
            ErrorKind::PoolTimeout
        );
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
        let lost = DbError::from_sqlx(sqlx::Error::Io(io), Some(Backend::Postgres));
        assert!(lost.is_connection_lost());
        assert!(std::error::Error::source(&lost).is_some());
    }

    #[test]
    fn messages_and_predicates() {
        let error = DbError::invalid("two values, one placeholder");
        assert_eq!(
            error.to_string(),
            "invalid statement: two values, one placeholder"
        );
        assert_eq!(error.kind(), ErrorKind::InvalidStatement);
        assert!(DbError::new(ErrorKind::Deadlock, "x").is_retryable());
        assert!(!DbError::not_found("none").is_retryable());
        assert!(DbError::not_found("none").is_not_found());
    }
}
