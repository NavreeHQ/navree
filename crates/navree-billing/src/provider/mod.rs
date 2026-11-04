//! [`BillingProvider`]: the narrow, typed client of a payment provider, and
//! the provider-neutral types it speaks.
//!
//! The trait is open: any crate or app implements it and registers it with
//! [`BillingModule::provider`](crate::BillingModule::provider). Three
//! implementations ship: [`StripeProvider`](crate::StripeProvider) and
//! [`PaddleProvider`](crate::PaddleProvider) over navree-http-client, and
//! the in-memory fake behind [`BillingFake`](crate::BillingFake). Every one
//! maps the provider's objects to the same [`ProviderSubscription`],
//! [`ProviderInvoice`], [`ProviderPayment`], [`ProviderRefund`] and
//! [`WebhookEvent`] values, which is all [`Billing`](crate::Billing) reads.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use http::HeaderMap;
use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::invoice::Invoice;
use crate::plan::Interval;
use crate::subscription::{Proration, SubscriptionStatus};

pub(crate) mod fake;
pub(crate) mod paddle;
pub(crate) mod payments;
pub(crate) mod registry;
pub(crate) mod signature;
pub(crate) mod stripe;

pub use payments::{
    Capability, MandateStatus, Money, PaymentAction, PaymentCustomer, PaymentLinkRequest,
    PaymentLinkStatus, PaymentMethod, PaymentRequest, PaymentStatus, ProviderMandate,
    ProviderPayment, ProviderPaymentLink, ProviderRefund, RefundReason, RefundRequest,
    RefundStatus,
};

/// A future that fails at once with [`ProviderError::Unsupported`]: the
/// default of every operation a provider does not implement.
fn unsupported<'a, T: Send + 'a>(
    provider: &'static str,
    operation: &'static str,
) -> BoxFuture<'a, Result<T, ProviderError>> {
    Box::pin(std::future::ready(Err(ProviderError::Unsupported {
        provider,
        operation,
    })))
}

/// An error from a provider call.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// The provider answered with an error status.
    #[error("{provider} answered {status}: {message}")]
    Api {
        /// `stripe`, `paddle` or `fake`.
        provider: &'static str,
        /// The HTTP status.
        status: u16,
        /// The provider's error message.
        message: String,
    },
    /// The request did not reach the provider, or its answer did not.
    #[error("{provider} request failed: {message}")]
    Transport {
        /// `stripe`, `paddle` or `fake`.
        provider: &'static str,
        /// What went wrong.
        message: String,
    },
    /// The provider's answer or event is not what the client expects.
    #[error("unexpected {provider} payload: {message}")]
    Decode {
        /// `stripe`, `paddle` or `fake`.
        provider: &'static str,
        /// What is missing or malformed.
        message: String,
    },
    /// The provider does not support the operation.
    #[error("{provider} does not support {operation}")]
    Unsupported {
        /// `stripe`, `paddle` or `fake`.
        provider: &'static str,
        /// The operation.
        operation: &'static str,
    },
}

impl ProviderError {
    pub(crate) fn decode(provider: &'static str, message: impl Into<String>) -> Self {
        ProviderError::Decode {
            provider,
            message: message.into(),
        }
    }
}

/// Why a webhook was refused (answered 400).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WebhookError {
    /// No signature header.
    #[error("the webhook has no `{0}` header")]
    MissingSignature(&'static str),
    /// The signature header cannot be parsed.
    #[error("the `{0}` header is malformed")]
    MalformedSignature(&'static str),
    /// No signature in the header matches the body.
    #[error("the webhook signature does not match")]
    SignatureMismatch,
    /// The signed timestamp is outside the tolerance.
    #[error("the webhook timestamp is outside the tolerance")]
    Expired,
    /// No signing secret is configured.
    #[error("no webhook signing secret is configured")]
    NoSecret,
}

/// A customer to create at the provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewCustomer {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The customer name.
    pub name: String,
    /// The billing email, if known.
    pub email: Option<String>,
    /// Makes a repeated request create one customer, where the provider
    /// supports keys. [`Billing`](crate::Billing) derives it from the
    /// billable, the name and the email.
    pub idempotency_key: String,
}

