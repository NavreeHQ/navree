//! [`CorsLayer`]: Navree's own CORS layer, after the
//! [Fetch standard](https://fetch.spec.whatwg.org/#http-cors-protocol).
//!
//! Used by the HTTP pipeline (`http.cors`) and by the gRPC edge for
//! gRPC-Web and Connect calls from browsers.
//!
//! - **Origins** are exact: a request's `Origin` is allowed when it equals a
//!   configured origin byte for byte. The allowed origin is echoed in
//!   `Access-Control-Allow-Origin`; `*` is never sent, so credentials are
//!   safe. A request with two `Origin` headers is treated as a disallowed
//!   origin.
//! - **Preflights** are `OPTIONS` requests with both `Origin` and
//!   `Access-Control-Request-Method`. They are answered here with an empty
//!   `200` and never reach the inner service. An allowed origin gets
//!   `Access-Control-Allow-Origin`, `-Allow-Credentials`, `-Allow-Methods`,
//!   `-Allow-Headers` and `-Max-Age`; a disallowed origin gets none of them,
//!   which makes the browser fail the preflight. An `OPTIONS` request
//!   without `Access-Control-Request-Method` is not a preflight: it reaches
//!   the inner service like any other request.
//! - **Other requests** always reach the inner service. When the origin is
//!   allowed, the response gets `Access-Control-Allow-Origin`,
//!   `-Allow-Credentials` and `-Expose-Headers`; otherwise it gets no CORS
//!   header, and the browser blocks the page from reading it.
//! - **`Vary`**: every response varies with `Origin`; a preflight response
//!   also varies with `Access-Control-Request-Method` and
//!   `Access-Control-Request-Headers`. Existing `Vary` values on the inner
//!   response are kept.
//!
//! The allowed methods and headers are lists the browser checks against its
//! request; this layer does not repeat that check on the server.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use http::header::{
    ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS, ACCESS_CONTROL_MAX_AGE,
    ACCESS_CONTROL_REQUEST_METHOD, InvalidHeaderValue, ORIGIN, VARY,
};
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response};
use pin_project_lite::pin_project;
use tower::{Layer, Service};

use crate::CorsOptions;

/// A Tower layer that applies CORS rules. See the [module
/// documentation](self).
///
/// ```
/// use std::time::Duration;
///
/// use http::{HeaderName, HeaderValue, Method};
/// use navree_http::CorsLayer;
///
/// let layer = CorsLayer::new([HeaderValue::from_static("https://app.example.com")])
///     .allow_credentials(true)
///     .allow_methods([Method::GET, Method::POST])
///     .allow_headers([HeaderName::from_static("content-type")])
///     .expose_headers([HeaderName::from_static("etag")])
///     .max_age(Duration::from_secs(600));
/// # let _ = layer;
/// ```
#[derive(Clone, Debug)]
pub struct CorsLayer {
    rules: Arc<Rules>,
}

#[derive(Clone, Debug, Default)]
struct Rules {
    origins: Vec<HeaderValue>,
    credentials: bool,
    methods: Option<HeaderValue>,
    headers: Option<HeaderValue>,
    expose: Option<HeaderValue>,
    max_age: Option<HeaderValue>,
}

impl CorsLayer {
    /// A layer that allows these exact origins (`https://app.example.com`),
    /// with credentials off, no methods or headers listed and no preflight
    /// caching. With no origins, no request is ever allowed.
    pub fn new(origins: impl IntoIterator<Item = HeaderValue>) -> Self {
        Self {
            rules: Arc::new(Rules {
                origins: origins.into_iter().collect(),
                ..Rules::default()
            }),
        }
    }

    /// The layer for `http.cors`: its origins, credentials and preflight
    /// cache time. `None` when no origin is listed (CORS is off). Set the
    /// methods and headers with the builder methods.
    ///
    /// Origins are trimmed; one that is not a valid header value is an
    /// error.
    pub fn from_options(options: &CorsOptions) -> Result<Option<Self>, InvalidHeaderValue> {
        if options.allowed_origins.is_empty() {
            return Ok(None);
        }
        let origins = options
            .allowed_origins
            .iter()
            .map(|origin| HeaderValue::from_str(origin.trim()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(
            Self::new(origins)
                .allow_credentials(options.allow_credentials)
                .max_age(Duration::from_secs(options.max_age_secs)),
        ))
    }

    /// Whether browsers may send cookies and other credentials
    /// (`Access-Control-Allow-Credentials: true`) from an allowed origin.
    #[must_use]
    pub fn allow_credentials(self, allow: bool) -> Self {
        self.with(|rules| rules.credentials = allow)
    }

    /// The methods listed in `Access-Control-Allow-Methods` on preflights.
    #[must_use]
    pub fn allow_methods(self, methods: impl IntoIterator<Item = Method>) -> Self {
        self.with(|rules| {
            rules.methods = comma_separated(methods.into_iter().map(|m| m.to_string()))
        })
    }

    /// The request headers listed in `Access-Control-Allow-Headers` on
    /// preflights. Name each one: with credentials, `*` would not be
    /// honoured by browsers.
    #[must_use]
    pub fn allow_headers(self, headers: impl IntoIterator<Item = HeaderName>) -> Self {
        self.with(|rules| {
            rules.headers = comma_separated(headers.into_iter().map(|h| h.to_string()))
        })
    }

    /// The response headers listed in `Access-Control-Expose-Headers` for
    /// allowed origins, so scripts may read them.
    #[must_use]
    pub fn expose_headers(self, headers: impl IntoIterator<Item = HeaderName>) -> Self {
        self.with(|rules| {
            rules.expose = comma_separated(headers.into_iter().map(|h| h.to_string()))
        })
    }

    /// How long browsers may cache a preflight answer
    /// (`Access-Control-Max-Age`, whole seconds).
    #[must_use]
    pub fn max_age(self, age: Duration) -> Self {
        self.with(|rules| rules.max_age = Some(HeaderValue::from(age.as_secs())))
    }

    fn with(mut self, change: impl FnOnce(&mut Rules)) -> Self {
        change(Arc::make_mut(&mut self.rules));
        self
    }
}

/// The names or methods as one comma-separated header value, or `None`
/// when there are none. Inputs are tokens, so the value is always valid.
fn comma_separated(items: impl Iterator<Item = String>) -> Option<HeaderValue> {
    let joined = items.collect::<Vec<_>>().join(",");
    if joined.is_empty() {
        return None;
    }
    HeaderValue::from_str(&joined).ok()
}

impl<S> Layer<S> for CorsLayer {
    type Service = Cors<S>;

