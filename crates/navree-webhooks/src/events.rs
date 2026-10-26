//! The webhooks module's own events, emitted through navree-events'
//! `EventBus` (when `EventsModule` is in the app) once the change that
//! caused them is committed.

use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A delivery used its last attempt (or its endpoint answered 410, or was
/// disabled) and is now `failed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDeliveryFailed {
    /// The delivery.
    pub delivery_id: Uuid,
    /// The endpoint.
    pub endpoint_id: Uuid,
    /// The endpoint's workspace.
    pub workspace_id: Option<Uuid>,
    /// The event type.
    pub event_type: String,
    /// The attempts made.
    pub attempts: u32,
    /// The status code of the last attempt, if the endpoint answered.
    pub last_status_code: Option<u16>,
}

impl Event for WebhookDeliveryFailed {
    const NAME: &'static str = "webhook.delivery_failed";
}

/// An endpoint was disabled after failing: it failed for
/// `webhooks.disable_after_days` (`reason` `failing`) or answered
/// `410 Gone` (`reason` `gone`). Tell its owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookEndpointDisabled {
    /// The endpoint.
    pub endpoint_id: Uuid,
    /// Its workspace.
    pub workspace_id: Option<Uuid>,
    /// Its URL.
    pub url: String,
    /// `failing` or `gone`.
    pub reason: String,
}

impl Event for WebhookEndpointDisabled {
    const NAME: &'static str = "webhook.endpoint_disabled";
}
