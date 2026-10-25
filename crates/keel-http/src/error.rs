//! [`AppError`]: every HTTP error, rendered as RFC 9457 problem+json.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::body::Body;
use axum::response::{IntoResponse, Response};
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER, X_CONTENT_TYPE_OPTIONS};
use http::{HeaderMap, StatusCode};
use keel_kernel::BoxError;
use serde::Serialize;

/// The content type of every error response.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// The result type of handlers: `Result<T>` fails with an [`AppError`].
pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// An HTTP error. Handlers, extractors, guards and layers return it; it
/// renders as an RFC 9457 problem+json body:
///
/// ```json
/// {"type": "validation_failed", "title": "The given data was invalid.", "status": 422,
///  "request_id": "0192...", "errors": {"email": ["The email field is required."]}}
/// ```
///
/// `type` is the machine-readable [`code`](AppError::code) (a relative URI
/// reference), `detail` and `errors` are omitted when empty, and `request_id`
/// is filled in by the request pipeline. Any error type converts into a 500
/// with `?` (its message is logged, never sent); modules turn their own error
/// types into specific statuses with
/// [`ModuleBuilderHttpExt::map_error`](crate::ModuleBuilderHttpExt::map_error).
///
/// `AppError` deliberately does not implement `std::error::Error`, which is
/// what makes the blanket `From<E: Into<BoxError>>` conversion possible.
pub struct AppError {
    /// The HTTP status.
    pub status: StatusCode,
    /// A short machine-readable code, rendered as the problem `type`.
    pub code: String,
    /// A short human-readable summary.
    pub title: String,
    /// A human-readable explanation of this occurrence (sent to the client).
    pub detail: Option<String>,
    /// Field errors (`{"field.path": ["message"]}`), for 422 responses.
    /// Boxed, like `headers`, to keep `Result<T, AppError>` small.
    pub errors: Option<Box<BTreeMap<String, Vec<String>>>>,
    /// Extra response headers (`Retry-After`, `WWW-Authenticate`, ...).
    pub headers: Box<HeaderMap>,
    /// The underlying error, logged for 5xx responses and never sent.
    pub source: Option<BoxError>,
}

