//! [`MicroError`] and [`RemoteError`]: what `send` and `emit` fail with,
//! and how each failure becomes an `AppError`.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use http::StatusCode;
use navree_http::AppError;
use navree_kernel::BoxError;
use serde_json::Value;

/// Why a [`ServiceClient`](crate::ServiceClient) call failed.
///
/// With `?` in a route or message handler, `MicroModule` maps each error to
/// an `AppError` ([`to_app_error`](MicroError::to_app_error)): a remote 4xx
/// passes through with its status, message and field errors; failures
/// between the services become 5xx errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MicroError {
    /// The handler answered with an error (4xx: refused or invalid; 5xx:
    /// failed).
    #[error("{0}")]
    Remote(RemoteError),
    /// No reply before the deadline.
    #[error("no reply to `{pattern}` within {after:?}")]
    Timeout {
        /// The pattern.
        pattern: String,
        /// How long the caller waited.
        after: Duration,
    },
    /// Nobody listens to the pattern (NATS, the memory transport, TCP
    /// without a route, and transports that know).
    #[error("nobody answers `{pattern}`")]
    NoResponders {
        /// The pattern.
        pattern: String,
    },
    /// The broker is unreachable, or the connection broke.
    #[error("transport failure: {0}")]
    Transport(BoxError),
    /// The reply does not decode into the requested type.
    #[error("the reply to `{pattern}` does not decode: {source}")]
    Decode {
        /// The pattern.
        pattern: String,
        /// The codec's error.
        source: BoxError,
    },
    /// The payload does not encode with the configured codec.
    #[error("the payload of `{pattern}` does not encode: {source}")]
    Encode {
        /// The pattern.
        pattern: String,
        /// The codec's error.
        source: BoxError,
    },
    /// The encoded payload is over `micro.max_message_bytes`.
    #[error("the payload is {size} bytes, over the limit of {limit} bytes")]
    PayloadTooLarge {
        /// The encoded size.
        size: usize,
        /// `micro.max_message_bytes`.
        limit: usize,
    },
    /// The pattern is not a valid pattern (see [`check_pattern`](crate::check_pattern)).
    #[error("invalid pattern `{pattern}`: {reason}")]
    InvalidPattern {
        /// The pattern.
        pattern: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The app is shutting down.
    #[error("the app is shutting down")]
    ShutDown,
}

impl MicroError {
    /// A [`Transport`](MicroError::Transport) error with `message`.
    pub fn transport(message: impl Into<BoxError>) -> Self {
        MicroError::Transport(message.into())
    }

