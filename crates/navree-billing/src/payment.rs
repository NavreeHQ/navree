//! [`PaymentBuilder`], [`PaymentLinkBuilder`] and [`RefundBuilder`]: one-time
//! payments, payment links and refunds through [`Billing`].

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::billable::{AsBillable, BillableRef};
use crate::billing::Billing;
use crate::error::BillingError;
use crate::provider::payments::is_reference;
use crate::provider::{
    Money, PaymentCustomer, PaymentLinkRequest, PaymentMethod, PaymentRequest, ProviderPayment,
    ProviderPaymentLink, ProviderRefund, RefundReason, RefundRequest,
};

/// The longest idempotency key providers accept.
const MAX_KEY_LEN: usize = 255;

/// `raw` as an idempotency key: itself when it fits, else a digest of it.
pub(crate) fn key_within_limit(raw: String) -> String {
    if raw.len() <= MAX_KEY_LEN {
        raw
    } else {
        format!("navree-{}", hex::encode(Sha256::digest(raw.as_bytes())))
    }
}

/// A new attempt id for keys and references.
fn attempt() -> String {
    Uuid::now_v7().simple().to_string()
}

/// Checks a caller's idempotency key: 1 to 255 visible ASCII characters
/// without spaces.
pub(crate) fn check_key(key: &str) -> Result<(), BillingError> {
    if key.is_empty() || key.len() > MAX_KEY_LEN || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(BillingError::invalid(
            "idempotency_key",
            "the idempotency key must be 1 to 255 visible ASCII characters without spaces",
        ));
    }
    Ok(())
}

/// Checks a caller's reference.
fn check_reference(reference: &str) -> Result<(), BillingError> {
    if is_reference(reference) {
        Ok(())
    } else {
        Err(BillingError::invalid(
            "reference",
            "the reference must be 1 to 40 letters, digits, `_` or `-`",
        ))
    }
}

/// Checks an amount to charge or refund.
fn check_amount(amount: &Money) -> Result<(), BillingError> {
    match amount.problem() {
        Some(problem) => Err(BillingError::invalid("amount", problem)),
        None => Ok(()),
    }
}

/// What payments and links share.
struct Common {
    billable: Option<BillableRef>,
    amount: Money,
    description: String,
    reference: Option<String>,
    customer: PaymentCustomer,
    methods: Vec<PaymentMethod>,
    metadata: BTreeMap<String, String>,
    idempotency_key: Option<String>,
}

impl Common {
    fn new(amount: Money, description: String) -> Common {
        Common {
            billable: None,
            amount,
            description,
            reference: None,
            customer: PaymentCustomer::default(),
            methods: Vec::new(),
            metadata: BTreeMap::new(),
            idempotency_key: None,
        }
    }

    /// Checks the parts and fills in the billable's customer, the
    /// reference and the key (`prefix` names the operation in the key).
    async fn finish(
        mut self,
        billing: &Billing,
        prefix: &str,
        per_attempt: bool,
    ) -> Result<Finished, BillingError> {
        check_amount(&self.amount)?;
        if self.description.trim().is_empty() {
            return Err(BillingError::invalid("description", "say what is paid for"));
        }
        let reference = match self.reference.take() {
            Some(reference) => {
                check_reference(&reference)?;
                reference
            }
            None => format!("{prefix}-{}", attempt()),
        };
        let idempotency_key = match self.idempotency_key.take() {
            Some(key) => {
                check_key(&key)?;
                key
            }
            None if per_attempt => {
                key_within_limit(format!("navree-{prefix}-{reference}-{}", attempt()))
            }
            None => key_within_limit(format!("navree-{prefix}-{reference}")),
        };
        if let Some(billable) = &self.billable {
            if self.customer.customer_id.is_none() {
                self.customer.customer_id = billing.provider_customer_id(billable).await?;
            }
            if self.customer.name.is_none() && !billable.name.is_empty() {
                self.customer.name = Some(billable.name.clone());
            }
            if self.customer.email.is_none() {
                self.customer.email = billable.email.clone();
            }
        }
        Ok(Finished {
            billable: self
                .billable
                .map(|billable| (billable.billable_type, billable.billable_id)),
            amount: self.amount,
            description: self.description,
            reference,
            customer: self.customer,
            methods: self.methods,
            metadata: self.metadata,
            idempotency_key,
        })
    }
}

/// The checked parts of a payment or link.
struct Finished {
    billable: Option<(String, Uuid)>,
    amount: Money,
    description: String,
    reference: String,
    customer: PaymentCustomer,
    methods: Vec<PaymentMethod>,
    metadata: BTreeMap<String, String>,
    idempotency_key: String,
}

