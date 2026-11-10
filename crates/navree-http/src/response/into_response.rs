//! [`IntoResponse`] and what handlers return out of the box.

use std::borrow::Cow;
use std::convert::Infallible;
use std::fmt;

use bytes::{Bytes, BytesMut};
use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use http::{Extensions, StatusCode};
use navree_kernel::BoxError;

use super::{IntoResponseParts, Response, ResponseParts};
use crate::body::{Body, HttpBody};

const TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
const OCTET_STREAM: HeaderValue = HeaderValue::from_static("application/octet-stream");

/// What a handler, a middleware or a rejection can answer with.
///
/// A string is `text/plain`, bytes are `application/octet-stream`, a
/// [`StatusCode`] or `()` is an empty body, a `Result` is either side. A tuple
/// builds a response from the last element and applies the others to it, in
/// order, through [`IntoResponseParts`]; it may start with a [`StatusCode`].
///
/// ```
/// use http::StatusCode;
/// use navree_http::response::{IntoResponse, Response};
///
/// enum Refusal {
///     TooSoon,
/// }
///
/// impl IntoResponse for Refusal {
///     fn into_response(self) -> Response {
///         (StatusCode::TOO_EARLY, [("retry-after", "60")], "Too early").into_response()
///     }
/// }
///
/// let response = Refusal::TooSoon.into_response();
/// assert_eq!(response.status(), StatusCode::TOO_EARLY);
/// assert_eq!(response.headers()["retry-after"], "60");
/// assert_eq!(response.headers()["content-type"], "text/plain; charset=utf-8");
/// ```
pub trait IntoResponse {
    /// The response.
    #[must_use]
    fn into_response(self) -> Response;
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        let mut response = ().into_response();
        *response.status_mut() = self;
        response
    }
}

impl IntoResponse for () {
    fn into_response(self) -> Response {
        Body::empty().into_response()
    }
}

impl IntoResponse for Infallible {
    fn into_response(self) -> Response {
        match self {}
    }
}

impl<T, E> IntoResponse for Result<T, E>
where
    T: IntoResponse,
    E: IntoResponse,
{
    fn into_response(self) -> Response {
        match self {
            Ok(value) => value.into_response(),
            Err(error) => error.into_response(),
        }
    }
}

impl<B> IntoResponse for Response<B>
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    fn into_response(self) -> Response {
        self.map(Body::new)
    }
}

impl IntoResponse for http::response::Parts {
    fn into_response(self) -> Response {
        Response::from_parts(self, Body::empty())
    }
}

impl IntoResponse for Body {
    fn into_response(self) -> Response {
        Response::new(self)
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        Cow::Borrowed(self).into_response()
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        Cow::<'static, str>::Owned(self).into_response()
    }
}

impl IntoResponse for Box<str> {
    fn into_response(self) -> Response {
        String::from(self).into_response()
    }
}

impl IntoResponse for Cow<'static, str> {
    fn into_response(self) -> Response {
        let mut response = Body::from(self).into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, TEXT_PLAIN);
        response
    }
}

impl IntoResponse for Bytes {
    fn into_response(self) -> Response {
        let mut response = Body::from(self).into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, OCTET_STREAM);
        response
    }
}

impl IntoResponse for BytesMut {
    fn into_response(self) -> Response {
        self.freeze().into_response()
    }
}

impl IntoResponse for &'static [u8] {
    fn into_response(self) -> Response {
        Cow::Borrowed(self).into_response()
    }
}

impl<const N: usize> IntoResponse for &'static [u8; N] {
    fn into_response(self) -> Response {
        self.as_slice().into_response()
    }
}

impl<const N: usize> IntoResponse for [u8; N] {
    fn into_response(self) -> Response {
        self.to_vec().into_response()
    }
}

impl IntoResponse for Vec<u8> {
    fn into_response(self) -> Response {
        Cow::<'static, [u8]>::Owned(self).into_response()
    }
}

