//! [`MailOptions`] and [`TransportConfig`].

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::message::mailbox;
use crate::{
    ArrayTransport, FailoverTransport, LogTransport, MailError, MailTransport, RenderOptions,
    SmtpTransport,
};

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
            let url = url::Url::parse(base)
                .map_err(|error| format!("`tracking.base_url`: {error}"))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err("`tracking.base_url` must be an http(s) URL".to_owned());
            }
        }
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
/// key = "..."                           # default: derived from app.key
/// ```
///
/// Tracking needs `mail.delivery.enabled`: links and the image name the
/// delivery, and each open or click is logged as an event on it.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
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
    /// The key that signs tracking links (default: derived from the app
    /// key, so `CryptoModule` with `app.key` is needed).
    pub key: Option<Secret<String>>,
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
/// topic_arns = ["arn:aws:sns:eu-west-1:123456789012:ses-events"]
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

/// `mail.webhooks.ses`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SesWebhookOptions {
    /// The secret in the subscription URL's `token` query parameter.
    pub token: Option<Secret<String>>,
    /// The SNS topics accepted (every topic when empty).
    pub topic_arns: Vec<String>,
    /// Confirm SNS subscriptions by fetching their `SubscribeURL` (default
    /// `true`; only `https://sns.<region>.amazonaws.com` URLs).
    pub confirm_subscriptions: bool,
}

impl Default for SesWebhookOptions {
    fn default() -> Self {
        Self {
            token: None,
            topic_arns: Vec::new(),
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

/// Which [`MailTransport`] delivers mail.
///
/// In configuration, a table with a `type`, or a shorthand string (handy in
/// environment variables):
///
/// | Transport | Table | Shorthand |
/// |---|---|---|
/// | [`SmtpTransport`] | `{ type = "smtp", url = "smtps://user:pass@host" }` | `"smtps://user:pass@host"` / `"smtp://..."` |
/// | [`LogTransport`] | `{ type = "log" }` | `"log"` |
/// | [`ArrayTransport`] | `{ type = "array" }` | `"array"` |
/// | [`FailoverTransport`] | `{ type = "failover", transports = ["smtps://...", "log"] }` | — |
///
/// The failover's `transports` may also be one comma-separated string of
/// shorthands (`APP_MAIL__TRANSPORT__TRANSPORTS=smtps://...,log`). The SMTP
/// URL is a [`Secret`]: `Debug` never prints it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TransportConfig {
    /// SMTP at `url` (see [`SmtpTransport::from_url`]).
    Smtp {
        /// The connection URL; it usually holds a password.
        url: Secret<String>,
    },
    /// Write messages to the log (development).
    #[default]
    Log,
    /// Record messages in memory (tests; see
    /// [`Mailer::array_transport`](crate::Mailer::array_transport)).
    Array,
    /// Try each transport in order until one delivers.
    Failover(Vec<TransportConfig>),
}

impl TransportConfig {
    /// SMTP at `url`.
    pub fn smtp(url: impl Into<String>) -> Self {
        TransportConfig::Smtp {
            url: Secret::new(url.into()),
        }
    }

    /// Builds the transport. Makes no network connection; an invalid SMTP
    /// URL or an empty failover list is [`MailError::Config`].
    pub fn build(&self) -> Result<Arc<dyn MailTransport>, MailError> {
        Ok(match self {
            TransportConfig::Smtp { url } => Arc::new(SmtpTransport::from_url(url.expose())?),
            TransportConfig::Log => Arc::new(LogTransport),
            TransportConfig::Array => Arc::new(ArrayTransport::new()),
            TransportConfig::Failover(transports) => {
                if transports.is_empty() {
                    return Err(MailError::Config(
                        "a failover transport needs at least one transport".to_owned(),
                    ));
                }
                let built = transports
                    .iter()
                    .map(TransportConfig::build)
                    .collect::<Result<Vec<_>, _>>()?;
                Arc::new(FailoverTransport::new(built))
            }
        })
    }

    /// The transport a shorthand string names, if any.
    fn shorthand(s: &str) -> Option<Self> {
        let s = s.trim();
        let scheme = s.split_once("://").map(|(scheme, _)| scheme);
        if s.eq_ignore_ascii_case("log") {
            Some(TransportConfig::Log)
        } else if s.eq_ignore_ascii_case("array") {
            Some(TransportConfig::Array)
        } else if scheme.is_some_and(|scheme| {
            scheme.eq_ignore_ascii_case("smtp") || scheme.eq_ignore_ascii_case("smtps")
        }) {
            Some(TransportConfig::smtp(s))
        } else {
            None
        }
    }
}

/// What a transport must look like (never echoes the value: it may be a
/// mistyped URL with a password).
const EXPECTED: &str = "`log`, `array`, an `smtp://` or `smtps://` URL, or a table with `type`";

impl<'de> Deserialize<'de> for TransportConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(TransportVisitor)
    }
}

/// Reads a shorthand string or a tagged table.
struct TransportVisitor;

impl<'de> Visitor<'de> for TransportVisitor {
    type Value = TransportConfig;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a mail transport: {EXPECTED}")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<TransportConfig, E> {
        TransportConfig::shorthand(value)
            .ok_or_else(|| E::custom(format!("unknown mail transport (expected {EXPECTED})")))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<TransportConfig, A::Error> {
        Tagged::deserialize(de::value::MapAccessDeserializer::new(map)).map(Into::into)
    }
}

/// The table form, tagged by `type`.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Tagged {
    Smtp {
        url: Secret<String>,
    },
    Log,
    Array,
    Failover {
        #[serde(deserialize_with = "transport_list")]
        transports: Vec<TransportConfig>,
    },
}

impl From<Tagged> for TransportConfig {
    fn from(tagged: Tagged) -> Self {
        match tagged {
            Tagged::Smtp { url } => TransportConfig::Smtp { url },
            Tagged::Log => TransportConfig::Log,
            Tagged::Array => TransportConfig::Array,
            Tagged::Failover { transports } => TransportConfig::Failover(transports),
        }
    }
}

/// A list of transports, or one comma-separated string of shorthands.
fn transport_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TransportConfig>, D::Error> {
    struct ListVisitor;

    impl<'de> Visitor<'de> for ListVisitor {
        type Value = Vec<TransportConfig>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list of mail transports or a comma-separated string of shorthands")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut transports = Vec::new();
            while let Some(transport) = seq.next_element::<TransportConfig>()? {
                transports.push(transport);
            }
            Ok(transports)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            value
                .split(',')
                .filter(|part| !part.trim().is_empty())
                .map(|part| TransportVisitor.visit_str(part))
                .collect()
        }
    }

    deserializer.deserialize_any(ListVisitor)
}
