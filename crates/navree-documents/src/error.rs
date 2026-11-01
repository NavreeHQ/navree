//! [`DocumentsError`] and its mapping to error responses.

use http::StatusCode;
use navree_http::AppError;

/// What can go wrong while rendering or editing a document.
///
/// Returned from a handler with `?`, a variant is a 500 response unless it
/// says otherwise (`EngineUnavailable` and `Timeout` are 503, `Pdf` and
/// `InvalidDocx` are 400, `TooLarge` is 413). A 500 is logged with the full
/// message and the response carries only the reason phrase, so template
/// paths and data never reach the client; the 400 and 413 say what is wrong
/// with the input, and their messages name no template file.
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
    /// connection dropped, the browser answered with an error), or writing
    /// an edited PDF failed.
    #[error("PDF engine `{engine}` failed: {message}")]
    Engine {
        /// `native` or `chrome`; `pdf` for editing a PDF.
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
    /// The bytes are not a PDF this crate can read: the client's error. A
    /// fault of the server while writing an edited PDF is
    /// [`Engine`](Self::Engine), and an edit the app should not have asked
    /// for is [`Edit`](Self::Edit).
    #[error("invalid PDF: {0}")]
    Pdf(String),
    /// The app asked for an edit of a PDF that cannot be done: merging no
    /// documents, a text stamp in a script Helvetica cannot show, an
    /// overlay without pages. A bug in the app, so a 500.
    #[error("the PDF cannot be edited: {0}")]
    Edit(String),
    /// An input is over a limit this crate sets on what reading it may
    /// take: a PDF whose objects take too much memory or work to read (see
    /// [`Pdf::from_bytes`](crate::Pdf::from_bytes)), whose text takes too
    /// much work or memory to read or holds too much text, a DOCX file over
    /// one of its `DocxLimits`, or an image with more pixels than
    /// `documents.native.max_image_pixels`. The message names the limit, and
    /// no template file.
    #[error("{0}")]
    TooLarge(String),
    /// Building the DOCX file failed: a fault of the server or of the app.
    #[error("DOCX: {0}")]
    Docx(String),
    /// The bytes are not a Word document this crate can read: a damaged
    /// archive or XML, a package whose main part is not a Word document, a
    /// DTD, two entries of one name. The client's error, like
    /// [`Pdf`](Self::Pdf). A file over a reading limit is
    /// [`TooLarge`](Self::TooLarge).
    #[error("invalid DOCX: {0}")]
    InvalidDocx(String),
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
    /// [`Timeout`](Self::Timeout), 400 for [`Pdf`](Self::Pdf) and
    /// [`InvalidDocx`](Self::InvalidDocx), 413 for
    /// [`TooLarge`](Self::TooLarge), 500 otherwise.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::EngineUnavailable { .. } | Self::Timeout { .. } => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::Pdf(_) | Self::InvalidDocx(_) => StatusCode::BAD_REQUEST,
            Self::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// `map_error` for [`DocumentsError`]: the status from
/// [`DocumentsError::status`], the error kept as the source for the logs.
/// A client error also tells the client what is wrong.
pub(crate) fn map_documents_error(error: &DocumentsError) -> Option<AppError> {
    let status = error.status();
    let mapped = AppError::from_status(status);
    let mapped = if status.is_client_error() {
        mapped.with_detail(error.to_string())
    } else {
        mapped
    };
    Some(mapped.with_source(error.to_string()))
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
    fn unavailable_engines_and_timeouts_are_503_input_errors_4xx_everything_else_500() {
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
        let unreadable = DocumentsError::Pdf("no pages".into());
        assert_eq!(unreadable.status(), StatusCode::BAD_REQUEST);
        let too_large = DocumentsError::TooLarge("a page needs too much work".into());
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(too_large.to_string(), "a page needs too much work");
        let edit = DocumentsError::Edit("nothing to merge".into());
        assert_eq!(edit.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // A Word file that cannot be read is the client's; writing one is not.
        let unreadable = DocumentsError::InvalidDocx("the package has no main document".into());
        assert_eq!(unreadable.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            unreadable.to_string(),
            "invalid DOCX: the package has no main document"
        );
        let writing = DocumentsError::Docx("an image must be a PNG or a JPEG file".into());
        assert_eq!(writing.status(), StatusCode::INTERNAL_SERVER_ERROR);
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

    #[test]
    fn an_unreadable_word_file_answers_400_with_its_message_and_a_failed_write_500_without() {
        let unreadable = DocumentsError::InvalidDocx("the package has no main document".into());
        let body = map_documents_error(&unreadable).unwrap().body();
        assert_eq!(body["statusCode"], 400, "{body}");
        assert_eq!(
            body["message"], "invalid DOCX: the package has no main document",
            "{body}"
        );
        let writing = DocumentsError::Docx("zip: disk full".into());
        let body = map_documents_error(&writing).unwrap().body();
        assert_eq!(body["statusCode"], 500, "{body}");
        assert!(
            !body["message"].as_str().unwrap().contains("disk full"),
            "{body}"
        );
    }
}
