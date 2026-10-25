//! [`TestClient`] and [`TestRequest`]: in-process requests through the app router.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use bytes::Bytes;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE};
use http::{HeaderName, HeaderValue, Method, Request};
use http_body_util::BodyExt;
use keel_contracts::Principal;
use serde::Serialize;
use tower::ServiceExt;

use crate::TestResponse;
use crate::auth::ActingAs;
use crate::cookies::CookieJar;

/// The peer address of every test request (what `ClientIp` reports).
const PEER: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49152);

/// The route that hands out the XSRF cookie.
const CSRF_COOKIE_PATH: &str = "/sanctum/csrf-cookie";

/// The cookie holding the CSRF token for JavaScript clients.
const XSRF_COOKIE: &str = "XSRF-TOKEN";

/// The header carrying the [`XSRF_COOKIE`] value back.
const X_XSRF_TOKEN: &str = "x-xsrf-token";

/// Sends requests to a [`TestApp`](crate::TestApp) in-process, through the
/// full router.
///
/// Every request carries `Accept: application/json` and, when the app
/// allows CORS origins, `Origin` set to the first one (a request's own
/// headers replace these). Cookies set by responses are kept in the
/// client's jar and sent back on later requests whose path matches; clones
/// of a client share the jar. Requests come from `127.0.0.1` (what
/// `ClientIp` reports), so the app's per-IP rate limit (600 per minute by
/// default) applies; request-heavy tests turn it off with
/// `.config("http.per_ip_rate_limit", serde_json::Value::Null)`. Redirects
/// are returned, not followed.
///
/// ```no_run
/// # async fn demo(app: keel_testing::TestApp) {
/// use serde_json::json;
///
/// let client = app.client();
/// client.csrf_cookie().await;
/// client
///     .post("/api/v1/login")
///     .xsrf()
///     .json(&json!({ "email": "ada@example.com", "password": "secret" }))
///     .send()
///     .await
///     .assert_status(200);
/// // The session cookie set by the login is sent from now on.
/// client.get("/api/v1/me").send().await.assert_status(200);
/// # }
/// ```
#[derive(Clone)]
pub struct TestClient {
    target: Arc<Target>,
    jar: Arc<Mutex<CookieJar>>,
    acting_as: Option<Principal>,
}

/// What every client of one app shares.
pub(crate) struct Target {
    /// The app's full router.
    pub(crate) router: Router,
    /// The default `Origin` header.
    pub(crate) origin: Option<HeaderValue>,
    /// Whether the app authenticates with the `TestAuthenticator`.
    pub(crate) test_auth: bool,
}

impl TestClient {
    pub(crate) fn new(target: Arc<Target>) -> Self {
        Self {
            target,
            jar: Arc::default(),
            acting_as: None,
        }
    }

    /// This client (sharing its cookie jar) with every request
    /// authenticated as `principal` by the
    /// [`TestAuthenticator`](crate::TestAuthenticator).
    ///
    /// # Panics
    ///
    /// When the app authenticates with an authenticator of its own: build it
    /// with [`TestAppBuilder::fake_auth`](crate::TestAppBuilder::fake_auth)
    /// or [`TestAuthenticator::install`](crate::TestAuthenticator::install).
    pub fn acting_as(mut self, principal: Principal) -> Self {
        assert!(
            self.target.test_auth,
            "acting_as needs the TestAuthenticator, but the app registers its own \
             `dyn Authenticator`; build the app with `TestApp::builder().fake_auth()` \
             or `TestApp::from_builder(TestAuthenticator::install(builder))`"
        );
        self.acting_as = Some(principal);
        self
    }

    /// This client (sharing its cookie jar) without an
    /// [`acting_as`](TestClient::acting_as) principal.
    pub fn as_guest(mut self) -> Self {
        self.acting_as = None;
        self
    }

