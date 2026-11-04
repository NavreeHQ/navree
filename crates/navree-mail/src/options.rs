//! [`MailOptions`] and the settings of its sections.

use std::collections::BTreeMap;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;

use crate::message::mailbox;
use crate::{RenderOptions, TransportConfig};

/// Mail settings. Configuration section `mail`:
///
/// ```toml
/// [mail]
/// from = "Acme <hello@acme.test>"   # APP_MAIL__FROM
/// queue = "mail"                    # APP_MAIL__QUEUE (queue for Mailer::queue)
///
/// [mail.transport]                  # or APP_MAIL__TRANSPORT=smtps://user:pass@smtp.acme.test
/// type = "smtp"
/// url = "smtps://user:pass@smtp.acme.test"
///
/// [mail.transports.simulation]      # a named transport: mailer.via("simulation")
/// type = "ses"
/// region = "eu-west-1"
///
/// [mail.delivery]
/// enabled = true                    # log, suppress, hold, tenant transports
///
/// [mail.tracking]
/// opens = true
/// clicks = true
/// ```
///
/// See [`TransportConfig`] for every transport and its shorthand,
/// [`DeliveryOptions`], [`TrackingOptions`], [`WebhookOptions`] and
/// [`RenderOptions`] for the rest.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct MailOptions {
    /// The default sender, `hello@example.com` or `Acme <hello@acme.test>`
    /// (default `hello@example.com`). Empty means no default: every message
    /// must then set its own sender.
    pub from: String,
    /// How messages are delivered (default [`TransportConfig::Log`]).
    pub transport: TransportConfig,
    /// The queue [`Mailer::queue`](crate::Mailer::queue) dispatches to
    /// (default: the job's own queue, `default`).
    pub queue: Option<String>,
    /// Named transports that [`Mailer::via`](crate::Mailer::via) selects
    /// (lanes such as `simulation` and `notifications`).
    pub transports: BTreeMap<String, TransportConfig>,
    /// CSS inlining and the generated text body.
    pub render: RenderOptions,
    /// The delivery log, suppressions, the outbox and tenant transports.
    pub delivery: DeliveryOptions,
    /// Open and click tracking.
    pub tracking: TrackingOptions,
    /// The provider webhooks that report deliveries, bounces and
    /// complaints.
    pub webhooks: WebhookOptions,
    /// Inbound mail: size limit, the disk for raw messages, reply tokens
    /// and the provider sources (crate feature `inbound`).
    #[cfg(feature = "inbound")]
    pub inbound: crate::inbound::InboundOptions,
}

impl Default for MailOptions {
    fn default() -> Self {
        Self {
            from: "hello@example.com".to_owned(),
            transport: TransportConfig::Log,
            queue: None,
            transports: BTreeMap::new(),
            render: RenderOptions::default(),
            delivery: DeliveryOptions::default(),
            tracking: TrackingOptions::default(),
            webhooks: WebhookOptions::default(),
            #[cfg(feature = "inbound")]
            inbound: crate::inbound::InboundOptions::default(),
        }
    }
}

impl ConfigSection for MailOptions {
    const KEY: &'static str = "mail";

    /// The sender parses, every transport builds (no connection is made),
    /// and tracking has the delivery log it needs.
    fn validate(&self) -> Result<(), String> {
        if !self.from.is_empty() {
            mailbox("from", &self.from).map_err(|error| format!("`from`: {error}"))?;
        }
        self.transport
            .build()
            .map_err(|error| format!("`transport`: {error}"))?;
        for (name, transport) in &self.transports {
            if name.is_empty() {
                return Err("`transports`: a transport name must not be empty".to_owned());
            }
            transport
                .build()
                .map_err(|error| format!("`transports.{name}`: {error}"))?;
        }
        if self.tracking.is_on() && !self.delivery.enabled {
            return Err(
                "`tracking.opens` and `tracking.clicks` need `delivery.enabled = true` (tracked links name the delivery)"
                    .to_owned(),
            );
        }
        if let Some(base) = self.tracking.base_url.as_deref().filter(|b| !b.is_empty()) {
            let url =
                url::Url::parse(base).map_err(|error| format!("`tracking.base_url`: {error}"))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err("`tracking.base_url` must be an http(s) URL".to_owned());
            }
        }
        let ses = &self.webhooks.ses;
        if ses.token.is_some() && ses.topic_arns.is_empty() {
            return Err(
                "`webhooks.ses.topic_arns` must list the SNS topics when `webhooks.ses.token` is set: SNS signs every topic's messages with the same keys"
                    .to_owned(),
            );
        }
        if ses.tolerance_secs == 0 {
            return Err("`webhooks.ses.tolerance_secs` must be greater than 0".to_owned());
        }
        #[cfg(feature = "inbound")]
        self.inbound.validate()?;
        Ok(())
    }
}

