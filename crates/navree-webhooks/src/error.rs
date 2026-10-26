//! [`WebhooksError`] and how it becomes an HTTP error.

use std::collections::BTreeMap;

use navree_http::AppError;
use navree_kernel::BoxError;
use sea_orm::DbErr;
use uuid::Uuid;

use crate::DeliveryStatus;

/// What can go wrong in the [`Webhooks`](crate::Webhooks) service.
///
/// Returned from a handler with `?`, it becomes a 404 (unknown endpoint or
/// delivery), a 400 with `errors.<field>` (invalid input, an unknown event
/// type, a payload over the limit), a 409 (retrying a delivery that has not
/// failed) or a 500.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WebhooksError {
    /// No endpoint has this id (or it belongs to another workspace).
    #[error("webhook endpoint {0} not found")]
    EndpointNotFound(Uuid),
    /// No delivery has this id (or it belongs to another workspace).
    #[error("webhook delivery {0} not found")]
    DeliveryNotFound(Uuid),
    /// An input field is invalid; the message names the field.
    #[error("{message}")]
    Invalid {
        /// The field (`url`, `events`, `headers`, `description`).
        field: &'static str,
        /// What is wrong, starting with the field name.
        message: String,
    },
    /// The event type was never registered (`WebhooksModule::for_feature`
    /// or `webhook_type`).
    #[error("`{0}` is not a registered webhook event type")]
    UnknownEventType(String),
    /// The payload is larger than `webhooks.max_payload_bytes`.
    #[error("the webhook payload is {size} bytes, more than webhooks.max_payload_bytes ({limit})")]
    PayloadTooLarge {
        /// The payload size in bytes.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// Only a failed delivery can be retried.
    #[error("only a failed delivery can be retried; this one is {0}")]
    NotRetryable(DeliveryStatus),
    /// Outgoing webhooks need a module the app does not include.
    #[error("outgoing webhooks need {0} in the app")]
    Missing(&'static str),
    /// A database error.
    #[error(transparent)]
    Db(#[from] DbErr),
    /// Building the payload of an event failed.
    #[error("cannot build the payload of `{event_type}`: {source}")]
    Payload {
        /// The event type.
        event_type: String,
        /// What went wrong.
        #[source]
        source: BoxError,
    },
    /// Queueing a job failed.
    #[error("cannot queue a webhook job: {0}")]
    Queue(#[source] BoxError),
    /// An endpoint secret could not be encrypted or decrypted (a changed
    /// app key without the old one in `app.previous_keys`).
    #[error("cannot encrypt or decrypt a webhook endpoint secret: {0}")]
    Crypto(#[from] navree_crypto::CryptoError),
}

impl WebhooksError {
    /// A 400 naming `field`.
    pub(crate) fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        WebhooksError::Invalid {
            field,
            message: message.into(),
        }
    }

    /// The HTTP error for this error, or `None` for a 500.
    pub fn to_app_error(&self) -> Option<AppError> {
        let field_error = |field: &str, message: String| {
            AppError::validation(BTreeMap::from([(field.to_owned(), vec![message])]))
        };
        match self {
            WebhooksError::EndpointNotFound(_) | WebhooksError::DeliveryNotFound(_) => {
                Some(AppError::not_found())
            }
            WebhooksError::Invalid { field, message } => Some(field_error(field, message.clone())),
            WebhooksError::UnknownEventType(name) => Some(field_error(
                "events",
                format!("events contains `{name}`, which is not a registered event type"),
            )),
            WebhooksError::PayloadTooLarge { .. } => Some(AppError::bad_request(self.to_string())),
            WebhooksError::NotRetryable(_) => Some(AppError::conflict(self.to_string())),
            _ => None,
        }
    }
}

/// The error mapper registered by `WebhooksModule`.
pub(crate) fn map_webhooks_error(error: &WebhooksError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::*;

    #[test]
    fn errors_map_to_their_statuses() {
        let status = |error: WebhooksError| error.to_app_error().map(|e| e.status);
        assert_eq!(
            status(WebhooksError::EndpointNotFound(Uuid::nil())),
            Some(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            status(WebhooksError::invalid("url", "url must use https")),
            Some(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            status(WebhooksError::UnknownEventType("x".into())),
            Some(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            status(WebhooksError::NotRetryable(DeliveryStatus::Pending)),
            Some(StatusCode::CONFLICT)
        );
        assert_eq!(status(WebhooksError::Missing("QueueModule")), None);
        let body = WebhooksError::invalid("url", "url must use https")
            .to_app_error()
            .unwrap()
            .body();
        assert_eq!(body["errors"]["url"][0], "url must use https");
    }
}
