//! [`VerifiedWebhook<S>`]: the extractor that verifies an incoming webhook
//! before the handler runs.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{FromRequest, Request};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::HeaderMap;
use navree_contracts::{Clock, SystemClock};
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::AppContext;
use sea_orm::ConnectionTrait;
use sea_orm::sea_query::{OnConflict, Query};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entity::receipt;
use crate::verify::{Tolerance, WebhookSource, WebhookVerifier};
use crate::{WebhooksError, WebhooksOptions};

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
/// beyond), verifies it with `S::Verifier` and the secret under
/// `webhooks.incoming.<S::NAME>.secret`, and checks the signed timestamp
/// against the tolerance (`webhooks.incoming.<S::NAME>.tolerance_secs`, else
/// `webhooks.incoming.tolerance_secs`) and the app's `dyn Clock`. A request
/// that fails is answered with 400 `Invalid webhook signature` and never
/// reaches the handler. A source with no secret configured is a 500 (and an
/// error log naming the key). The app must include `WebhooksModule`.
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
        let tx = uow.tx().await?;
        let mut insert = Query::insert();
        insert
            .into_table(receipt::Entity)
            .columns([
                receipt::Column::Id,
                receipt::Column::Source,
                receipt::Column::MessageId,
                receipt::Column::ReceivedAt,
            ])
            .values_panic([
                Uuid::now_v7().into(),
                S::NAME.into(),
                self.id.clone().into(),
                self.received_at.into(),
            ])
            .on_conflict(
                OnConflict::columns([receipt::Column::Source, receipt::Column::MessageId])
                    .do_nothing()
                    .to_owned(),
            );
        let result = tx.execute(&insert).await?;
        Ok(result.rows_affected() > 0)
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

impl<S: WebhookSource, St: Send + Sync> FromRequest<St> for VerifiedWebhook<S> {
    type Rejection = AppError;

    async fn from_request(request: Request, _state: &St) -> Result<Self, Self::Rejection> {
        let ctx = request
            .extensions()
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| {
                AppError::internal("VerifiedWebhook needs the navree request pipeline")
            })?;
        let options = ctx
            .get::<WebhooksOptions>()
            .await
            .map_err(|_| AppError::internal("VerifiedWebhook needs WebhooksModule in the app"))?;
        let incoming = &options.incoming;
        let Some(source) = incoming
            .source(S::NAME)
            .filter(|source| !source.secret.is_empty())
        else {
            let key = format!("webhooks.incoming.{}.secret", S::NAME);
            tracing::error!(
                target: "navree::webhooks",
                source = S::NAME,
                "incoming webhook refused: `{key}` is not set ({})",
                format_args!("APP_WEBHOOKS__INCOMING__{}__SECRET", S::NAME.to_ascii_uppercase())
            );
            return Err(AppError::internal(format!("`{key}` is not set")));
        };
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
            .map_err(|_| AppError::payload_too_large(incoming.max_body_bytes))?;
        let verified = S::Verifier::verify_request(source, &parts.headers, &body, Some(tolerance))
            .map_err(|error| {
                tracing::debug!(
                    target: "navree::webhooks",
                    source = S::NAME,
                    %error,
                    "incoming webhook failed verification"
                );
                AppError::bad_request(INVALID_SIGNATURE).with_code(INVALID_SIGNATURE_CODE)
            })?;
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
