//! [`InboundController`]: the routes the providers post received mail to.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::{HeaderMap, Method, header};
use navree_contracts::{Clock, SystemClock};
use navree_http::body::{BodyLimit, Bytes, read_body};
use navree_http::extract::{FromRequest, Multipart, MultipartLimits, Request};
use navree_http::{ApiMeta, AppError, Controller, Json, Query, RouteDef};
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde_json::Value;

use super::sources::{self, FormFile, MailgunForm};
use super::{Inbound, InboundError, Incoming, Received};
use crate::MailOptions;
use crate::delivery::controller::{
    INVALID_SIGNATURE_CODE, basic_auth_or_token, confirm_sns_subscription, mailgun_signature_valid,
};

/// The routes the providers post received mail to, under
/// `{http.api_prefix}/mail/inbound`. Each is public, checked with its
/// source's secret, and mounted only when that secret is set:
///
/// | Route | Source | Checked with |
/// |---|---|---|
/// | `POST /mail/inbound/ses?token=..` | an SNS topic of SES receipt rules (SNS or S3 action) | `mail.inbound.ses.token`, `topic_arns` |
/// | `POST /mail/inbound/mailgun`, `/mailgun/mime` | a Mailgun route's `forward()` | the signature, `mail.inbound.mailgun.signing_key` |
/// | `POST /mail/inbound/postmark` | a Postmark inbound stream | basic auth or `?token=`, `mail.inbound.postmark.token` |
/// | `POST /mail/inbound/raw` | raw MIME (`message/rfc822`) from your server | `Authorization: Bearer`, `mail.inbound.raw.token` |
///
/// Each answers 200 with the [`Received`] JSON; 401 `invalid_signature`
/// when the secret doesn't match, 413 `mail_too_large` over
/// `mail.inbound.max_size`, 400 `invalid_inbound_mail` for a payload or
/// message that cannot be read, and 500 when the mailbox failed (the
/// provider delivers again). The raw route reads the envelope from
/// `X-Envelope-From` and `X-Envelope-To` (comma-separated) when given.
pub struct InboundController {
    inbound: Arc<Inbound>,
    options: Arc<MailOptions>,
    client: Option<HttpClient>,
    clock: Arc<dyn Clock>,
}

impl Injectable for InboundController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Inbound>(),
            ProviderKey::of::<MailOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let client = match app.try_get::<HttpClient>().await {
                Some(client) => Some((*client).clone()),
                None => HttpClient::new(HttpClientOptions::default()).ok(),
            };
            Ok(InboundController {
                inbound: r.get::<Inbound>().await?,
                options: r.get::<MailOptions>().await?,
                client,
                clock: match app.try_get::<dyn Clock>().await {
                    Some(clock) => clock,
                    None => Arc::new(SystemClock),
                },
            })
        })
    }
}

fn api(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Mail".to_owned()],
        responses: vec![400, 401, 413],
        ..ApiMeta::default()
    }
}

fn invalid_signature() -> AppError {
    AppError::new(
        http::StatusCode::UNAUTHORIZED,
        INVALID_SIGNATURE_CODE,
        "Unauthorized",
    )
    .with_detail("Invalid webhook signature")
}

/// The answer of every source route.
fn answer(result: Result<Received, InboundError>) -> Result<Json<Received>, AppError> {
    result.map(Json).map_err(to_app_error)
}

/// The error response of `error`: its own (413, 400), or a 500 that is
/// logged.
fn to_app_error(error: InboundError) -> AppError {
    match error.to_app_error() {
        Some(app_error) => app_error,
        None => {
            tracing::warn!(target: "navree::mail", %error, "receiving an inbound message failed");
            AppError::internal(error)
        }
    }
}

/// A 400 `invalid_inbound_mail` for a payload `provider` doesn't send.
fn malformed(provider: &'static str, reason: &str) -> AppError {
    to_app_error(InboundError::Malformed {
        provider,
        reason: reason.to_owned(),
    })
}