/// The delivery log, suppressions, the outbox and tenant transports.
/// Configuration section `mail.delivery`:
///
/// ```toml
/// [mail.delivery]
/// enabled = true        # APP_MAIL__DELIVERY__ENABLED
/// suppress = true       # leave suppressed recipients out
/// auto_suppress = true  # hard bounces and complaints suppress the address
/// tenants = true        # resolve tenant transports (domain, workspace, organization, platform)
/// routes = true         # the admin routes under /api/mail
/// ```
///
/// Everything here needs `DatabaseModule` (migration set `mail`); stored
/// transports and held messages are encrypted with the app key
/// (`CryptoModule`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DeliveryOptions {
    /// Turn the delivery pipeline on (default `false`): every message is
    /// logged in `mail_deliveries`, suppressed recipients are left out,
    /// held messages go to the outbox and tenant transports are resolved.
    pub enabled: bool,
    /// Leave out recipients in `mail_suppressions` (default `true`).
    pub suppress: bool,
    /// Suppress an address after a hard bounce or a complaint (default
    /// `true`), in the delivery's workspace (app-wide for mail sent without
    /// one).
    pub auto_suppress: bool,
    /// Look up tenant transports in `mail_transports` (default `true`).
    pub tenants: bool,
    /// Mount the admin routes under `{api_prefix}/mail` (default `true`;
    /// permission `mail.manage`).
    pub routes: bool,
}

impl Default for DeliveryOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            suppress: true,
            auto_suppress: true,
            tenants: true,
            routes: true,
        }
    }
}

/// Open and click tracking. Configuration section `mail.tracking`:
///
/// ```toml
/// [mail.tracking]
/// opens = true                          # a 1x1 image in the HTML body
/// clicks = true                         # links go through a signed redirect
/// base_url = "https://links.acme.test"  # default: app.url
/// ttl_days = 365                        # how long links stay valid
/// ```
///
/// Tracking needs `mail.delivery.enabled`: links and the image name the
/// delivery, and each open or click is logged as an event on it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TrackingOptions {
    /// Add an open-tracking image to HTML mail (default `false`).
    pub opens: bool,
    /// Send links through a signed redirect that logs the click (default
    /// `false`).
    pub clicks: bool,
    /// Where the tracking routes are served, without a trailing slash
    /// (default: `app.url`).
    pub base_url: Option<String>,
    /// How long tracked links stay valid, in days (default 365). Links are
    /// signed with the app's `UrlSigner` (`http.signing_key`, or a key
    /// derived from `app.key`).
    pub ttl_days: u32,
}

impl Default for TrackingOptions {
    fn default() -> Self {
        Self {
            opens: false,
            clicks: false,
            base_url: None,
            ttl_days: 365,
        }
    }
}

impl TrackingOptions {
    /// Whether opens or clicks are tracked.
    pub fn is_on(&self) -> bool {
        self.opens || self.clicks
    }
}

/// The provider webhooks that report what happened to sent mail.
/// Configuration section `mail.webhooks`; a provider's route is mounted
/// when its secret is set:
///
/// ```toml
/// [mail.webhooks.ses]
/// token = "..."           # POST /api/mail/webhooks/ses?token=...
/// topic_arns = ["arn:aws:sns:eu-west-1:123456789012:ses-events"]   # required
/// tolerance_secs = 3600   # how old a message may be
///
/// [mail.webhooks.mailgun]
/// signing_key = "..."     # POST /api/mail/webhooks/mailgun
///
/// [mail.webhooks.postmark]
/// token = "..."           # POST /api/mail/webhooks/postmark (basic auth password or ?token=)
///
/// [mail.webhooks.resend]
/// secret = "whsec_..."    # POST /api/mail/webhooks/resend
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct WebhookOptions {
    /// Amazon SES events through an SNS topic.
    pub ses: SesWebhookOptions,
    /// Mailgun event webhooks.
    pub mailgun: MailgunWebhookOptions,
    /// Postmark webhooks.
    pub postmark: PostmarkWebhookOptions,
    /// Resend webhooks.
    pub resend: ResendWebhookOptions,
}

/// `mail.webhooks.ses`: SES events through an SNS topic. Every message is
/// checked before its events are recorded: its SNS signature (version 1 or
/// 2, with the signing certificate fetched from
/// `https://sns.<region>.amazonaws.com`), its topic, its age, and the URL
/// token.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SesWebhookOptions {
    /// The secret in the subscription URL's `token` query parameter,
    /// checked on top of the signature. The route is mounted when it is
    /// set.
    pub token: Option<Secret<String>>,
    /// The SNS topics accepted. Required with `token`: SNS signs every
    /// topic's messages with the same keys.
    pub topic_arns: Vec<String>,
    /// How far a message's `Timestamp` may be from now, in seconds
    /// (default 3600: SNS retries a delivery for at most an hour, with the
    /// first timestamp).
    pub tolerance_secs: u64,
    /// Confirm SNS subscriptions by fetching their `SubscribeURL` (default
    /// `true`; only `https://sns.<region>.amazonaws.com` URLs).
    pub confirm_subscriptions: bool,
}

impl Default for SesWebhookOptions {
    fn default() -> Self {
        Self {
            token: None,
            topic_arns: Vec::new(),
            tolerance_secs: crate::sns::DEFAULT_TOLERANCE_SECS,
            confirm_subscriptions: true,
        }
    }
}

/// `mail.webhooks.mailgun`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct MailgunWebhookOptions {
    /// The webhook signing key (Mailgun's "HTTP webhook signing key").
    pub signing_key: Option<Secret<String>>,
}

/// `mail.webhooks.postmark`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct PostmarkWebhookOptions {
    /// The secret: the basic-auth password of the webhook URL, or its
    /// `token` query parameter.
    pub token: Option<Secret<String>>,
}

/// `mail.webhooks.resend`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct ResendWebhookOptions {
    /// The webhook's signing secret (`whsec_...`).
    pub secret: Option<Secret<String>>,
}
