//! [`SchemaError`] and [`SchemaResult`].

use sea_orm::DbErr;

use crate::target::Dialect;

/// The result of a schema operation; `Ok(())` by default, so a migration's
/// `up` can return `SchemaResult`.
pub type SchemaResult<T = ()> = Result<T, SchemaError>;

/// Why a schema operation failed: what, on which table or statement, and
/// what to do about it.
///
/// ```
/// use navree_schema::{Dialect, SchemaError};
///
/// let error = SchemaError::unsupported(Dialect::Sqlite, "spatial indexes", "use PostgreSQL with PostGIS or MySQL");
/// assert_eq!(
///     error.to_string(),
///     "SQLite does not support spatial indexes: use PostgreSQL with PostGIS or MySQL"
/// );
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// The database rejected a statement.
    #[error("{source}\n  while running: {statement}")]
    Database {
        /// The statement that failed (or the query, for introspection).
        statement: String,
        /// The driver's error.
        #[source]
        source: DbErr,
    },
    /// The database cannot do what the blueprint asks.
    #[error("{dialect} does not support {feature}: {hint}")]
    Unsupported {
        /// The database family.
        dialect: Dialect,
        /// What was asked, e.g. `set columns`.
        feature: String,
        /// What to do instead.
        hint: String,
    },
    /// The blueprint itself is invalid (two primary keys, a foreign key
    /// without a table, a generated column with a default, ...).
    #[error("invalid definition of `{table}`: {message}")]
    Definition {
        /// The table being defined.
        table: String,
        /// What is wrong and how to fix it.
        message: String,
    },
    /// A hypertable cannot be created or converted as asked.
    #[error("cannot make `{table}` a hypertable: {message}")]
    Hypertable {
        /// The table.
        table: String,
        /// What is wrong and how to fix it.
        message: String,
    },
    /// An interval (`"7 days"`) that does not parse.
    #[error(
        "`{input}` is not an interval: write `<number> <unit>`, with unit second, minute, hour, day, week, month or year (for example `7 days`)"
    )]
    Interval {
        /// The text given.
        input: String,
    },
    /// The operation needs a live database, but the schema is offline
    /// ([`Schema::offline`](crate::Schema::offline)).
    #[error(
        "{operation} needs a database connection; this schema is offline (it only records statements)"
    )]
    Offline {
        /// What was attempted, e.g. `reading the columns of \`deals\``.
        operation: String,
    },
    /// A table, column or index the operation needs does not exist.
    #[error("{message}")]
    Missing {
        /// What is missing, and where.
        message: String,
    },
    /// A schema dump could not be written or read.
    #[error("schema dump: {message}")]
    Dump {
        /// What failed.
        message: String,
    },
}

impl SchemaError {
    /// An [`Unsupported`](SchemaError::Unsupported) error.
    pub fn unsupported(
        dialect: Dialect,
        feature: impl Into<String>,
        hint: impl Into<String>,
    ) -> SchemaError {
        SchemaError::Unsupported {
            dialect,
            feature: feature.into(),
            hint: hint.into(),
        }
    }

    /// A [`Definition`](SchemaError::Definition) error for `table`.
    pub fn definition(table: impl Into<String>, message: impl Into<String>) -> SchemaError {
        SchemaError::Definition {
            table: table.into(),
            message: message.into(),
        }
    }

    /// A [`Hypertable`](SchemaError::Hypertable) error for `table`.
    pub fn hypertable(table: impl Into<String>, message: impl Into<String>) -> SchemaError {
        SchemaError::Hypertable {
            table: table.into(),
            message: message.into(),
        }
    }

    /// A [`Database`](SchemaError::Database) error for `statement`.
    pub fn database(statement: impl Into<String>, source: DbErr) -> SchemaError {
        SchemaError::Database {
            statement: statement.into(),
            source,
        }
    }

    /// A [`Missing`](SchemaError::Missing) error.
    pub fn missing(message: impl Into<String>) -> SchemaError {
        SchemaError::Missing {
            message: message.into(),
        }
    }
}

impl From<SchemaError> for DbErr {
    /// Inside the migrator a schema error becomes `DbErr::Migration` with
    /// the same text.
    fn from(error: SchemaError) -> DbErr {
        match error {
            SchemaError::Database { source, statement } => {
                DbErr::Migration(format!("{source}\n  while running: {statement}"))
            }
            other => DbErr::Migration(other.to_string()),
        }
    }
}
