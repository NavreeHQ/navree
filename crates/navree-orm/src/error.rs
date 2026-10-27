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
/// Every message names the model and, where one is involved, the column,
/// and says how to fix the problem.
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
    /// A row lacks a model column (a partial select left it out) and the
    /// strict mode `prevent_missing_attributes` is on, or the field has no
    /// default to read instead.
    #[error(
        "cannot read column `{column}` of {model} as {expected}: the row has no such column (it has: {}); select it, or mark the field #[model(default)]",
        selected.join(", ")
    )]
    MissingAttribute {
        /// The model.
        model: Cow<'static, str>,
        /// The missing column.
        column: String,
        /// The field's Rust type.
        expected: Cow<'static, str>,
        /// The columns the row has.
        selected: Vec<String>,
    },
    /// Mass assignment (`fill`) was given columns it may not set: the
    /// model declares no fillable columns at all, or the app refuses to
    /// discard attributes silently and these are not fillable.
    #[error("{}", mass_assignment_message(model, columns, *totally_guarded))]
    MassAssignment {
        /// The model.
        model: Cow<'static, str>,
        /// The columns (or keys) that were refused.
        columns: Vec<String>,
        /// Whether the model declares neither `fillable` nor `guarded`
        /// columns (every `fill` is refused).
        totally_guarded: bool,
    },
    /// A relation was loaded lazily for a record that came with others (one
    /// query per record), and the strict mode `prevent_lazy_loading` is on
    /// (or [`LazyLoading`](crate::relations::LazyLoading) prevents it).
    #[error(
        "relation `{relation}` of {model} was loaded lazily, one record at a time: eager load it (with(\"{relation}\")) on the query that read the records, or on the collection (load)"
    )]
    LazyLoading {
        /// The model.
        model: Cow<'static, str>,
        /// The relation.
        relation: Cow<'static, str>,
    },
    /// A relation path (`with("company.owner")`, `has("activities")`, ...)
    /// names a relation the model does not declare.
    #[error("{model} has no relation `{relation}`{}", known_relations(known))]
    RelationNotFound {
        /// The model the relation was looked up on.
        model: Cow<'static, str>,
        /// The relation name that was asked for.
        relation: String,
        /// The relations the model does declare.
        known: Vec<String>,
    },
    /// A model was used in a polymorphic relation without an entry in the
    /// morph map while the map is required (see
    /// [`MorphMap`](crate::relations::MorphMap)).
    #[error(
        "{model} has no entry in the morph map, which is required: register it with MorphMap::register::<{model}>()"
    )]
    MorphNotMapped {
        /// The model.
        model: Cow<'static, str>,
    },
    /// A polymorphic type column holds a name no model is registered
    /// under.
    #[error(
        "no model is registered under the morph name `{name}`: register the model with MorphMap::register, or list it on the morph_to relation with `.types::<..>()`"
    )]
    UnknownMorph {
        /// The stored type name.
        name: String,
    },
    /// A record could not be serialized.
    #[error("{}", serialize_message(model, column.as_deref(), message))]
    Serialize {
        /// The model.
        model: Cow<'static, str>,
        /// The column, appended value or relation whose value failed;
        /// `None` when the model as a whole did not serialize.
        column: Option<String>,
        /// What went wrong.
        message: String,
    },
    /// A workspace-scoped model was used outside the rules of workspaces:
    /// no current workspace, a record created in or moved to another
    /// workspace. (A record of another workspace is
    /// [`ModelNotFound`](OrmError::ModelNotFound).)
    #[error("{message}")]
    Workspace {
        /// The model.
        model: Cow<'static, str>,
        /// What went wrong, and how to fix it.
        message: String,
    },
    /// A model factory could not build or create its records: a state
    /// failed, a relationship is misdeclared, a hook cancelled a save.
    #[error("the {model} factory failed: {message}")]
    Factory {
        /// The model the factory builds.
        model: Cow<'static, str>,
        /// What went wrong.
        message: String,
    },
}

fn known_relations(known: &[String]) -> String {
    match known {
        [] => " (it declares no relations: add them in a #[relations] impl block)".to_owned(),
        known => format!(" (it has: {})", known.join(", ")),
    }
}

fn mass_assignment_message(model: &str, columns: &[String], totally_guarded: bool) -> String {
    let list = columns
        .iter()
        .map(|column| format!("`{column}`"))
        .collect::<Vec<_>>()
        .join(", ");
    if totally_guarded {
        format!(
            "cannot mass-assign {list} on {model}: {model} declares no fillable columns; list them with #[model(fillable(..))] (or guard some with #[model(guarded(..))]), or use force_fill"
        )
    } else {
        format!(
            "cannot mass-assign {list} on {model}: not fillable; add {} to #[model(fillable(..))], or use force_fill",
            if columns.len() == 1 { "it" } else { "them" }
        )
    }
}

