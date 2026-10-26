//! [`AppError`]: every HTTP error, rendered in Navree's JSON error shape.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::body::Body;
use axum::response::{IntoResponse, Response};
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER, X_CONTENT_TYPE_OPTIONS};
use http::{HeaderMap, StatusCode};
use navree_kernel::BoxError;
use serde::Serialize;

/// The content type of every error response.
pub const ERROR_CONTENT_TYPE: &str = "application/json";

/// The default message of a 500: internals never reach the client.
const INTERNAL_MESSAGE: &str = "Internal server error";

/// The result type of handlers: `Result<T>` fails with an [`AppError`].
pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// An HTTP error. Handlers, extractors, guards and layers return it; it
/// renders in Navree's JSON error shape, as `application/json`:
///
/// ```json
/// {"statusCode": 404, "message": "Deal not found", "error": "Not Found"}
/// ```
///
/// - `statusCode` is the HTTP status.
/// - `message` is the [`detail`](AppError::detail) when there is one, else
///   the [`title`](AppError::title) (the reason phrase by default, and
///   `"Internal server error"` for 500s). For validation errors it is every
///   field message, flattened in field order.
/// - `error` is the reason phrase of the status (`"Not Found"`).
/// - `errors` (validation errors only) maps fields to their messages:
///
/// ```json
/// {"statusCode": 400, "message": ["email must be a valid email address"],
///  "error": "Bad Request", "errors": {"email": ["email must be a valid email address"]}}
/// ```
///
/// The request id is in the `X-Request-Id` response header, not the body.
/// Any error type converts into a 500 with `?` (its message is logged,
/// never sent); modules turn their own error types into specific statuses
/// with [`ModuleBuilderHttpExt::map_error`](crate::ModuleBuilderHttpExt::map_error).
///
/// `AppError` deliberately does not implement `std::error::Error`, which is
/// what makes the blanket `From<E: Into<BoxError>>` conversion possible.
pub struct AppError {
    /// The HTTP status.
    pub status: StatusCode,
    /// A short machine-readable code (`"not_found"`, `"validation_failed"`)
    /// for logs, tests and error mappers; not rendered.
    pub code: String,
    /// The default message, rendered as `message` when there is no
    /// [`detail`](AppError::detail): the reason phrase for statuses built
    /// with [`from_status`](AppError::from_status).
    pub title: String,
    /// A human-readable explanation of this occurrence, rendered as
    /// `message`.
    pub detail: Option<String>,
    /// Field errors (`{"field.path": ["message"]}`), for 400 validation
    /// responses. Boxed, like `headers`, to keep `Result<T, AppError>` small.
    pub errors: Option<Box<BTreeMap<String, Vec<String>>>>,
    /// Extra response headers (`Retry-After`, `WWW-Authenticate`, ...).
    pub headers: Box<HeaderMap>,
    /// The underlying error, logged for 5xx responses and never sent.
    pub source: Option<BoxError>,
}

impl AppError {
    /// An error with an explicit status, code and title (the default
    /// message).
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

    /// The default error for `status`: its reason phrase as the title, and
    /// a code derived from it.
    pub fn from_status(status: StatusCode) -> Self {
        let reason = status.canonical_reason().unwrap_or("Error");
        match status.as_u16() {
            401 => Self::new(status, "unauthenticated", reason),
            500 => Self::new(status, "internal_error", INTERNAL_MESSAGE),
            _ => Self::new(status, slug(reason), reason),
        }
    }

    /// 400 with a message.
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

    /// 409 with a message.
    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::from_status(StatusCode::CONFLICT).with_detail(detail)
    }

    /// 403 `"Invalid CSRF token"`: the CSRF token is missing or does not
    /// match the session.
    pub fn csrf_mismatch() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "csrf_token_mismatch",
            "Invalid CSRF token",
        )
    }

    /// 400 with field errors (`{"field.path": ["message"]}`): `message` is
    /// every field message, flattened in field order, and `errors` the map.
    pub fn validation(errors: BTreeMap<String, Vec<String>>) -> Self {
        let mut error = Self::new(StatusCode::BAD_REQUEST, "validation_failed", "Bad Request");
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

    /// 503 with a message.
    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self::from_status(StatusCode::SERVICE_UNAVAILABLE).with_detail(detail)
    }

    /// 500 caused by `err`. The client sees `"Internal server error"`; the
    /// error is logged.
    pub fn internal(err: impl Into<BoxError>) -> Self {
        let mut error = Self::from_status(StatusCode::INTERNAL_SERVER_ERROR);
        error.source = Some(err.into());
        error
    }

    /// Sets the message.
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

    /// The response body:
    /// `{"statusCode", "message", "error"}`, plus `"errors"` for validation
    /// errors.
    pub fn body(&self) -> serde_json::Value {
        serde_json::to_value(self.wire()).unwrap_or_default()
    }

    fn wire(&self) -> ErrorBody<'_> {
        let errors = self.errors.as_deref();
        let message = match errors {
            Some(errors) if !errors.is_empty() => {
                Message::Many(errors.values().flatten().map(String::as_str).collect())
            }
            _ => Message::One(self.detail.as_deref().unwrap_or(&self.title)),
        };
        ErrorBody {
            status_code: self.status.as_u16(),
            message,
            error: self.status.canonical_reason().unwrap_or(&self.title),
            errors,
        }
    }

    /// Renders the response (the pipeline sets `X-Request-Id` itself).
    pub(crate) fn render(&self) -> Response {
        let body = serde_json::to_vec(&self.wire()).unwrap_or_default();
        let mut response = Response::new(Body::from(body));
        *response.status_mut() = self.status;
        let headers = response.headers_mut();
        for (name, value) in self.headers.iter() {
            headers.insert(name, value.clone());
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(ERROR_CONTENT_TYPE));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        response
    }

    /// An Axum extractor rejection (or a plain-text error response) with
    /// `status` and `text`: a client error shows `text` as its message, a
    /// server error logs it. Axum's 422 (well-formed JSON of the wrong
    /// shape) is a 400, like every other input error.
    pub(crate) fn rejection(status: StatusCode, text: impl Into<String>) -> Self {
        let status = if status == StatusCode::UNPROCESSABLE_ENTITY {
            StatusCode::BAD_REQUEST
        } else {
            status
        };
        let error = Self::from_status(status);
        let text = text.into();
        let text = text.trim();
        if text.is_empty() {
            error
        } else if status.is_client_error() {
            error.with_detail(text)
        } else {
            error.with_source(text.to_owned())
        }
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

/// The wire format, members in their documented order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody<'a> {
    status_code: u16,
    message: Message<'a>,
    error: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a BTreeMap<String, Vec<String>>>,
}

