//! [`NotifyCallbacksController`]: the provider callbacks of navree-notify's
//! SMS and WhatsApp channels, verified as incoming webhooks (crate feature
//! `notify`).

use std::sync::Arc;

use http::{Method, StatusCode, header};
use navree_http::{ApiMeta, AppError, Controller, IntoResponse, Response, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_notify::{ChannelCallback, DeliveryLog, NotifyError};

use crate::{Meta, Twilio, VerifiedWebhook, Vonage, WebhookSource, WebhooksOptions};

/// Where the callback routes are mounted, under the API prefix.
pub const NOTIFY_CALLBACKS_PREFIX: &str = "/webhooks/notify";

/// Mounts, under `{api_prefix}/webhooks/notify`, one public route per
/// configured source:
///
/// - `POST /twilio` ([`Twilio`], `webhooks.incoming.twilio`): message
///   status callbacks and incoming SMS and WhatsApp messages; answered
///   with an empty TwiML `<Response/>`;
/// - `POST /vonage` ([`Vonage`], `webhooks.incoming.vonage`): Messages API
///   status and inbound webhooks;
/// - `GET` and `POST /meta` ([`Meta`], `webhooks.incoming.meta`): the
///   WhatsApp Cloud API's subscription handshake, statuses and messages.
///
/// Each request is verified like any [`VerifiedWebhook`] of its source,
/// read with navree-notify's `ChannelCallback::from_*` and applied with
/// `DeliveryLog::ingest`: statuses move deliveries forward, STOP and START
/// update the opt-outs. Nothing is mounted without `NotifyModule` in the
/// app. Point the providers at these URLs (`notify.sms.status_callback`,
/// the number's messaging webhook, the Vonage application, the Meta app).
pub struct NotifyCallbacksController {
    options: Arc<WebhooksOptions>,
    log: Option<Arc<DeliveryLog>>,
}

impl Injectable for NotifyCallbacksController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<WebhooksOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(NotifyCallbacksController {
                options: r.get::<WebhooksOptions>().await?,
                log: r.app().try_get::<DeliveryLog>().await,
            })
        })
    }
}

impl NotifyCallbacksController {
    /// Whether source `S` has a secret.
    fn configured<S: WebhookSource>(&self) -> bool {
        self.options
            .incoming
            .source(S::NAME)
            .is_some_and(|source| !source.secret.expose().trim().is_empty())
    }
}

impl Controller for NotifyCallbacksController {
    fn prefix(&self) -> &'static str {
        NOTIFY_CALLBACKS_PREFIX
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let Some(log) = self.log.clone() else {
            return Vec::new();
        };
        let mut routes = Vec::new();
        if self.configured::<Twilio>() {
            let log = log.clone();
            let twilio = move |hook: VerifiedWebhook<Twilio>| {
                let log = log.clone();
                async move {
                    ingest(&log, ChannelCallback::from_twilio(hook.body())).await?;
                    Ok::<_, AppError>(empty_twiml())
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/twilio", twilio)
                    .public()
                    .name("webhooks.notify.twilio")
                    .api(api("Receive Twilio message statuses and replies")),
            );
        }
        if self.configured::<Vonage>() {
            let log = log.clone();
            let vonage = move |hook: VerifiedWebhook<Vonage>| {
                let log = log.clone();
                async move {
                    ingest(&log, ChannelCallback::from_vonage(hook.body())).await?;
                    Ok::<_, AppError>(StatusCode::OK)
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/vonage", vonage)
                    .public()
                    .name("webhooks.notify.vonage")
                    .api(api("Receive Vonage message statuses and replies")),
            );
        }
        if self.configured::<Meta>() {
            for method in [Method::GET, Method::POST] {
                let log = log.clone();
                let meta = move |hook: VerifiedWebhook<Meta>| {
                    let log = log.clone();
                    async move {
                        ingest(&log, ChannelCallback::from_meta(hook.body())).await?;
                        Ok::<_, AppError>(StatusCode::OK)
                    }
                };
                let name = if method == Method::GET {
                    "webhooks.notify.meta.verify"
                } else {
                    "webhooks.notify.meta"
                };
                routes.push(
                    RouteDef::on(method, "/meta", meta)
                        .public()
                        .name(name)
                        .api(api("Receive WhatsApp Cloud API statuses and messages")),
                );
            }
        }
        routes
    }
}

/// Applies `callbacks`; a failure is a 500, so the provider sends again.
async fn ingest(log: &DeliveryLog, callbacks: Vec<ChannelCallback>) -> Result<(), AppError> {
    log.ingest(callbacks).await.map(|_| ()).map_err(|error: NotifyError| {
        tracing::warn!(target: "navree::webhooks", %error, "applying a notification callback failed");
        AppError::internal(error)
    })
}

/// `200` with an empty TwiML document: no reply to the sender.
fn empty_twiml() -> Response {
    (
        [(header::CONTENT_TYPE, "text/xml")],
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response></Response>",
    )
        .into_response()
}

/// The OpenAPI metadata of the callback routes.
fn api(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Webhooks".to_owned()],
        responses: vec![400],
        ..ApiMeta::default()
    }
}