    fn jar(&self) -> MutexGuard<'_, CookieJar> {
        self.jar.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A request with `method` to `path` (and optional query string).
    pub fn request(&self, method: Method, path: &str) -> TestRequest {
        TestRequest {
            client: self.clone(),
            method,
            path: path.to_owned(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// A `GET` request.
    pub fn get(&self, path: &str) -> TestRequest {
        self.request(Method::GET, path)
    }

    /// A `POST` request.
    pub fn post(&self, path: &str) -> TestRequest {
        self.request(Method::POST, path)
    }

    /// A `PUT` request.
    pub fn put(&self, path: &str) -> TestRequest {
        self.request(Method::PUT, path)
    }

    /// A `PATCH` request.
    pub fn patch(&self, path: &str) -> TestRequest {
        self.request(Method::PATCH, path)
    }

    /// A `DELETE` request.
    pub fn delete(&self, path: &str) -> TestRequest {
        self.request(Method::DELETE, path)
    }

    /// `GET /sanctum/csrf-cookie`, which sets the `XSRF-TOKEN` cookie (see
    /// [`TestRequest::xsrf`]).
    ///
    /// # Panics
    ///
    /// When the response sets no `XSRF-TOKEN` cookie (is the session module
    /// in the app?).
    pub async fn csrf_cookie(&self) -> TestResponse {
        let response = self.get(CSRF_COOKIE_PATH).send().await;
        if self
            .jar()
            .value_for(XSRF_COOKIE, CSRF_COOKIE_PATH)
            .is_none()
        {
            panic!(
                "GET {CSRF_COOKIE_PATH} set no {XSRF_COOKIE} cookie (status {}, body: {}); is the session module in the app?",
                response.status(),
                response.text()
            );
        }
        response
    }

    /// The raw (not percent-decoded) value of cookie `name` in the jar.
    pub fn cookie(&self, name: &str) -> Option<String> {
        self.jar().get(name)
    }

    /// Puts the session cookie `name=value` (raw, sent as is) for path `/`
    /// into the jar, as if a response had set it.
    pub fn set_cookie(&self, name: &str, value: &str) {
        self.jar().set(name, value);
    }

    /// Removes every cookie named `name` from the jar.
    pub fn remove_cookie(&self, name: &str) {
        self.jar().remove(name);
    }

    /// Empties the jar.
    pub fn clear_cookies(&self) {
        self.jar().clear();
    }
}

impl std::fmt::Debug for TestClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestClient")
            .field("acting_as", &self.acting_as.as_ref().map(|p| p.user_id))
            .finish_non_exhaustive()
    }
}

/// A request being built; [`send`](TestRequest::send) runs it.
#[derive(Debug)]
#[must_use = "a TestRequest does nothing until `send().await`"]
pub struct TestRequest {
    client: TestClient,
    method: Method,
    path: String,
    headers: http::HeaderMap,
    body: Bytes,
}

impl TestRequest {
    /// A JSON body (`Content-Type: application/json`).
    ///
    /// # Panics
    ///
    /// When `value` cannot be serialized.
    pub fn json<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.body = serde_json::to_vec(value)
            .expect("the JSON body serializes")
            .into();
        self.headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self
    }

