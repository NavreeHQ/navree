//! The mail routes: provider webhooks, tracking, and the admin routes of
//! the delivery log, suppressions, the outbox and transport checks.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use http::{HeaderMap, Method, StatusCode, Uri, header};
use navree_contracts::{Clock, SystemClock};
use navree_db::{ErrorKind, PageQuery};
use navree_http::body::Bytes;
use navree_http::response::{IntoResponse, Response};
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, Path, Query,
    RouteDef, UrlSigner, schema_fn,
};
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::log::{Deliveries, Delivery, DeliveryEvent, DeliveryFilter};
use super::outbox::{HeldMail, Outbox, OutboxStatus};
use super::resolver::TransportResolver;
use super::suppressions::{Suppression, SuppressionReason, Suppressions};
use super::tracking::{CLICK_PATH, OPEN_PATH};
use crate::{
    DeliveryStatus, MailError, MailEventKind, MailOptions, ProviderEvent, SendReport,
    TransportConfig, TransportTier,
};

/// The permission every admin mail route requires.
pub const MANAGE_PERMISSION: &str = "mail.manage";

/// The `code` of a refused provider webhook signature.
pub const INVALID_SIGNATURE_CODE: &str = "invalid_signature";

/// The `code` of a transport that failed its `usable` check.
pub const UNUSABLE_TRANSPORT_CODE: &str = "mail_transport_unusable";

/// How old a signed webhook may be (Mailgun, Resend).
const SIGNATURE_TOLERANCE_SECS: i64 = 15 * 60;

/// A transparent 1x1 GIF.
const PIXEL: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

/// The body of `POST /mail/suppressions`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct AddSuppressionInput {
    /// The address.
    pub email: String,
    /// The workspace it applies to (every workspace when absent).
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    /// Why (default `manual`).
    #[serde(default)]
    pub reason: Option<SuppressionReason>,
    /// A note.
    #[serde(default)]
    pub detail: Option<String>,
}

/// What `GET /mail/deliveries/{id}` returns.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DeliveryWithEvents {
    /// The delivery.
    #[serde(flatten)]
    pub delivery: Delivery,
    /// What happened to it, oldest first.
    pub events: Vec<DeliveryEvent>,
}

/// What `POST /mail/outbox/{id}/release` returns.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ReleasedMail {
    /// `sent`, or `suppressed` when every recipient is suppressed by now.
    pub status: DeliveryStatus,
    /// The delivery.
    pub delivery_id: Option<Uuid>,
    /// The transport that sent it.
    pub transport: Option<String>,
    /// Where the transport came from.
    pub tier: Option<TransportTier>,
}

impl From<SendReport> for ReleasedMail {
    fn from(report: SendReport) -> Self {
        Self {
            status: report.status,
            delivery_id: report.delivery_id,
            transport: report.transport.map(str::to_owned),
            tier: report.tier,
        }
    }
}

/// What `POST /mail/transports/usable` returns when the transport works.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct UsableTransport {
    /// Always `true` (a transport that does not work is a 422).
    pub usable: bool,
    /// The transport type.
    pub kind: String,
}

/// The query of the list routes.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ListQuery {
    workspace_id: Option<Uuid>,
    status: Option<String>,
    recipient: Option<String>,
    email: Option<String>,
    template: Option<String>,
}

