//! What the [`Webhooks`](crate::Webhooks) service takes and returns:
//! [`NewEndpoint`], [`EndpointChanges`], [`Endpoint`], [`CreatedEndpoint`],
//! [`Delivery`], [`DeliveryStatus`], [`Attempt`] and [`EventType`].

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::entity::{attempt, delivery, endpoint};

/// An endpoint to create: a URL, the event types it receives, and
/// optionally a workspace, a description and extra headers.
///
/// ```
/// use navree_webhooks::NewEndpoint;
/// # let workspace_id = uuid::Uuid::nil();
///
/// let new = NewEndpoint::new("https://hooks.lakhotia.io/navree")
///     .events(["deal.won", "deal.lost"])
///     .workspace(workspace_id)
///     .description("Revenue dashboard")
///     .header("X-Team", "revenue");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewEndpoint {
    /// The URL deliveries are posted to.
    pub url: String,
    /// The event types it receives; `*` is every type.
    pub events: Vec<String>,
    /// The owning workspace; `None` for an app-wide endpoint.
    pub workspace_id: Option<Uuid>,
    /// A note for people.
    pub description: Option<String>,
    /// Extra headers sent with every delivery.
    pub headers: BTreeMap<String, String>,
}

impl NewEndpoint {
    /// An endpoint posting to `url`.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// The event types it receives (`*` for every type).
    pub fn events<I, S>(mut self, events: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.events = events.into_iter().map(Into::into).collect();
        self
    }

    /// The owning workspace.
    pub fn workspace(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    /// A note for people.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// An extra header sent with every delivery.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Changes to an endpoint: every field left `None` stays as it is.
///
/// ```
/// use navree_webhooks::EndpointChanges;
///
/// let changes = EndpointChanges::new()
///     .url("https://hooks.lakhotia.io/v2")
///     .events(["*"]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(default)]
pub struct EndpointChanges {
    /// A new URL.
    pub url: Option<String>,
    /// New event types.
    pub events: Option<Vec<String>>,
    /// A new description; an empty one removes it.
    pub description: Option<String>,
    /// New extra headers (replacing the old ones).
    pub headers: Option<BTreeMap<String, String>>,
}

impl EndpointChanges {
    /// No changes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Changes the URL.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Changes the event types.
    pub fn events<I, S>(mut self, events: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.events = Some(events.into_iter().map(Into::into).collect());
        self
    }

    /// Changes the description (`""` removes it).
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Replaces the extra headers.
    pub fn headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }
}

/// An endpoint, as the service and the API return it (never with its
/// secret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Endpoint {
    /// The id.
    pub id: Uuid,
    /// The owning workspace; `null` for an app-wide endpoint.
    pub workspace_id: Option<Uuid>,
    /// The URL deliveries are posted to.
    pub url: String,
    /// A note for people.
    pub description: Option<String>,
    /// The subscribed event types (`*` is every type).
    pub events: Vec<String>,
    /// Extra headers sent with every delivery.
    pub headers: BTreeMap<String, String>,
    /// Whether deliveries are sent.
    pub enabled: bool,
    /// When it was disabled.
    pub disabled_at: Option<DateTime<Utc>>,
    /// Why it was disabled: `manual`, `failing` (it failed for
    /// `webhooks.disable_after_days`) or `gone` (it answered 410).
    pub disabled_reason: Option<String>,
    /// Since when every attempt has failed; `null` after a success.
    pub failing_since: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl Endpoint {
    /// Whether it receives events of type `event_type`.
    pub fn subscribes_to(&self, event_type: &str) -> bool {
        self.events.iter().any(|e| e == "*" || e == event_type)
    }
}

impl From<&endpoint::Model> for Endpoint {
    fn from(model: &endpoint::Model) -> Self {
        Self {
            id: model.id,
            workspace_id: model.workspace_id,
            url: model.url.clone(),
            description: model.description.clone(),
            events: serde_json::from_value(model.events.clone()).unwrap_or_default(),
            headers: serde_json::from_value(model.headers.clone()).unwrap_or_default(),
            enabled: model.enabled,
            disabled_at: model.disabled_at,
            disabled_reason: model.disabled_reason.clone(),
            failing_since: model.failing_since,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

/// A new endpoint with its secret, which is only ever shown here.
#[derive(Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CreatedEndpoint {
    /// The endpoint.
    #[serde(flatten)]
    pub endpoint: Endpoint,
    /// The signing secret (`whsec_...`): give it to the receiver.
    pub secret: String,
}

impl fmt::Debug for CreatedEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreatedEndpoint")
            .field("endpoint", &self.endpoint)
            .field("secret", &"[redacted]")
            .finish()
    }
}

/// Where a delivery stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = WebhookDeliveryStatus)]
#[non_exhaustive]
pub enum DeliveryStatus {
    /// Waiting for its next attempt.
    Pending,
    /// An attempt got a 2xx.
    Succeeded,
    /// It used its last attempt, its endpoint was disabled, or it answered
    /// 410.
    Failed,
}

impl DeliveryStatus {
    /// `pending`, `succeeded` or `failed`.
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryStatus::Pending => "pending",
            DeliveryStatus::Succeeded => "succeeded",
            DeliveryStatus::Failed => "failed",
        }
    }

    /// The status stored as `text` (`failed` for anything unknown).
    pub fn parse(text: &str) -> DeliveryStatus {
        match text {
            "pending" => DeliveryStatus::Pending,
            "succeeded" => DeliveryStatus::Succeeded,
            _ => DeliveryStatus::Failed,
        }
    }
}

