//! [`OrmError`]: what can go wrong in the ORM, and [`Result`].

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use sea_orm::sqlx::error::{DatabaseError, ErrorKind};
use sea_orm::{DbErr, RuntimeErr, SqlErr};

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

    /// A unique-constraint violation a document store reported (MongoDB's
    /// duplicate key error). [`is_unique_violation`](Self::is_unique_violation)
    /// is true for it, so `create_or_first` and friends recover from it,
    /// and returned from a handler it answers 409 like an SQL database's
    /// violation: its source is a [`DbErr`] that navree-db maps. `message`
    /// is the store's own, for the logs; it never reaches a client.
    ///
    /// ```
    /// use navree_orm::OrmError;
    ///
    /// let error = OrmError::unique_violation("mongodb", "E11000 duplicate key error collection: app.users");
    /// assert!(error.is_unique_violation());
    /// assert!(error.to_string().ends_with("mongodb: E11000 duplicate key error collection: app.users"));
    /// ```
    pub fn unique_violation(driver: &'static str, message: impl Into<String>) -> Self {
        let violation = StoreViolation {
            driver,
            message: message.into(),
        };
        OrmError::Db(DbErr::Exec(RuntimeErr::SqlxError(Arc::new(
            sea_orm::sqlx::Error::Database(Box::new(violation)),
        ))))
    }

    /// Whether the database refused a statement because it violates a
    /// unique constraint: on every SQL database, and for a document store's
    /// [`unique_violation`](Self::unique_violation).
    pub fn is_unique_violation(&self) -> bool {
        let OrmError::Db(error) = self else {
            return false;
        };
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            return true;
        }
        // Violations SeaORM does not read by code, classified by the driver
        // that raised them: a document store's, MySQL's 1859.
        match error {
            DbErr::Exec(RuntimeErr::SqlxError(error))
            | DbErr::Query(RuntimeErr::SqlxError(error)) => error
                .as_database_error()
                .is_some_and(|error| matches!(error.kind(), ErrorKind::UniqueViolation)),
            _ => false,
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

/// A document store's unique-constraint violation, in the shape SQLx gives
/// database errors, so navree-db's error mapping and
/// [`OrmError::is_unique_violation`] read it like an SQL database's.
#[derive(Debug)]
struct StoreViolation {
    driver: &'static str,
    message: String,
}

impl fmt::Display for StoreViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.driver, self.message)
    }
}

impl std::error::Error for StoreViolation {}

impl DatabaseError for StoreViolation {
    fn message(&self) -> &str {
        &self.message
    }

    fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self
    }

    fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
        self
    }

    fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
        self
    }

    fn kind(&self) -> ErrorKind {
        ErrorKind::UniqueViolation
    }
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
    fn database_errors_expose_the_db_err_as_their_source() {
        let error = OrmError::from(DbErr::Custom("boom".into()));
        assert!(error.source().unwrap().downcast_ref::<DbErr>().is_some());
        assert!(!error.is_unique_violation());
        assert!(!error.is_not_found());
    }

    /// A document store's duplicate key is a unique violation, and its
    /// `DbErr` answers 409 over HTTP like the SQL databases' violations.
    #[test]
    fn document_store_duplicates_are_unique_violations() {
        let error = OrmError::unique_violation(
            "mongodb",
            "E11000 duplicate key error collection: app.users index: email_1",
        );
        assert!(error.is_unique_violation());
        assert!(!error.is_not_found());
        assert!(
            error
                .to_string()
                .ends_with("mongodb: E11000 duplicate key error collection: app.users index: email_1"),
            "{error}"
        );
        let source = error.source().unwrap().downcast_ref::<DbErr>().unwrap();
        let response = navree_db::map_db_error(source).expect("a violation");
        assert_eq!(response.status.as_u16(), 409);
        assert_eq!(
            response.body()["message"],
            "A record with the same unique value already exists."
        );
    }
}