    /// A form body (`Content-Type: application/x-www-form-urlencoded`).
    ///
    /// # Panics
    ///
    /// When `value` cannot be form-encoded.
    pub fn form<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.body = serde_urlencoded::to_string(value)
            .expect("the form body serializes")
            .into();
        self.headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        self
    }

    /// Appends `value`, form-encoded (`page=2&q=a+b`), to the query string.
    ///
    /// # Panics
    ///
    /// When `value` cannot be form-encoded.
    pub fn query<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        let query = serde_urlencoded::to_string(value).expect("the query serializes");
        if !query.is_empty() {
            let separator = if self.path.contains('?') { '&' } else { '?' };
            self.path.push(separator);
            self.path.push_str(&query);
        }
        self
    }

    /// A raw body; set its `Content-Type` with [`header`](TestRequest::header).
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// `Authorization: Bearer <token>`.
    ///
    /// # Panics
    ///
    /// When `token` is not a valid header value.
    pub fn bearer(self, token: &str) -> Self {
        self.header(AUTHORIZATION, format!("Bearer {token}"))
    }

    /// Sets header `name` (replacing a default such as `Accept` or `Origin`).
    ///
    /// # Panics
    ///
    /// When `name` or `value` is not a valid header name or value.
    pub fn header<K, V>(mut self, name: K, value: V) -> Self
    where
        K: TryInto<HeaderName>,
        K::Error: std::fmt::Display,
        V: TryInto<HeaderValue>,
        V::Error: std::fmt::Display,
    {
        let name = name
            .try_into()
            .unwrap_or_else(|error| panic!("invalid header name: {error}"));
        let value = value
            .try_into()
            .unwrap_or_else(|error| panic!("invalid value for header `{name}`: {error}"));
        self.headers.insert(name, value);
        self
    }

    /// Sends the `XSRF-TOKEN` cookie's raw value as `X-XSRF-TOKEN`, as a
    /// browser SPA does (the server URL-decodes it).
    ///
    /// # Panics
    ///
    /// When the jar holds no `XSRF-TOKEN` cookie for this path: call
    /// [`TestClient::csrf_cookie`] first.
    pub fn xsrf(mut self) -> Self {
        let path = request_path(&self.path);
        let token = self
            .client
            .jar()
            .value_for(XSRF_COOKIE, path)
            .unwrap_or_else(|| {
                panic!(
                    "no {XSRF_COOKIE} cookie in the jar for `{path}`; call `client.csrf_cookie().await` first"
                )
            });
        let value = HeaderValue::from_str(&token).expect("stored cookies are valid header values");
        self.headers.insert(X_XSRF_TOKEN, value);
        self
    }

    /// Sends the request through the app's router, stores the cookies the
    /// response sets and collects the whole body (so not for endless
    /// streams such as server-sent events).
    ///
    /// # Panics
    ///
    /// When the path is not a valid URI or the response body fails.
    pub async fn send(self) -> TestResponse {
        let mut request = Request::builder()
            .method(self.method)
            .uri(&self.path)
            .body(Body::from(self.body))
            .unwrap_or_else(|error| panic!("invalid request to `{}`: {error}", self.path));
        *request.headers_mut() = self.headers;
        let headers = request.headers_mut();
        if !headers.contains_key(ACCEPT) {
            headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        }
        if let Some(origin) = &self.client.target.origin
            && !headers.contains_key(ORIGIN)
        {
            headers.insert(ORIGIN, origin.clone());
        }
        let path = request.uri().path().to_owned();
        if let Some(cookies) = self.client.jar().header_for(&path) {
            let headers = request.headers_mut();
            // An explicit `Cookie` header goes first, so its values win.
            let cookies = match headers.get(COOKIE).and_then(|v| v.to_str().ok()) {
                Some(explicit) => format!("{explicit}; {cookies}"),
                None => cookies,
            };
            let header =
                HeaderValue::from_str(&cookies).expect("stored cookies are valid header values");
            headers.insert(COOKIE, header);
        }
        request.extensions_mut().insert(ConnectInfo(PEER));
        if let Some(principal) = &self.client.acting_as {
            request.extensions_mut().insert(ActingAs(principal.clone()));
        }
        let response = self
            .client
            .target
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|never| match never {});
        let (parts, body) = response.into_parts();
        {
            let mut jar = self.client.jar();
            for set_cookie in parts.headers.get_all(SET_COOKIE) {
                if let Ok(set_cookie) = set_cookie.to_str() {
                    jar.store(&path, set_cookie);
                }
            }
        }
        let body = body
            .collect()
            .await
            .expect("the response body is readable")
            .to_bytes();
        TestResponse::from_parts(parts.status, parts.headers, body)
    }
}

/// The path of `uri` without the query string or fragment.
fn request_path(uri: &str) -> &str {
    let end = uri.find(['?', '#']).unwrap_or(uri.len());
    &uri[..end]
}