/// `message`: one string, or every validation message.
#[derive(Serialize)]
#[serde(untagged)]
enum Message<'a> {
    One(&'a str),
    Many(Vec<&'a str>),
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
/// and log it.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut response = self.render();
        response
            .extensions_mut()
            .insert(PendingError(Arc::new(Mutex::new(Some(self)))));
        response
    }
}

/// An [`AppError`] travelling in response extensions until the error layer
/// maps, logs and renders it.
#[derive(Clone)]
pub(crate) struct PendingError(Arc<Mutex<Option<AppError>>>);

impl PendingError {
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
    /// `navree_http::Result<T>` would have to silence it.
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
        assert_eq!(AppError::unauthenticated().status, StatusCode::UNAUTHORIZED);
        assert_eq!(AppError::unauthenticated().title, "Unauthorized");
        assert_eq!(AppError::forbidden().code, "forbidden");
        assert_eq!(AppError::forbidden().title, "Forbidden");
        assert_eq!(
            AppError::from_status(StatusCode::TOO_MANY_REQUESTS).title,
            "Too Many Requests"
        );
        let teapot = AppError::from_status(StatusCode::IM_A_TEAPOT);
        assert_eq!(teapot.code, "i_m_a_teapot");
        assert_eq!(teapot.title, "I'm a teapot");
    }

    #[test]
    fn csrf_mismatch_is_403_invalid_csrf_token() {
        let error = AppError::csrf_mismatch();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert_eq!(error.code, "csrf_token_mismatch");
        assert_eq!(
            error.body(),
            serde_json::json!({
                "statusCode": 403,
                "message": "Invalid CSRF token",
                "error": "Forbidden",
            })
        );
    }

    #[test]
    fn body_uses_detail_as_message_and_reason_phrase_as_error() {
        assert_eq!(
            AppError::not_found().body(),
            serde_json::json!({"statusCode": 404, "message": "Not Found", "error": "Not Found"})
        );
        assert_eq!(
            AppError::conflict("Email taken").body(),
            serde_json::json!({"statusCode": 409, "message": "Email taken", "error": "Conflict"})
        );
        let custom = AppError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "Invalid signature.",
        );
        assert_eq!(
            custom.body(),
            serde_json::json!({
                "statusCode": 401,
                "message": "Invalid signature.",
                "error": "Unauthorized",
            })
        );
        let odd = AppError::from_status(StatusCode::from_u16(599).unwrap());
        assert_eq!(odd.body()["error"], "Error", "no reason phrase");
    }

    #[test]
    fn validation_is_400_with_flattened_messages_in_field_order() {
        let errors = BTreeMap::from([
            (
                "name".to_owned(),
                vec!["name should not be empty".to_owned()],
            ),
            (
                "email".to_owned(),
                vec![
                    "email must be a valid email address".to_owned(),
                    "email must be shorter than or equal to 255 characters".to_owned(),
                ],
            ),
        ]);
        let error = AppError::validation(errors.clone()).with_detail("ignored when rendering");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.code, "validation_failed");
        assert_eq!(
            error.body(),
            serde_json::json!({
                "statusCode": 400,
                "message": [
                    "email must be a valid email address",
                    "email must be shorter than or equal to 255 characters",
                    "name should not be empty",
                ],
                "error": "Bad Request",
                "errors": errors,
            })
        );
        let empty = AppError::validation(BTreeMap::new());
        assert_eq!(empty.body()["message"], "Bad Request");
        assert_eq!(empty.body()["errors"], serde_json::json!({}));
    }

    #[test]
    fn rendered_errors_are_application_json() {
        let response = AppError::not_found().render();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[CONTENT_TYPE], ERROR_CONTENT_TYPE);
        assert_eq!(ERROR_CONTENT_TYPE, "application/json");
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
        assert_eq!(
            error.body(),
            serde_json::json!({
                "statusCode": 500,
                "message": "Internal server error",
                "error": "Internal Server Error",
            })
        );
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