    fn layer(&self, inner: S) -> Cors<S> {
        Cors {
            inner,
            rules: self.rules.clone(),
        }
    }
}

/// The service [`CorsLayer`] makes.
#[derive(Clone, Debug)]
pub struct Cors<S> {
    inner: S,
    rules: Arc<Rules>,
}

/// What a request's `Origin` header says.
enum Origin {
    /// No `Origin` header.
    Absent,
    /// An origin on the list.
    Allowed(HeaderValue),
    /// An origin that is not on the list, or two `Origin` headers.
    Denied,
}

impl Rules {
    fn origin(&self, headers: &HeaderMap) -> Origin {
        let mut values = headers.get_all(ORIGIN).iter();
        match (values.next(), values.next()) {
            (None, _) => Origin::Absent,
            (Some(origin), None) if self.origins.contains(origin) => {
                Origin::Allowed(origin.clone())
            }
            _ => Origin::Denied,
        }
    }

    /// The headers of a preflight answer.
    fn preflight(&self, origin: Origin) -> HeaderMap {
        let mut headers = HeaderMap::new();
        // A preflight response varies with all three request headers.
        headers.insert(
            VARY,
            HeaderValue::from_static(
                "Origin, Access-Control-Request-Method, Access-Control-Request-Headers",
            ),
        );
        if let Origin::Allowed(origin) = origin {
            headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            self.insert_credentials(&mut headers);
            if let Some(methods) = &self.methods {
                headers.insert(ACCESS_CONTROL_ALLOW_METHODS, methods.clone());
            }
            if let Some(allowed) = &self.headers {
                headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, allowed.clone());
            }
            if let Some(age) = &self.max_age {
                headers.insert(ACCESS_CONTROL_MAX_AGE, age.clone());
            }
        }
        headers
    }

    fn insert_credentials(&self, headers: &mut HeaderMap) {
        if self.credentials {
            headers.insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
    }

    /// Adds the CORS headers to the inner service's response.
    fn apply(&self, origin: Option<HeaderValue>, headers: &mut HeaderMap) {
        let varies_already = headers.get_all(VARY).iter().any(|value| {
            value.to_str().is_ok_and(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .any(|name| name == "*" || name.eq_ignore_ascii_case("origin"))
            })
        });
        if !varies_already {
            headers.append(VARY, HeaderValue::from_static("Origin"));
        }
        if let Some(origin) = origin {
            headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            self.insert_credentials(headers);
            if let Some(expose) = &self.expose {
                headers.insert(ACCESS_CONTROL_EXPOSE_HEADERS, expose.clone());
            }
        }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for Cors<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let origin = self.rules.origin(request.headers());
        let preflight = request.method() == Method::OPTIONS
            && !matches!(origin, Origin::Absent)
            && request
                .headers()
                .contains_key(ACCESS_CONTROL_REQUEST_METHOD);
        if preflight {
            return ResponseFuture {
                state: State::Preflight {
                    headers: Some(self.rules.preflight(origin)),
                },
            };
        }
        ResponseFuture {
            state: State::Forward {
                future: self.inner.call(request),
                rules: self.rules.clone(),
                origin: match origin {
                    Origin::Allowed(origin) => Some(origin),
                    Origin::Absent | Origin::Denied => None,
                },
            },
        }
    }
}

pin_project! {
    /// The response future of [`Cors`].
    pub struct ResponseFuture<F> {
        #[pin]
        state: State<F>,
    }
}

pin_project! {
    #[project = StateProj]
    enum State<F> {
        Preflight { headers: Option<HeaderMap> },
        Forward {
            #[pin]
            future: F,
            rules: Arc<Rules>,
            origin: Option<HeaderValue>,
        },
    }
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Default,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().state.project() {
            StateProj::Preflight { headers } => {
                let mut response = Response::new(B::default());
                if let Some(headers) = headers.take() {
                    *response.headers_mut() = headers;
                }
                Poll::Ready(Ok(response))
            }
            StateProj::Forward {
                future,
                rules,
                origin,
            } => {
                let mut response = match future.poll(cx) {
                    Poll::Ready(Ok(response)) => response,
                    other => return other,
                };
                rules.apply(origin.take(), response.headers_mut());
                Poll::Ready(Ok(response))
            }
        }
    }
}

#[cfg(test)]
mod tests;
