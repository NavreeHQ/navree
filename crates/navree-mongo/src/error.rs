//! [`MongoError`]: what can go wrong outside a model query (connecting,
//! sessions, the schema builder, migrations), and how MongoDB's own errors
//! become [`OrmError`]s.

use std::borrow::Cow;

use navree_orm::OrmError;

/// The result of an operation of this crate.
pub type MongoResult<T = (), E = MongoError> = std::result::Result<T, E>;

/// The driver's name in ORM driver errors (`OrmError::Driver`: `mongodb:
/// ...`) and [`DocumentDriver::name`](navree_orm::DocumentDriver::name).
pub(crate) const DRIVER: &str = "mongodb";

/// The database's name in [`OrmError::Unsupported`] (`... is not supported
/// on MongoDB`).
pub(crate) const DATABASE: &str = "MongoDB";

/// A MongoDB error: invalid options, a connection, session or command
/// failure, something MongoDB cannot do, or a migration that failed.
///
/// Messages never carry a connection string's password, and the values of a
/// failed command's documents stay out of them.
///
/// ```
/// use navree_mongo::MongoError;
///
/// let error = MongoError::unsupported("foreign keys", "MongoDB does not enforce references");
/// assert_eq!(error.to_string(), "foreign keys is not supported on MongoDB: MongoDB does not enforce references");
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MongoError {
    /// The options are invalid (each problem names its key).
    #[error("invalid MongoDB options: {}", problems.join("; "))]
    Options {
        /// The problems.
        problems: Vec<String>,
    },
    /// MongoDB (or the driver) refused or failed a command.
    #[error("MongoDB: {0}")]
    Driver(#[from] mongodb::error::Error),
    /// A document could not be converted (BSON, extended JSON).
    #[error("MongoDB: {0}")]
    Bson(String),
    /// The deployment cannot run transactions (a standalone server).
    #[error(
        "MongoDB transactions need a replica set or a sharded cluster: {message} (a single-node replica set works: start mongod with --replSet rs0 and run rs.initiate())"
    )]
    Transactions {
        /// What the server said.
        message: String,
    },
    /// MongoDB has no equivalent of what was asked.
    #[error("{feature} is not supported on MongoDB: {hint}")]
    Unsupported {
        /// What was asked for.
        feature: Cow<'static, str>,
        /// Why not, and what to do instead.
        hint: Cow<'static, str>,
    },
    /// A collection described by a blueprint cannot be created or changed
    /// as described.
    #[error("collection `{collection}`: {message}")]
    Schema {
        /// The collection.
        collection: String,
        /// What is wrong, and how to fix it.
        message: String,
    },
    /// A migration failed.
    #[error("migration `{migration}` failed: {source}")]
    Migration {
        /// `module/name`.
        migration: String,
        /// Why.
        #[source]
        source: Box<MongoError>,
    },
    /// The migration sets cannot be ordered or are inconsistent with the
    /// migrations collection.
    #[error("{0}")]
    Migrations(String),
    /// A unit of work was used after it committed or rolled back.
    #[error("the MongoDB unit of work is already {0}")]
    Finished(&'static str),
    /// A model operation failed.
    #[error(transparent)]
    Orm(#[from] OrmError),
}

impl MongoError {
    /// An [`Unsupported`](MongoError::Unsupported) error.
    pub fn unsupported(
        feature: impl Into<Cow<'static, str>>,
        hint: impl Into<Cow<'static, str>>,
    ) -> MongoError {
        MongoError::Unsupported {
            feature: feature.into(),
            hint: hint.into(),
        }
    }

    /// A [`Schema`](MongoError::Schema) error for `collection`.
    pub fn schema(collection: impl Into<String>, message: impl Into<String>) -> MongoError {
        MongoError::Schema {
            collection: collection.into(),
            message: message.into(),
        }
    }

    /// Whether MongoDB refused a write because it duplicates a unique key.
    pub fn is_duplicate_key(&self) -> bool {
        match self {
            MongoError::Driver(error) => is_duplicate_key(error),
            MongoError::Orm(error) => error.is_unique_violation(),
            _ => false,
        }
    }

    /// The error with `migration` (`module/name`) named.
    pub(crate) fn in_migration(self, migration: impl Into<String>) -> MongoError {
        MongoError::Migration {
            migration: migration.into(),
            source: Box::new(self),
        }
    }
}

impl From<bson::error::Error> for MongoError {
    fn from(error: bson::error::Error) -> Self {
        MongoError::Bson(error.to_string())
    }
}

/// A MongoDB error as the ORM reports it: duplicate keys are unique
/// violations (409 over HTTP), errors the ORM raised stay themselves,
/// anything else is a driver error.
impl From<MongoError> for OrmError {
    fn from(error: MongoError) -> Self {
        match error {
            MongoError::Orm(error) => error,
            MongoError::Driver(error) => orm_error(&error),
            MongoError::Unsupported { feature, hint } => OrmError::Unsupported {
                feature: Cow::Owned(format!("{feature} ({hint})")),
                driver: Cow::Borrowed(DATABASE),
            },
            other => OrmError::Driver {
                driver: Cow::Borrowed(DRIVER),
                message: other.to_string(),
            },
        }
    }
}

/// The codes MongoDB reports duplicate keys with: 11000, the legacy 11001,
/// and 12582 (a duplicate found while an update ran).
const DUPLICATE_KEY_CODES: [i32; 3] = [11000, 11001, 12582];

/// The server's error codes in `error`: the command's, each write error's
/// and the write concern error's.
pub(crate) fn error_codes(error: &mongodb::error::Error) -> Vec<i32> {
    use mongodb::error::{ErrorKind, WriteFailure};

    match error.kind.as_ref() {
        ErrorKind::Command(command) => vec![command.code],
        ErrorKind::Write(WriteFailure::WriteError(write)) => vec![write.code],
        ErrorKind::Write(WriteFailure::WriteConcernError(concern)) => vec![concern.code],
        ErrorKind::InsertMany(insert) => {
            let mut codes: Vec<i32> = insert
                .write_errors
                .iter()
                .flatten()
                .map(|write| write.code)
                .collect();
            codes.extend(insert.write_concern_error.iter().map(|concern| concern.code));
            codes
        }
        ErrorKind::BulkWrite(bulk) => {
            let mut codes: Vec<i32> = bulk.write_errors.values().map(|write| write.code).collect();
            codes.extend(bulk.write_concern_errors.iter().map(|concern| concern.code));
            codes
        }
        _ => Vec::new(),
    }
}

/// Whether `error` reports a duplicate key.
pub(crate) fn is_duplicate_key(error: &mongodb::error::Error) -> bool {
    error_codes(error)
        .iter()
        .any(|code| DUPLICATE_KEY_CODES.contains(code))
}

/// Whether `error` says the deployment cannot run transactions: a
/// standalone server refuses transaction numbers (code 20,
/// IllegalOperation) and sessions it cannot track.
pub(crate) fn is_transactions_unsupported(error: &mongodb::error::Error) -> bool {
    use mongodb::error::ErrorKind;

    if matches!(error.kind.as_ref(), ErrorKind::SessionsNotSupported) {
        return true;
    }
    let message = error.to_string();
    error_codes(error).contains(&20)
        && (message.contains("Transaction numbers are only allowed")
            || message.contains("transaction"))
}

/// `error` as an [`OrmError`]: a duplicate key is a unique violation, a
/// standalone server's refusal explains transactions, anything else is a
/// driver error with MongoDB's message.
pub(crate) fn orm_error(error: &mongodb::error::Error) -> OrmError {
    if is_duplicate_key(error) {
        return OrmError::unique_violation(DRIVER, error.to_string());
    }
    if is_transactions_unsupported(error) {
        return OrmError::Driver {
            driver: Cow::Borrowed(DRIVER),
            message: MongoError::Transactions {
                message: error.to_string(),
            }
            .to_string(),
        };
    }
    OrmError::Driver {
        driver: Cow::Borrowed(DRIVER),
        message: error.to_string(),
    }
}

/// An [`OrmError::Unsupported`] for MongoDB: `feature` names what was
/// asked and, after a colon or in parentheses, what to do instead.
pub(crate) fn unsupported(feature: impl Into<Cow<'static, str>>) -> OrmError {
    OrmError::Unsupported {
        feature: feature.into(),
        driver: Cow::Borrowed(DATABASE),
    }
}

/// An [`OrmError::InvalidQuery`].
pub(crate) fn invalid(message: impl Into<String>) -> OrmError {
    OrmError::InvalidQuery(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_features_name_the_hint() {
        let error: OrmError = MongoError::unsupported("row locks", "use a transaction").into();
        assert!(matches!(&error, OrmError::Unsupported { feature, driver }
            if feature == "row locks (use a transaction)" && driver == "MongoDB"));
        assert_eq!(
            error.to_string(),
            "row locks (use a transaction) is not supported on MongoDB"
        );
    }

    #[test]
    fn orm_errors_pass_through() {
        let error: OrmError = MongoError::Orm(OrmError::not_found("Event", Vec::new())).into();
        assert!(error.is_not_found());
        assert!(
            MongoError::Orm(OrmError::unique_violation("mongodb", "E11000")).is_duplicate_key()
        );
    }

    #[test]
    fn transaction_errors_explain_replica_sets() {
        let error = MongoError::Transactions {
            message: "Transaction numbers are only allowed on a replica set member or mongos"
                .into(),
        };
        let text = error.to_string();
        assert!(text.contains("replica set") && text.contains("--replSet"), "{text}");
    }
}
