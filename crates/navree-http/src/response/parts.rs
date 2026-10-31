//! [`IntoResponseParts`]: what a tuple response applies to its last element.

use std::convert::Infallible;
use std::fmt;

use http::header::{HeaderMap, HeaderName, HeaderValue};
use http::{Extensions, StatusCode};

use super::{IntoResponse, Response};

/// Something that sets headers or extensions on a response.
///
/// An array of pairs sets each header, replacing what was there;
/// [`AppendHeaders`] adds to it; a [`HeaderMap`] extends the headers;
/// [`Extensions`] extend the extensions; `None` does nothing.
///
/// ```
/// use http::StatusCode;
/// use navree_http::response::{IntoResponse, IntoResponseParts, ResponseParts};
///
/// struct Deprecated;
///
/// impl IntoResponseParts for Deprecated {
///     type Error = StatusCode;
///
///     fn into_response_parts(self, mut response: ResponseParts) -> Result<ResponseParts, StatusCode> {
///         let value = "true".parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
///         response.headers_mut().insert("deprecation", value);
///         Ok(response)
///     }
/// }
///
/// let response = (Deprecated, "still here").into_response();
/// assert_eq!(response.headers()["deprecation"], "true");
/// ```
pub trait IntoResponseParts {
    /// What to answer with instead when this cannot be applied.
    type Error: IntoResponse;

    /// Applies this to the response.
    fn into_response_parts(self, response: ResponseParts) -> Result<ResponseParts, Self::Error>;
}

/// The response so far, for [`IntoResponseParts`].
#[derive(Debug)]
pub struct ResponseParts {
    pub(super) response: Response,
}

impl ResponseParts {
    /// The headers.
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        self.response.headers()
    }

    /// The headers, to change.
    #[must_use]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        self.response.headers_mut()
    }

    /// The extensions.
    #[must_use]
    pub fn extensions(&self) -> &Extensions {
        self.response.extensions()
    }

    /// The extensions, to change.
    #[must_use]
    pub fn extensions_mut(&mut self) -> &mut Extensions {
        self.response.extensions_mut()
    }
}

impl<T: IntoResponseParts> IntoResponseParts for Option<T> {
    type Error = T::Error;

    fn into_response_parts(self, response: ResponseParts) -> Result<ResponseParts, Self::Error> {
        match self {
            Some(inner) => inner.into_response_parts(response),
            None => Ok(response),
        }
    }
}

impl IntoResponseParts for HeaderMap {
    type Error = Infallible;

    fn into_response_parts(
        self,
        mut response: ResponseParts,
    ) -> Result<ResponseParts, Self::Error> {
        response.headers_mut().extend(self);
        Ok(response)
    }
}

impl IntoResponseParts for Extensions {
    type Error = Infallible;

    fn into_response_parts(
        self,
        mut response: ResponseParts,
    ) -> Result<ResponseParts, Self::Error> {
        response.extensions_mut().extend(self);
        Ok(response)
    }
}

impl IntoResponseParts for () {
    type Error = Infallible;

    fn into_response_parts(self, response: ResponseParts) -> Result<ResponseParts, Self::Error> {
        Ok(response)
    }
}

impl<K, V, const N: usize> IntoResponseParts for [(K, V); N]
where
    K: TryInto<HeaderName>,
    K::Error: fmt::Display,
    V: TryInto<HeaderValue>,
    V::Error: fmt::Display,
{
    type Error = TryIntoHeaderError<K::Error, V::Error>;

    fn into_response_parts(
        self,
        mut response: ResponseParts,
    ) -> Result<ResponseParts, Self::Error> {
        for (name, value) in self {
            let name = name.try_into().map_err(TryIntoHeaderError::name)?;
            let value = value.try_into().map_err(TryIntoHeaderError::value)?;
            response.headers_mut().insert(name, value);
        }
        Ok(response)
    }
}

/// Headers to add to a response without replacing those already there, such
/// as several `Set-Cookie`.
///
/// ```
/// use http::header::SET_COOKIE;
/// use navree_http::response::{AppendHeaders, IntoResponse};
///
/// let response = (
///     AppendHeaders([(SET_COOKIE, "a=1"), (SET_COOKIE, "b=2")]),
///     "signed in",
/// )
///     .into_response();
/// assert_eq!(response.headers().get_all(SET_COOKIE).iter().count(), 2);
/// ```
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct AppendHeaders<I>(pub I);

impl<I, K, V> IntoResponse for AppendHeaders<I>
where
    I: IntoIterator<Item = (K, V)>,
    K: TryInto<HeaderName>,
    K::Error: fmt::Display,
    V: TryInto<HeaderValue>,
    V::Error: fmt::Display,
{
    fn into_response(self) -> Response {
        (self, ()).into_response()
    }
}

impl<I, K, V> IntoResponseParts for AppendHeaders<I>
where
    I: IntoIterator<Item = (K, V)>,
    K: TryInto<HeaderName>,
    K::Error: fmt::Display,
    V: TryInto<HeaderValue>,
    V::Error: fmt::Display,
{
    type Error = TryIntoHeaderError<K::Error, V::Error>;

    fn into_response_parts(
        self,
        mut response: ResponseParts,
    ) -> Result<ResponseParts, Self::Error> {
        for (name, value) in self.0 {
            let name = name.try_into().map_err(TryIntoHeaderError::name)?;
            let value = value.try_into().map_err(TryIntoHeaderError::value)?;
            response.headers_mut().append(name, value);
        }
        Ok(response)
    }
}

/// A name or value that is not a valid header: a 500 with the reason as text
/// (the pipeline renders it as an error).
#[derive(Debug)]
pub enum TryIntoHeaderError<K, V> {
    /// Not a header name.
    Name(K),
    /// Not a header value.
    Value(V),
}

impl<K, V> TryIntoHeaderError<K, V> {
    fn name(error: K) -> Self {
        Self::Name(error)
    }

    fn value(error: V) -> Self {
        Self::Value(error)
    }
}

impl<K: fmt::Display, V: fmt::Display> IntoResponse for TryIntoHeaderError<K, V> {
    fn into_response(self) -> Response {
        let text = match self {
            Self::Name(error) => error.to_string(),
            Self::Value(error) => error.to_string(),
        };
        (StatusCode::INTERNAL_SERVER_ERROR, text).into_response()
    }
}

impl<K, V> fmt::Display for TryIntoHeaderError<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Name(_) => "failed to convert key to a header name",
            Self::Value(_) => "failed to convert value to a header value",
        })
    }
}

impl<K, V> std::error::Error for TryIntoHeaderError<K, V>
where
    K: std::error::Error + 'static,
    V: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Name(error) => error,
            Self::Value(error) => error,
        })
    }
}

macro_rules! tuple_parts {
    ($($part:ident),*) => {
        #[allow(non_snake_case)]
        impl<$($part: IntoResponseParts,)*> IntoResponseParts for ($($part,)*) {
            type Error = Response;

            fn into_response_parts(
                self,
                response: ResponseParts,
            ) -> Result<ResponseParts, Response> {
                let ($($part,)*) = self;
                $(
                    let response = match $part.into_response_parts(response) {
                        Ok(response) => response,
                        Err(error) => return Err(error.into_response()),
                    };
                )*
                Ok(response)
            }
        }
    };
}

tuple_parts!(T1);
tuple_parts!(T1, T2);
tuple_parts!(T1, T2, T3);
tuple_parts!(T1, T2, T3, T4);
tuple_parts!(T1, T2, T3, T4, T5);
tuple_parts!(T1, T2, T3, T4, T5, T6);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
tuple_parts!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
tuple_parts!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
tuple_parts!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
);
