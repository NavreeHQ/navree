//! Rejections of the built-in extractors. Each renders as an
//! [`AppError`](crate::AppError) in the JSON error format: a client error
//! shows its text as `message`, a server error is logged and shows
//! `"Internal server error"`. Each is also a `std::error::Error`, so `?` on
//! one in a handler answers the same as the extractor would (the HTTP
//! module maps them back from the 500 they become).

use std::fmt;

use http::StatusCode;

pub use super::path::PathErrorKind;
use crate::AppError;
use crate::body::BodyError;
use crate::response::{IntoResponse, Response};

/// Implements `Display`, `Error`, `IntoResponse` and `From<_> for AppError`
/// in terms of `status()` and `body_text()`.
macro_rules! rejection {
    ($($name:ident),* $(,)?) => {
        $(
            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str(&self.body_text())
                }
            }

            impl std::error::Error for $name {}

            impl IntoResponse for $name {
                fn into_response(self) -> Response {
                    self.to_app_error().into_response()
                }
            }

            impl $name {
                /// The rejection as an [`AppError`].
                pub fn to_app_error(&self) -> AppError {
                    AppError::rejection(self.status(), self.body_text())
                }
            }
        )*
    };
}

rejection!(
    PathRejection,
    QueryRejection,
    FormRejection,
    JsonRejection,
    ExtensionRejection,
);

/// [`Path`](crate::Path) could not deserialize the route's parameters.
#[derive(Debug)]
pub struct PathRejection(pub(crate) PathErrorKind);

impl PathRejection {
    /// What went wrong.
    pub fn kind(&self) -> &PathErrorKind {
        &self.0
    }

    /// 400 for a value the client sent, 500 for a `T` that does not fit the
    /// route.
    pub fn status(&self) -> StatusCode {
        self.0.status()
    }

    /// The message (`"Invalid URL: ..."` for client errors).
    pub fn body_text(&self) -> String {
        if self.status().is_client_error() {
            format!("Invalid URL: {}", self.0)
        } else {
            self.0.to_string()
        }
    }
}

/// [`Query`](crate::Query) could not deserialize the query string (400).
#[derive(Debug)]
pub struct QueryRejection(pub(crate) String);

impl QueryRejection {
    /// 400.
    pub fn status(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }

    /// `"Failed to deserialize query string: ..."`.
    pub fn body_text(&self) -> String {
        format!("Failed to deserialize query string: {}", self.0)
    }
}

/// [`Form`](crate::Form) could not read the form.
#[derive(Debug)]
#[non_exhaustive]
pub enum FormRejection {
    /// A body without `Content-Type: application/x-www-form-urlencoded`
    /// (415).
    ContentType,
    /// The body could not be read (413 past the limit, else 400).
    Body(BodyError),
    /// A `GET` or `HEAD` query string that does not deserialize (400).
    Query(String),
    /// A body that does not deserialize (400).
    Deserialize(String),
}

impl FormRejection {
    /// The status.
    pub fn status(&self) -> StatusCode {
        match self {
            FormRejection::ContentType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            FormRejection::Body(error) => error.status(),
            FormRejection::Query(_) | FormRejection::Deserialize(_) => StatusCode::BAD_REQUEST,
        }
    }

    /// The message.
    pub fn body_text(&self) -> String {
        match self {
            FormRejection::ContentType => {
                "Form requests must have `Content-Type: application/x-www-form-urlencoded`"
                    .to_owned()
            }
            FormRejection::Body(error) => error.to_string(),
            FormRejection::Query(error) => format!("Failed to deserialize form: {error}"),
            FormRejection::Deserialize(error) => {
                format!("Failed to deserialize form body: {error}")
            }
        }
    }
}

/// [`Json`](crate::Json) could not read the body.
#[derive(Debug)]
#[non_exhaustive]
pub enum JsonRejection {
    /// No `Content-Type: application/json` (or `+json`) (415).
    ContentType,
    /// The body could not be read (413 past the limit, else 400).
    Body(BodyError),
    /// Not JSON (400).
    Syntax(String),
    /// JSON of the wrong shape (400).
    Data(String),
}

impl JsonRejection {
    /// The status.
    pub fn status(&self) -> StatusCode {
        match self {
            JsonRejection::ContentType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            JsonRejection::Body(error) => error.status(),
            JsonRejection::Syntax(_) | JsonRejection::Data(_) => StatusCode::BAD_REQUEST,
        }
    }

    /// The message.
    pub fn body_text(&self) -> String {
        match self {
            JsonRejection::ContentType => {
                "Expected request with `Content-Type: application/json`".to_owned()
            }
            JsonRejection::Body(error) => error.to_string(),
            JsonRejection::Syntax(error) => {
                format!("Failed to parse the request body as JSON: {error}")
            }
            JsonRejection::Data(error) => {
                format!("Failed to deserialize the JSON body into the target type: {error}")
            }
        }
    }
}

/// [`Extension<T>`](crate::Extension): the request has no `T` (500: the app
/// is missing the layer that adds it).
#[derive(Debug)]
pub struct ExtensionRejection(pub(crate) &'static str);

impl ExtensionRejection {
    /// 500.
    pub fn status(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }

    /// The message (logged).
    pub fn body_text(&self) -> String {
        format!(
            "Missing request extension: Extension of type `{}` was not found",
            self.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejections_render_in_the_error_format() {
        let error = JsonRejection::ContentType.to_app_error();
        assert_eq!(error.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            error.body(),
            serde_json::json!({
                "statusCode": 415,
                "message": "Expected request with `Content-Type: application/json`",
                "error": "Unsupported Media Type",
                "code": "unsupported_media_type",
            })
        );
        let error = FormRejection::Body(BodyError::TooLarge).to_app_error();
        assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            error.body()["message"],
            "Failed to buffer the request body: length limit exceeded"
        );
        let error = ExtensionRejection("navree_kernel::AppContext").to_app_error();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.body()["message"], "Internal server error");
        assert!(error.log_chain().contains("AppContext"), "logged");
        let error = PathRejection(PathErrorKind::ParseError {
            value: "x".into(),
            expected_type: "u32",
        });
        assert_eq!(
            error.to_string(),
            "Invalid URL: Cannot parse `x` to a `u32`"
        );
        assert_eq!(
            QueryRejection("page: invalid digit found in string".into()).to_string(),
            "Failed to deserialize query string: page: invalid digit found in string"
        );
    }
}