/// A hosted checkout to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutRequest {
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The provider's customer, when one exists.
    pub customer_id: Option<String>,
    /// The email to prefill when there is no customer email.
    pub customer_email: Option<String>,
    /// The plan key.
    pub plan: String,
    /// Monthly or yearly.
    pub interval: Interval,
    /// The plan's base price for the chosen interval.
    pub price_id: String,
    /// The plan's metered prices.
    pub metered_prices: Vec<String>,
    /// Seats.
    pub quantity: u32,
    /// Trial days (none, or at least 1).
    pub trial_days: Option<u32>,
    /// A coupon or promotion code.
    pub coupon: Option<String>,
    /// Lets the customer type a promotion code.
    pub allow_promotion_codes: bool,
    /// Where the customer returns after paying.
    pub success_url: String,
    /// Where the customer returns when they leave the checkout.
    pub cancel_url: String,
    /// Stored on the provider's subscription (the billable and plan keys
    /// are added).
    pub metadata: BTreeMap<String, String>,
    /// Makes a repeated request create one checkout, where the provider
    /// supports keys. [`Billing`](crate::Billing) derives it from the
    /// billable, the plan and an attempt id (or takes the one given to
    /// [`CheckoutBuilder::idempotency_key`](crate::CheckoutBuilder::idempotency_key)).
    pub idempotency_key: String,
}

/// A created hosted checkout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCheckout {
    /// The provider's checkout (session or transaction) id.
    pub id: String,
    /// The hosted page to redirect to.
    pub url: String,
}

/// One price of a provider subscription.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderItem {
    /// The provider's item id (Stripe `si_...`; none on Paddle).
    pub id: Option<String>,
    /// The price id.
    pub price_id: String,
    /// The quantity (none for metered prices).
    pub quantity: Option<u64>,
}

/// A subscription as the provider reports it, in provider-neutral terms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSubscription {
    /// The provider's subscription id.
    pub id: String,
    /// The provider's customer id.
    pub customer_id: String,
    /// The status.
    pub status: SubscriptionStatus,
    /// The prices.
    pub items: Vec<ProviderItem>,
    /// When the trial ends.
    pub trial_ends_at: Option<DateTime<Utc>>,
    /// The start of the current period.
    pub current_period_start: Option<DateTime<Utc>>,
    /// The end of the current period.
    pub current_period_end: Option<DateTime<Utc>>,
    /// When it ends, once canceled (now or at the period's end).
    pub ends_at: Option<DateTime<Utc>>,
    /// The metadata (`billable_type`, `billable_id`, `plan`, ...).
    pub metadata: BTreeMap<String, String>,
}

impl ProviderSubscription {
    /// The billable named by the metadata, if any.
    pub fn billable(&self) -> Option<(String, Uuid)> {
        billable_of(&self.metadata)
    }
}

/// The billable named by `billable_type` and `billable_id` metadata.
pub(crate) fn billable_of(metadata: &BTreeMap<String, String>) -> Option<(String, Uuid)> {
    let kind = metadata.get("billable_type")?.clone();
    let id = metadata.get("billable_id")?.parse().ok()?;
    Some((kind, id))
}

/// An invoice (or Paddle transaction) a webhook reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInvoice {
    /// The provider's invoice (or transaction) id.
    pub id: String,
    /// The provider's customer id.
    pub customer_id: Option<String>,
    /// The provider's subscription id.
    pub subscription_id: Option<String>,
    /// The amount, in the currency's smallest unit.
    pub amount_cents: i64,
    /// The currency (lower-case ISO code).
    pub currency: String,
}

/// A plan or quantity change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionChange {
    /// The provider's id of the base item, when the provider has item ids.
    pub base_item_id: Option<String>,
    /// The new base price.
    pub price_id: String,
    /// The new quantity.
    pub quantity: u32,
    /// Metered prices to add.
    pub add_prices: Vec<String>,
    /// Provider item ids of metered prices to remove.
    pub remove_item_ids: Vec<String>,
    /// How the change is charged.
    pub proration: Proration,
}

/// Usage to report to the provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageReport {
    /// The provider's customer id.
    pub customer_id: String,
    /// The provider's subscription id.
    pub subscription_id: String,
    /// The meter key.
    pub meter: String,
    /// The metered price id.
    pub price_id: String,
    /// The total to report.
    pub quantity: u64,
    /// A key that makes a repeated report a no-op where the provider
    /// supports it.
    pub idempotency_key: String,
    /// When the usage happened (the latest recorded row).
    pub timestamp: DateTime<Utc>,
}

/// A webhook endpoint created by `billing:webhook`. `Debug` never shows the
/// signing secret.
#[derive(Clone, PartialEq, Eq)]
pub struct WebhookEndpoint {
    /// The provider's endpoint id.
    pub id: String,
    /// The signing secret, when the provider returns it.
    pub secret: Option<String>,
    /// The events it receives.
    pub events: Vec<String>,
}

