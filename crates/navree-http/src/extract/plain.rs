//! The request's own parts as extractors: [`HeaderMap`], [`Method`], [`Uri`],
//! [`Version`], [`Extensions`] and the whole head, [`Parts`] (clones of what
//! the request carries), and [`Bytes`] and [`String`] (the body, read whole
//! within the route's [`BodyLimit`](crate::body::BodyLimit)).
//!
//! None of the head ones can fail. A body that cannot be read answers with
//! the same rejection as [`Json`](crate::Json) and [`Form`](crate::Form):
//! 413 past the limit, 408 for a client that sent it too slowly, 400 when the
//! connection failed; a [`String`] that is not UTF-8 is a 400.

use std::convert::Infallible;

use http::request::Parts;
use http::{Extensions, HeaderMap, Method, Uri, Version};

use super::{FromRequest, FromRequestParts, Request};
use crate::AppError;
use crate::body::{BodyError, BodyLimit, Bytes, read_body};

macro_rules! cloned_from_head {
    ($($ty:ty => $field:ident),* $(,)?) => {
        $(
            impl FromRequestParts for $ty {
                type Rejection = Infallible;

                async fn from_request_parts(parts: &mut Parts) -> Result<Self, Infallible> {
                    Ok(parts.$field.clone())
                }
            }
        )*
    };
}

impl FromRequestParts for Parts {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Infallible> {
        Ok(parts.clone())
    }
}

cloned_from_head!(
    HeaderMap => headers,
    Method => method,
    Uri => uri,
    Version => version,
    Extensions => extensions,
);

impl FromRequest for Bytes {
    type Rejection = AppError;

    async fn from_request(request: Request) -> Result<Self, AppError> {
        let limit = BodyLimit::of(request.extensions());
        read_body(request.into_body(), limit)
            .await
            .map_err(body_rejection)
    }
}

impl FromRequest for String {
    type Rejection = AppError;

    async fn from_request(request: Request) -> Result<Self, AppError> {
        let bytes = Bytes::from_request(request).await?;
        String::from_utf8(bytes.into()).map_err(|error| {
            AppError::rejection(
                http::StatusCode::BAD_REQUEST,
                format!("Request body didn't contain valid UTF-8: {error}"),
            )
        })
    }
}

