//! [`TestResponse`]: a collected response with assertion helpers.

use bytes::Bytes;
use cookie::Cookie;
use http::header::{CONTENT_TYPE, SET_COOKIE};
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Bodies longer than this are cut in assertion messages.
const MAX_SHOWN_BODY: usize = 8 * 1024;

/// A response collected by [`TestRequest::send`](crate::TestRequest::send).
///
/// Assertions take and return the response, so they chain; on failure they
/// panic with the status and the body:
///
/// ```no_run
/// # async fn demo(client: navree_testing::TestClient) {
/// use serde_json::json;
///
/// client
///     .get("/api/v1/deals")
///     .send()
///     .await
///     .assert_status(200)
///     .assert_json_path("data.0.title", json!("Agra renewal"));
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    extensions: http::Extensions,
}

impl TestResponse {
    /// A response from its parts (to use the assertions on a response
    /// obtained some other way).
    pub fn from_parts(status: StatusCode, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body,
            extensions: http::Extensions::new(),
        }
    }

    /// The same response, holding what the app left on it beside its
    /// headers and its body (`http::Response::extensions`).
    #[must_use]
    pub fn with_extensions(mut self, extensions: http::Extensions) -> Self {
        self.extensions = extensions;
        self
    }

    /// What a handler or a layer left on the response for the code around
    /// it, by its type: nothing a client over the network sees, and what a
    /// test of an app in the same process can read.
    ///
    /// ```
    /// use bytes::Bytes;
    /// use http::{Extensions, HeaderMap, StatusCode};
    /// use navree_testing::TestResponse;
    ///
    /// #[derive(Clone, Debug, PartialEq)]
    /// struct Graded(&'static str);
    ///
    /// let mut left = Extensions::new();
    /// left.insert(Graded("designed"));
    /// let response = TestResponse::from_parts(StatusCode::OK, HeaderMap::new(), Bytes::new())
    ///     .with_extensions(left);
    /// assert_eq!(response.extension::<Graded>(), Some(&Graded("designed")));
    /// assert_eq!(response.extension::<String>(), None);
    /// ```
    pub fn extension<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions.get::<T>()
    }

    /// The status code (compares with a `u16`: `response.status() == 200`).
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The first value of header `name`, if present and valid UTF-8.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// Every response header.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// The cookie `name` set by this response (`Set-Cookie`), with its
    /// attributes; the last one wins if it is set twice.
    pub fn set_cookie(&self, name: &str) -> Option<Cookie<'static>> {
        self.headers
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| Cookie::parse(v.to_owned()).ok())
            .rfind(|c| c.name() == name)
    }

    /// The raw body.
    pub fn bytes(&self) -> &Bytes {
        &self.body
    }

    /// The body as text (invalid UTF-8 replaced).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body deserialized as JSON into `T`.
    ///
    /// # Panics
    ///
    /// When the body does not deserialize (the message shows the body).
    #[track_caller]
    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "the response body is not a JSON `{}` ({error})\n{}",
                std::any::type_name::<T>(),
                self.describe()
            )
        })
    }

    /// The body as a JSON value.
    ///
    /// # Panics
    ///
    /// When the body is not JSON (the message shows the body).
    #[track_caller]
    pub fn json_value(&self) -> Value {
        self.json()
    }

    /// The body as a JSON value, or `null` when it is not JSON.
    pub(crate) fn json_value_or_null(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    /// Asserts the status code.
    ///
    /// # Panics
    ///
    /// When the status differs (the message shows the body).
    #[track_caller]
    pub fn assert_status(self, expected: u16) -> Self {
        if self.status != expected {
            panic!(
                "expected status {expected}, got {}\n{}",
                self.status,
                self.describe()
            );
        }
        self
    }

    /// Asserts that the JSON value at `path` equals `expected`. The path is
    /// dotted; numeric segments index arrays (`"data.0.name"`).
    ///
    /// # Panics
    ///
    /// When the value differs or the path does not exist; the message shows
    /// the actual value (or the first missing segment) and the body.
    #[track_caller]
    pub fn assert_json_path(self, path: &str, expected: Value) -> Self {
        let body = self.json_value();
        let actual = match lookup(&body, path) {
            Ok(actual) if *actual == expected => return self,
            Ok(actual) => compact(actual),
            Err(missing) => format!("<missing> (`{missing}` does not exist)"),
        };
        panic!(
            "JSON path `{path}`\n  expected: {}\n  actual: {actual}\n{}",
            compact(&expected),
            self.describe()
        );
    }

    /// Asserts a JSON error response: `status`, content type
    /// `application/json`, a body `{"statusCode", "message", "error"}` with a
    /// matching `statusCode`, an `X-Request-Id` header and, when `message`
    /// is given, that message (`message` is a string, or for validation
    /// errors a list that must contain it).
    ///
    /// ```no_run
    /// # fn demo(response: navree_testing::TestResponse) {
    /// response.clone().assert_error(404, "Not Found");
    /// response.assert_error(404, None);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// When any of those does not hold (the message shows the body).
    #[track_caller]
    pub fn assert_error<'a>(self, status: u16, message: impl Into<Option<&'a str>>) -> Self {
        let this = self.assert_status(status);
        let content_type = this.header(CONTENT_TYPE.as_str()).unwrap_or("<none>");
        if content_type != navree_http::ERROR_CONTENT_TYPE {
            panic!(
                "expected content type {}, got {content_type}\n{}",
                navree_http::ERROR_CONTENT_TYPE,
                this.describe()
            );
        }
        let body = this.json_value();
        let problem = if body["statusCode"] != status {
            Some(format!(
                "expected statusCode {status}, got {}",
                compact(&body["statusCode"])
            ))
        } else if !body["error"].is_string() {
            Some("the error body has no `error` reason phrase".to_owned())
        } else if !is_message(&body["message"]) {
            Some("the error body has no `message` (a string or a list of strings)".to_owned())
        } else if this.header("x-request-id").is_none() {
            Some("the response has no X-Request-Id header".to_owned())
        } else {
            message
                .into()
                .filter(|expected| !has_message(&body["message"], expected))
                .map(|expected| {
                    format!(
                        "expected message \"{expected}\", got {}",
                        compact(&body["message"])
                    )
                })
        };
        if let Some(problem) = problem {
            panic!("{problem}\n{}", this.describe());
        }
        this
    }

    /// Asserts a 400 validation error (see [`assert_error`](Self::assert_error))
    /// with at least one message for `field` (a dotted path) in `errors`.
    ///
    /// ```no_run
    /// # fn demo(response: navree_testing::TestResponse) {
    /// response
    ///     .assert_validation_error("email")
    ///     .assert_error(400, "email must be a valid email address");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// When the response is not a 400 error or has no message for `field`
    /// (the message shows the body).
    #[track_caller]
    pub fn assert_validation_error(self, field: &str) -> Self {
        let this = self.assert_error(400, None);
        let body = this.json_value();
        let messages = body["errors"].get(field).and_then(Value::as_array);
        if !messages.is_some_and(|messages| !messages.is_empty()) {
            panic!(
                "no validation error for `{field}`; errors: {}\n{}",
                compact(&body["errors"]),
                this.describe()
            );
        }
        this
    }

    /// Asserts a JSON error response (see [`assert_error`](Self::assert_error))
    /// whose machine-readable `code` is `code`.
    ///
    /// ```no_run
    /// # fn demo(response: navree_testing::TestResponse) {
    /// response
    ///     .assert_error_code(401, "invalid_credentials")
    ///     .assert_error(401, "Invalid credentials");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// When the response is not an error with `status`, or its body has no
    /// `code` or another one (the message shows the body).
    #[track_caller]
    pub fn assert_error_code(self, status: u16, code: &str) -> Self {
        let this = self.assert_error(status, None);
        let body = this.json_value();
        match body.get("code") {
            Some(actual) if actual == code => this,
            Some(actual) => panic!(
                "expected code \"{code}\", got {}\n{}",
                compact(actual),
                this.describe()
            ),
            None => panic!(
                "expected code \"{code}\", but the error body has no `code` (is http.errors.code off?)\n{}",
                this.describe()
            ),
        }
    }

    /// An older name for [`assert_error`](Self::assert_error) and
    /// [`assert_error_code`](Self::assert_error_code), kept so a test written with it still
    /// builds: it asserts an error response with `status` like `assert_error(status, None)`
    /// and, when `code` is given, its `code` like `assert_error_code`.
    ///
    /// # Panics
    ///
    /// Like [`assert_error_code`](Self::assert_error_code).
    #[deprecated(
        since = "1.0.0",
        note = "use `assert_error(status, message)` or `assert_error_code(status, code)`: errors use Navree's JSON error shape, not RFC 9457 problem details"
    )]
    #[doc(hidden)]
    #[track_caller]
    pub fn assert_problem<'a>(self, status: u16, code: impl Into<Option<&'a str>>) -> Self {
        match code.into() {
            Some(code) => self.assert_error_code(status, code),
            None => self.assert_error(status, None),
        }
    }

    /// The status and the (possibly shortened) body, for failure messages.
    fn describe(&self) -> String {
        let text = self.text();
        let body = if text.len() > MAX_SHOWN_BODY {
            let mut end = MAX_SHOWN_BODY;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}... ({} bytes)", &text[..end], text.len())
        } else {
            text
        };
        format!("status {}, body: {body}", self.status)
    }
}

