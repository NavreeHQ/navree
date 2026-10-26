//! The queue jobs: [`FanOut`] (`webhooks.fan_out`) and [`Deliver`]
//! (`webhooks.deliver`), and the retry schedule.

use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::{BoxError, Job, JobContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::Webhooks;

/// When each attempt of a delivery runs, counted from the previous one:
/// at once, then 5 seconds, 5 minutes, 30 minutes, 2 hours, 5 hours, 10
/// hours and 10 hours (eight attempts over about 27 hours).
pub const RETRY_SCHEDULE: [Duration; 8] = [
    Duration::ZERO,
    Duration::from_secs(5),
    Duration::from_secs(5 * 60),
    Duration::from_secs(30 * 60),
    Duration::from_secs(2 * 60 * 60),
    Duration::from_secs(5 * 60 * 60),
    Duration::from_secs(10 * 60 * 60),
    Duration::from_secs(10 * 60 * 60),
];

/// The attempts a delivery gets before it is marked failed.
pub const MAX_ATTEMPTS: u32 = RETRY_SCHEDULE.len() as u32;

/// The wait before attempt `attempt + 1` once attempt `attempt` (from 1)
/// failed; `None` after the last one.
pub fn retry_delay(attempt: u32) -> Option<Duration> {
    RETRY_SCHEDULE.get(attempt as usize).copied()
}

/// The job that turns one event (or one [`dispatch_json`](Webhooks::dispatch_json)
/// call) into deliveries: it builds the payload once, stores a delivery
/// for every enabled endpoint subscribed to the type in the event's
/// workspace, and queues a [`Deliver`] job for each.
///
/// Assert on it with a `QueueFake` to check that an event fans out without
/// sending anything:
///
/// ```ignore
/// fake.assert_dispatched::<FanOut>(|job| job.event_type == "deal.won");
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FanOut {
    /// The event type (`deal.won`).
    pub event_type: String,
    /// The workspace whose endpoints receive it; `None` for app-wide ones.
    pub workspace_id: Option<Uuid>,
    /// The `webhook-id` of the message (`msg_...`).
    pub message_id: String,
    /// When it happened: the payload's `timestamp`.
    pub timestamp: DateTime<Utc>,
    /// The event as JSON, when the payload is built from a
    /// [`WebhookEvent`](crate::WebhookEvent).
    pub event: Option<Value>,
    /// The payload's `data`, when it was dispatched by name.
    pub data: Option<Value>,
}

impl Job for FanOut {
    const NAME: &'static str = "webhooks.fan_out";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let webhooks = ctx.app.get::<Webhooks>().await?;
        webhooks.fan_out(self).await?;
        Ok(())
    }
}

/// The job that makes one attempt of a delivery and, when it fails,
/// queues the next one after [`RETRY_SCHEDULE`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deliver {
    /// The delivery.
    pub delivery_id: Uuid,
}

impl Job for Deliver {
    const NAME: &'static str = "webhooks.deliver";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let webhooks = ctx.app.get::<Webhooks>().await?;
        webhooks.deliver(self.delivery_id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_is_eight_attempts_over_about_27_hours() {
        assert_eq!(MAX_ATTEMPTS, 8);
        assert_eq!(retry_delay(1), Some(Duration::from_secs(5)));
        assert_eq!(retry_delay(2), Some(Duration::from_secs(300)));
        assert_eq!(retry_delay(7), Some(Duration::from_secs(36_000)));
        assert_eq!(retry_delay(8), None);
        let total: Duration = RETRY_SCHEDULE.iter().sum();
        assert_eq!(total.as_secs() / 3600, 27);
    }
}