impl std::fmt::Debug for WebhookEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookEndpoint")
            .field("id", &self.id)
            .field("secret", &self.secret.as_ref().map(|_| "[redacted]"))
            .field("events", &self.events)
            .finish()
    }
}

/// What a webhook event means for the local copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookAction {
    /// A provider customer belongs to a billable (a checkout completed).
    CustomerLinked {
        /// The provider's customer id.
        customer_id: String,
        /// The billable, from the checkout's metadata.
        billable: Option<(String, Uuid)>,
        /// The email the customer paid with.
        email: Option<String>,
    },
    /// A subscription's current state.
    Subscription(ProviderSubscription),
    /// A trial ends soon; carries the subscription's current state.
    TrialEnding(ProviderSubscription),
    /// A payment succeeded.
    InvoicePaid(ProviderInvoice),
    /// A payment failed.
    PaymentFailed(ProviderInvoice),
    /// A one-time payment (or a payment link's payment) changed state.
    Payment(ProviderPayment),
    /// A refund changed state.
    Refund(ProviderRefund),
    /// A mandate changed state: approved, paused, or revoked by the
    /// customer in their UPI app or with their bank.
    Mandate(ProviderMandate),
}

/// A verified webhook, parsed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// The provider's event id.
    pub id: String,
    /// The provider's event type.
    pub event_type: String,
    /// When the provider created the event.
    pub occurred_at: DateTime<Utc>,
    /// What it means (empty for events Navree does not use).
    pub actions: Vec<WebhookAction>,
}

