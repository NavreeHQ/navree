//! The billing events, emitted through navree-events' `EventBus` once the
//! change that caused them commits, when `EventsModule` is in the app.

use chrono::{DateTime, Utc};
use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::plan::Interval;
use crate::subscription::SubscriptionStatus;

/// A subscription appeared (a checkout completed).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionCreated {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The local subscription id.
    pub subscription_id: Uuid,
    /// The plan key.
    pub plan: String,
    /// Monthly or yearly.
    pub interval: Interval,
    /// The status.
    pub status: SubscriptionStatus,
    /// Seats.
    pub quantity: u32,
}

impl Event for SubscriptionCreated {
    const NAME: &'static str = "subscription.created";
}

/// A subscription's plan, quantity, status or period changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUpdated {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The local subscription id.
    pub subscription_id: Uuid,
    /// The plan key.
    pub plan: String,
    /// Monthly or yearly.
    pub interval: Interval,
    /// The status.
    pub status: SubscriptionStatus,
    /// Seats.
    pub quantity: u32,
}

impl Event for SubscriptionUpdated {
    const NAME: &'static str = "subscription.updated";
}

/// A subscription was canceled, now or at the end of its period.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionCanceled {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The local subscription id.
    pub subscription_id: Uuid,
    /// The plan key.
    pub plan: String,
    /// When it ends (or ended).
    pub ends_at: Option<DateTime<Utc>>,
}

impl Event for SubscriptionCanceled {
    const NAME: &'static str = "subscription.canceled";
}

/// A trial with a subscription ends soon (three days before, on Stripe).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialEnding {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The local subscription id.
    pub subscription_id: Uuid,
    /// The plan key.
    pub plan: String,
    /// When the trial ends.
    pub trial_ends_at: Option<DateTime<Utc>>,
}

impl Event for TrialEnding {
    const NAME: &'static str = "subscription.trial_ending";
}

/// A payment succeeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoicePaid {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The local subscription id, when the invoice is a subscription's.
    pub subscription_id: Option<Uuid>,
    /// The subscription's plan key, when there is one.
    pub plan: Option<String>,
    /// The provider's invoice (or transaction) id.
    pub invoice_id: String,
    /// The amount paid, in the currency's smallest unit.
    pub amount_cents: i64,
    /// The currency (lower-case ISO code).
    pub currency: String,
}

impl Event for InvoicePaid {
    const NAME: &'static str = "invoice.paid";
}

/// A payment failed; the provider retries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentFailed {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The local subscription id, when the invoice is a subscription's.
    pub subscription_id: Option<Uuid>,
    /// The subscription's plan key, when there is one.
    pub plan: Option<String>,
    /// The provider's invoice (or transaction) id.
    pub invoice_id: String,
    /// The amount due, in the currency's smallest unit.
    pub amount_cents: i64,
    /// The currency (lower-case ISO code).
    pub currency: String,
}

impl Event for PaymentFailed {
    const NAME: &'static str = "invoice.payment_failed";
}

/// An event waiting for its unit of work to commit.
#[derive(Clone, Debug)]
pub(crate) enum Pending {
    Created(SubscriptionCreated),
    Updated(SubscriptionUpdated),
    Canceled(SubscriptionCanceled),
    TrialEnding(TrialEnding),
    InvoicePaid(InvoicePaid),
    PaymentFailed(PaymentFailed),
}

impl Pending {
    /// The event's name.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Pending::Created(_) => SubscriptionCreated::NAME,
            Pending::Updated(_) => SubscriptionUpdated::NAME,
            Pending::Canceled(_) => SubscriptionCanceled::NAME,
            Pending::TrialEnding(_) => TrialEnding::NAME,
            Pending::InvoicePaid(_) => InvoicePaid::NAME,
            Pending::PaymentFailed(_) => PaymentFailed::NAME,
        }
    }

    /// Emits the event on `bus`.
    pub(crate) async fn emit(
        self,
        bus: &navree_events::EventBus,
    ) -> Result<(), navree_kernel::BoxError> {
        match self {
            Pending::Created(event) => bus.emit(event).await,
            Pending::Updated(event) => bus.emit(event).await,
            Pending::Canceled(event) => bus.emit(event).await,
            Pending::TrialEnding(event) => bus.emit(event).await,
            Pending::InvoicePaid(event) => bus.emit(event).await,
            Pending::PaymentFailed(event) => bus.emit(event).await,
        }
    }
}
