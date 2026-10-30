//! [`VerifiedWebhook<S>`]: the extractor that verifies an incoming webhook
//! before the handler runs.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::response::{IntoResponse as _, Response};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::HeaderMap;
use http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use navree_contracts::{Clock, SystemClock};
use navree_db::Uow;
use navree_http::AppError;
use navree_http::extract::{FromRequest, Request};
use navree_kernel::AppContext;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entity::receipt;
use crate::request::{Handshake, IncomingRequest, Verdict, public_url};
use crate::verify::{Tolerance, VerifyError, WebhookSource, WebhookVerifier};
use crate::{SourceOptions, WebhooksError, WebhooksOptions};

/// The message every failed verification answers with (400).
pub const INVALID_SIGNATURE: &str = "Invalid webhook signature";

/// The `code` every failed verification answers with.
pub const INVALID_SIGNATURE_CODE: &str = "invalid_webhook_signature";

/// An incoming webhook from source `S` whose signature (and, when the
/// scheme signs one, timestamp) has been checked. Take it instead of
/// `Json<T>`, as the last argument of a handler on a `#[public]` route:
///
/// ```ignore
/// #[post("/")]
/// #[public]
/// async fn receive(&self, uow: Uow, hook: VerifiedWebhook<GitHub>) -> Result<NoContent> {
///     if !hook.mark_processed(&uow).await? {
///         return Ok(NoContent); // a redelivery we already handled
///     }
///     let event: IssueEvent = hook.json()?;
///     // ...
///     Ok(NoContent)
/// }
/// ```
///
/// It reads the raw body (up to `webhooks.incoming.max_body_bytes`, a 413
/// beyond), verifies it with `S::Verifier` and the options under
/// `webhooks.incoming.<S::NAME>` (the secret, or the settings of a source
/// that verifies with the sender's keys), and checks the signed timestamp
/// against the tolerance (`webhooks.incoming.<S::NAME>.tolerance_secs`, else
/// `webhooks.incoming.tolerance_secs`) and the app's `dyn Clock`. A request
/// that fails is answered with 400 `Invalid webhook signature` and never
/// reaches the handler; a handshake ([`Verdict::Handshake`]) is answered
/// as the sender expects, without the handler. A source missing its secret
/// (or required setting) is a 500, with an error log naming the key; a key
/// that cannot be fetched from the sender is a 503, so the sender retries.
/// The app must include `WebhooksModule`.
pub struct VerifiedWebhook<S> {
    body: Bytes,
    headers: HeaderMap,
    id: String,
    event_type: Option<String>,
    timestamp: Option<DateTime<Utc>>,
    received_at: DateTime<Utc>,
    source: PhantomData<fn() -> S>,
}

impl<S: WebhookSource> VerifiedWebhook<S> {
    /// The body as `T`; a body that does not match is a 400.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, AppError> {
        serde_json::from_slice(&self.body)
            .map_err(|error| AppError::bad_request(format!("Invalid webhook payload: {error}")))
    }

    /// The form-encoded body (Twilio, Slack commands, Mailgun routes) as
    /// `T`; a body that does not match is a 400.
    pub fn form<T: DeserializeOwned>(&self) -> Result<T, AppError> {
        serde_urlencoded::from_bytes(&self.body)
            .map_err(|error| AppError::bad_request(format!("Invalid webhook payload: {error}")))
    }

    /// The raw bytes that were verified.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The request headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// The sender's delivery id (`webhook-id`, `X-GitHub-Delivery`, the
    /// Stripe event id, Paddle's `event_id`), or the hex SHA-256 of the
    /// body when the sender has none.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The sender's event type, when it sends one (GitHub's
    /// `X-GitHub-Event`, the `type` of a Stripe or Standard Webhooks
    /// payload, Paddle's `event_type`).
    pub fn event_type(&self) -> Option<&str> {
        self.event_type.as_deref()
    }

    /// The signed timestamp (`None` for schemes that sign none, such as
    /// GitHub's).
    pub fn timestamp(&self) -> Option<DateTime<Utc>> {
        self.timestamp
    }

    /// The source name ([`S::NAME`](WebhookSource::NAME)).
    pub fn source(&self) -> &'static str {
        S::NAME
    }

    /// Records the message id in `webhook_receipts`, in `uow`'s transaction:
    /// `true` the first time, `false` when it was already there (a
    /// redelivery). A message whose handling rolled back is handled again
    /// on the next delivery; one that committed is not.
    pub async fn mark_processed(&self, uow: &Uow) -> Result<bool, WebhooksError> {
        use navree_orm::Model as _;

        let receipt = receipt::Model {
            id: Uuid::now_v7(),
            source: S::NAME.to_owned(),
            message_id: self.id.clone(),
            received_at: self.received_at,
        };
        // Inserted without a change event (a receipt is not a record anyone
        // audits); a conflict on (source, message_id) inserts nothing.
        Ok(receipt::Model::insert_or_ignore(uow, [receipt]).await? > 0)
    }
}