/// The value at dotted `path`; `Err` holds the path up to the first missing
/// segment.
fn lookup<'v>(root: &'v Value, path: &str) -> Result<&'v Value, String> {
    let mut node = root;
    let mut walked: Vec<&str> = Vec::new();
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        walked.push(segment);
        let next = match node {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        };
        node = next.ok_or_else(|| walked.join("."))?;
    }
    Ok(node)
}

/// An error `message`: a string, or a list of strings.
fn is_message(message: &Value) -> bool {
    match message {
        Value::String(_) => true,
        Value::Array(items) => items.iter().all(Value::is_string),
        _ => false,
    }
}

/// Whether `message` is `expected`, or a list containing it.
fn has_message(message: &Value, expected: &str) -> bool {
    match message {
        Value::String(message) => message == expected,
        Value::Array(items) => items.iter().any(|item| item == expected),
        _ => false,
    }
}

/// `value` as compact JSON.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_walks_objects_and_arrays_and_names_the_missing_segment() {
        let body = serde_json::json!({"a": {"b": [10, {"c": true}]}});
        assert_eq!(lookup(&body, "a.b.1.c"), Ok(&Value::Bool(true)));
        assert_eq!(lookup(&body, ""), Ok(&body));
        assert_eq!(lookup(&body, "a.x.y"), Err("a.x".to_owned()));
        assert_eq!(lookup(&body, "a.b.7"), Err("a.b.7".to_owned()));
        assert_eq!(lookup(&body, "a.b.first"), Err("a.b.first".to_owned()));
        assert_eq!(
            lookup(&body, "a.b.0.deeper"),
            Err("a.b.0.deeper".to_owned())
        );
    }
}
