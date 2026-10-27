//! [`DocumentsError`] and its mapping to error responses.

use http::StatusCode;
use navree_http::AppError;

/// What can go wrong while rendering or editing a document.
///
/// Returned from a handler with `?`, every variant is a 500 response
/// (`EngineUnavailable` and `Timeout` are 503), logged with the full
/// message; the response carries only the reason phrase, so template
/// paths and data never reach the client.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentsError {
    /// No template has this name: no `<name>.html` in a template
    /// directory or among the embedded templates.
    #[error("document template `{name}` not found (tried {})", tried.join(", "))]
    TemplateNotFound {
        /// The name asked for.
        name: String,
        /// The file names tried, in order.
        tried: Vec<String>,
    },
    /// The template failed to render: a syntax error, an undefined
    /// variable, an image that does not decode. `line` and `column` (from 1)
    /// point into `file` when the error has a location.
    #[error("{}", template_message(file, *line, *column, message))]
    Template {
        /// The file the error is in (`certificates/classic.html`).
        file: String,
        /// The line, from 1.
        line: Option<usize>,
        /// The column, from 1.
        column: Option<usize>,
        /// What went wrong.
        message: String,
    },
    /// The data could not be turned into JSON.
    #[error("document data is not valid JSON: {0}")]
    Data(String),
    /// The engine is not compiled in (a crate feature is off) or cannot
    /// start (no Chrome executable, no fonts).
    #[error("PDF engine `{engine}` is unavailable: {reason}")]
    EngineUnavailable {
        /// `native` or `chrome`.
        engine: &'static str,
        /// Why.
        reason: String,
    },
    /// The engine failed while rendering (Chrome crashed, the DevTools
    /// connection dropped, the browser answered with an error).
    #[error("PDF engine `{engine}` failed: {message}")]
    Engine {
        /// `native` or `chrome`.
        engine: &'static str,
        /// What went wrong.
        message: String,
    },
    /// Rendering took longer than the engine's time limit.
    #[error("rendering with `{engine}` took longer than {secs} s")]
    Timeout {
        /// `native` or `chrome`.
        engine: &'static str,
        /// The limit.
        secs: u64,
    },
    /// The bytes are not a PDF this crate can read, or editing failed.
    #[error("invalid PDF: {0}")]
    Pdf(String),
    /// Building the DOCX file failed.
    #[error("DOCX: {0}")]
    Docx(String),
    /// Reading or writing a storage disk failed.
    #[error(transparent)]
    Storage(#[from] navree_storage::StorageError),
    /// Queueing a render failed, or the operation store failed.
    #[error("queued render: {0}")]
    Queue(String),
}

fn template_message(
    file: &str,
    line: Option<usize>,
    column: Option<usize>,
    message: &str,
) -> String {
    match (line, column) {
        (Some(line), Some(column)) => format!("{file}:{line}:{column}: {message}"),
        (Some(line), None) => format!("{file}:{line}: {message}"),
        _ => format!("{file}: {message}"),
    }
}

impl DocumentsError {
    /// The status code of the error response: 503 for
    /// [`EngineUnavailable`](Self::EngineUnavailable) and
    /// [`Timeout`](Self::Timeout), 500 otherwise.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::EngineUnavailable { .. } | Self::Timeout { .. } => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// `map_error` for [`DocumentsError`]: the status from
/// [`DocumentsError::status`], the error kept as the source for the logs.
pub(crate) fn map_documents_error(error: &DocumentsError) -> Option<AppError> {
    Some(AppError::from_status(error.status()).with_source(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_errors_name_the_file_line_and_column() {
        let error = DocumentsError::Template {
            file: "certificates/classic.html".into(),
            line: Some(3),
            column: Some(14),
            message: "undefined value".into(),
        };
        assert_eq!(
            error.to_string(),
            "certificates/classic.html:3:14: undefined value"
        );
        let error = DocumentsError::Template {
            file: "invoices/show.html".into(),
            line: None,
            column: None,
            message: "undefined value".into(),
        };
        assert_eq!(error.to_string(), "invoices/show.html: undefined value");
    }

    #[test]
    fn unavailable_engines_and_timeouts_are_503_everything_else_500() {
        let unavailable = DocumentsError::EngineUnavailable {
            engine: "chrome",
            reason: "no Chrome executable".into(),
        };
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
        let timeout = DocumentsError::Timeout {
            engine: "chrome",
            secs: 30,
        };
        assert_eq!(timeout.status(), StatusCode::SERVICE_UNAVAILABLE);
        let missing = DocumentsError::TemplateNotFound {
            name: "x".into(),
            tried: vec!["x.html".into()],
        };
        assert_eq!(missing.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            missing.to_string(),
            "document template `x` not found (tried x.html)"
        );
    }
}
