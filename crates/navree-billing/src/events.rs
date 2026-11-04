//! The billing events, emitted through navree-events' `EventBus` once the
//! change that caused them commits, when `EventsModule` is in the app.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::plan::Interval;
use crate::provider::{
    MandateStatus, Money, PaymentMethod, PaymentStatus, ProviderMandate, ProviderPayment,
    ProviderRefund, RefundStatus,
};
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

/// A one-time payment, or a payment link's payment, changed state: paid,
/// failed, expired. Match it to your order by `reference`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentUpdated {
    /// The billable type, when the payment names one.
    pub billable_type: Option<String>,
    /// The billable id, when the payment names one.
    pub billable_id: Option<Uuid>,
    /// The provider (`stripe`, `paddle`, `razorpay`, ...).
    pub provider: String,
    /// The provider's id of what the payer opened (a Checkout Session, an
    /// order, a transaction).
    pub id: String,
    /// The id of the captured payment, which [`Billing::refund`](crate::Billing::refund)
    /// takes.
    pub payment_id: Option<String>,
    /// The payment link it came from, if any.
    pub link_id: Option<String>,
    /// Your reference.
    pub reference: Option<String>,
    /// Where it is now.
    pub status: PaymentStatus,
    /// The amount, in the currency's smallest unit.
    pub amount: Money,
    /// How it was paid, when the provider says.
    pub method: Option<PaymentMethod>,
    /// The payment's metadata.
    pub metadata: BTreeMap<String, String>,
}

impl PaymentUpdated {
    pub(crate) fn of(
        provider: &str,
        payment: &ProviderPayment,
        billable: Option<(String, Uuid)>,
    ) -> Self {
        let (billable_type, billable_id) = billable.unzip();
        PaymentUpdated {
            billable_type,
            billable_id,
            provider: provider.to_owned(),
            id: payment.id.clone(),
            payment_id: payment.payment_id.clone(),
            link_id: payment.link_id.clone(),
            reference: payment.reference.clone(),
            status: payment.status,
            amount: payment.amount.clone(),
            method: payment.method.clone(),
            metadata: payment.metadata.clone(),
        }
    }
}

impl Event for PaymentUpdated {
    const NAME: &'static str = "payment.updated";
}

/// A refund changed state: created, succeeded, failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefundUpdated {
    /// The billable type, when the refund names one.
    pub billable_type: Option<String>,
    /// The billable id, when the refund names one.
    pub billable_id: Option<Uuid>,
    /// The provider.
    pub provider: String,
    /// The provider's refund id.
    pub refund_id: String,
    /// The payment refunded.
    pub payment_id: Option<String>,
    /// Your refund reference.
    pub reference: Option<String>,
    /// Where it is now.
    pub status: RefundStatus,
    /// The amount refunded.
    pub amount: Money,
    /// The refund's metadata.
    pub metadata: BTreeMap<String, String>,
}

impl RefundUpdated {
    pub(crate) fn of(
        provider: &str,
        refund: &ProviderRefund,
        billable: Option<(String, Uuid)>,
    ) -> Self {
        let (billable_type, billable_id) = billable.unzip();
        RefundUpdated {
            billable_type,
            billable_id,
            provider: provider.to_owned(),
            refund_id: refund.id.clone(),
            payment_id: refund.payment_id.clone(),
            reference: refund.reference.clone(),
            status: refund.status,
            amount: refund.amount.clone(),
            metadata: refund.metadata.clone(),
        }
    }
}

impl Event for RefundUpdated {
    const NAME: &'static str = "refund.updated";
}

/// A mandate changed state: approved, paused, or revoked by the customer
/// (in their UPI app or with their bank). A revoked mandate means the next
/// recurring charge will fail: ask the customer to set up a new one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MandateUpdated {
    /// The billable type, when the mandate's customer is linked to one.
    pub billable_type: Option<String>,
    /// The billable id, when the mandate's customer is linked to one.
    pub billable_id: Option<Uuid>,
    /// The provider.
    pub provider: String,
    /// The provider's mandate id.
    pub mandate_id: String,
    /// The provider's customer.
    pub customer_id: Option<String>,
    /// The provider's subscription it pays, if any.
    pub subscription_id: Option<String>,
    /// The method it debits.
    pub method: PaymentMethod,
    /// Where it is now.
    pub status: MandateStatus,
    /// The most one debit may take.
    pub max_amount: Option<Money>,
}

impl MandateUpdated {
    pub(crate) fn of(
        provider: &str,
        mandate: &ProviderMandate,
        billable: Option<(String, Uuid)>,
    ) -> Self {
        let (billable_type, billable_id) = billable.unzip();
        MandateUpdated {
            billable_type,
            billable_id,
            provider: provider.to_owned(),
            mandate_id: mandate.id.clone(),
            customer_id: mandate.customer_id.clone(),
            subscription_id: mandate.subscription_id.clone(),
            method: mandate.method.clone(),
            status: mandate.status,
            max_amount: mandate.max_amount.clone(),
        }
    }
}

impl Event for MandateUpdated {
    const NAME: &'static str = "mandate.updated";
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
    Payment(PaymentUpdated),
    Refund(RefundUpdated),
    Mandate(MandateUpdated),
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
            Pending::Payment(_) => PaymentUpdated::NAME,
            Pending::Refund(_) => RefundUpdated::NAME,
            Pending::Mandate(_) => MandateUpdated::NAME,
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
            Pending::Payment(event) => bus.emit(event).await,
            Pending::Refund(event) => bus.emit(event).await,
            Pending::Mandate(event) => bus.emit(event).await,
        }
    }
}