/// The mail routes, mounted under `{http.api_prefix}/mail`:
///
/// - **Provider webhooks** (public, verified by each provider's
///   signature), mounted when `mail.delivery.enabled` and the provider's
///   secret is set: `POST /mail/webhooks/ses`, `/mailgun`, `/postmark`,
///   `/resend`. Each records the reported events
///   ([`Deliveries::record`]) and answers 204; a bad signature is 401
///   `invalid_signature`.
/// - **Tracking** (public, signed links), mounted when opens or clicks are
///   tracked: `GET /mail/t/o/{delivery}` (the open image) and
///   `GET /mail/t/c/{delivery}?u=..` (a 302 to the link), at the root
///   (no API prefix).
/// - **Admin** (permission `mail.manage`), mounted when
///   `mail.delivery.enabled` and `mail.delivery.routes`:
///
/// | Route | Answer |
/// |---|---|
/// | `GET /mail/deliveries?workspace_id&status&recipient&template&page&per_page` | a page of deliveries, newest first |
/// | `GET /mail/deliveries/{id}` | the delivery with its `events` |
/// | `GET /mail/suppressions?workspace_id&email` | a page of suppressions |
/// | `POST /mail/suppressions` `{email, workspace_id?, reason?, detail?}` | 201, the suppression |
/// | `DELETE /mail/suppressions/{id}` | 204 |
/// | `GET /mail/outbox?status&workspace_id` | a page of held mail (`status` defaults to `held`) |
/// | `POST /mail/outbox/{id}/release` | the send report (409 unless held) |
/// | `POST /mail/outbox/{id}/discard` | the held mail (409 unless held) |
/// | `POST /mail/transports/usable` (a transport's settings) | `{usable: true, kind}`, or 422 `mail_transport_unusable` |
pub struct MailController {
    deliveries: Arc<Deliveries>,
    suppressions: Arc<Suppressions>,
    outbox: Arc<Outbox>,
    resolver: Arc<TransportResolver>,
    options: Arc<MailOptions>,
    admin: bool,
    events: bool,
    signer: Option<Arc<UrlSigner>>,
    clock: Arc<dyn Clock>,
    client: Option<HttpClient>,
}

impl Injectable for MailController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Deliveries>(),
            ProviderKey::of::<Suppressions>(),
            ProviderKey::of::<Outbox>(),
            ProviderKey::of::<TransportResolver>(),
            ProviderKey::of::<MailOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let options = r.get::<MailOptions>().await?;
            let has_database = app.try_get::<navree_db::Db>().await.is_some();
            let delivery = options.delivery.enabled && has_database;
            let signer = if delivery && options.tracking.is_on() {
                app.try_get::<UrlSigner>().await
            } else {
                None
            };
            let client = match app.try_get::<HttpClient>().await {
                Some(client) => Some((*client).clone()),
                None => HttpClient::new(HttpClientOptions::default()).ok(),
            };
            Ok(MailController {
                deliveries: r.get::<Deliveries>().await?,
                suppressions: r.get::<Suppressions>().await?,
                outbox: r.get::<Outbox>().await?,
                resolver: r.get::<TransportResolver>().await?,
                admin: delivery && options.delivery.routes,
                events: delivery,
                options,
                signer,
                clock: match app.try_get::<dyn Clock>().await {
                    Some(clock) => clock,
                    None => Arc::new(SystemClock),
                },
                client,
            })
        })
    }
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Mail".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

fn invalid_signature() -> AppError {
    AppError::new(
        StatusCode::UNAUTHORIZED,
        INVALID_SIGNATURE_CODE,
        "Unauthorized",
    )
    .with_detail("Invalid webhook signature")
}

impl Controller for MailController {
    fn prefix(&self) -> &'static str {
        "/mail"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        if self.events {
            routes.extend(self.clone().webhook_routes());
        }
        if self.signer.is_some() {
            routes.extend(self.clone().tracking_routes());
        }
        if self.admin {
            routes.extend(self.admin_routes());
        }
        routes
    }
}