macro_rules! common_setters {
    () => {
        /// The billable paying: named in the payment's metadata (so its
        /// events name it), and its provider customer, name and email fill
        /// in the customer.
        pub fn billable(mut self, billable: &impl AsBillable) -> Self {
            self.common.billable = Some(billable.billable_ref());
            self
        }

        /// Your own reference: an order or invoice number, 1 to 40
        /// letters, digits, `_` or `-`. Every event of the payment carries
        /// it. Default: a new one for each call.
        pub fn reference(mut self, reference: impl Into<String>) -> Self {
            self.common.reference = Some(reference.into());
            self
        }

        /// The customer's name.
        pub fn customer_name(mut self, name: impl Into<String>) -> Self {
            self.common.customer.name = Some(name.into());
            self
        }

        /// The customer's email.
        pub fn customer_email(mut self, email: impl Into<String>) -> Self {
            self.common.customer.email = Some(email.into());
            self
        }

        /// The customer's phone number in E.164 form (`+919876543210`);
        /// UPI and Indian gateways ask for one.
        pub fn customer_phone(mut self, phone: impl Into<String>) -> Self {
            self.common.customer.phone = Some(phone.into());
            self
        }

        /// Offers only these methods (the provider's defaults when none
        /// is given).
        pub fn methods(mut self, methods: impl IntoIterator<Item = PaymentMethod>) -> Self {
            self.common.methods = methods.into_iter().collect();
            self
        }

        /// Stored with the payment; its events carry it.
        pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
            self.common.metadata.insert(key.into(), value.into());
            self
        }

        /// The idempotency key: a repeated call with the same key creates
        /// one, where the provider honours keys. 1 to 255 visible ASCII
        /// characters without spaces.
        pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
            self.common.idempotency_key = Some(key.into());
            self
        }
    };
}

/// A one-time payment being configured: from
/// [`Billing::payment`](crate::Billing::payment).
///
/// The default idempotency key is new for each call, so calling again
/// starts a new attempt; pass your own (`order-42-attempt-3`) to make a
/// repeated call return the same payment.
#[must_use = "a payment does nothing until `create` is awaited"]
pub struct PaymentBuilder<'a> {
    billing: &'a Billing,
    common: Common,
    success_url: Option<String>,
    cancel_url: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

impl<'a> PaymentBuilder<'a> {
    pub(crate) fn new(billing: &'a Billing, amount: Money, description: String) -> Self {
        PaymentBuilder {
            billing,
            common: Common::new(amount, description),
            success_url: None,
            cancel_url: None,
            expires_at: None,
        }
    }

    common_setters!();

    /// Where the customer returns after paying (default
    /// `billing.return_url`). Not proof of payment: wait for the
    /// `PaymentUpdated` event.
    pub fn success_url(mut self, url: impl Into<String>) -> Self {
        self.success_url = Some(url.into());
        self
    }

    /// Where the customer returns when they leave (default
    /// `billing.return_url`).
    pub fn cancel_url(mut self, url: impl Into<String>) -> Self {
        self.cancel_url = Some(url.into());
        self
    }

    /// When the payment page stops accepting the payment, where the
    /// provider supports it.
    pub fn expires_at(mut self, at: DateTime<Utc>) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// Creates the payment at the provider. Its
    /// [`next_action`](ProviderPayment::next_action) says what the payer
    /// does next; its outcome arrives as a `PaymentUpdated` event.
    ///
    /// [`BillingError::Invalid`] for an amount that is not above zero, an
    /// empty description, or a malformed reference or key (400);
    /// [`BillingError::Provider`] when the provider refuses it or does not
    /// take one-time payments.
    pub async fn create(self) -> Result<ProviderPayment, BillingError> {
        let billing = self.billing;
        let finished = self.common.finish(billing, "pay", true).await?;
        let request = PaymentRequest {
            billable: finished.billable,
            amount: finished.amount,
            description: finished.description,
            reference: finished.reference,
            customer: finished.customer,
            methods: finished.methods,
            success_url: self
                .success_url
                .or_else(|| billing.return_url().map(str::to_owned)),
            cancel_url: self
                .cancel_url
                .or_else(|| billing.return_url().map(str::to_owned)),
            expires_at: self.expires_at,
            metadata: finished.metadata,
            idempotency_key: finished.idempotency_key,
        };
        Ok(billing.provider().create_payment(&request).await?)
    }
}

/// A payment link being configured: from
/// [`Billing::payment_link`](crate::Billing::payment_link).
///
/// The default idempotency key follows the reference: creating a link
/// again for the same reference returns the same link, where the provider
/// honours keys.
#[must_use = "a payment link does nothing until `create` is awaited"]
pub struct PaymentLinkBuilder<'a> {
    billing: &'a Billing,
    common: Common,
    expires_at: Option<DateTime<Utc>>,
    notify_by_email: bool,
    notify_by_sms: bool,
    return_url: Option<String>,
}

impl<'a> PaymentLinkBuilder<'a> {
    pub(crate) fn new(billing: &'a Billing, amount: Money, description: String) -> Self {
        PaymentLinkBuilder {
            billing,
            common: Common::new(amount, description),
            expires_at: None,
            notify_by_email: false,
            notify_by_sms: false,
            return_url: None,
        }
    }

    common_setters!();

