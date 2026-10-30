//! Responses: [`IntoResponse`], [`Response`] and the shapes handlers return
//! most ([`Json`](crate::Json), [`Created`], [`NoContent`], [`AppError`]).
//!
//! `IntoResponse`, `Response`, [`Html`], [`Redirect`] and the server-sent
//! events types are still the previous framework's, re-exported here so
//! handlers and middleware import them from navree-http; they become
//! navree-http's own behind the same names.

pub use axum::response::sse;
pub use axum::response::{
    AppendHeaders, Html, IntoResponse, IntoResponseParts, Redirect, Response,
};
use http::header::LOCATION;
use http::{HeaderValue, StatusCode};
use serde::Serialize;

use crate::{AppError, Json};

/// 201 Created with a `Location` header and a JSON body.
#[derive(Clone, Debug)]
pub struct Created<T>(pub String, pub T);

impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        let Created(location, body) = self;
        let location = match HeaderValue::from_str(&location) {
            Ok(value) => value,
            Err(error) => {
                return AppError::internal(format!("invalid Location header: {error}"))
                    .into_response();
            }
        };
        let mut response = (StatusCode::CREATED, Json(body)).into_response();
        response.headers_mut().insert(LOCATION, location);
        response
    }
}

/// 204 No Content.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_sets_location_and_status() {
        let response =
            Created("/api/deals/1".to_owned(), serde_json::json!({"id": 1})).into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[LOCATION], "/api/deals/1");
        assert_eq!(
            response.headers()[http::header::CONTENT_TYPE],
            "application/json"
        );
        assert_eq!(NoContent.into_response().status(), StatusCode::NO_CONTENT);
        let bad = Created("/a\nb".to_owned(), ()).into_response();
        assert_eq!(bad.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
