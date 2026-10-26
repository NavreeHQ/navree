use std::fmt;

use actix_web::{
    HttpRequest, HttpResponse, ResponseError,
    error::{JsonPayloadError, QueryPayloadError},
    http::StatusCode,
};
use serde_json::json;
use validator::ValidationErrors;

/// Every error a handler or extractor returns, rendered as a JSON body.
#[derive(Debug)]
pub enum AppError {
    Unauthorized,
    /// An extractor could not parse the query string or body.
    Rejected(StatusCode, String),
    /// The input parsed but broke a `validator` rule.
    Validation(ValidationErrors),
    Database(sqlx::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("Unauthenticated."),
            Self::Rejected(_, message) => f.write_str(message),
            Self::Validation(_) => f.write_str("Validation failed"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Rejected(status, _) => *status,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let body = match self {
            Self::Validation(errors) => json!({ "message": "Validation failed", "errors": errors }),
            Self::Database(_) => {
                eprintln!("{self}");
                json!({ "message": "Internal Server Error" })
            }
            _ => json!({ "message": self.to_string() }),
        };
        HttpResponse::build(self.status_code()).json(body)
    }
}

impl From<ValidationErrors> for AppError {
    fn from(errors: ValidationErrors) -> Self {
        Self::Validation(errors)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// `JsonConfig` error handler: JSON bodies for unparseable request bodies.
pub fn json_error(error: JsonPayloadError, _req: &HttpRequest) -> actix_web::Error {
    AppError::Rejected(error.status_code(), error.to_string()).into()
}

/// `QueryConfig` error handler: JSON bodies for unparseable query strings.
pub fn query_error(error: QueryPayloadError, _req: &HttpRequest) -> actix_web::Error {
    AppError::Rejected(error.status_code(), error.to_string()).into()
}
