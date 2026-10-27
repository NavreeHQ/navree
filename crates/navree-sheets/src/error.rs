//! [`SheetsError`] and its error responses.

use std::collections::BTreeMap;

use navree_contracts::BoxError;
use navree_http::AppError;

/// What can go wrong in an export or an import.
///
/// Returned from a handler with `?` in an app with `SheetsModule`, it
/// becomes the error response [`SheetsError::to_app_error`] describes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SheetsError {
    /// Writing CSV failed.
    #[error("CSV: {0}")]
    Csv(#[from] csv::Error),
    /// Writing XLSX failed.
    #[error("XLSX: {0}")]
    Xlsx(String),
    /// Reading or writing bytes failed.
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    /// The storage disk failed.
    #[error("storage: {0}")]
    Storage(#[from] navree_storage::StorageError),
    /// The queue or an operation failed.
    #[error("queue: {0}")]
    Queue(#[from] navree_queue::QueueError),
    /// The database failed.
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    /// A value did not (de)serialize.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The app's export or import code failed.
    #[error("{0}")]
    App(BoxError),
    /// A module the call needs is not in the app (`QueueModule`,
    /// `StorageModule`, `DatabaseModule`).
    #[error("{0}")]
    Missing(&'static str),
    /// No export of this name is registered.
    #[error("no export named `{0}` is registered (m.export::<E>())")]
    UnknownExport(String),
    /// No importer of this name is registered.
    #[error("no importer named `{0}` is registered (m.importer::<I>())")]
    UnknownImporter(String),
    /// The import does not exist.
    #[error("the import does not exist")]
    NotFound,
    /// The uploaded file is not a CSV, TSV, XLSX, XLS or ODS file, or it
    /// cannot be read.
    #[error("{0}")]
    UnsupportedFile(String),
    /// The file is larger than `sheets.imports.max_upload_bytes`.
    #[error("the file is larger than {limit} bytes")]
    FileTooLarge {
        /// The limit in bytes.
        limit: u64,
    },
    /// The file or export has more rows than allowed.
    #[error("more than {limit} rows")]
    TooManyRows {
        /// The limit.
        limit: u64,
    },
    /// The import is not in a state that allows this (applying a file
    /// that is not staged, discarding one that is being applied).
    #[error("{0}")]
    NotReady(String),
    /// The import has duplicate rows and was applied with
    /// `duplicates = "fail"`.
    #[error("{count} rows are duplicates; fix them or choose skip or update")]
    Duplicates {
        /// How many rows are duplicates.
        count: u64,
    },
    /// The mapping names a column that is not in the file, a field the
    /// importer does not have, or leaves a required field unmapped.
    #[error("invalid mapping")]
    Mapping(BTreeMap<String, Vec<String>>),
    /// Something else is not valid (options, arguments).
    #[error("{0}")]
    Invalid(String),
}

impl SheetsError {
    /// The app's own error.
    pub fn app(error: impl Into<BoxError>) -> Self {
        SheetsError::App(error.into())
    }

    /// The error response:
    ///
    /// | Error | Status | `code` |
    /// |---|---|---|
    /// | `NotFound`, `UnknownImporter`, `UnknownExport`, a missing stored file | 404 | `not_found` |
    /// | `UnsupportedFile` | 400 | `unsupported_file` |
    /// | `FileTooLarge` | 413 | `payload_too_large` |
    /// | `TooManyRows` | 400 | `too_many_rows` |
    /// | `NotReady` | 409 | `import_not_ready` |
    /// | `Duplicates` | 409 | `import_has_duplicates` |
    /// | `Mapping` | 400 | `validation_failed` (`errors` by field) |
    /// | `Invalid` | 400 | `bad_request` |
    /// | anything else | 500 | `internal_error` |
    pub fn to_app_error(&self) -> AppError {
        match self {
            SheetsError::NotFound
            | SheetsError::UnknownImporter(_)
            | SheetsError::UnknownExport(_)
            | SheetsError::Storage(navree_storage::StorageError::NotFound { .. }) => {
                AppError::not_found()
            }
            SheetsError::UnsupportedFile(message) => {
                AppError::bad_request(message.clone()).with_code("unsupported_file")
            }
            SheetsError::FileTooLarge { limit } => {
                AppError::payload_too_large(usize::try_from(*limit).unwrap_or(usize::MAX))
            }
            SheetsError::TooManyRows { limit } => {
                AppError::bad_request(format!("The file has more than {limit} rows."))
                    .with_code("too_many_rows")
            }
            SheetsError::NotReady(message) => {
                AppError::conflict(message.clone()).with_code("import_not_ready")
            }
            SheetsError::Duplicates { count } => AppError::conflict(format!(
                "{count} rows are duplicates. Fix them, or apply with duplicates set to skip or update."
            ))
            .with_code("import_has_duplicates"),
            SheetsError::Mapping(errors) => AppError::validation(errors.clone()),
            SheetsError::Invalid(message) => AppError::bad_request(message.clone()),
            other => AppError::internal(other.to_string()),
        }
    }
}

/// The error mapper `SheetsModule` registers: a `SheetsError` returned from
/// a handler with `?` becomes its own status instead of a 500.
pub(crate) fn map_sheets_error(error: &SheetsError) -> Option<AppError> {
    let app = error.to_app_error();
    (!app.status.is_server_error()).then_some(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_have_their_own_codes() {
        let cases = [
            (SheetsError::NotFound, 404, "not_found"),
            (SheetsError::UnknownExport("x".into()), 404, "not_found"),
            (
                SheetsError::UnsupportedFile("x".into()),
                400,
                "unsupported_file",
            ),
            (
                SheetsError::FileTooLarge { limit: 10 },
                413,
                "payload_too_large",
            ),
            (SheetsError::TooManyRows { limit: 5 }, 400, "too_many_rows"),
            (SheetsError::NotReady("x".into()), 409, "import_not_ready"),
            (
                SheetsError::Duplicates { count: 2 },
                409,
                "import_has_duplicates",
            ),
            (
                SheetsError::Mapping(BTreeMap::new()),
                400,
                "validation_failed",
            ),
            (SheetsError::Missing("x"), 500, "internal_error"),
        ];
        for (error, status, code) in cases {
            let app = error.to_app_error();
            assert_eq!(app.status.as_u16(), status, "{error}");
            assert_eq!(app.code, code, "{error}");
        }
    }

    #[test]
    fn refusals_say_what_went_wrong_in_the_message() {
        let app = SheetsError::TooManyRows { limit: 5 }.to_app_error();
        assert_eq!(app.title, "Bad Request");
        assert_eq!(
            app.detail.as_deref(),
            Some("The file has more than 5 rows.")
        );
        let app = SheetsError::Duplicates { count: 2 }.to_app_error();
        assert_eq!(
            app.detail.as_deref(),
            Some(
                "2 rows are duplicates. Fix them, or apply with duplicates set to skip or update."
            )
        );
    }
}
