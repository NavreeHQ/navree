//! Responses: [`IntoResponse`], [`Response`] and the shapes handlers return
//! most ([`Json`], [`Created`], [`NoContent`], [`Html`], [`Redirect`],
//! [`AppError`], server-sent events in [`sse`]).

use http::header::{CONTENT_TYPE, LOCATION};
use http::{HeaderValue, StatusCode};
use serde::Serialize;

use crate::body::Body;
use crate::{AppError, Json};

mod into_response;
mod parts;
pub mod sse;

pub use into_response::IntoResponse;
pub use parts::{AppendHeaders, IntoResponseParts, ResponseParts, TryIntoHeaderError};

/// A response, as handlers and middleware see it.
pub type Response<B = Body> = http::Response<B>;

/// An HTML page: `T`'s response with `Content-Type: text/html;
/// charset=utf-8`.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct Html<T>(pub T);

impl<T: IntoResponse> IntoResponse for Html<T> {
    fn into_response(self) -> Response {
        let content_type = HeaderValue::from_static("text/html; charset=utf-8");
        ([(CONTENT_TYPE, content_type)], self.0).into_response()
    }
}

impl<T> From<T> for Html<T> {
    fn from(inner: T) -> Self {
        Self(inner)
    }
}

/// A redirect to another location.
///
/// A location that cannot be a header (it has a newline) is a 500, not a
/// redirect.
#[must_use = "a redirect does nothing until it is returned from a handler"]
#[derive(Clone, Debug)]
pub struct Redirect {
    status: StatusCode,
    location: String,
}

impl Redirect {
    /// 303 See Other: the client follows it with a `GET`, as after a form
    /// or an upload.
    pub fn to(location: &str) -> Self {
        Self::with_status(StatusCode::SEE_OTHER, location)
    }

    /// 307 Temporary Redirect: the client repeats the method and the body.
    pub fn temporary(location: &str) -> Self {
        Self::with_status(StatusCode::TEMPORARY_REDIRECT, location)
    }

    /// 308 Permanent Redirect: the client repeats the method and the body.
    pub fn permanent(location: &str) -> Self {
        Self::with_status(StatusCode::PERMANENT_REDIRECT, location)
    }

    /// The status.
    #[must_use]
    pub fn status_code(&self) -> StatusCode {
        self.status
    }

    /// Where it leads.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    fn with_status(status: StatusCode, location: &str) -> Self {
        Self {
            status,
            location: location.to_owned(),
        }
    }
}

impl IntoResponse for Redirect {
    fn into_response(self) -> Response {
        match HeaderValue::try_from(self.location) {
            Ok(location) => (self.status, [(LOCATION, location)]).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    }
}

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

    #[test]
    fn redirects_have_their_status_and_location() {
        let response = Redirect::to("/next").into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/next");
        assert_eq!(
            Redirect::temporary("/t").status_code(),
            StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            Redirect::permanent("/p").status_code(),
            StatusCode::PERMANENT_REDIRECT
        );
        assert_eq!(Redirect::permanent("/p").location(), "/p");
    }

    #[test]
    fn a_location_that_cannot_be_a_header_is_a_500() {
        let response = Redirect::to("/a\nb").into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!response.headers().contains_key(LOCATION));
    }
}
