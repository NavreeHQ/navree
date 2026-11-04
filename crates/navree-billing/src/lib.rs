//! # navree-billing
//!
//! `BillingModule`: charge users or workspaces through Stripe or Paddle, and
//! keep a copy of their subscriptions in your database, so "is this
//! workspace on Pro?" is a local query, not an API call.
//!
//! - **Billables.** [`Billable`] is implemented for the workspace model, the
//!   auth user and their extractors (`CurrentWorkspace`, `CurrentUser`);
//!   implement it for a type of your own. Register the type you bill with
//!   [`BillingModule::for_feature`].
//! - **Plans** come from configuration (`billing.plans.<key>`, see
//!   [`BillingOptions`]): monthly and yearly price ids, trial days, seats
//!   and meters.
//! - **[`Billing`]**: [`checkout`](Billing::checkout) (a hosted checkout,
//!   with or without a trial), [`subscription`](Billing::subscription) and
//!   [`subscribed`](Billing::subscribed) (local reads),
//!   [`swap`](Billing::swap), [`update_quantity`](Billing::update_quantity),
//!   [`cancel`](Billing::cancel), [`resume`](Billing::resume),
//!   [`pause`](Billing::pause), trials without a subscription
//!   ([`start_trial`](Billing::start_trial)), metered usage
//!   ([`report_usage`](Billing::report_usage), reported by
//!   `billing:report-usage`), [`invoices`](Billing::invoices) and the
//!   customer [`portal_url`](Billing::portal_url).
//! - **[`Subscribed`]**: an extractor that answers 402 unless the request's
//!   billable has a valid subscription.
//! - **Webhooks.** `POST /api/billing/webhook` verifies `Stripe-Signature`
//!   or `Paddle-Signature` (HMAC-SHA256 with a timestamp tolerance), stores
//!   each event once in `billing_events` and processes it with the
//!   `billing.process_event` job, which updates the local copy and emits
//!   the [`events`] once its unit of work commits.
//! - **One-time payments, payment links and refunds**:
//!   [`payment`](Billing::payment), [`payment_link`](Billing::payment_link)
//!   and [`refund`](Billing::refund), with amounts as [`Money`] in the
//!   currency's smallest unit (`Money::inr(149_900)` is ₹1,499.00); their
//!   outcomes arrive as [`PaymentUpdated`] and [`RefundUpdated`] events.
//! - **Providers** ([`BillingProvider`]): an open trait. [`StripeProvider`]
//!   and [`PaddleProvider`] are narrow, typed clients on
//!   navree-http-client; [`BillingFake`] (`billing.provider = "fake"`)
//!   records checkouts, payments and usage and simulates the provider's
//!   webhooks in tests; any other provider registers with
//!   [`BillingModule::provider`] and is selected by its name. Writes carry
//!   idempotency keys, so they are retried without acting twice.
//!
//! ```no_run
//! use navree_billing::BillingModule;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     // EventsModule, QueueModule, AuthModule, WorkspaceModule, ...
//!     .module(BillingModule::from_config()) // section `billing`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod entity;
pub mod events;

mod billable;
mod billing;
mod checkout;
mod commands;
mod config;
mod controller;
mod error;
mod extract;
mod invoice;
mod migration;
mod module;
mod payment;
mod plan;
mod provider;
mod seats;
mod subscription;
mod webhook;

pub use billable::{AsBillable, Billable, BillableRef, USER_BILLABLE, WORKSPACE_BILLABLE};
pub use billing::{Billing, UsageReportSummary};
pub use checkout::{Checkout, CheckoutBuilder, CheckoutUrl};
pub use config::{
    BillingOptions, PaddleEnvironment, PaddleOptions, PlanOptions, PlanPrices, ProviderKind,
    StripeOptions,
};
pub use controller::{
    BillingController, BillingOverview, CheckoutInput, PlanPricesResource, PlanResource,
    PortalInput, SubscriptionResource, SwapInput, TrialResource, WebhookAnswer,
};
pub use entity::SUBSCRIPTION_SUBJECT;
pub use error::BillingError;
pub use events::{
    InvoicePaid, MandateUpdated, PaymentFailed, PaymentUpdated, RefundUpdated,
    SubscriptionCanceled, SubscriptionCreated, SubscriptionUpdated, TrialEnding,
};
pub use extract::Subscribed;
pub use invoice::{Invoice, InvoiceStatus};
pub use migration::BILLING_MIGRATIONS;
pub use module::BillingModule;
pub use payment::{PaymentBuilder, PaymentLinkBuilder, RefundBuilder};
pub use plan::{Interval, Plan};
pub use provider::fake::{
    BillingFake, FAKE_UNIT_AMOUNT_CENTS, FakeCheckout, FakePayment, FakePaymentLink, FakeRefund,
};
pub use provider::paddle::{PADDLE_API_VERSION, PADDLE_EVENTS, PaddleProvider};
pub use provider::payments::{KIND_KEY, REFERENCE_KEY};
pub use provider::registry::ProviderContext;
pub use provider::signature::{
    PADDLE_SIGNATURE_HEADER, STRIPE_SIGNATURE_HEADER, sign_paddle_webhook, sign_stripe_webhook,
};
pub use provider::stripe::{STRIPE_API_VERSION, STRIPE_EVENTS, StripeProvider};
pub use provider::{
    BillingProvider, Capability, CheckoutRequest, MandateStatus, Money, NewCustomer, PaymentAction,
    PaymentCustomer, PaymentLinkRequest, PaymentLinkStatus, PaymentMethod, PaymentRequest,
    PaymentStatus, ProviderCheckout, ProviderError, ProviderInvoice, ProviderItem, ProviderMandate,
    ProviderPayment, ProviderPaymentLink, ProviderRefund, ProviderSubscription, RefundReason,
    RefundRequest, RefundStatus, SubscriptionChange, UsageReport, WebhookAction, WebhookEndpoint,
    WebhookError, WebhookEvent,
};
pub use seats::SeatSync;
pub use subscription::{Proration, Subscription, SubscriptionStatus};
pub use webhook::{ProcessBillingEvent, WebhookReceipt};
