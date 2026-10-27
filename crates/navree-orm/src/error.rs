//! [`OrmError`]: what can go wrong in the ORM, and [`Result`].

use std::borrow::Cow;

use sea_orm::DbErr;

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

    /// Whether the database refused a statement because it violates a
    /// unique constraint.
    pub fn is_unique_violation(&self) -> bool {
        matches!(
            self,
            OrmError::Db(error)
                if matches!(error.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_)))
        )
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
    fn database_errors_expose_the_db_err_as_their_source() {
        let error = OrmError::from(DbErr::Custom("boom".into()));
        assert!(error.source().unwrap().downcast_ref::<DbErr>().is_some());
        assert!(!error.is_unique_violation());
        assert!(!error.is_not_found());
    }
}
