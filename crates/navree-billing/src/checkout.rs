//! [`CheckoutBuilder`], [`Checkout`] and [`CheckoutUrl`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::billable::BillableRef;
use crate::billing::Billing;
use crate::error::BillingError;
use crate::plan::Interval;

/// A hosted checkout being configured: from
/// [`Billing::checkout`](crate::Billing::checkout).
///
/// | Method | Effect |
/// |---|---|
/// | [`yearly`](Self::yearly), [`monthly`](Self::monthly) | Which of the plan's prices to charge (monthly by default) |
/// | [`trial_days`](Self::trial_days) | Overrides the plan's `trial_days`; `0` skips the trial |
/// | [`quantity`](Self::quantity) | Seats; with `seats = true` the default is the workspace's member count |
/// | [`coupon`](Self::coupon) | A provider coupon or promotion code |
/// | [`allow_promotion_codes`](Self::allow_promotion_codes) | Lets the customer type a code on the checkout page |
/// | [`customer_email`](Self::customer_email) | Prefills the email when the billable has none |
/// | [`success_url`](Self::success_url), [`cancel_url`](Self::cancel_url) | Where the customer returns; both default to `billing.return_url` |
/// | [`metadata`](Self::metadata) | Stored on the provider's subscription |
#[must_use = "a checkout does nothing until `create` is awaited"]
pub struct CheckoutBuilder<'a> {
    billing: &'a Billing,
    parts: CheckoutParts,
}

/// The settings of a checkout.
pub(crate) struct CheckoutParts {
    pub(crate) billable: BillableRef,
    pub(crate) plan: String,
    pub(crate) interval: Interval,
    pub(crate) trial_days: Option<u32>,
    pub(crate) quantity: Option<u32>,
    pub(crate) coupon: Option<String>,
    pub(crate) allow_promotion_codes: bool,
    pub(crate) customer_email: Option<String>,
    pub(crate) success_url: Option<String>,
    pub(crate) cancel_url: Option<String>,
    pub(crate) metadata: BTreeMap<String, String>,
}

impl<'a> CheckoutBuilder<'a> {
    pub(crate) fn new(billing: &'a Billing, billable: BillableRef, plan: &str) -> Self {
        CheckoutBuilder {
            billing,
            parts: CheckoutParts {
                billable,
                plan: plan.to_owned(),
                interval: Interval::Monthly,
                trial_days: None,
                quantity: None,
                coupon: None,
                allow_promotion_codes: false,
                customer_email: None,
                success_url: None,
                cancel_url: None,
                metadata: BTreeMap::new(),
            },
        }
    }

    pub(crate) fn into_parts(self) -> CheckoutParts {
        self.parts
    }

    /// Charges the plan's yearly price.
    pub fn yearly(mut self) -> Self {
        self.parts.interval = Interval::Yearly;
        self
    }

    /// Charges the plan's monthly price (the default).
    pub fn monthly(mut self) -> Self {
        self.parts.interval = Interval::Monthly;
        self
    }

    /// Charges the plan's price for `interval`.
    pub fn interval(mut self, interval: Interval) -> Self {
        self.parts.interval = interval;
        self
    }

    /// Overrides the plan's `trial_days`; `0` skips the trial.
    pub fn trial_days(mut self, days: u32) -> Self {
        self.parts.trial_days = Some(days);
        self
    }

    /// Seats (at least 1); with `seats = true` the default is the
    /// workspace's member count, otherwise 1.
    pub fn quantity(mut self, quantity: u32) -> Self {
        self.parts.quantity = Some(quantity);
        self
    }

    /// A provider coupon or promotion code (Stripe: a coupon id, or a
    /// `promo_...` promotion code id; Paddle: a discount id).
    pub fn coupon(mut self, code: impl Into<String>) -> Self {
        self.parts.coupon = Some(code.into());
        self
    }

    /// Lets the customer type a promotion code on the checkout page
    /// (Stripe; ignored with a [`coupon`](Self::coupon)).
    pub fn allow_promotion_codes(mut self) -> Self {
        self.parts.allow_promotion_codes = true;
        self
    }

    /// Prefills the email when the billable has none
    /// (`Billable::billing_email` returns `None`).
    pub fn customer_email(mut self, email: impl Into<String>) -> Self {
        self.parts.customer_email = Some(email.into());
        self
    }

    /// Where the customer returns after paying (default
    /// `billing.return_url`). Not proof of payment: wait for the webhook.
    pub fn success_url(mut self, url: impl Into<String>) -> Self {
        self.parts.success_url = Some(url.into());
        self
    }

    /// Where the customer returns when they leave the checkout (default
    /// `billing.return_url`).
    pub fn cancel_url(mut self, url: impl Into<String>) -> Self {
        self.parts.cancel_url = Some(url.into());
        self
    }

    /// Stored on the provider's subscription (and checkout).
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parts.metadata.insert(key.into(), value.into());
        self
    }

    /// Creates the checkout at the provider (creating the billable's
    /// customer first if it has none) and returns its hosted page.
    ///
    /// [`BillingError::UnknownPlan`] for an unknown plan (400),
    /// [`BillingError::NoPrice`] when the plan has no price for the
    /// interval (400), [`BillingError::AlreadySubscribed`] when the
    /// billable has a valid subscription (409).
    pub async fn create(self) -> Result<Checkout, BillingError> {
        let billing = self.billing;
        billing.create_checkout(self).await
    }
}

/// A created checkout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkout {
    /// The provider's checkout id (Stripe `cs_...`, Paddle `txn_...`).
    pub id: String,
    /// The hosted page to redirect the browser to.
    pub url: String,
    /// The plan key.
    pub plan: String,
    /// Monthly or yearly.
    pub interval: Interval,
    /// Seats.
    pub quantity: u32,
    /// The trial, in days.
    pub trial_days: Option<u32>,
}

/// `{ "url": "..." }`: what a checkout or portal route returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CheckoutUrl {
    /// The hosted page to redirect the browser to.
    pub url: String,
}

impl From<Checkout> for CheckoutUrl {
    fn from(checkout: Checkout) -> Self {
        CheckoutUrl { url: checkout.url }
    }
}