impl Controller for InboundController {
    fn prefix(&self) -> &'static str {
        "/mail/inbound"
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let inbound = &self.options.inbound;
        let limit = inbound.body_limit();
        let mut routes = Vec::new();
        if inbound.ses.token.is_some() {
            let this = self.clone();
            let ses = move |Query(query): Query<HashMap<String, String>>, body: Bytes| {
                let this = this.clone();
                async move { this.ses(&query, &body).await }
            };
            routes.push(
                RouteDef::on(Method::POST, "/ses", ses)
                    .public()
                    .body_limit(limit)
                    .name("mail.inbound.ses")
                    .api(api("Receive mail from Amazon SES (through SNS)")),
            );
        }
        if self.mailgun_key().is_some() {
            for (path, name) in [
                ("/mailgun", "mail.inbound.mailgun"),
                ("/mailgun/mime", "mail.inbound.mailgun.mime"),
            ] {
                let this = self.clone();
                let mailgun = move |request: Request| {
                    let this = this.clone();
                    async move { this.mailgun(request).await }
                };
                routes.push(
                    RouteDef::on(Method::POST, path, mailgun)
                        .public()
                        .body_limit(limit)
                        .name(name)
                        .api(api("Receive mail from a Mailgun route")),
                );
            }
        }
        if let Some(token) = inbound.postmark.token.clone() {
            let this = self.clone();
            let postmark = move |headers: HeaderMap,
                                 Query(query): Query<HashMap<String, String>>,
                                 body: Bytes| {
                let this = this.clone();
                let token = token.clone();
                async move {
                    if !basic_auth_or_token(&headers, &query, token.expose()) {
                        return Err(invalid_signature());
                    }
                    let payload: Value = serde_json::from_slice(&body)
                        .map_err(|_| malformed("postmark", "the body is not JSON"))?;
                    let incoming = Incoming::from_postmark(&payload).map_err(to_app_error)?;
                    answer(this.inbound.receive(incoming).await)
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/postmark", postmark)
                    .public()
                    .body_limit(limit)
                    .name("mail.inbound.postmark")
                    .api(api("Receive mail from Postmark")),
            );
        }
        if let Some(token) = inbound.raw.token.clone() {
            let this = self.clone();
            let raw = move |headers: HeaderMap,
                            Query(query): Query<HashMap<String, String>>,
                            body: Bytes| {
                let this = this.clone();
                let token = token.clone();
                async move {
                    if !bearer_or_token(&headers, &query, token.expose()) {
                        return Err(invalid_signature());
                    }
                    let mut incoming = Incoming::new("raw", body);
                    if let Some(sender) = header_value(&headers, "x-envelope-from") {
                        incoming = incoming.sender(sender);
                    }
                    if let Some(recipients) = header_value(&headers, "x-envelope-to") {
                        for recipient in recipients.split(',') {
                            let recipient = recipient.trim();
                            if !recipient.is_empty() {
                                incoming = incoming.recipient(recipient);
                            }
                        }
                    }
                    answer(this.inbound.receive(incoming).await)
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/raw", raw)
                    .public()
                    .body_limit(limit)
                    .name("mail.inbound.raw")
                    .api(api("Receive a raw MIME message")),
            );
        }
        routes
    }
}

impl InboundController {
    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// The Mailgun signing key: `mail.inbound.mailgun.signing_key`, else
    /// the webhooks' one.
    fn mailgun_key(&self) -> Option<&str> {
        self.options
            .inbound
            .mailgun
            .signing_key
            .as_ref()
            .or(self.options.webhooks.mailgun.signing_key.as_ref())
            .map(|key| key.expose().as_str())
            .filter(|key| !key.is_empty())
    }

