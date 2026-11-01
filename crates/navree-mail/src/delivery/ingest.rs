//! [`ProviderEvent`]: what a provider (or tracking) reports about a sent
//! message, parsed from each provider's webhook payload.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, TimeZone as _, Utc};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// What happened to a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailEventKind {
    /// Delivered to the recipient's server.
    Delivered,
    /// Bounced (see [`ProviderEvent::permanent`]).
    Bounced,
    /// Delivery is delayed; the provider keeps trying.
    Deferred,
    /// Marked as spam.
    Complained,
    /// Opened.
    Opened,
    /// A link was clicked.
    Clicked,
}

impl MailEventKind {
    /// The stored name (`delivered`, `bounced`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            MailEventKind::Delivered => "delivered",
            MailEventKind::Bounced => "bounced",
            MailEventKind::Deferred => "deferred",
            MailEventKind::Complained => "complained",
            MailEventKind::Opened => "opened",
            MailEventKind::Clicked => "clicked",
        }
    }
}

impl fmt::Display for MailEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MailEventKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "delivered" => MailEventKind::Delivered,
            "bounced" => MailEventKind::Bounced,
            "deferred" => MailEventKind::Deferred,
            "complained" => MailEventKind::Complained,
            "opened" => MailEventKind::Opened,
            "clicked" => MailEventKind::Clicked,
            other => return Err(format!("unknown mail event `{other}`")),
        })
    }
}

/// One thing a provider reported about one recipient of a sent message.
/// [`Deliveries::record`](crate::Deliveries::record) logs it against the
/// delivery its provider id (or delivery id) names.
///
/// Build one by hand, or parse a provider's webhook payload:
///
/// ```
/// use navree_mail::{MailEventKind, ProviderEvent};
/// use serde_json::json;
///
/// let events = ProviderEvent::from_postmark(&json!({
///     "RecordType": "Bounce",
///     "MessageID": "b7bc2f4a",
///     "Type": "HardBounce",
///     "Email": "ada@example.com",
///     "Description": "The server was unable to deliver your message",
///     "BouncedAt": "2026-10-27T10:00:00Z"
/// }));
/// assert_eq!(events[0].kind, MailEventKind::Bounced);
/// assert!(events[0].permanent);
/// assert_eq!(events[0].provider_id.as_deref(), Some("b7bc2f4a"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEvent {
    /// Who reported it: `ses`, `mailgun`, `postmark`, `resend`, `tracking`,
    /// `fake`.
    pub provider: String,
    /// The provider's message id.
    pub provider_id: Option<String>,
    /// The delivery, when the reporter knows it (tracking does).
    pub delivery_id: Option<Uuid>,
    /// What happened.
    pub kind: MailEventKind,
    /// The recipient it happened to.
    pub recipient: Option<String>,
    /// For bounces: a hard (permanent) bounce rather than a soft one.
    pub permanent: bool,
    /// For clicks: the link.
    pub url: Option<String>,
    /// The provider's diagnostic.
    pub detail: Option<String>,
    /// When it happened (default: when it is recorded).
    pub occurred_at: Option<DateTime<Utc>>,
}

impl ProviderEvent {
    /// An event of `kind` from `provider` about the message with
    /// `provider_id`.
    pub fn new(
        provider: impl Into<String>,
        provider_id: impl Into<String>,
        kind: MailEventKind,
    ) -> Self {
        Self {
            provider: provider.into(),
            provider_id: Some(provider_id.into()),
            delivery_id: None,
            kind,
            recipient: None,
            permanent: false,
            url: None,
            detail: None,
            occurred_at: None,
        }
    }

    /// With the recipient.
    #[must_use]
    pub fn recipient(mut self, recipient: impl Into<String>) -> Self {
        self.recipient = Some(recipient.into());
        self
    }

    /// A permanent (hard) bounce.
    #[must_use]
    pub fn permanent(mut self) -> Self {
        self.permanent = true;
        self
    }

    /// With the clicked link.
    #[must_use]
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// With the provider's diagnostic.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// When it happened.
    #[must_use]
    pub fn at(mut self, occurred_at: DateTime<Utc>) -> Self {
        self.occurred_at = Some(occurred_at);
        self
    }

    /// Whether the event suppresses its recipient: a hard bounce or a
    /// complaint.
    pub fn suppresses(&self) -> bool {
        matches!(self.kind, MailEventKind::Complained)
            || (self.kind == MailEventKind::Bounced && self.permanent)
    }

