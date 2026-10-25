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
///     .assert_json_path("data.0.title", json!("Acme renewal"));
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl TestResponse {
    /// A response from its parts (to use the assertions on a response
    /// obtained some other way).
    pub fn from_parts(status: StatusCode, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body,
        }
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

    /// Asserts an RFC 9457 problem response: `status`, content type
    /// `application/problem+json`, a matching `status` member, a `title`,
    /// a `request_id` equal to the `X-Request-Id` header and, when `code` is
    /// given, the problem `type` (e.g. `"validation_failed"`).
    ///
    /// ```no_run
    /// # fn demo(response: navree_testing::TestResponse) {
    /// response.clone().assert_problem(422, "validation_failed");
    /// response.assert_problem(404, None);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// When any of those does not hold (the message shows the body).
    #[track_caller]
    pub fn assert_problem<'a>(self, status: u16, code: impl Into<Option<&'a str>>) -> Self {
        let this = self.assert_status(status);
        let content_type = this.header(CONTENT_TYPE.as_str()).unwrap_or("<none>");
        if content_type != navree_http::PROBLEM_JSON {
            panic!(
                "expected content type {}, got {content_type}\n{}",
                navree_http::PROBLEM_JSON,
                this.describe()
            );
        }
        let body = this.json_value();
        let request_id = this.header("x-request-id");
        let problem = if body["status"] != status {
            Some(format!(
                "expected problem status {status}, got {}",
                compact(&body["status"])
            ))
        } else if !body["title"].is_string() {
            Some("the problem has no `title`".to_owned())
        } else if request_id.is_none() || body["request_id"].as_str() != request_id {
            Some(format!(
                "the problem's request_id {} does not match the X-Request-Id header {request_id:?}",
                compact(&body["request_id"])
            ))
        } else {
            code.into()
                .filter(|code| body["type"] != *code)
                .map(|code| {
                    format!(
                        "expected problem type \"{code}\", got {}",
                        compact(&body["type"])
                    )
                })
        };
        if let Some(problem) = problem {
            panic!("{problem}\n{}", this.describe());
        }
        this
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