impl AppError {
    /// An error with an explicit status, code and title.
    pub fn new(status: StatusCode, code: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            title: title.into(),
            detail: None,
            errors: None,
            headers: Box::default(),
            source: None,
        }
    }

    /// The default error for `status` (code and title derived from it).
    pub fn from_status(status: StatusCode) -> Self {
        let (code, title) = match status.as_u16() {
            400 => ("bad_request", "Bad Request"),
            401 => ("unauthenticated", "Unauthenticated."),
            403 => ("forbidden", "This action is unauthorized."),
            404 => ("not_found", "Not Found"),
            405 => ("method_not_allowed", "Method Not Allowed"),
            408 => ("request_timeout", "Request Timeout"),
            409 => ("conflict", "Conflict"),
            413 => ("payload_too_large", "Payload Too Large"),
            415 => ("unsupported_media_type", "Unsupported Media Type"),
            419 => ("csrf_token_mismatch", "CSRF token mismatch."),
            422 => ("validation_failed", "The given data was invalid."),
            429 => ("too_many_requests", "Too Many Attempts."),
            500 => ("internal_error", "Server Error"),
            503 => ("service_unavailable", "Service Unavailable"),
            _ => {
                let title = status.canonical_reason().unwrap_or("Error");
                return Self::new(status, slug(title), title);
            }
        };
        Self::new(status, code, title)
    }

    /// 400 with a detail message.
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::from_status(StatusCode::BAD_REQUEST).with_detail(detail)
    }

    /// 401: the request is not authenticated.
    pub fn unauthenticated() -> Self {
        Self::from_status(StatusCode::UNAUTHORIZED)
    }

    /// 403: the user may not do this.
    pub fn forbidden() -> Self {
        Self::from_status(StatusCode::FORBIDDEN)
    }

    /// 404.
    pub fn not_found() -> Self {
        Self::from_status(StatusCode::NOT_FOUND)
    }

    /// 409 with a detail message.
    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::from_status(StatusCode::CONFLICT).with_detail(detail)
    }

    /// 419: the CSRF token is missing or does not match the session.
    pub fn csrf_mismatch() -> Self {
        Self::from_status(csrf_status())
    }

    /// 422 with field errors (`{"field.path": ["message"]}`).
    pub fn validation(errors: BTreeMap<String, Vec<String>>) -> Self {
        let mut error = Self::from_status(StatusCode::UNPROCESSABLE_ENTITY);
        error.errors = Some(Box::new(errors));
        error
    }

    /// 413: the request body is larger than `limit` bytes.
    pub fn payload_too_large(limit: usize) -> Self {
        Self::from_status(StatusCode::PAYLOAD_TOO_LARGE)
            .with_detail(format!("The request body must not exceed {limit} bytes."))
    }

    /// 429 with a `Retry-After` header (whole seconds, at least 1).
    pub fn too_many_requests(retry_after: Duration) -> Self {
        let secs = retry_after_secs(retry_after);
        Self::from_status(StatusCode::TOO_MANY_REQUESTS)
            .with_header(RETRY_AFTER, HeaderValue::from(secs))
    }

    /// 503 with a detail message.
    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self::from_status(StatusCode::SERVICE_UNAVAILABLE).with_detail(detail)
    }

    /// 500 caused by `err`. The client sees no detail; the error is logged.
    pub fn internal(err: impl Into<BoxError>) -> Self {
        let mut error = Self::from_status(StatusCode::INTERNAL_SERVER_ERROR);
        error.source = Some(err.into());
        error
    }

    /// Sets the detail message.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Records the underlying error (logged for 5xx responses, never sent).
    pub fn with_source(mut self, source: impl Into<BoxError>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// The problem+json body, with `request_id` when known.
    pub fn problem(&self, request_id: Option<&str>) -> serde_json::Value {
        serde_json::to_value(self.body(request_id)).unwrap_or_default()
    }

    fn body<'a>(&'a self, request_id: Option<&'a str>) -> ProblemBody<'a> {
        ProblemBody {
            kind: if self.code.is_empty() {
                "about:blank"
            } else {
                &self.code
            },
            title: &self.title,
            status: self.status.as_u16(),
            detail: self.detail.as_deref(),
            request_id,
            errors: self.errors.as_deref(),
        }
    }

    /// Renders the response. `request_id` goes into the body; the pipeline
    /// sets the `X-Request-Id` header itself.
    pub(crate) fn render(&self, request_id: Option<&str>) -> Response {
        let body = serde_json::to_vec(&self.body(request_id)).unwrap_or_default();
        let mut response = Response::new(Body::from(body));
        *response.status_mut() = self.status;
        let headers = response.headers_mut();
        for (name, value) in self.headers.iter() {
            headers.insert(name, value.clone());
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        response
    }

    /// The error and its source chain, for logs.
    pub(crate) fn log_chain(&self) -> String {
        let mut out = format!("{} {}", self.status.as_u16(), self.title);
        if let Some(detail) = &self.detail {
            out.push_str(": ");
            out.push_str(detail);
        }
        let mut next: Option<&(dyn StdError + 'static)> = self
            .source
            .as_deref()
            .map(|e| e as &(dyn StdError + 'static));
        while let Some(error) = next {
            out.push_str(": ");
            out.push_str(&error.to_string());
            next = error.source();
        }
        out
    }
}

/// The 419 status Laravel uses for CSRF failures.
fn csrf_status() -> StatusCode {
    StatusCode::from_u16(419).unwrap_or(StatusCode::FORBIDDEN)
}

/// `Retry-After` in whole seconds, rounded up, at least 1.
pub(crate) fn retry_after_secs(d: Duration) -> u64 {
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    secs.max(1)
}

/// `"Payment Required"` → `"payment_required"`.
fn slug(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Serialize)]
struct ProblemBody<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    title: &'a str,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
    request_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a BTreeMap<String, Vec<String>>>,
}

impl fmt::Debug for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppError")
            .field("status", &self.status)
            .field("code", &self.code)
            .field("title", &self.title)
            .field("detail", &self.detail)
            .field("errors", &self.errors)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status.as_u16(), self.title)?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

/// Any error converts into a 500 (`?` in handlers), like `anyhow`.
impl<E: Into<BoxError>> From<E> for AppError {
    fn from(error: E) -> Self {
        Self::internal(error)
    }
}

/// The rendered response carries the error itself, so the pipeline can map
/// it, log it and add the request id.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut response = self.render(None);
        response
            .extensions_mut()
            .insert(PendingProblem(Arc::new(Mutex::new(Some(self)))));
        response
    }
}