/// A payment provider's API, as [`Billing`](crate::Billing) uses it.
///
/// The trait is open: implement it for any provider and register it with
/// [`BillingModule::provider`](crate::BillingModule::provider), then select
/// it with `billing.provider = "<name>"`. Stripe, Paddle and the fake
/// implement it the same way.
///
/// Every method returns provider-neutral values. Only [`name`](Self::name),
/// [`verify_webhook`](Self::verify_webhook) and
/// [`parse_webhook`](Self::parse_webhook) are required: every other
/// operation answers [`ProviderError::Unsupported`] unless the provider
/// implements it, and [`capabilities`](Self::capabilities) lists what it
/// implements.
///
/// - **Subscriptions**: customers, hosted checkouts, plan and quantity
///   changes, cancel, resume, pause, usage, invoices, the portal.
/// - **Payments**: one-time payments
///   ([`create_payment`](Self::create_payment)), payment links
///   ([`create_payment_link`](Self::create_payment_link)) and refunds
///   ([`refund`](Self::refund)), with amounts as [`Money`] in the
///   currency's smallest unit (paise for rupees) and the methods to offer
///   ([`PaymentMethod::Upi`], cards, net banking, ...).
/// - **Webhooks**: each provider checks its own signature scheme in
///   [`verify_webhook`](Self::verify_webhook) and maps its events to
///   [`WebhookAction`]s, mandates included.
/// - **Idempotency**: the requests that create something carry an
///   `idempotency_key`. A provider that honours keys
///   ([`Capability::IdempotencyKeys`]) sends it, and gives every other
///   write a key of its own, so the HTTP client retries writes without
///   acting twice.
pub trait BillingProvider: Send + Sync + 'static {
    /// The provider's name, as `billing.provider` selects it and the
    /// billing tables store it (`stripe`, `paddle`, `fake`, `razorpay`).
    fn name(&self) -> &'static str;

    /// What the provider can do; the operations it does not list answer
    /// [`ProviderError::Unsupported`]. Default: nothing beyond webhooks.
    fn capabilities(&self) -> &'static [Capability] {
        &[]
    }

    /// Creates a customer; `None` when the provider needs more (Paddle needs
    /// an email) or has no customers, and the checkout creates the customer
    /// instead. Default: `None`.
    fn create_customer<'a>(
        &'a self,
        customer: &'a NewCustomer,
    ) -> BoxFuture<'a, Result<Option<String>, ProviderError>> {
        let _ = customer;
        Box::pin(std::future::ready(Ok(None)))
    }

    /// Creates a hosted checkout for a plan.
    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<ProviderCheckout, ProviderError>> {
        let _ = request;
        unsupported(self.name(), "subscriptions")
    }

    /// Changes a subscription's price or quantity.
    fn update_subscription<'a>(
        &'a self,
        id: &'a str,
        change: &'a SubscriptionChange,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        let _ = (id, change);
        unsupported(self.name(), "subscriptions")
    }

    /// Cancels a subscription at the end of its period, or at once.
    fn cancel_subscription<'a>(
        &'a self,
        id: &'a str,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        let _ = (id, at_period_end);
        unsupported(self.name(), "subscriptions")
    }

    /// Undoes a cancel at the end of the period.
    fn resume_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        let _ = id;
        unsupported(self.name(), "resuming subscriptions")
    }

    /// Stops charging a subscription, keeping it.
    fn pause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        let _ = id;
        unsupported(self.name(), "pausing subscriptions")
    }

    /// Starts charging a paused subscription again.
    fn unpause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        let _ = id;
        unsupported(self.name(), "pausing subscriptions")
    }

    /// Every subscription of a customer. Default: none.
    fn subscriptions<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ProviderSubscription>, ProviderError>> {
        let _ = customer_id;
        Box::pin(std::future::ready(Ok(Vec::new())))
    }

    /// Reports metered usage.
    fn report_usage<'a>(
        &'a self,
        report: &'a UsageReport,
    ) -> BoxFuture<'a, Result<(), ProviderError>> {
        let _ = report;
        unsupported(self.name(), "metered usage")
    }

    /// A customer's invoices, newest first. Default: none.
    fn invoices<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Invoice>, ProviderError>> {
        let _ = customer_id;
        Box::pin(std::future::ready(Ok(Vec::new())))
    }

    /// A customer portal session's URL.
    fn portal_url<'a>(
        &'a self,
        customer_id: &'a str,
        return_url: &'a str,
        subscription_ids: &'a [String],
    ) -> BoxFuture<'a, Result<String, ProviderError>> {
        let _ = (customer_id, return_url, subscription_ids);
        unsupported(self.name(), "the customer portal")
    }

    /// Whether the price exists at the provider.
    fn price_exists<'a>(&'a self, price_id: &'a str) -> BoxFuture<'a, Result<bool, ProviderError>> {
        let _ = price_id;
        unsupported(self.name(), "price lookups")
    }

    /// Creates a webhook endpoint for the events Navree handles.
    fn create_webhook_endpoint<'a>(
        &'a self,
        url: &'a str,
    ) -> BoxFuture<'a, Result<WebhookEndpoint, ProviderError>> {
        let _ = url;
        unsupported(self.name(), "creating webhook endpoints")
    }

    /// Creates a one-time payment and says what the payer does next
    /// ([`ProviderPayment::next_action`]): open a hosted page, run the
    /// provider's checkout script with a token, or open a UPI app. Its
    /// outcome arrives by webhook as [`WebhookAction::Payment`].
    fn create_payment<'a>(
        &'a self,
        request: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<ProviderPayment, ProviderError>> {
        let _ = request;
        unsupported(self.name(), "one-time payments")
    }

    /// Creates a payment link: a page that takes one payment. Its payment
    /// arrives by webhook as [`WebhookAction::Payment`] with
    /// [`ProviderPayment::link_id`] set.
    fn create_payment_link<'a>(
        &'a self,
        request: &'a PaymentLinkRequest,
    ) -> BoxFuture<'a, Result<ProviderPaymentLink, ProviderError>> {
        let _ = request;
        unsupported(self.name(), "payment links")
    }

    /// Refunds a payment, in whole or in part. Later changes arrive by
    /// webhook as [`WebhookAction::Refund`].
    fn refund<'a>(
        &'a self,
        request: &'a RefundRequest,
    ) -> BoxFuture<'a, Result<ProviderRefund, ProviderError>> {
        let _ = request;
        unsupported(self.name(), "refunds")
    }

    /// The id of a webhook delivery when the provider sends it in a header
    /// instead of the body (Razorpay's `X-Razorpay-Event-Id`): it is what
    /// makes a redelivery a duplicate. `None` (the default) uses
    /// [`WebhookEvent::id`].
    fn webhook_event_id(&self, headers: &HeaderMap) -> Option<String> {
        let _ = headers;
        None
    }

    /// Checks a webhook's signature header against its raw body.
    fn verify_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: DateTime<Utc>,
        tolerance: Duration,
    ) -> Result<(), WebhookError>;

    /// Parses a verified webhook body.
    fn parse_webhook(&self, body: &[u8]) -> Result<WebhookEvent, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_created_webhook_endpoint_never_shows_its_secret_in_debug_output() {
        let endpoint = WebhookEndpoint {
            id: "we_1Q2w3E4r".to_owned(),
            secret: Some("whsec_b1ll1ng5ecret".to_owned()),
            events: vec!["invoice.paid".to_owned()],
        };
        let printed = format!("{endpoint:?}");
        assert!(!printed.contains("b1ll1ng5ecret"), "{printed}");
        assert!(printed.contains("we_1Q2w3E4r"), "{printed}");
    }
}