impl MailController {
    /// Records provider events and answers 204. A failure is returned as the
    /// response (a 500 for most store errors), so the provider delivers the
    /// events again.
    async fn ingest(&self, events: Vec<ProviderEvent>) -> Result<NoContent, AppError> {
        self.deliveries.record_all(events).await?;
        Ok(NoContent)
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    #[allow(clippy::too_many_lines)]
    fn webhook_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let webhooks = &self.options.webhooks;
        let mut routes = Vec::new();
        if webhooks.ses.token.is_some() {
            let this = self.clone();
            let ses = move |Query(query): Query<HashMap<String, String>>, body: Bytes| {
                let this = this.clone();
                async move { this.ses(&query, &body).await }
            };
            routes.push(
                RouteDef::on(Method::POST, "/webhooks/ses", ses)
                    .public()
                    .name("mail.webhooks.ses")
                    .api(api("Receive Amazon SES events (through SNS)", &[401])),
            );
        }
        if webhooks.mailgun.signing_key.is_some() {
            let this = self.clone();
            let mailgun = move |body: Bytes| {
                let this = this.clone();
                async move {
                    let payload: Value = serde_json::from_slice(&body)
                        .map_err(|_| AppError::bad_request("The body is not JSON"))?;
                    if !this.mailgun_signed(&payload) {
                        return Err(invalid_signature());
                    }
                    this.ingest(ProviderEvent::from_mailgun(&payload)).await
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/webhooks/mailgun", mailgun)
                    .public()
                    .name("mail.webhooks.mailgun")
                    .api(api("Receive Mailgun events", &[400, 401])),
            );
        }
        if webhooks.postmark.token.is_some() {
            let this = self.clone();
            let postmark = move |headers: HeaderMap,
                                 Query(query): Query<HashMap<String, String>>,
                                 body: Bytes| {
                let this = this.clone();
                async move {
                    if !this.postmark_signed(&headers, &query) {
                        return Err(invalid_signature());
                    }
                    let payload: Value = serde_json::from_slice(&body)
                        .map_err(|_| AppError::bad_request("The body is not JSON"))?;
                    this.ingest(ProviderEvent::from_postmark(&payload)).await
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/webhooks/postmark", postmark)
                    .public()
                    .name("mail.webhooks.postmark")
                    .api(api("Receive Postmark events", &[400, 401])),
            );
        }
        if webhooks.resend.secret.is_some() {
            let this = self.clone();
            let resend = move |headers: HeaderMap, body: Bytes| {
                let this = this.clone();
                async move {
                    if !this.resend_signed(&headers, &body) {
                        return Err(invalid_signature());
                    }
                    let payload: Value = serde_json::from_slice(&body)
                        .map_err(|_| AppError::bad_request("The body is not JSON"))?;
                    this.ingest(ProviderEvent::from_resend(&payload)).await
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/webhooks/resend", resend)
                    .public()
                    .name("mail.webhooks.resend")
                    .api(api("Receive Resend events", &[400, 401])),
            );
        }
        routes
    }

    /// SES events through SNS: the URL token, the topic, then the message.
    async fn ses(
        &self,
        query: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<NoContent, AppError> {
        let options = &self.options.webhooks.ses;
        let expected = options
            .token
            .as_ref()
            .map(|t| t.expose().as_str())
            .unwrap_or_default();
        let given = query.get("token").map(String::as_str).unwrap_or_default();
        if expected.is_empty()
            || !navree_crypto::constant_time_eq(given.as_bytes(), expected.as_bytes())
        {
            return Err(invalid_signature());
        }
        let envelope: Value = serde_json::from_slice(body)
            .map_err(|_| AppError::bad_request("The body is not JSON"))?;
        let topic = envelope["TopicArn"].as_str().unwrap_or_default();
        if !options.topic_arns.is_empty() && !options.topic_arns.iter().any(|t| t == topic) {
            return Err(invalid_signature());
        }
        match envelope["Type"].as_str().unwrap_or_default() {
            "SubscriptionConfirmation" => {
                self.confirm_subscription(&envelope).await;
                Ok(NoContent)
            }
            "Notification" => {
                let message = envelope["Message"].as_str().unwrap_or_default();
                let event: Value = serde_json::from_str(message)
                    .map_err(|_| AppError::bad_request("The SNS message is not JSON"))?;
                self.ingest(ProviderEvent::from_ses(&event)).await
            }
            _ => Ok(NoContent),
        }
    }

    /// Fetches an SNS `SubscribeURL` (only `https://sns.<region>.amazonaws.com`).
    async fn confirm_subscription(&self, envelope: &Value) {
        confirm_sns_subscription(
            self.client.as_ref(),
            envelope,
            self.options.webhooks.ses.confirm_subscriptions,
        )
        .await;
    }

    /// Mailgun: HMAC-SHA256 of `timestamp + token` with the signing key,
    /// within the tolerance.
    fn mailgun_signed(&self, payload: &Value) -> bool {
        let Some(key) = &self.options.webhooks.mailgun.signing_key else {
            return false;
        };
        let signature = &payload["signature"];
        mailgun_signature_valid(
            key.expose(),
            signature["timestamp"].as_str().unwrap_or_default(),
            signature["token"].as_str().unwrap_or_default(),
            signature["signature"].as_str().unwrap_or_default(),
            self.now(),
        )
    }

    /// Postmark: the token as the basic-auth password or `?token=`.
    fn postmark_signed(&self, headers: &HeaderMap, query: &HashMap<String, String>) -> bool {
        let Some(token) = &self.options.webhooks.postmark.token else {
            return false;
        };
        basic_auth_or_token(headers, query, token.expose())
    }

    /// Resend (Svix): `v1,` HMAC-SHA256 of `id.timestamp.body` with the
    /// `whsec_` secret, within the tolerance.
    fn resend_signed(&self, headers: &HeaderMap, body: &[u8]) -> bool {
        let Some(secret) = &self.options.webhooks.resend.secret else {
            return false;
        };
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        let (id, timestamp, signatures) = (
            header("svix-id"),
            header("svix-timestamp"),
            header("svix-signature"),
        );
        let Ok(seconds) = timestamp.parse::<i64>() else {
            return false;
        };
        if id.is_empty() || (self.now().timestamp() - seconds).abs() > SIGNATURE_TOLERANCE_SECS {
            return false;
        }
        let secret = secret.expose();
        let Ok(key) = STANDARD.decode(secret.strip_prefix("whsec_").unwrap_or(secret)) else {
            return false;
        };
        let mut signed = format!("{id}.{timestamp}.").into_bytes();
        signed.extend_from_slice(body);
        let expected = STANDARD.encode(navree_crypto::hmac_sha256(&key, &signed));
        signatures.split_whitespace().any(|candidate| {
            candidate.strip_prefix("v1,").is_some_and(|sig| {
                navree_crypto::constant_time_eq(sig.as_bytes(), expected.as_bytes())
            })
        })
    }

    fn tracking_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let this = self.clone();
        let open = move |Path(id): Path<String>, uri: Uri| {
            let this = this.clone();
            async move {
                let Some(id) = this.verified(&uri, &id) else {
                    return Err(AppError::not_found());
                };
                this.track(ProviderEvent {
                    delivery_id: Some(id),
                    ..ProviderEvent::new("tracking", "", MailEventKind::Opened)
                })
                .await;
                Ok::<Response, AppError>(
                    (
                        [
                            (header::CONTENT_TYPE, "image/gif"),
                            (header::CACHE_CONTROL, "no-store, max-age=0"),
                        ],
                        PIXEL,
                    )
                        .into_response(),
                )
            }
        };
        let this = self.clone();
        let click = move |Path(id): Path<String>,
                          Query(query): Query<HashMap<String, String>>,
                          uri: Uri| {
            let this = this.clone();
            async move {
                let (Some(id), Some(target)) = (this.verified(&uri, &id), query.get("u")) else {
                    return Err(AppError::not_found());
                };
                let lower = target.to_ascii_lowercase();
                if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                    return Err(AppError::not_found());
                }
                this.track(ProviderEvent {
                    delivery_id: Some(id),
                    url: Some(target.clone()),
                    ..ProviderEvent::new("tracking", "", MailEventKind::Clicked)
                })
                .await;
                Ok::<Response, AppError>(
                    (
                        StatusCode::FOUND,
                        [
                            (header::LOCATION, target.as_str()),
                            (header::CACHE_CONTROL, "no-store"),
                        ],
                    )
                        .into_response(),
                )
            }
        };
        vec![
            RouteDef::on(Method::GET, format!("!{OPEN_PATH}/{{delivery}}"), open)
                .public()
                .name("mail.tracking.open")
                .api(api("Record an open (tracking image)", &[404])),
            RouteDef::on(Method::GET, format!("!{CLICK_PATH}/{{delivery}}"), click)
                .public()
                .name("mail.tracking.click")
                .api(api("Record a click and redirect to the link", &[302, 404])),
        ]
    }

    /// The delivery a signed tracking URL names, when the signature holds.
    fn verified(&self, uri: &Uri, id: &str) -> Option<Uuid> {
        let signer = self.signer.as_ref()?;
        if !signer.verify(uri, self.now()) {
            return None;
        }
        Uuid::parse_str(id).ok()
    }

    /// Records a tracking event; a failure is logged, never shown.
    async fn track(&self, mut event: ProviderEvent) {
        event.provider_id = None;
        if let Err(error) = self.deliveries.record(event).await {
            tracing::warn!(target: "navree::mail", error = %error, "recording a tracking event failed");
        }
    }

    #[allow(clippy::too_many_lines)]
    fn admin_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let deliveries = self.deliveries.clone();
        let list_deliveries = move |Query(query): Query<ListQuery>, page: PageQuery| {
            let deliveries = deliveries.clone();
            async move {
                let status = match query.status.as_deref() {
                    Some(status) => Some(
                        status
                            .parse::<DeliveryStatus>()
                            .map_err(AppError::bad_request)?,
                    ),
                    None => None,
                };
                let filter = DeliveryFilter {
                    workspace_id: query.workspace_id,
                    status,
                    recipient: query.recipient,
                    template: query.template,
                };
                Ok::<_, AppError>(Json(deliveries.list(&filter, page).await?))
            }
        };
        let deliveries = self.deliveries.clone();
        let show_delivery = move |Path(id): Path<Uuid>| {
            let deliveries = deliveries.clone();
            async move {
                let delivery = deliveries.find(id).await?;
                let events = deliveries.events(id).await?;
                Ok::<_, AppError>(Json(DeliveryWithEvents { delivery, events }))
            }
        };
        let suppressions = self.suppressions.clone();
        let list_suppressions = move |Query(query): Query<ListQuery>, page: PageQuery| {
            let suppressions = suppressions.clone();
            async move {
                Ok::<_, AppError>(Json(
                    suppressions
                        .list(query.workspace_id, query.email.as_deref(), page)
                        .await?,
                ))
            }
        };
        let suppressions = self.suppressions.clone();
        let add_suppression =
            move |uow: navree_db::Uow, uri: Uri, Json(input): Json<AddSuppressionInput>| {
                let suppressions = suppressions.clone();
                async move {
                    let suppression = suppressions
                        .add(
                            &uow,
                            input.workspace_id,
                            &input.email,
                            input.reason.unwrap_or(SuppressionReason::Manual),
                            input.detail,
                        )
                        .await?;
                    let location =
                        format!("{}/{}", uri.path().trim_end_matches('/'), suppression.id);
                    Ok::<_, AppError>(Created(location, suppression))
                }
            };
        let suppressions = self.suppressions.clone();
        let remove_suppression = move |uow: navree_db::Uow, Path(id): Path<Uuid>| {
            let suppressions = suppressions.clone();
            async move {
                suppressions.remove(&uow, id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let outbox = self.outbox.clone();
        let list_outbox = move |Query(query): Query<ListQuery>, page: PageQuery| {
            let outbox = outbox.clone();
            async move {
                let status = match query.status.as_deref() {
                    None => Some(OutboxStatus::Held),
                    Some("all") => None,
                    Some(status) => Some(
                        status
                            .parse::<OutboxStatus>()
                            .map_err(AppError::bad_request)?,
                    ),
                };
                Ok::<_, AppError>(Json(outbox.list(status, query.workspace_id, page).await?))
            }
        };
        let outbox = self.outbox.clone();
        let release = move |CurrentPrincipal(principal), Path(id): Path<Uuid>| {
            let outbox = outbox.clone();
            async move {
                let report = outbox.release(id, Some(principal.user_id)).await?;
                Ok::<_, AppError>(Json(ReleasedMail::from(report)))
            }
        };
        let outbox = self.outbox.clone();
        let discard = move |CurrentPrincipal(principal), Path(id): Path<Uuid>| {
            let outbox = outbox.clone();
            async move { Ok::<_, AppError>(Json(outbox.discard(id, Some(principal.user_id)).await?)) }
        };
        let resolver = self.resolver.clone();
        let usable = move |Json(config): Json<TransportConfig>| {
            let resolver = resolver.clone();
            async move {
                resolver.usable(&config).await?;
                Ok::<_, AppError>(Json(UsableTransport {
                    usable: true,
                    kind: config.kind().to_owned(),
                }))
            }
        };
        let route = |route: RouteDef| route.permissions(&[MANAGE_PERMISSION]);
        vec![
            route(RouteDef::on(Method::GET, "/deliveries", list_deliveries))
                .name("mail.deliveries.index")
                .api(ApiMeta {
                    response: Some(schema_fn::<Delivery>),
                    ..api("List the delivery log", &[400, 401, 403])
                }),
            route(RouteDef::on(Method::GET, "/deliveries/{id}", show_delivery))
                .name("mail.deliveries.show")
                .api(ApiMeta {
                    response: Some(schema_fn::<DeliveryWithEvents>),
                    ..api("Show a delivery and its events", &[401, 403, 404])
                }),
            route(RouteDef::on(
                Method::GET,
                "/suppressions",
                list_suppressions,
            ))
            .name("mail.suppressions.index")
            .api(ApiMeta {
                response: Some(schema_fn::<Suppression>),
                ..api("List suppressed addresses", &[401, 403])
            }),
            route(RouteDef::on(Method::POST, "/suppressions", add_suppression))
                .name("mail.suppressions.store")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(schema_fn::<AddSuppressionInput>),
                    response: Some(schema_fn::<Suppression>),
                    ..api("Suppress an address", &[400, 401, 403])
                }),
            route(RouteDef::on(
                Method::DELETE,
                "/suppressions/{id}",
                remove_suppression,
            ))
            .name("mail.suppressions.destroy")
            .api(api("Remove a suppression", &[401, 403, 404])),
            route(RouteDef::on(Method::GET, "/outbox", list_outbox))
                .name("mail.outbox.index")
                .api(ApiMeta {
                    response: Some(schema_fn::<HeldMail>),
                    ..api("List mail held for approval", &[400, 401, 403])
                }),
            route(RouteDef::on(Method::POST, "/outbox/{id}/release", release))
                .name("mail.outbox.release")
                .api(ApiMeta {
                    response: Some(schema_fn::<ReleasedMail>),
                    ..api("Release and send held mail", &[401, 403, 404, 409])
                }),
            route(RouteDef::on(Method::POST, "/outbox/{id}/discard", discard))
                .name("mail.outbox.discard")
                .api(ApiMeta {
                    response: Some(schema_fn::<HeldMail>),
                    ..api("Discard held mail", &[401, 403, 404, 409])
                }),
            route(RouteDef::on(Method::POST, "/transports/usable", usable))
                .name("mail.transports.usable")
                .api(ApiMeta {
                    response: Some(schema_fn::<UsableTransport>),
                    ..api(
                        "Check that transport settings work, without saving them",
                        &[400, 401, 403, 422],
                    )
                }),
        ]
    }
}

impl MailError {
    /// The error response this error maps to in a route: 404 for
    /// [`NotFound`](MailError::NotFound), 409 for
    /// [`Conflict`](MailError::Conflict), 422 `mail_transport_unusable` for
    /// [`Unusable`](MailError::Unusable), 400 for invalid addresses,
    /// headers and settings; for a database error
    /// ([`db_error`](MailError::db_error)), 404 when the row is gone and 409
    /// for a unique or foreign-key violation; `None` (a 500) otherwise.
    pub fn to_app_error(&self) -> Option<AppError> {
        if let Some(error) = self.db_error() {
            return match error.kind() {
                ErrorKind::NotFound => Some(AppError::not_found()),
                ErrorKind::UniqueViolation | ErrorKind::ForeignKeyViolation => Some(
                    AppError::conflict("the change conflicts with another record"),
                ),
                _ => None,
            };
        }
        match self {
            MailError::NotFound(_) => Some(AppError::not_found()),
            MailError::Conflict(message) => Some(AppError::conflict(message.clone())),
            MailError::Unusable(message) => Some(
                AppError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    UNUSABLE_TRANSPORT_CODE,
                    "Unprocessable Entity",
                )
                .with_detail(message.clone()),
            ),
            MailError::InvalidAddress { .. }
            | MailError::InvalidHeader { .. }
            | MailError::Header { .. }
            | MailError::NoRecipients
            | MailError::MissingSender
            | MailError::Config(_) => Some(AppError::bad_request(self.to_string())),
            _ => None,
        }
    }
}

/// Fetches an SNS subscription's `SubscribeURL` when `confirm` is on (only
/// `https://sns.<region>.amazonaws.com` URLs); logs otherwise.
pub(crate) async fn confirm_sns_subscription(
    client: Option<&HttpClient>,
    envelope: &Value,
    confirm: bool,
) {
    let url = envelope["SubscribeURL"].as_str().unwrap_or_default();
    let allowed = url::Url::parse(url).is_ok_and(|parsed| {
        parsed.scheme() == "https"
            && parsed.host_str().is_some_and(|host| {
                host.starts_with("sns.")
                    && (host.ends_with(".amazonaws.com") || host.ends_with(".amazonaws.com.cn"))
            })
    });
    if !allowed {
        tracing::warn!(target: "navree::mail", "SNS subscription confirmation with an unexpected SubscribeURL ignored");
        return;
    }
    if !confirm {
        tracing::info!(target: "navree::mail", topic = envelope["TopicArn"].as_str().unwrap_or_default(), "SNS subscription waiting for confirmation: open its SubscribeURL");
        return;
    }
    let Some(client) = client else {
        return;
    };
    match client.get(url).send().await {
        Ok(response) if response.is_success() => {
            tracing::info!(target: "navree::mail", topic = envelope["TopicArn"].as_str().unwrap_or_default(), "SNS subscription confirmed");
        }
        Ok(response) => {
            tracing::warn!(target: "navree::mail", status = response.status().as_u16(), "SNS subscription confirmation failed");
        }
        Err(error) => {
            tracing::warn!(target: "navree::mail", error = %error, "SNS subscription confirmation failed");
        }
    }
}

/// Mailgun's signature: HMAC-SHA256 of `timestamp + token` with the
/// signing key, hex, with the timestamp within 15 minutes of `now`.
pub(crate) fn mailgun_signature_valid(
    key: &str,
    timestamp: &str,
    token: &str,
    signature: &str,
    now: DateTime<Utc>,
) -> bool {
    let Ok(seconds) = timestamp.parse::<i64>() else {
        return false;
    };
    if (now.timestamp() - seconds).abs() > SIGNATURE_TOLERANCE_SECS {
        return false;
    }
    let expected = hex::encode(navree_crypto::hmac_sha256(
        key.as_bytes(),
        format!("{timestamp}{token}").as_bytes(),
    ));
    navree_crypto::constant_time_eq(expected.as_bytes(), signature.as_bytes())
}

/// `token` as the `token` query parameter, or as the password of HTTP
/// basic authentication.
pub(crate) fn basic_auth_or_token(
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    token: &str,
) -> bool {
    let token = token.as_bytes();
    if token.is_empty() {
        return false;
    }
    if let Some(given) = query.get("token") {
        return navree_crypto::constant_time_eq(given.as_bytes(), token);
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|encoded| STANDARD.decode(encoded.trim()).ok())
        .and_then(|decoded| String::from_utf8(decoded).ok())
        .and_then(|pair| {
            pair.split_once(':')
                .map(|(_, password)| password.to_owned())
        })
        .is_some_and(|password| navree_crypto::constant_time_eq(password.as_bytes(), token))
}

/// The error mapper registered by `MailModule`.
pub(crate) fn map_mail_error(error: &MailError) -> Option<AppError> {
    error.to_app_error()
}