impl IntoResponse for Box<[u8]> {
    fn into_response(self) -> Response {
        Vec::from(self).into_response()
    }
}

impl IntoResponse for Cow<'static, [u8]> {
    fn into_response(self) -> Response {
        let mut response = Body::from(self).into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, OCTET_STREAM);
        response
    }
}

impl IntoResponse for HeaderMap {
    fn into_response(self) -> Response {
        let mut response = ().into_response();
        *response.headers_mut() = self;
        response
    }
}

impl IntoResponse for Extensions {
    fn into_response(self) -> Response {
        let mut response = ().into_response();
        *response.extensions_mut() = self;
        response
    }
}

impl<K, V, const N: usize> IntoResponse for [(K, V); N]
where
    K: TryInto<HeaderName>,
    K::Error: fmt::Display,
    V: TryInto<HeaderValue>,
    V::Error: fmt::Display,
{
    fn into_response(self) -> Response {
        (self, ()).into_response()
    }
}

impl<R: IntoResponse> IntoResponse for (R,) {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

impl<R: IntoResponse> IntoResponse for (StatusCode, R) {
    fn into_response(self) -> Response {
        let (status, response) = self;
        let mut response = response.into_response();
        *response.status_mut() = status;
        response
    }
}

impl<R: IntoResponse> IntoResponse for (http::response::Parts, R) {
    fn into_response(self) -> Response {
        let (parts, response) = self;
        (parts.status, parts.headers, parts.extensions, response).into_response()
    }
}

impl<R: IntoResponse> IntoResponse for (Response<()>, R) {
    fn into_response(self) -> Response {
        let (template, response) = self;
        (template.into_parts().0, response).into_response()
    }
}

/// Builds the response and applies each part in order; a part that fails is
/// the answer, as it is: returned from the enclosing function, so nothing
/// outside the parts (a status) overrides it.
macro_rules! with_parts {
    ($response:ident, $($part:ident),*) => {{
        let parts = ResponseParts { response: $response.into_response() };
        $(
            let parts = match $part.into_response_parts(parts) {
                Ok(parts) => parts,
                Err(error) => return error.into_response(),
            };
        )*
        parts.response
    }};
}

macro_rules! tuple_responses {
    ($($part:ident),*) => {
        #[allow(non_snake_case)]
        impl<R: IntoResponse, $($part: IntoResponseParts,)*> IntoResponse for ($($part,)* R,) {
            fn into_response(self) -> Response {
                let ($($part,)* response,) = self;
                with_parts!(response, $($part),*)
            }
        }

        #[allow(non_snake_case)]
        impl<R: IntoResponse, $($part: IntoResponseParts,)*> IntoResponse
            for (StatusCode, $($part,)* R,)
        {
            fn into_response(self) -> Response {
                let (status, $($part,)* response,) = self;
                let response = with_parts!(response, $($part),*);
                (status, response).into_response()
            }
        }

        #[allow(non_snake_case)]
        impl<R: IntoResponse, $($part: IntoResponseParts,)*> IntoResponse
            for (http::response::Parts, $($part,)* R,)
        {
            fn into_response(self) -> Response {
                let (outer, $($part,)* response,) = self;
                let response = with_parts!(response, $($part),*);
                (outer, response).into_response()
            }
        }

        #[allow(non_snake_case)]
        impl<R: IntoResponse, $($part: IntoResponseParts,)*> IntoResponse
            for (Response<()>, $($part,)* R,)
        {
            fn into_response(self) -> Response {
                let (template, $($part,)* response,) = self;
                (template.into_parts().0, $($part,)* response).into_response()
            }
        }
    };
}

tuple_responses!(T1);
tuple_responses!(T1, T2);
tuple_responses!(T1, T2, T3);
tuple_responses!(T1, T2, T3, T4);
tuple_responses!(T1, T2, T3, T4, T5);
tuple_responses!(T1, T2, T3, T4, T5, T6);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
tuple_responses!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
tuple_responses!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
tuple_responses!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
);
