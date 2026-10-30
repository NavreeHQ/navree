//! [`Json`]: a JSON request body, and a JSON response.

use std::ops::{Deref, DerefMut};

use bytes::{BufMut, BytesMut};
use http::HeaderMap;
use http::header::{CONTENT_TYPE, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::media::MediaType;
use super::rejection::JsonRejection;
use super::{FromRequest, OptionalFromRequest, Request};
use crate::AppError;
use crate::body::{BodyLimit, read_body};
use crate::response::{IntoResponse, Response};

/// JSON: as an extractor, the request body deserialized into `T`; as a
/// response, `T` serialized with `Content-Type: application/json`.
///
/// The request must say `Content-Type: application/json` or a `+json` type
/// (`application/merge-patch+json`), parameters allowed, in one header
/// (415 otherwise; see `MediaType::is_json` for the exact rule);
/// the body is read within the route's [`BodyLimit`] (413 past it). A body
/// that is not JSON, or has anything but whitespace after the value, is a
/// 400 `"Failed to parse the request body as JSON: ..."`; JSON of the wrong
/// shape is a 400 `"Failed to deserialize the JSON body into the target
/// type: ..."` naming the field (`title: invalid type: ...`).
///
/// ```
/// use http::Method;
/// use navree_http::{Json, RouteDef};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, Serialize)]
/// struct Deal {
///     title: String,
/// }
///
/// async fn store(Json(deal): Json<Deal>) -> Json<Deal> {
///     Json(deal)
/// }
///
/// let route = RouteDef::on(Method::POST, "/deals", store).public();
/// # let _ = route;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Json<T>(pub T);

impl<T> Deref for Json<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> From<T> for Json<T> {
    fn from(value: T) -> Self {
        Json(value)
    }
}

impl<T: DeserializeOwned> Json<T> {
    /// Deserializes `T` from a JSON body's bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, JsonRejection> {
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        let value =
            serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
                match error.inner().classify() {
                    serde_json::error::Category::Data => JsonRejection::Data(error.to_string()),
                    _ => JsonRejection::Syntax(error.to_string()),
                }
            })?;
        deserializer
            .end()
            .map_err(|error| JsonRejection::Syntax(error.to_string()))?;
        Ok(Json(value))
    }
}

impl<T: DeserializeOwned + Send> FromRequest for Json<T> {
    type Rejection = JsonRejection;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        if !is_json(request.headers()) {
            return Err(JsonRejection::ContentType);
        }
        let limit = BodyLimit::of(request.extensions());
        let bytes = read_body(request.into_body(), limit)
            .await
            .map_err(JsonRejection::Body)?;
        Self::from_bytes(&bytes)
    }
}

/// `Option<Json<T>>`: `None` for a request without `Content-Type`; a
/// request with another content type is still a 415.
impl<T: DeserializeOwned + Send> OptionalFromRequest for Json<T> {
    type Rejection = JsonRejection;

    async fn from_request(request: Request) -> Result<Option<Self>, Self::Rejection> {
        if request.headers().get(CONTENT_TYPE).is_none() {
            return Ok(None);
        }
        <Self as FromRequest>::from_request(request).await.map(Some)
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        let mut buffer = BytesMut::with_capacity(128).writer();
        match serde_json::to_writer(&mut buffer, &self.0) {
            Ok(()) => {
                let mut response = Response::new(buffer.into_inner().freeze().into());
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static(crate::ERROR_CONTENT_TYPE),
                );
                response
            }
            Err(error) => AppError::internal(error).into_response(),
        }
    }
}

