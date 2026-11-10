//! The webhook pipeline's job ([`ProcessBillingEvent`]) and receipt
//! ([`WebhookReceipt`]).

use navree_contracts::{Backoff, BoxError, Job, JobContext, JobOptions};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::billing::Billing;

/// What receiving a webhook did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WebhookReceipt {
    /// The `billing_events` row (nil when a concurrent delivery stored it).
    pub event_id: Uuid,
    /// The provider's event id.
    pub provider_event_id: String,
    /// The provider's event type.
    pub event_type: String,
    /// Whether the event had been received before (it is not stored or
    /// processed again).
    pub duplicate: bool,
}

/// The job that processes a stored webhook event: `billing.process_event`,
/// on the queue named by `billing.queue`; 8 attempts with exponential
/// backoff from 10 seconds to 10 minutes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessBillingEvent {
    /// The `billing_events` row.
    pub event_id: Uuid,
}

impl Job for ProcessBillingEvent {
    const NAME: &'static str = "billing.process_event";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 8,
            backoff: Backoff::Exponential {
                base_secs: 10,
                max_secs: 600,
            },
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let billing = ctx.app.get::<Billing>().await?;
        billing.process_event(self.event_id).await?;
        Ok(())
    }
}