fn body_rejection(error: BodyError) -> AppError {
    AppError::rejection(error.status(), error.to_string())
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, Method, StatusCode, Uri, Version};
    use http_body_util::BodyExt;

    use crate::AppError;
    use crate::body::{Body, BodyLimit, Bytes};
    use crate::extract::{FromRequest, FromRequestParts, Request};
    use crate::response::IntoResponse;

    fn message(rejection: &AppError) -> &str {
        rejection.detail.as_deref().unwrap_or_default()
    }

    fn request(body: impl Into<Body>) -> Request {
        http::Request::builder()
            .method(Method::PUT)
            .uri("/deals/7?page=2")
            .version(Version::HTTP_2)
            .header("x-one", "1")
            .header("x-many", "a")
            .header("x-many", "b")
            .extension(7_u8)
            .body(body.into())
            .unwrap()
    }

    #[tokio::test]
    async fn the_head_is_cloned() {
        let (mut parts, _) = request(Body::empty()).into_parts();

        let headers = HeaderMap::from_request_parts(&mut parts).await.unwrap();
        assert_eq!(headers["x-one"], "1");
        assert_eq!(headers.get_all("x-many").iter().count(), 2);
        assert_eq!(
            Method::from_request_parts(&mut parts).await.unwrap(),
            Method::PUT
        );
        assert_eq!(
            Uri::from_request_parts(&mut parts).await.unwrap(),
            "/deals/7?page=2"
        );
        assert_eq!(
            Version::from_request_parts(&mut parts).await.unwrap(),
            Version::HTTP_2
        );
        let extensions = http::Extensions::from_request_parts(&mut parts)
            .await
            .unwrap();
        assert_eq!(extensions.get::<u8>(), Some(&7));
        let head = http::request::Parts::from_request_parts(&mut parts)
            .await
            .unwrap();
        assert_eq!(head.method, Method::PUT);
        assert_eq!(head.uri, "/deals/7?page=2");
        assert_eq!(head.headers.len(), 3);
        assert_eq!(head.extensions.get::<u8>(), Some(&7));

        // The request keeps all of it for the arguments after.
        assert_eq!(parts.headers.len(), 3);
        assert_eq!(parts.extensions.get::<u8>(), Some(&7));
    }

    #[tokio::test]
    async fn each_is_usable_as_the_last_argument() {
        // A head extractor may come last: it reads the head and drops the body.
        let headers = <HeaderMap as FromRequest<crate::extract::via::ViaParts>>::from_request(
            request("ignored"),
        )
        .await
        .unwrap();
        assert_eq!(headers["x-one"], "1");
    }

    #[tokio::test]
    async fn a_body_is_read_whole() {
        let bytes = Bytes::from_request(request("hello")).await.unwrap();
        assert_eq!(bytes, "hello");
        let text = String::from_request(request("héllo")).await.unwrap();
        assert_eq!(text, "héllo");
        assert!(
            Bytes::from_request(request(Body::empty()))
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(String::from_request(request("")).await.unwrap(), "");
        // Frames are joined in order.
        let chunks = futures_util::stream::iter(["a", "", "bc"].map(Ok::<_, std::io::Error>));
        let bytes = Bytes::from_request(request(Body::from_stream(chunks)))
            .await
            .unwrap();
        assert_eq!(bytes, "abc");
    }

    #[tokio::test]
    async fn a_body_past_the_limit_is_a_413() {
        let mut at = request("0123456789");
        at.extensions_mut().insert(BodyLimit(10));
        assert_eq!(Bytes::from_request(at).await.unwrap().len(), 10);

        for body in [Body::from("0123456789!"), {
            // No length to check: cut off at the limit as it is read.
            let chunks =
                futures_util::stream::iter((0..10).map(|_| Ok::<_, std::io::Error>("0123456789")));
            Body::from_stream(chunks)
        }] {
            let mut over = request(body);
            over.extensions_mut().insert(BodyLimit(10));
            let rejection = Bytes::from_request(over).await.unwrap_err();
            assert_eq!(rejection.status, StatusCode::PAYLOAD_TOO_LARGE);
            assert_eq!(
                message(&rejection),
                "Failed to buffer the request body: length limit exceeded"
            );
        }

        let mut over = request("0123456789!");
        over.extensions_mut().insert(BodyLimit(10));
        let rejection = String::from_request(over).await.unwrap_err();
        assert_eq!(rejection.status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_request_outside_the_pipeline_has_the_default_limit() {
        let body = vec![b'x'; crate::body::DEFAULT_BODY_LIMIT];
        assert!(Bytes::from_request(request(body.clone())).await.is_ok());
        let mut over = body;
        over.push(b'x');
        let rejection = Bytes::from_request(request(over)).await.unwrap_err();
        assert_eq!(rejection.status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_body_that_fails_while_it_is_read_is_a_400() {
        let chunks = futures_util::stream::iter([
            Ok::<_, std::io::Error>("start"),
            Err(std::io::Error::other("reset")),
        ]);
        let rejection = Bytes::from_request(request(Body::from_stream(chunks)))
            .await
            .unwrap_err();
        assert_eq!(rejection.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message(&rejection),
            "Failed to buffer the request body: reset"
        );
    }

    #[tokio::test]
    async fn a_body_the_client_sent_too_slowly_is_a_408() {
        let chunks = futures_util::stream::iter([Err::<Bytes, _>(crate::serve::BodyReadTimeout)]);
        let rejection = Bytes::from_request(request(Body::from_stream(chunks)))
            .await
            .unwrap_err();
        assert_eq!(rejection.status, StatusCode::REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn a_string_must_be_utf8() {
        let rejection = String::from_request(request(vec![b'a', 0xff, b'b']))
            .await
            .unwrap_err();
        assert_eq!(rejection.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message(&rejection),
            "Request body didn't contain valid UTF-8: invalid utf-8 sequence of 1 bytes from index 1"
        );
        // Bytes takes any bytes.
        assert_eq!(
            Bytes::from_request(request(vec![b'a', 0xff, b'b']))
                .await
                .unwrap()
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn a_rejection_answers_in_the_json_error_format() {
        let mut over = request("0123456789!");
        over.extensions_mut().insert(BodyLimit(10));
        let response = Bytes::from_request(over).await.unwrap_err().into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["message"],
            "Failed to buffer the request body: length limit exceeded"
        );
    }

    #[tokio::test]
    async fn handlers_take_them_as_arguments() {
        use crate::handler::Handler;

        async fn all(
            method: Method,
            uri: Uri,
            version: Version,
            headers: HeaderMap,
            extensions: http::Extensions,
            body: String,
        ) -> String {
            format!(
                "{method} {uri} {version:?} {} {:?} {body}",
                headers.len(),
                extensions.get::<u8>()
            )
        }
        let response = all.call(request("text")).await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"PUT /deals/7?page=2 HTTP/2.0 3 Some(7) text");

        async fn raw(body: Bytes) -> String {
            body.len().to_string()
        }
        let response = raw.call(request("four")).await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"4");
    }
}
