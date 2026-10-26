use axum::{
    Json,
    extract::rejection::{JsonRejection, QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use validator::ValidationErrors;

/// Every error a handler or extractor returns, rendered as a JSON body.
pub enum AppError {
    Unauthorized,
    /// An extractor could not parse the query string or body.
    Rejected(StatusCode, String),
    /// The input parsed but broke a `validator` rule.
    Validation(ValidationErrors),
    Database(sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, json!({ "message": "Unauthenticated." })),
            Self::Rejected(status, message) => (status, json!({ "message": message })),
            Self::Validation(errors) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "message": "Validation failed", "errors": errors }),
            ),
            Self::Database(error) => {
                eprintln!("database error: {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "message": "Internal Server Error" }),
                )
            }
        };
        (status, Json(body)).into_response()
    }
}

impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        Self::Rejected(rejection.status(), rejection.body_text())
    }
}

impl From<QueryRejection> for AppError {
    fn from(rejection: QueryRejection) -> Self {
        Self::Rejected(rejection.status(), rejection.body_text())
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
