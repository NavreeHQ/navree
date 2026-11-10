//! The mail tables as navree-db records (migration set `mail`).
//!
//! - [`TransportRow`] `mail_transports(id, tier, workspace_id,
//!   organization_id, domain, kind, config, enabled, checked_at,
//!   created_at, updated_at)`: tenant transports, `config` encrypted with
//!   the app key.
//! - [`DeliveryRow`] `mail_deliveries(id, workspace_id, organization_id,
//!   status, transport, tier, provider_id, template, subject, sender,
//!   recipient_hash, recipients, error, opened_at, clicked_at, created_at,
//!   updated_at)`: one row per message sent.
//! - [`EventRow`] `mail_events(id, delivery_id → mail_deliveries ON DELETE
//!   CASCADE, workspace_id, provider, kind, recipient_hash, bounce_type,
//!   url, detail, occurred_at, created_at)`: what providers and tracking
//!   reported.
//! - [`SuppressionRow`] `mail_suppressions(id, workspace_id, email,
//!   reason, detail, delivery_id, created_at)`: addresses mail is not sent
//!   to.
//! - [`OutboxRow`] `mail_outbox(id, workspace_id, organization_id, reason,
//!   status, subject, recipients, message, via, template, delivery_id,
//!   decided_by, decided_at, created_at, updated_at)`: mail held for
//!   approval, `message` encrypted with the app key.
//!
//! Change them through [`TransportResolver`](crate::TransportResolver),
//! [`Suppressions`](crate::Suppressions), [`Outbox`](crate::Outbox) and
//! [`Deliveries`](crate::Deliveries); read them freely with
//! [`navree_db::Record::select`] or [`navree_db::record::find`].

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

navree_db::record! {
    /// A transport stored for a domain, a workspace, an organization or
    /// the platform (`mail_transports`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct TransportRow in "mail_transports" {
        /// The id (UUID v7).
        pub id: Uuid,
        /// `domain`, `workspace`, `organization` or `platform`.
        pub tier: String,
        /// The owning workspace (`workspace` tier, or a workspace's domain).
        pub workspace_id: Option<Uuid>,
        /// The owning organization (`organization` tier, or an
        /// organization's domain).
        pub organization_id: Option<Uuid>,
        /// The sending domain (`domain` tier), lower case.
        pub domain: Option<String>,
        /// The transport type (`smtp`, `ses`, ...).
        pub kind: String,
        /// The settings as JSON, encrypted with the app key. Never
        /// serialized.
        #[serde(skip_serializing)]
        pub config: String,
        /// Whether it is used.
        pub enabled: bool,
        /// When its `usable` check last passed.
        pub checked_at: Option<DateTime<Utc>>,
        /// When it was created.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// A delivery: one message in the log (`mail_deliveries`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct DeliveryRow in "mail_deliveries" {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace it was sent for.
        pub workspace_id: Option<Uuid>,
        /// The organization it was sent for.
        pub organization_id: Option<Uuid>,
        /// `sending`, `sent`, `delivered`, `bounced`, `complained`,
        /// `failed`, `suppressed`, `held` or `discarded`.
        pub status: String,
        /// The transport (`ses`, `smtp`, ...).
        pub transport: String,
        /// Where the transport came from (`workspace`, `config`, ...).
        pub tier: String,
        /// The provider's message id.
        pub provider_id: Option<String>,
        /// The template it was rendered from.
        pub template: Option<String>,
        /// The subject line.
        pub subject: String,
        /// The sender's address.
        pub sender: String,
        /// SHA-256 (hex) of the first recipient's address, lower-cased.
        pub recipient_hash: String,
        /// How many recipients it had.
        pub recipients: i32,
        /// Why the transport refused it.
        pub error: Option<String>,
        /// When it was first opened (tracking).
        pub opened_at: Option<DateTime<Utc>>,
        /// When a link was first clicked (tracking).
        pub clicked_at: Option<DateTime<Utc>>,
        /// When it was handed to the transport.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// Something that happened to a delivery (`mail_events`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct EventRow in "mail_events" {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The delivery, when it is known.
        pub delivery_id: Option<Uuid>,
        /// The delivery's workspace.
        pub workspace_id: Option<Uuid>,
        /// Who reported it: `ses`, `mailgun`, `postmark`, `resend`,
        /// `tracking`, `fake`.
        pub provider: String,
        /// `delivered`, `bounced`, `deferred`, `complained`, `opened` or
        /// `clicked`.
        pub kind: String,
        /// SHA-256 (hex) of the recipient's address, lower-cased.
        pub recipient_hash: Option<String>,
        /// `hard` or `soft`, for bounces.
        pub bounce_type: Option<String>,
        /// The link, for clicks.
        pub url: Option<String>,
        /// The provider's diagnostic.
        pub detail: Option<String>,
        /// When it happened, by the provider's clock.
        pub occurred_at: DateTime<Utc>,
        /// When it was recorded.
        pub created_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// A suppressed address (`mail_suppressions`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct SuppressionRow in "mail_suppressions" {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace it applies to; `None` for every workspace.
        pub workspace_id: Option<Uuid>,
        /// The address, lower case.
        pub email: String,
        /// `hard_bounce`, `complaint`, `unsubscribed` or `manual`.
        pub reason: String,
        /// The provider's diagnostic or an admin's note.
        pub detail: Option<String>,
        /// The delivery that caused it.
        pub delivery_id: Option<Uuid>,
        /// When it was added.
        pub created_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// A message held for approval (`mail_outbox`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct OutboxRow in "mail_outbox" {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace it was sent for.
        pub workspace_id: Option<Uuid>,
        /// The organization it was sent for.
        pub organization_id: Option<Uuid>,
        /// Why it is held.
        pub reason: String,
        /// `held`, `released` or `discarded`.
        pub status: String,
        /// The subject line.
        pub subject: String,
        /// The recipients (a JSON array of addresses).
        pub recipients: serde_json::Value,
        /// The rendered message as JSON, encrypted with the app key. Never
        /// serialized.
        #[serde(skip_serializing)]
        pub message: String,
        /// The named transport it goes through.
        pub via: Option<String>,
        /// The template it was rendered from.
        pub template: Option<String>,
        /// The delivery, once released.
        pub delivery_id: Option<Uuid>,
        /// Who released or discarded it.
        pub decided_by: Option<Uuid>,
        /// When.
        pub decided_at: Option<DateTime<Utc>>,
        /// When it was held.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}