    /// The events of an SES event notification (the `Message` of an SNS
    /// notification from a configuration set's event destination, or an
    /// identity notification): `Delivery`, `Bounce` (`Permanent` is hard),
    /// `Complaint`, `DeliveryDelay`, `Open` and `Click`, one per recipient.
    /// Other types give none.
    pub fn from_ses(event: &Value) -> Vec<ProviderEvent> {
        let kind = event
            .get("eventType")
            .or_else(|| event.get("notificationType"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mail = &event["mail"];
        let provider_id = mail["messageId"].as_str().map(str::to_owned);
        let base = |kind: MailEventKind, section: &Value| ProviderEvent {
            provider: "ses".to_owned(),
            provider_id: provider_id.clone(),
            delivery_id: None,
            kind,
            recipient: None,
            permanent: false,
            url: None,
            detail: None,
            occurred_at: time(&section["timestamp"]).or_else(|| time(&mail["timestamp"])),
        };
        let addresses = |list: &Value, key: Option<&str>| -> Vec<(String, Option<String>)> {
            list.as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| match key {
                            Some(key) => item[key].as_str().map(|address| {
                                (
                                    address.to_owned(),
                                    item["diagnosticCode"].as_str().map(str::to_owned),
                                )
                            }),
                            None => item.as_str().map(|address| (address.to_owned(), None)),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let each =
            |kind: MailEventKind, section: &Value, recipients: Vec<(String, Option<String>)>| {
                let template = base(kind, section);
                if recipients.is_empty() {
                    return vec![template];
                }
                recipients
                    .into_iter()
                    .map(|(recipient, detail)| ProviderEvent {
                        recipient: Some(recipient),
                        detail,
                        ..template.clone()
                    })
                    .collect()
            };
        match kind {
            "Delivery" => {
                let section = &event["delivery"];
                each(
                    MailEventKind::Delivered,
                    section,
                    addresses(&section["recipients"], None),
                )
            }
            "Bounce" => {
                let section = &event["bounce"];
                let permanent = section["bounceType"].as_str() == Some("Permanent");
                each(
                    MailEventKind::Bounced,
                    section,
                    addresses(&section["bouncedRecipients"], Some("emailAddress")),
                )
                .into_iter()
                .map(|mut e| {
                    e.permanent = permanent;
                    if e.detail.is_none() {
                        e.detail = section["bounceSubType"].as_str().map(str::to_owned);
                    }
                    e
                })
                .collect()
            }
            "Complaint" => {
                let section = &event["complaint"];
                each(
                    MailEventKind::Complained,
                    section,
                    addresses(&section["complainedRecipients"], Some("emailAddress")),
                )
            }
            "DeliveryDelay" => {
                let section = &event["deliveryDelay"];
                each(
                    MailEventKind::Deferred,
                    section,
                    addresses(&section["delayedRecipients"], Some("emailAddress")),
                )
            }
            "Open" => {
                let section = &event["open"];
                each(
                    MailEventKind::Opened,
                    section,
                    addresses(&mail["destination"], None),
                )
            }
            "Click" => {
                let section = &event["click"];
                each(
                    MailEventKind::Clicked,
                    section,
                    addresses(&mail["destination"], None),
                )
                .into_iter()
                .map(|mut e| {
                    e.url = section["link"].as_str().map(str::to_owned);
                    e
                })
                .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The event of a Mailgun webhook (its `event-data`): `delivered`,
    /// `failed` (`severity: permanent` is a hard bounce, `temporary` a soft
    /// one), `complained`, `opened` and `clicked`. Other events give none.
    pub fn from_mailgun(payload: &Value) -> Vec<ProviderEvent> {
        let data = if payload.get("event-data").is_some() {
            &payload["event-data"]
        } else {
            payload
        };
        let (kind, permanent) = match data["event"].as_str().unwrap_or_default() {
            "delivered" => (MailEventKind::Delivered, false),
            "failed" => (
                MailEventKind::Bounced,
                data["severity"].as_str() != Some("temporary"),
            ),
            "complained" => (MailEventKind::Complained, false),
            "opened" => (MailEventKind::Opened, false),
            "clicked" => (MailEventKind::Clicked, false),
            _ => return Vec::new(),
        };
        let provider_id = data["message"]["headers"]["message-id"]
            .as_str()
            .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_owned());
        let detail = data["delivery-status"]["description"]
            .as_str()
            .filter(|d| !d.is_empty())
            .or_else(|| data["delivery-status"]["message"].as_str())
            .or_else(|| data["reason"].as_str())
            .map(str::to_owned);
        vec![ProviderEvent {
            provider: "mailgun".to_owned(),
            provider_id,
            delivery_id: None,
            kind,
            recipient: data["recipient"].as_str().map(str::to_owned),
            permanent,
            url: data["url"].as_str().map(str::to_owned),
            detail,
            occurred_at: data["timestamp"]
                .as_f64()
                .and_then(|seconds| Utc.timestamp_millis_opt((seconds * 1000.0) as i64).single()),
        }]
    }

    /// The event of a Postmark webhook: `Delivery`, `Bounce` (hard for
    /// `HardBounce`, `BadEmailAddress`, `ManuallyDeactivated` or an
    /// `Inactive` recipient), `SpamComplaint`, `Open` and `Click`. Other
    /// record types give none.
    pub fn from_postmark(payload: &Value) -> Vec<ProviderEvent> {
        let record_type = payload["RecordType"].as_str().unwrap_or_default();
        let (kind, at) = match record_type {
            "Delivery" => (MailEventKind::Delivered, "DeliveredAt"),
            "Bounce" => (MailEventKind::Bounced, "BouncedAt"),
            "SpamComplaint" => (MailEventKind::Complained, "BouncedAt"),
            "Open" => (MailEventKind::Opened, "ReceivedAt"),
            "Click" => (MailEventKind::Clicked, "ReceivedAt"),
            _ => return Vec::new(),
        };
        let permanent = record_type == "Bounce"
            && (payload["Inactive"].as_bool() == Some(true)
                || matches!(
                    payload["Type"].as_str(),
                    Some("HardBounce" | "BadEmailAddress" | "ManuallyDeactivated")
                ));
        vec![ProviderEvent {
            provider: "postmark".to_owned(),
            provider_id: payload["MessageID"].as_str().map(str::to_owned),
            delivery_id: None,
            kind,
            recipient: payload["Recipient"]
                .as_str()
                .or_else(|| payload["Email"].as_str())
                .map(str::to_owned),
            permanent,
            url: payload["OriginalLink"].as_str().map(str::to_owned),
            detail: payload["Description"]
                .as_str()
                .or_else(|| payload["Details"].as_str())
                .filter(|d| !d.is_empty())
                .map(str::to_owned),
            occurred_at: time(&payload[at]),
        }]
    }

    /// The events of a Resend webhook: `email.delivered`, `email.bounced`
    /// (`Permanent` is hard), `email.complained`, `email.delivery_delayed`,
    /// `email.opened` and `email.clicked`, one per recipient. Other types
    /// give none.
    pub fn from_resend(payload: &Value) -> Vec<ProviderEvent> {
        let data = &payload["data"];
        let (kind, permanent) = match payload["type"].as_str().unwrap_or_default() {
            "email.delivered" => (MailEventKind::Delivered, false),
            "email.bounced" => (
                MailEventKind::Bounced,
                data["bounce"]["type"].as_str() != Some("Transient"),
            ),
            "email.complained" => (MailEventKind::Complained, false),
            "email.delivery_delayed" => (MailEventKind::Deferred, false),
            "email.opened" => (MailEventKind::Opened, false),
            "email.clicked" => (MailEventKind::Clicked, false),
            _ => return Vec::new(),
        };
        let template = ProviderEvent {
            provider: "resend".to_owned(),
            provider_id: data["email_id"].as_str().map(str::to_owned),
            delivery_id: None,
            kind,
            recipient: None,
            permanent,
            url: data["click"]["link"].as_str().map(str::to_owned),
            detail: data["bounce"]["message"].as_str().map(str::to_owned),
            occurred_at: time(&payload["created_at"]),
        };
        let recipients: Vec<String> = match &data["to"] {
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            Value::String(one) => vec![one.clone()],
            _ => Vec::new(),
        };
        if recipients.is_empty() {
            return vec![template];
        }
        recipients
            .into_iter()
            .map(|recipient| ProviderEvent {
                recipient: Some(recipient),
                ..template.clone()
            })
            .collect()
    }
}

/// An RFC 3339 timestamp.
fn time(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&Utc))
}