    /// SES through SNS: the URL token, the topic, then the notification.
    async fn ses(
        &self,
        query: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<Json<Received>, AppError> {
        let options = &self.options.inbound.ses;
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
        let envelope: Value =
            serde_json::from_slice(body).map_err(|_| malformed("ses", "the body is not JSON"))?;
        let topic = envelope["TopicArn"].as_str().unwrap_or_default();
        if !options.topic_arns.is_empty() && !options.topic_arns.iter().any(|t| t == topic) {
            return Err(invalid_signature());
        }
        match envelope["Type"].as_str().unwrap_or_default() {
            "SubscriptionConfirmation" => {
                confirm_sns_subscription(
                    self.client.as_ref(),
                    &envelope,
                    options.confirm_subscriptions,
                )
                .await;
                Ok(Json(self.empty()))
            }
            "Notification" => {
                let message = envelope["Message"].as_str().unwrap_or_default();
                let notification: Value = serde_json::from_str(message)
                    .map_err(|_| malformed("ses", "the SNS message is not JSON"))?;
                answer(self.inbound.receive_ses(&notification).await)
            }
            _ => Ok(Json(self.empty())),
        }
    }

    /// A [`Received`] for an SNS message that carried no mail.
    fn empty(&self) -> Received {
        Received {
            id: uuid::Uuid::nil(),
            status: super::InboundStatus::Unrouted,
            mailbox: None,
            address: None,
            reason: Some("no message".to_owned()),
            stored: None,
        }
    }

    /// A Mailgun route: a multipart or URL-encoded form, signed.
    async fn mailgun(&self, request: Request) -> Result<Json<Received>, AppError> {
        let malformed = |reason: &str| malformed("mailgun", reason);
        let multipart = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("multipart/"));
        let mut form = MailgunForm::default();
        if multipart {
            let parts = Multipart::from_request(request)
                .await
                .map_err(|_| malformed("the body is not a multipart form"))?;
            // Mailgun sends the whole message as a field, not as a file.
            let limits = MultipartLimits {
                max_field_bytes: self.options.inbound.body_limit() as u64,
                ..parts.limits()
            };
            let mut parts = parts.with_limits(limits);
            while let Some(field) = parts.next_field().await.map_err(|error| {
                if error.status() == http::StatusCode::PAYLOAD_TOO_LARGE {
                    to_app_error(InboundError::TooLarge {
                        size: self.options.inbound.body_limit(),
                        max: self.options.inbound.max_size,
                    })
                } else {
                    malformed("the multipart form is damaged")
                }
            })? {
                let name = field.name().unwrap_or_default().to_owned();
                let filename = field.file_name().map(str::to_owned);
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_owned();
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| malformed("the multipart form is damaged"))?;
                if filename.is_some() {
                    form.files.push(FormFile {
                        field: name,
                        filename,
                        content_type,
                        content: data,
                    });
                } else {
                    form.fields
                        .push((name, String::from_utf8_lossy(&data).into_owned()));
                }
            }
        } else {
            let limit = BodyLimit::of(request.extensions());
            let body = read_body(request.into_body(), limit)
                .await
                .map_err(|_| malformed("the body could not be read"))?;
            form.fields = url::form_urlencoded::parse(&body)
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect();
        }
        let Some(key) = self.mailgun_key() else {
            return Err(invalid_signature());
        };
        if !mailgun_signature_valid(
            key,
            form.field("timestamp").unwrap_or_default(),
            form.field("token").unwrap_or_default(),
            form.field("signature").unwrap_or_default(),
            self.now(),
        ) {
            return Err(invalid_signature());
        }
        let incoming = sources::mailgun(&form).map_err(to_app_error)?;
        answer(self.inbound.receive(incoming).await)
    }
}

/// `token` as a bearer token or the `token` query parameter.
fn bearer_or_token(headers: &HeaderMap, query: &HashMap<String, String>, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if let Some(given) = query.get("token") {
        return navree_crypto::constant_time_eq(given.as_bytes(), token.as_bytes());
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|given| {
            navree_crypto::constant_time_eq(given.trim().as_bytes(), token.as_bytes())
        })
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