impl<S> fmt::Debug for VerifiedWebhook<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedWebhook")
            .field("id", &self.id)
            .field("event_type", &self.event_type)
            .field("timestamp", &self.timestamp)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

impl<S: WebhookSource> FromRequest for VerifiedWebhook<S> {
    type Rejection = Response;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let ctx = request
            .extensions()
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| {
                AppError::internal("VerifiedWebhook needs the navree request pipeline")
                    .into_response()
            })?;
        let options = ctx.get::<WebhooksOptions>().await.map_err(|_| {
            AppError::internal("VerifiedWebhook needs WebhooksModule in the app").into_response()
        })?;
        let incoming = &options.incoming;
        let unconfigured = SourceOptions::default();
        let source = incoming.source(S::NAME).unwrap_or(&unconfigured);
        if let Some(setting) = S::Verifier::missing_setting(source) {
            let key = format!("webhooks.incoming.{}.{setting}", S::NAME);
            tracing::error!(
                target: "navree::webhooks",
                source = S::NAME,
                "incoming webhook refused: `{key}` is not set ({})",
                format_args!(
                    "APP_WEBHOOKS__INCOMING__{}__{}",
                    S::NAME.to_ascii_uppercase(),
                    setting.to_ascii_uppercase()
                )
            );
            return Err(AppError::internal(format!("`{key}` is not set")).into_response());
        }
        let clock: Arc<dyn Clock> = match ctx.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        let now = clock.now();
        let tolerance = Tolerance::at(
            now,
            source.tolerance_secs.unwrap_or(incoming.tolerance_secs),
        );
        let (parts, body) = request.into_parts();
        let body = to_bytes(body, incoming.max_body_bytes)
            .await
            .map_err(|_| AppError::payload_too_large(incoming.max_body_bytes).into_response())?;
        let url = public_url(source.url.as_deref(), &parts.headers, &parts.uri);
        let request = IncomingRequest::new(source, &parts.headers, &body)
            .with_url(url)
            .at(now)
            .with_tolerance(tolerance)
            .with_app(ctx.clone());
        let verified = match S::Verifier::verify_incoming(&request).await {
            Ok(Verdict::Verified(verified)) => verified,
            Ok(Verdict::Handshake(handshake)) => return Err(answer(handshake)),
            Err(error) => return Err(refuse(S::NAME, &error)),
        };
        drop(request);
        let id = verified
            .id
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| hex::encode(Sha256::digest(&body)));
        Ok(VerifiedWebhook {
            body,
            headers: parts.headers,
            id,
            event_type: verified.event_type,
            timestamp: verified.timestamp,
            received_at: now,
            source: PhantomData,
        })
    }
}

/// The response to a handshake: its status and `text/plain` body (which
/// may echo the request, so it is never sniffed as anything else).
fn answer(handshake: Handshake) -> Response {
    if handshake.body.is_empty() {
        return handshake.status.into_response();
    }
    (
        handshake.status,
        [
            (CONTENT_TYPE, "text/plain; charset=utf-8"),
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        handshake.body,
    )
        .into_response()
}

/// The response to a request that failed: 400 `Invalid webhook signature`
/// for a request that is not authentic, 503 when a key could not be
/// fetched (the sender retries), 500 when the source is misconfigured.
fn refuse(source: &'static str, error: &VerifyError) -> Response {
    match error {
        VerifyError::Unavailable(_) => {
            tracing::warn!(
                target: "navree::webhooks",
                source,
                %error,
                "incoming webhook not verified"
            );
            AppError::service_unavailable("The webhook cannot be verified right now")
                .into_response()
        }
        VerifyError::Misconfigured(_) | VerifyError::MissingSecret => {
            tracing::error!(
                target: "navree::webhooks",
                source,
                %error,
                "incoming webhook refused: the source is misconfigured"
            );
            AppError::internal(error.to_string()).into_response()
        }
        _ => {
            tracing::debug!(
                target: "navree::webhooks",
                source,
                %error,
                "incoming webhook failed verification"
            );
            AppError::bad_request(INVALID_SIGNATURE)
                .with_code(INVALID_SIGNATURE_CODE)
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;
    use crate::Stripe;

    #[test]
    fn handlers_on_a_verified_webhook_are_native() {
        async fn handler(_: VerifiedWebhook<Stripe>) {}
        assert!(Endpoint::new(handler).is_native());
    }
}