fn serialize_message(model: &str, column: Option<&str>, message: &str) -> String {
    match column {
        Some(column) => format!("cannot serialize `{column}` of {model}: {message}"),
        None => format!("cannot serialize {model}: {message}"),
    }
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

    pub(crate) fn factory(model: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        OrmError::Factory {
            model: model.into(),
            message: message.into(),
        }
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

/// An ORM error as a SeaORM [`DbErr`], for code whose API speaks `DbErr`
/// (navree-db's repositories, the framework crates' services) and runs ORM
/// operations inside: `?` converts.
///
/// The database's own error comes back as it was, so a unique or
/// foreign-key violation still answers 409 over HTTP; a missing record is
/// `DbErr::RecordNotFound` (a 404); a timeout is the database's error;
/// anything else (a column that does not decode, a cast that fails, an
/// invalid query) is `DbErr::Custom` with the ORM's message.
///
/// ```
/// use navree_orm::OrmError;
/// use sea_orm::DbErr;
///
/// let missing = DbErr::from(OrmError::not_found("Deal", vec!["42".to_owned()]));
/// assert!(matches!(missing, DbErr::RecordNotFound(message) if message == "no Deal found for key 42"));
/// let refused = DbErr::from(OrmError::from(DbErr::Custom("UNIQUE constraint failed".into())));
/// assert!(matches!(refused, DbErr::Custom(message) if message == "UNIQUE constraint failed"));
/// ```
impl From<OrmError> for DbErr {
    fn from(error: OrmError) -> Self {
        match error {
            OrmError::Db(source)
            | OrmError::ModelNotFound { source, .. }
            | OrmError::Timeout { source, .. } => source,
            other => DbErr::Custom(other.to_string()),
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
    fn strict_mode_errors_say_what_to_do() {
        let missing = OrmError::MissingAttribute {
            model: "Deal".into(),
            column: "title".into(),
            expected: "String".into(),
            selected: vec!["id".into(), "stage".into()],
        };
        assert_eq!(
            missing.to_string(),
            "cannot read column `title` of Deal as String: the row has no such column (it has: id, stage); select it, or mark the field #[model(default)]"
        );
        let discarded = OrmError::MassAssignment {
            model: "User".into(),
            columns: vec!["role".into()],
            totally_guarded: false,
        };
        assert_eq!(
            discarded.to_string(),
            "cannot mass-assign `role` on User: not fillable; add it to #[model(fillable(..))], or use force_fill"
        );
        let lazy = OrmError::LazyLoading {
            model: "Deal".into(),
            relation: "company".into(),
        };
        assert!(
            lazy.to_string()
                .starts_with("relation `company` of Deal was loaded lazily")
        );
    }

    #[test]
    fn mass_assignment_messages_name_the_fix() {
        let guarded = OrmError::MassAssignment {
            model: "Deal".into(),
            columns: vec!["title".into()],
            totally_guarded: true,
        };
        assert_eq!(
            guarded.to_string(),
            "cannot mass-assign `title` on Deal: Deal declares no fillable columns; list them with #[model(fillable(..))] (or guard some with #[model(guarded(..))]), or use force_fill"
        );
        let refused = OrmError::MassAssignment {
            model: "Deal".into(),
            columns: vec!["owner_id".into(), "is_admin".into()],
            totally_guarded: false,
        };
        assert_eq!(
            refused.to_string(),
            "cannot mass-assign `owner_id`, `is_admin` on Deal: not fillable; add them to #[model(fillable(..))], or use force_fill"
        );
    }

    #[test]
    fn serialize_errors_name_the_model_and_column() {
        let error = OrmError::Serialize {
            model: "Deal".into(),
            column: Some("meta".into()),
            message: "key must be a string".into(),
        };
        assert_eq!(
            error.to_string(),
            "cannot serialize `meta` of Deal: key must be a string"
        );
        let whole = OrmError::Serialize {
            model: "Deal".into(),
            column: None,
            message: "it serializes as a string, not an object".into(),
        };
        assert_eq!(
            whole.to_string(),
            "cannot serialize Deal: it serializes as a string, not an object"
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

    #[test]
    fn orm_errors_become_db_errors_keeping_the_database_s_own() {
        assert!(matches!(
            DbErr::from(OrmError::from(DbErr::RecordNotUpdated)),
            DbErr::RecordNotUpdated
        ));
        assert!(matches!(
            DbErr::from(OrmError::not_found("Deal", Vec::new())),
            DbErr::RecordNotFound(message) if message == "no Deal found"
        ));
        let timeout = OrmError::from(DbErr::Custom(
            "canceling statement due to statement timeout".into(),
        ))
        .timed_out(std::time::Duration::from_secs(1));
        assert!(matches!(
            DbErr::from(timeout),
            DbErr::Custom(message) if message.contains("statement timeout")
        ));
        let decode = OrmError::Decode {
            model: "Deal".into(),
            column: "amount".into(),
            expected: "i64".into(),
            message: "not a number".into(),
        };
        assert!(matches!(
            DbErr::from(decode),
            DbErr::Custom(message)
                if message == "cannot read column `amount` of Deal as i64: not a number"
        ));
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
            error.to_string().ends_with(
                "mongodb: E11000 duplicate key error collection: app.users index: email_1"
            ),
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
