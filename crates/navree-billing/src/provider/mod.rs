//! [`BillingProvider`]: the narrow, typed client of a payment provider, and
//! the provider-neutral types it speaks.
//!
//! Three implementations: [`StripeProvider`](crate::StripeProvider) and
//! [`PaddleProvider`](crate::PaddleProvider) over navree-http-client, and
//! the in-memory fake behind [`BillingFake`](crate::BillingFake). Every one
//! maps the provider's objects to the same [`ProviderSubscription`],
//! [`ProviderInvoice`] and [`WebhookEvent`] values, which is all
//! [`Billing`](crate::Billing) reads.

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
pub(crate) mod signature;
pub(crate) mod stripe;

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

/// A webhook endpoint created by `billing:webhook`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebhookEndpoint {
    /// The provider's endpoint id.
    pub id: String,
    /// The signing secret, when the provider returns it.
    pub secret: Option<String>,
    /// The events it receives.
    pub events: Vec<String>,
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
/// Implemented by the Stripe and Paddle clients and the fake. Every method
/// returns provider-neutral values; a provider that has no equivalent for an
/// operation returns [`ProviderError::Unsupported`].
pub trait BillingProvider: Send + Sync + 'static {
    /// `stripe`, `paddle` or `fake`.
    fn name(&self) -> &'static str;

    /// Creates a customer; `None` when the provider needs more (Paddle needs
    /// an email) and the checkout creates the customer instead.
    fn create_customer<'a>(
        &'a self,
        customer: &'a NewCustomer,
    ) -> BoxFuture<'a, Result<Option<String>, ProviderError>>;

    /// Creates a hosted checkout.
    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<ProviderCheckout, ProviderError>>;

    /// Changes a subscription's price or quantity.
    fn update_subscription<'a>(
        &'a self,
        id: &'a str,
        change: &'a SubscriptionChange,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>>;

    /// Cancels a subscription at the end of its period, or at once.
    fn cancel_subscription<'a>(
        &'a self,
        id: &'a str,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>>;

    /// Undoes a cancel at the end of the period.
    fn resume_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>>;

    /// Stops charging a subscription, keeping it.
    fn pause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>>;

    /// Starts charging a paused subscription again.
    fn unpause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>>;

    /// Every subscription of a customer.
    fn subscriptions<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ProviderSubscription>, ProviderError>>;

    /// Reports metered usage.
    fn report_usage<'a>(
        &'a self,
        report: &'a UsageReport,
    ) -> BoxFuture<'a, Result<(), ProviderError>>;

    /// A customer's invoices, newest first.
    fn invoices<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Invoice>, ProviderError>>;

    /// A customer portal session's URL.
    fn portal_url<'a>(
        &'a self,
        customer_id: &'a str,
        return_url: &'a str,
        subscription_ids: &'a [String],
    ) -> BoxFuture<'a, Result<String, ProviderError>>;

    /// Whether the price exists at the provider.
    fn price_exists<'a>(&'a self, price_id: &'a str) -> BoxFuture<'a, Result<bool, ProviderError>>;

    /// Creates a webhook endpoint for the events Navree handles.
    fn create_webhook_endpoint<'a>(
        &'a self,
        url: &'a str,
    ) -> BoxFuture<'a, Result<WebhookEndpoint, ProviderError>>;

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
