//! The mail module's events, emitted through navree-events' `EventBus`
//! (when `EventsModule` is in the app) once a provider event or a tracked
//! open or click is recorded, and (crate feature `inbound`) once a message
//! is received.
//!
//! Each carries the delivery (when the message was found in the log), its
//! workspace and the recipient's address as the provider reported it.

use chrono::{DateTime, Utc};
use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The provider delivered the message to the recipient's server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDelivered {
    /// The delivery, when it is in the log.
    pub delivery_id: Option<Uuid>,
    /// Its workspace.
    pub workspace_id: Option<Uuid>,
    /// Who reported it (`ses`, `mailgun`, `postmark`, `resend`, `fake`).
    pub provider: String,
    /// The recipient.
    pub recipient: Option<String>,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl Event for MailDelivered {
    const NAME: &'static str = "mail.delivered";
}

/// The message bounced. A permanent (hard) bounce suppresses the address
/// when `mail.delivery.auto_suppress` is on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailBounced {
    /// The delivery, when it is in the log.
    pub delivery_id: Option<Uuid>,
    /// Its workspace.
    pub workspace_id: Option<Uuid>,
    /// Who reported it.
    pub provider: String,
    /// The recipient.
    pub recipient: Option<String>,
    /// A hard bounce (the address does not exist) rather than a soft one.
    pub permanent: bool,
    /// The provider's diagnostic.
    pub detail: Option<String>,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl Event for MailBounced {
    const NAME: &'static str = "mail.bounced";
}

/// The recipient marked the message as spam; the address is suppressed
/// when `mail.delivery.auto_suppress` is on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailComplained {
    /// The delivery, when it is in the log.
    pub delivery_id: Option<Uuid>,
    /// Its workspace.
    pub workspace_id: Option<Uuid>,
    /// Who reported it.
    pub provider: String,
    /// The recipient.
    pub recipient: Option<String>,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl Event for MailComplained {
    const NAME: &'static str = "mail.complained";
}

/// The recipient opened the message (open tracking, or the provider's).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailOpened {
    /// The delivery, when it is in the log.
    pub delivery_id: Option<Uuid>,
    /// Its workspace.
    pub workspace_id: Option<Uuid>,
    /// Who reported it (`tracking` for Navree's own image).
    pub provider: String,
    /// The recipient, when the provider names one.
    pub recipient: Option<String>,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl Event for MailOpened {
    const NAME: &'static str = "mail.opened";
}

/// The recipient clicked a link in the message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailClicked {
    /// The delivery, when it is in the log.
    pub delivery_id: Option<Uuid>,
    /// Its workspace.
    pub workspace_id: Option<Uuid>,
    /// Who reported it (`tracking` for Navree's own redirect).
    pub provider: String,
    /// The recipient, when the provider names one.
    pub recipient: Option<String>,
    /// The link.
    pub url: String,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl Event for MailClicked {
    const NAME: &'static str = "mail.clicked";
}

/// A message was received (crate feature `inbound`): emitted once its
/// mailbox handled it, rejected it, or no mailbox matched. It carries what
/// a listener needs to find the message, not the message itself: read the
/// raw `.eml` back with `Inbound::read(&event.stored)`.
#[cfg(feature = "inbound")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundMailReceived {
    /// The receipt's id.
    pub id: Uuid,
    /// The source (`ses`, `mailgun`, `postmark`, `raw`, `gmail`, `graph`,
    /// `test`).
    pub source: String,
    /// The provider's id for the message.
    pub provider_id: Option<String>,
    /// `Message-ID`, without angle brackets.
    pub message_id: Option<String>,
    /// The `From` address.
    pub from: Option<String>,
    /// The recipients, in the order mailboxes tried them.
    pub recipients: Vec<String>,
    /// `Subject`.
    pub subject: Option<String>,
    /// The raw message's size in bytes.
    pub size: usize,
    /// How many attachments it has.
    pub attachments: usize,
    /// The SHA-256 of the raw message, in hex.
    pub fingerprint: String,
    /// `handled`, `unrouted` or `rejected`.
    pub status: crate::inbound::InboundStatus,
    /// The mailbox pattern that matched.
    pub mailbox: Option<String>,
    /// The recipient address that matched it.
    pub address: Option<String>,
    /// Where the raw message is stored.
    pub stored: Option<crate::inbound::StoredMail>,
    /// When it was received.
    pub received_at: DateTime<Utc>,
}

#[cfg(feature = "inbound")]
impl Event for InboundMailReceived {
    const NAME: &'static str = "mail.inbound.received";
}