    /// Whether `send(..).retries(n)` retries after this error: timeouts,
    /// transport failures and no responders, never a reply.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            MicroError::Timeout { .. } | MicroError::Transport(_) | MicroError::NoResponders { .. }
        )
    }

    /// The `AppError` this failure becomes in a handler:
    ///
    /// | Error | Becomes |
    /// |---|---|
    /// | `Remote` with a 4xx status | the same status, message and field errors |
    /// | `Remote` with a 5xx status | 502 Bad Gateway |
    /// | `Timeout` | 504 Gateway Timeout |
    /// | `NoResponders`, `Transport`, `ShutDown` | 503 Service Unavailable |
    /// | `Decode` | 502 Bad Gateway |
    /// | `Encode`, `PayloadTooLarge`, `InvalidPattern` | 500 Internal Server Error |
    pub fn to_app_error(&self) -> AppError {
        match self {
            MicroError::Remote(remote) if remote.status.is_client_error() => remote.to_app_error(),
            MicroError::Remote(_) | MicroError::Decode { .. } => {
                AppError::from_status(StatusCode::BAD_GATEWAY)
            }
            MicroError::Timeout { .. } => AppError::from_status(StatusCode::GATEWAY_TIMEOUT),
            MicroError::NoResponders { .. } | MicroError::Transport(_) | MicroError::ShutDown => {
                AppError::from_status(StatusCode::SERVICE_UNAVAILABLE)
            }
            MicroError::Encode { .. }
            | MicroError::PayloadTooLarge { .. }
            | MicroError::InvalidPattern { .. } => {
                AppError::from_status(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

/// An error reply: the status and the JSON error body the handler's service
/// sent (`{ "statusCode", "message", "error" }`, plus `errors` for
/// validation failures).
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteError {
    status: StatusCode,
    body: Value,
}

impl RemoteError {
    /// An error reply with `status` and `body`.
    pub fn new(status: StatusCode, body: Value) -> Self {
        Self { status, body }
    }

    /// The reply's status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The message, or the first of several (validation errors).
    pub fn message(&self) -> &str {
        match &self.body["message"] {
            Value::String(message) => message,
            Value::Array(messages) => messages.first().and_then(Value::as_str).unwrap_or(""),
            _ => self.status.canonical_reason().unwrap_or(""),
        }
    }

    /// The field errors of a 400 (`{"position": ["position must not be less
    /// than 0"]}`); empty for other errors.
    pub fn errors(&self) -> BTreeMap<String, Vec<String>> {
        let Some(errors) = self.body["errors"].as_object() else {
            return BTreeMap::new();
        };
        errors
            .iter()
            .map(|(field, messages)| {
                let messages = match messages {
                    Value::Array(list) => list
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                    Value::String(one) => vec![one.clone()],
                    _ => Vec::new(),
                };
                (field.clone(), messages)
            })
            .collect()
    }

    /// The whole JSON body.
    pub fn body(&self) -> &Value {
        &self.body
    }

    /// The same error as an `AppError`: status, message and field errors.
    pub fn to_app_error(&self) -> AppError {
        let errors = self.errors();
        if !errors.is_empty() {
            let mut error = AppError::validation(errors);
            error.status = self.status;
            return error;
        }
        let error = AppError::from_status(self.status);
        match self.body["message"].as_str() {
            Some(message) if Some(message) != self.status.canonical_reason() => {
                error.with_detail(message)
            }
            _ => error,
        }
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status.as_u16(), self.message())
    }
}

impl std::error::Error for RemoteError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn remote(status: u16, body: Value) -> MicroError {
        MicroError::Remote(RemoteError::new(
            StatusCode::from_u16(status).unwrap(),
            body,
        ))
    }

    #[test]
    fn the_error_table() {
        let not_found = json!({ "statusCode": 404, "message": "Not Found", "error": "Not Found" });
        let cases = [
            (remote(404, not_found.clone()), 404),
            (
                remote(403, json!({ "statusCode": 403, "message": "Forbidden" })),
                403,
            ),
            (remote(500, json!({ "statusCode": 500 })), 502),
            (
                MicroError::Timeout {
                    pattern: "deals.get".into(),
                    after: Duration::from_secs(5),
                },
                504,
            ),
            (
                MicroError::NoResponders {
                    pattern: "deals.get".into(),
                },
                503,
            ),
            (MicroError::transport("down"), 503),
            (
                MicroError::Decode {
                    pattern: "deals.get".into(),
                    source: "bad".into(),
                },
                502,
            ),
            (MicroError::PayloadTooLarge { size: 2, limit: 1 }, 500),
            (MicroError::ShutDown, 503),
        ];
        for (error, status) in cases {
            assert_eq!(error.to_app_error().status.as_u16(), status, "{error}");
        }
        assert_eq!(
            remote(404, not_found.clone()).to_app_error().body(),
            not_found
        );
    }

    #[test]
    fn a_remote_4xx_keeps_its_message_and_field_errors() {
        let body = json!({
            "statusCode": 400,
            "message": ["position must not be less than 0"],
            "error": "Bad Request",
            "errors": { "position": ["position must not be less than 0"] },
        });
        let error = RemoteError::new(StatusCode::BAD_REQUEST, body.clone());
        assert_eq!(error.message(), "position must not be less than 0");
        assert_eq!(
            error.errors(),
            BTreeMap::from([(
                "position".to_owned(),
                vec!["position must not be less than 0".to_owned()]
            )])
        );
        assert_eq!(error.to_app_error().body(), body);

        let conflict = RemoteError::new(
            StatusCode::CONFLICT,
            json!({ "statusCode": 409, "message": "Email taken", "error": "Conflict" }),
        );
        assert_eq!(conflict.message(), "Email taken");
        assert!(conflict.errors().is_empty());
        assert_eq!(conflict.to_app_error().body()["message"], "Email taken");
        assert_eq!(conflict.to_string(), "409 Email taken");
    }

    #[test]
    fn only_failures_without_an_answer_are_retryable() {
        assert!(MicroError::transport("x").is_retryable());
        assert!(
            MicroError::NoResponders {
                pattern: "a".into()
            }
            .is_retryable()
        );
        assert!(
            MicroError::Timeout {
                pattern: "a".into(),
                after: Duration::ZERO
            }
            .is_retryable()
        );
        assert!(!remote(503, json!({})).is_retryable());
        assert!(!MicroError::ShutDown.is_retryable());
    }
}