impl fmt::Display for DeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One message to one endpoint, with where it stands.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
#[schema(as = WebhookDelivery)]
pub struct Delivery {
    /// The id.
    pub id: Uuid,
    /// The endpoint.
    pub endpoint_id: Uuid,
    /// The endpoint's workspace.
    pub workspace_id: Option<Uuid>,
    /// The `webhook-id` header (the same across retries).
    pub message_id: String,
    /// The event type.
    pub event_type: String,
    /// Where it stands.
    pub status: DeliveryStatus,
    /// Attempts made.
    pub attempts: u32,
    /// Attempts allowed.
    pub max_attempts: u32,
    /// When the next attempt is due.
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// The status code of the last attempt, if the endpoint answered.
    pub last_status_code: Option<u16>,
    /// The error of the last failed attempt.
    pub last_error: Option<String>,
    /// The body sent: `{"type", "timestamp", "data"}`.
    #[schema(value_type = Object)]
    pub payload: Value,
    /// When the message was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&delivery::Model> for Delivery {
    fn from(model: &delivery::Model) -> Self {
        Self {
            id: model.id,
            endpoint_id: model.endpoint_id,
            workspace_id: model.workspace_id,
            message_id: model.message_id.clone(),
            event_type: model.event_type.clone(),
            status: DeliveryStatus::parse(&model.status),
            attempts: u32::try_from(model.attempts).unwrap_or_default(),
            max_attempts: u32::try_from(model.max_attempts).unwrap_or_default(),
            next_attempt_at: model.next_attempt_at,
            last_status_code: model
                .last_status_code
                .and_then(|code| u16::try_from(code).ok()),
            last_error: model.last_error.clone(),
            payload: serde_json::from_str(&model.payload).unwrap_or(Value::Null),
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

/// One HTTP request of a delivery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Attempt {
    /// The id.
    pub id: Uuid,
    /// The delivery.
    pub delivery_id: Uuid,
    /// The endpoint.
    pub endpoint_id: Uuid,
    /// The attempt number, from 1.
    pub attempt: u32,
    /// The response status, if the endpoint answered.
    pub status_code: Option<u16>,
    /// Whether it succeeded (a 2xx within `webhooks.timeout_secs`).
    pub succeeded: bool,
    /// The first 4 KB of the response body.
    pub response_body: Option<String>,
    /// Why it failed without a response (timeout, refused connection, SSRF
    /// policy).
    pub error: Option<String>,
    /// How long it took, in milliseconds.
    pub duration_ms: u64,
    /// When it was made.
    pub created_at: DateTime<Utc>,
}

impl From<&attempt::Model> for Attempt {
    fn from(model: &attempt::Model) -> Self {
        Self {
            id: model.id,
            delivery_id: model.delivery_id,
            endpoint_id: model.endpoint_id,
            attempt: u32::try_from(model.attempt).unwrap_or_default(),
            status_code: model.status_code.and_then(|code| u16::try_from(code).ok()),
            succeeded: model.succeeded,
            response_body: model.response_body.clone(),
            error: model.error.clone(),
            duration_ms: u64::try_from(model.duration_ms).unwrap_or_default(),
            created_at: model.created_at,
        }
    }
}

/// A registered webhook event type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct EventType {
    /// The type (`deal.won`).
    pub name: String,
    /// What it means.
    pub description: String,
}