    /// When the link stops working, where the provider supports expiry
    /// (Stripe's links do not expire).
    pub fn expires_at(mut self, at: DateTime<Utc>) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// Asks the provider to email the link to the customer itself, where
    /// it can.
    pub fn notify_by_email(mut self) -> Self {
        self.notify_by_email = true;
        self
    }

    /// Asks the provider to text the link to the customer itself, where it
    /// can.
    pub fn notify_by_sms(mut self) -> Self {
        self.notify_by_sms = true;
        self
    }

    /// Where the customer goes after paying (default `billing.return_url`).
    pub fn return_url(mut self, url: impl Into<String>) -> Self {
        self.return_url = Some(url.into());
        self
    }

    /// Creates the link at the provider. Its payment arrives as a
    /// `PaymentUpdated` event with `link_id` set.
    pub async fn create(self) -> Result<ProviderPaymentLink, BillingError> {
        let billing = self.billing;
        let finished = self.common.finish(billing, "link", false).await?;
        let request = PaymentLinkRequest {
            billable: finished.billable,
            amount: finished.amount,
            description: finished.description,
            reference: finished.reference,
            customer: finished.customer,
            methods: finished.methods,
            expires_at: self.expires_at,
            notify_by_email: self.notify_by_email,
            notify_by_sms: self.notify_by_sms,
            return_url: self
                .return_url
                .or_else(|| billing.return_url().map(str::to_owned)),
            metadata: finished.metadata,
            idempotency_key: finished.idempotency_key,
        };
        Ok(billing.provider().create_payment_link(&request).await?)
    }
}

/// A refund being configured: from
/// [`Billing::refund`](crate::Billing::refund).
///
/// Refunding twice by mistake costs money, so the default idempotency key
/// follows the reference: give the refund your own reference (the support
/// ticket, the credit note) and a repeated call refunds once.
#[must_use = "a refund does nothing until `create` is awaited"]
pub struct RefundBuilder<'a> {
    billing: &'a Billing,
    payment_id: String,
    amount: Option<Money>,
    reason: RefundReason,
    note: Option<String>,
    reference: Option<String>,
    billable: Option<BillableRef>,
    metadata: BTreeMap<String, String>,
    idempotency_key: Option<String>,
}

impl<'a> RefundBuilder<'a> {
    pub(crate) fn new(billing: &'a Billing, payment_id: String) -> Self {
        RefundBuilder {
            billing,
            payment_id,
            amount: None,
            reason: RefundReason::default(),
            note: None,
            reference: None,
            billable: None,
            metadata: BTreeMap::new(),
            idempotency_key: None,
        }
    }

    /// Refunds this much (default: what is left of the payment).
    pub fn amount(mut self, amount: Money) -> Self {
        self.amount = Some(amount);
        self
    }

    /// Why (default: the customer asked).
    pub fn reason(mut self, reason: RefundReason) -> Self {
        self.reason = reason;
        self
    }

    /// A note for the provider's records.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Your refund reference, 1 to 40 letters, digits, `_` or `-`; the
    /// default idempotency key follows it.
    pub fn reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    /// The billable refunded, named in the refund's events.
    pub fn billable(mut self, billable: &impl AsBillable) -> Self {
        self.billable = Some(billable.billable_ref());
        self
    }

    /// Stored with the refund.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// The idempotency key (default: derived from the reference).
    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Asks the provider for the refund; later changes arrive as
    /// `RefundUpdated` events.
    pub async fn create(self) -> Result<ProviderRefund, BillingError> {
        if self.payment_id.trim().is_empty() {
            return Err(BillingError::invalid(
                "payment_id",
                "name the payment to refund",
            ));
        }
        if let Some(amount) = &self.amount {
            check_amount(amount)?;
        }
        let reference = match self.reference {
            Some(reference) => {
                check_reference(&reference)?;
                reference
            }
            None => format!("refund-{}", attempt()),
        };
        let idempotency_key = match self.idempotency_key {
            Some(key) => {
                check_key(&key)?;
                key
            }
            None => key_within_limit(format!("navree-refund-{reference}")),
        };
        let request = RefundRequest {
            payment_id: self.payment_id,
            amount: self.amount,
            reason: self.reason,
            note: self.note,
            reference,
            billable: self
                .billable
                .map(|billable| (billable.billable_type, billable.billable_id)),
            metadata: self.metadata,
            idempotency_key,
        };
        Ok(self.billing.provider().refund(&request).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CONN-103
    #[test]
    fn keys_stay_within_the_providers_limit() {
        assert_eq!(
            key_within_limit("navree-refund-RF-9".into()),
            "navree-refund-RF-9"
        );
        let long = key_within_limit("k".repeat(300));
        assert!(long.len() <= MAX_KEY_LEN, "{long}");
        assert_eq!(long, key_within_limit("k".repeat(300)), "stable");
        assert!(check_key("order-42-attempt-3").is_ok());
        for bad in ["", "two words", "line\nbreak"] {
            assert!(check_key(bad).is_err(), "{bad:?}");
        }
        assert!(check_key(&"k".repeat(256)).is_err());
    }
}