/// An [`AppError`] travelling in response extensions until the problem layer
/// renders it with the request id.
#[derive(Clone)]
pub(crate) struct PendingProblem(Arc<Mutex<Option<AppError>>>);

impl PendingProblem {
    pub(crate) fn take(&self) -> Option<AppError> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// Maps errors of one type (anywhere in an [`AppError`]'s source chain) to a
/// specific [`AppError`]. Contributed with
/// [`ModuleBuilderHttpExt::map_error`](crate::ModuleBuilderHttpExt::map_error).
#[derive(Clone)]
pub struct ErrorMapper {
    type_name: &'static str,
    map: Arc<MapFn>,
}

type MapFn = dyn Fn(&(dyn StdError + 'static)) -> Option<AppError> + Send + Sync;

impl ErrorMapper {
    /// A mapper for errors of type `E`; returning `None` keeps the 500.
    pub fn new<E: StdError + 'static>(f: fn(&E) -> Option<AppError>) -> Self {
        Self {
            type_name: std::any::type_name::<E>(),
            map: Arc::new(move |error| error.downcast_ref::<E>().and_then(f)),
        }
    }

    /// Applies the first matching mapper to `error`'s source chain.
    pub(crate) fn apply(mappers: &[ErrorMapper], error: &AppError) -> Option<AppError> {
        let mut next: Option<&(dyn StdError + 'static)> = error
            .source
            .as_deref()
            .map(|e| e as &(dyn StdError + 'static));
        while let Some(cause) = next {
            for mapper in mappers {
                if let Some(mapped) = (mapper.map)(cause) {
                    return Some(mapped);
                }
            }
            next = cause.source();
        }
        None
    }
}

impl fmt::Debug for ErrorMapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorMapper")
            .field("error_type", &self.type_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clippy's `result_large_err` fires at 128 bytes; every crate returning
    /// `keel_http::Result<T>` would have to silence it.
    #[test]
    fn app_error_stays_small_enough_for_result() {
        assert!(
            std::mem::size_of::<AppError>() < 128,
            "AppError is {} bytes",
            std::mem::size_of::<AppError>()
        );
    }

    #[test]
    fn statuses_have_codes_and_titles() {
        assert_eq!(AppError::csrf_mismatch().status.as_u16(), 419);
        assert_eq!(AppError::csrf_mismatch().title, "CSRF token mismatch.");
        assert_eq!(AppError::unauthenticated().status, StatusCode::UNAUTHORIZED);
        assert_eq!(AppError::forbidden().code, "forbidden");
        let teapot = AppError::from_status(StatusCode::IM_A_TEAPOT);
        assert_eq!(teapot.code, "i_m_a_teapot");
    }

    #[test]
    fn too_many_requests_rounds_retry_after_up() {
        let error = AppError::too_many_requests(Duration::from_millis(1500));
        assert_eq!(error.headers[RETRY_AFTER], "2");
        let error = AppError::too_many_requests(Duration::ZERO);
        assert_eq!(error.headers[RETRY_AFTER], "1");
    }

    #[test]
    fn internal_keeps_source_but_renders_no_detail() {
        let error = AppError::internal(std::io::Error::other("db password=hunter2"));
        let body = error.problem(Some("abc"));
        assert_eq!(body["status"], 500);
        assert_eq!(body["request_id"], "abc");
        assert!(body.get("detail").is_none());
        assert!(error.log_chain().contains("hunter2"), "logged, not sent");
    }

    #[test]
    fn strings_convert_with_question_mark() {
        fn fails() -> Result<()> {
            Err("boom")?
        }
        let error = fails().unwrap_err();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.source.unwrap().to_string(), "boom");
    }
}
