//! The webhooks module's tables, as navree-orm models
//! (`endpoint::Model::query()`, typed columns such as
//! `delivery::Model::EVENT_TYPE`).
//!
//! - `webhook_endpoints(id, workspace_id, url, description, events JSON,
//!   headers JSON, secret, previous_secret, previous_secret_expires_at,
//!   enabled, disabled_at, disabled_reason, failing_since, created_at,
//!   updated_at)`
//! - `webhook_deliveries(id, endpoint_id → webhook_endpoints ON DELETE
//!   CASCADE, workspace_id, message_id, event_type, payload, status,
//!   attempts, max_attempts, next_attempt_at, last_status_code, last_error,
//!   created_at, updated_at)`
//! - `webhook_attempts(id, delivery_id, endpoint_id → webhook_endpoints ON
//!   DELETE CASCADE, attempt, status_code, succeeded, response_body, error,
//!   duration_ms, created_at)`
//! - `webhook_receipts(id, source, message_id, received_at)`, unique per
//!   (source, message_id)
//!
//! Deliveries and attempts are time series: on TimescaleDB they are
//! hypertables on `created_at`, whose primary keys are `(id, created_at)`.
//! So no foreign key points at a delivery: an attempt's `delivery_id` is
//! indexed, and `webhooks:prune` deletes a delivery's attempts with it.
//!
//! Secrets are encrypted with the app key when the app has one, never
//! serialized and never part of a change event. Change endpoints through
//! [`Webhooks`](crate::Webhooks), which records them without their secrets;
//! read the tables freely.

/// `webhook_endpoints`: where webhooks are sent.
pub mod endpoint {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use serde_json::Value;
    use uuid::Uuid;

    /// An endpoint.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "webhook_endpoints")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace that owns it; `None` for an app-wide endpoint.
        pub workspace_id: Option<Uuid>,
        /// The URL deliveries are posted to.
        pub url: String,
        /// A note for people.
        pub description: Option<String>,
        /// The subscribed event types (a JSON array; `*` is every type).
        pub events: Value,
        /// Extra request headers (a JSON object of strings).
        pub headers: Value,
        /// The signing secret, encrypted with the app key when there is one.
        /// Never serialized.
        #[serde(skip_serializing)]
        #[model(hidden)]
        pub secret: String,
        /// The secret before the last rotation, still signing until
        /// `previous_secret_expires_at`. Never serialized.
        #[serde(skip_serializing)]
        #[model(hidden)]
        pub previous_secret: Option<String>,
        /// When the previous secret stops signing.
        pub previous_secret_expires_at: Option<DateTime<Utc>>,
        /// Whether deliveries are sent.
        pub enabled: bool,
        /// When it was disabled.
        pub disabled_at: Option<DateTime<Utc>>,
        /// Why: `manual`, `failing` or `gone`.
        pub disabled_reason: Option<String>,
        /// Since when every attempt has failed (cleared by a success).
        pub failing_since: Option<DateTime<Utc>>,
        /// When it was created.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// `webhook_deliveries`: one message to one endpoint.
pub mod delivery {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A delivery.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "webhook_deliveries")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The endpoint (`webhook_endpoints.id`, `ON DELETE CASCADE`).
        pub endpoint_id: Uuid,
        /// The endpoint's workspace.
        pub workspace_id: Option<Uuid>,
        /// The `webhook-id` header: the same for every endpoint and attempt
        /// of one message.
        pub message_id: String,
        /// The event type (`deal.won`).
        pub event_type: String,
        /// The exact body that is signed and sent.
        pub payload: String,
        /// `pending`, `succeeded` or `failed`.
        pub status: String,
        /// Attempts made.
        pub attempts: i32,
        /// Attempts allowed before it fails.
        pub max_attempts: i32,
        /// When the next attempt is due (pending deliveries).
        pub next_attempt_at: Option<DateTime<Utc>>,
        /// The status code of the last attempt, if the endpoint answered.
        pub last_status_code: Option<i32>,
        /// The error of the last failed attempt.
        pub last_error: Option<String>,
        /// When the message was created.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// `webhook_attempts`: one HTTP request of a delivery.
pub mod attempt {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// An attempt.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "webhook_attempts")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The delivery (`webhook_deliveries.id`; no foreign key:
        /// deliveries are a hypertable on TimescaleDB, pruning deletes the
        /// attempts).
        pub delivery_id: Uuid,
        /// The endpoint (`webhook_endpoints.id`, `ON DELETE CASCADE`).
        pub endpoint_id: Uuid,
        /// The attempt number, from 1.
        pub attempt: i32,
        /// The response status, if the endpoint answered.
        pub status_code: Option<i32>,
        /// Whether it succeeded (a 2xx in time).
        pub succeeded: bool,
        /// The first 4 KB of the response body.
        pub response_body: Option<String>,
        /// Why it failed, when it did without a response (timeout, refused).
        pub error: Option<String>,
        /// How long it took, in milliseconds.
        pub duration_ms: i64,
        /// When it was made.
        pub created_at: DateTime<Utc>,
    }
}

/// `webhook_receipts`: incoming messages already handled
/// ([`mark_processed`](crate::VerifiedWebhook::mark_processed)).
pub mod receipt {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A receipt.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "webhook_receipts")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The source name (`github`).
        pub source: String,
        /// The sender's message id.
        pub message_id: String,
        /// When it was handled.
        pub received_at: DateTime<Utc>,
    }
}