/// Whether the request says it sends JSON (see [`MediaType::is_json`]).
pub(crate) fn is_json(headers: &HeaderMap) -> bool {
    MediaType::of(headers).is_some_and(|media| media.is_json())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::body::Body;

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Deal {
        title: String,
        amount: Option<u32>,
    }

    fn post(content_type: Option<&str>, body: &'static str) -> Request {
        let mut builder = http::Request::post("/deals");
        if let Some(content_type) = content_type {
            builder = builder.header("content-type", content_type);
        }
        builder.body(Body::from(body)).unwrap()
    }

    async fn json(request: Request) -> Result<Deal, (u16, String)> {
        <Json<Deal> as FromRequest>::from_request(request)
            .await
            .map(|Json(deal)| deal)
            .map_err(|rejection| (rejection.status().as_u16(), rejection.to_string()))
    }

    #[tokio::test]
    async fn bodies_deserialize_with_the_field_in_errors() {
        let deal = json(post(
            Some("application/json"),
            r#"{"title":"A","amount":3}"#,
        ))
        .await
        .unwrap();
        assert_eq!(
            deal,
            Deal {
                title: "A".into(),
                amount: Some(3)
            }
        );
        assert_eq!(
            json(post(Some("application/json"), r#"{"title":1}"#))
                .await
                .unwrap_err(),
            (
                400,
                "Failed to deserialize the JSON body into the target type: title: invalid type: integer `1`, expected a string at line 1 column 10".into()
            )
        );
        assert_eq!(
            json(post(Some("application/json"), r#"{"amount":1}"#))
                .await
                .unwrap_err(),
            (
                400,
                "Failed to deserialize the JSON body into the target type: missing field `title` at line 1 column 12".into()
            )
        );
        assert_eq!(
            json(post(Some("application/json"), "{")).await.unwrap_err(),
            (
                400,
                "Failed to parse the request body as JSON: EOF while parsing an object at line 1 column 1".into()
            )
        );
        assert_eq!(
            json(post(Some("application/json"), r#"{"title":"A"} x"#))
                .await
                .unwrap_err()
                .1,
            "Failed to parse the request body as JSON: trailing characters at line 1 column 15"
        );
    }

    #[tokio::test]
    async fn the_content_type_must_say_json() {
        for ok in [
            "application/json",
            "application/json; charset=utf-8",
            "application/json;charset=utf-8",
            "Application/JSON",
            "application/merge-patch+json",
            "application/cloudevents+json",
        ] {
            assert!(
                json(post(Some(ok), r#"{"title":"A"}"#)).await.is_ok(),
                "{ok}"
            );
        }
        for refused in [
            None,
            Some("text/json"),
            Some("application/jsonx"),
            Some("application/json-patch"),
            Some("application/"),
            Some("json"),
            Some("application/ json"),
            Some("application/json; charset"),
            Some(""),
        ] {
            let error = json(post(refused, r#"{"title":"A"}"#)).await.unwrap_err();
            assert_eq!(
                error,
                (
                    415,
                    "Expected request with `Content-Type: application/json`".into()
                ),
                "{refused:?}"
            );
        }
    }

    #[tokio::test]
    async fn optional_json_is_none_without_a_content_type() {
        let none = <Json<Deal> as OptionalFromRequest>::from_request(post(None, ""))
            .await
            .unwrap();
        assert!(none.is_none());
        let other =
            <Json<Deal> as OptionalFromRequest>::from_request(post(Some("text/plain"), "")).await;
        assert!(matches!(other, Err(JsonRejection::ContentType)));
    }

    #[tokio::test]
    async fn bodies_stop_at_the_limit() {
        let mut request = post(Some("application/json"), r#"{"title":"long title"}"#);
        request.extensions_mut().insert(BodyLimit(8));
        assert_eq!(json(request).await.unwrap_err().0, 413);
    }

    #[tokio::test]
    async fn responses_are_application_json() {
        use http_body_util::BodyExt;

        let response = Json(Deal {
            title: "A".into(),
            amount: None,
        })
        .into_response();
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], br#"{"title":"A","amount":null}"#);

        let mut map = std::collections::HashMap::new();
        map.insert(vec![1u8], 1);
        let response = Json(map).into_response();
        assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
    }
}
