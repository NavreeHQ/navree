//! The payment half of [`BillingProvider`](crate::BillingProvider): money,
//! one-time payments, payment links, refunds and mandates, in
//! provider-neutral terms, and the [`Capability`] list a provider declares.
//!
//! Amounts are integers in the currency's smallest unit (paise for rupees,
//! cents for dollars), never floats: ₹1,499.00 is `Money::inr(149_900)`.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::provider::billable_of;

/// The metadata key that carries a payment's or refund's reference.
pub const REFERENCE_KEY: &str = "reference";

/// The metadata key that marks what Navree created: `payment` or
/// `payment_link`.
pub const KIND_KEY: &str = "navree_kind";

/// What a provider can do, as
/// [`BillingProvider::capabilities`](crate::BillingProvider::capabilities)
/// lists it. An operation a provider does not list answers
/// [`ProviderError::Unsupported`](crate::ProviderError::Unsupported).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Capability {
    /// Hosted checkouts for plans, and changes to subscriptions.
    Subscriptions,
    /// Metered usage reported to the provider.
    MeteredUsage,
    /// A hosted page where customers manage their billing.
    CustomerPortal,
    /// One-time payments.
    OneTimePayments,
    /// Payment links: a page that takes one payment, to send by email,
    /// SMS or chat.
    PaymentLinks,
    /// Refunds of a whole payment.
    Refunds,
    /// Refunds of part of a payment.
    PartialRefunds,
    /// UPI payments: intent links, QR codes and collect requests.
    Upi,
    /// Recurring payments against a mandate the customer approved once
    /// (UPI AutoPay, e-mandates on cards and bank accounts).
    Mandates,
    /// Writes carry an idempotency key the provider honours, so they are
    /// retried safely.
    IdempotencyKeys,
}

/// An amount of money in the currency's smallest unit: paise for rupees,
/// cents for dollars, yen for yen.
///
/// ```
/// use navree_billing::Money;
///
/// let price = Money::inr(149_900);
/// assert_eq!(price.major(), "1499.00");
/// assert_eq!(price.to_string(), "INR 1499.00");
/// assert_eq!(Money::parse_major("1499.5", "inr"), Some(Money::inr(149_950)));
/// assert_eq!(Money::new(500, "JPY").major(), "500");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    /// The amount, in the currency's smallest unit.
    pub amount: i64,
    /// The currency: a lower-case ISO 4217 code (`inr`, `usd`).
    pub currency: String,
}

/// Currencies without a minor unit (ISO 4217 exponent 0).
const ZERO_DECIMAL: &[&str] = &[
    "bif", "clp", "djf", "gnf", "isk", "jpy", "kmf", "krw", "pyg", "rwf", "ugx", "uyi", "vnd",
    "vuv", "xaf", "xof", "xpf",
];

/// Currencies with three digits after the point (ISO 4217 exponent 3).
const THREE_DECIMAL: &[&str] = &["bhd", "iqd", "jod", "kwd", "lyd", "omr", "tnd"];

impl Money {
    /// `amount` of `currency`'s smallest unit (the code is lower-cased).
    pub fn new(amount: i64, currency: impl AsRef<str>) -> Money {
        Money {
            amount,
            currency: currency.as_ref().trim().to_ascii_lowercase(),
        }
    }

    /// An amount in rupees, given in paise: `Money::inr(149_900)` is
    /// ₹1,499.00.
    pub fn inr(paise: i64) -> Money {
        Money::new(paise, "inr")
    }

    /// How many digits of the amount come after the decimal point: 2 for
    /// most currencies, 0 for `jpy`, `krw` and the other currencies without
    /// a minor unit, 3 for `bhd`, `kwd`, `omr` and the like.
    pub fn decimals(&self) -> u32 {
        let code = self.currency.as_str();
        if ZERO_DECIMAL.contains(&code) {
            0
        } else if THREE_DECIMAL.contains(&code) {
            3
        } else {
            2
        }
    }

    /// The amount in the main unit, as decimal text without grouping:
    /// `1499.00` for `Money::inr(149_900)`, `500` for 500 yen. What
    /// providers that take decimal amounts expect.
    pub fn major(&self) -> String {
        let decimals = self.decimals();
        let sign = if self.amount < 0 { "-" } else { "" };
        let units = self.amount.unsigned_abs();
        if decimals == 0 {
            return format!("{sign}{units}");
        }
        let scale = 10_u64.pow(decimals);
        format!(
            "{sign}{}.{:0width$}",
            units / scale,
            units % scale,
            width = decimals as usize
        )
    }

    /// Reads a decimal amount in the main unit (`1499.5` rupees is 149950
    /// paise) without floating point. `None` for text that is not a plain
    /// decimal number, has more digits after the point than the currency
    /// has, or does not fit.
    pub fn parse_major(text: &str, currency: impl AsRef<str>) -> Option<Money> {
        let money = Money::new(0, currency);
        let decimals = money.decimals() as usize;
        let text = text.trim();
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let plain = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() || !plain(whole) || !plain(fraction) || fraction.len() > decimals {
            return None;
        }
        let whole: i64 = whole.parse().ok()?;
        let fraction: i64 = if decimals == 0 {
            0
        } else {
            format!("{fraction:0<decimals$}").parse().ok()?
        };
        let scale = 10_i64.checked_pow(decimals as u32)?;
        let amount = whole.checked_mul(scale)?.checked_add(fraction)?;
        Some(Money {
            amount: if negative { -amount } else { amount },
            currency: money.currency,
        })
    }

    /// The currency code in upper case (`INR`), as most APIs spell it.
    pub fn currency_code(&self) -> String {
        self.currency.to_ascii_uppercase()
    }

    /// Why the amount cannot be charged: not above zero, or not a
    /// three-letter currency code.
    pub(crate) fn problem(&self) -> Option<&'static str> {
        if self.amount <= 0 {
            return Some("the amount must be above zero");
        }
        if self.currency.len() != 3 || !self.currency.bytes().all(|b| b.is_ascii_lowercase()) {
            return Some("the currency must be a three-letter ISO 4217 code such as `inr`");
        }
        None
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.currency_code(), self.major())
    }
}

/// How a customer pays.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PaymentMethod {
    /// Credit and debit cards (with the wallets that pay by card, such as
    /// Apple Pay and Google Pay on Stripe).
    Card,
    /// UPI: an intent link, a QR code or a collect request to a UPI id.
    Upi,
    /// Net banking: the customer signs in to their bank.
    NetBanking,
    /// Prepaid wallets (Paytm, PhonePe, Amazon Pay, ...).
    Wallet,
    /// A bank transfer the customer makes (NEFT, RTGS, IMPS, SEPA credit).
    BankTransfer,
    /// A direct debit from a bank account against a mandate (eNACH, SEPA
    /// Direct Debit, ACH).
    BankDebit,
    /// Buy now, pay later.
    PayLater,
    /// Card or cardless instalments (EMI).
    Emi,
    /// Another method, by the provider's name for it.
    Other(String),
}

impl PaymentMethod {
    /// The method's name: `card`, `upi`, `netbanking`, `wallet`,
    /// `bank_transfer`, `bank_debit`, `pay_later`, `emi`, or the other
    /// method's own name.
    pub fn as_str(&self) -> &str {
        match self {
            PaymentMethod::Card => "card",
            PaymentMethod::Upi => "upi",
            PaymentMethod::NetBanking => "netbanking",
            PaymentMethod::Wallet => "wallet",
            PaymentMethod::BankTransfer => "bank_transfer",
            PaymentMethod::BankDebit => "bank_debit",
            PaymentMethod::PayLater => "pay_later",
            PaymentMethod::Emi => "emi",
            PaymentMethod::Other(name) => name,
        }
    }
}

impl FromStr for PaymentMethod {
    type Err = std::convert::Infallible;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Ok(match name.trim().to_ascii_lowercase().as_str() {
            "card" => PaymentMethod::Card,
            "upi" => PaymentMethod::Upi,
            "netbanking" | "net_banking" => PaymentMethod::NetBanking,
            "wallet" => PaymentMethod::Wallet,
            "bank_transfer" => PaymentMethod::BankTransfer,
            "bank_debit" => PaymentMethod::BankDebit,
            "pay_later" | "paylater" => PaymentMethod::PayLater,
            "emi" => PaymentMethod::Emi,
            other => PaymentMethod::Other(other.to_owned()),
        })
    }
}

impl fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for PaymentMethod {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PaymentMethod {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(name.parse().unwrap_or(PaymentMethod::Other(name)))
    }
}

/// Who pays: what the provider gets to identify, prefill and notify the
/// customer. Every field is optional; some providers need some of them
/// (a phone number for UPI and for Indian gateways' customer records).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentCustomer {
    /// The provider's customer, when one exists.
    pub customer_id: Option<String>,
    /// The customer's name.
    pub name: Option<String>,
    /// The customer's email.
    pub email: Option<String>,
    /// The customer's phone number in E.164 form (`+919876543210`).
    pub phone: Option<String>,
}

/// A one-time payment to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentRequest {
    /// The billable paying, when there is one (stored in the metadata, so
    /// the payment's events name it).
    pub billable: Option<(String, Uuid)>,
    /// The amount.
    pub amount: Money,
    /// What is paid for, as the payment page shows it.
    pub description: String,
    /// Your own reference: an order or invoice number. 1 to 40 letters,
    /// digits, `_` or `-` (the strictest provider's rule), carried by
    /// every event of the payment.
    pub reference: String,
    /// Who pays.
    pub customer: PaymentCustomer,
    /// The methods offered; empty offers the provider's defaults.
    pub methods: Vec<PaymentMethod>,
    /// Where the customer returns after paying. Not proof of payment: wait
    /// for the webhook.
    pub success_url: Option<String>,
    /// Where the customer returns when they leave the payment page.
    pub cancel_url: Option<String>,
    /// When the payment page stops accepting the payment.
    pub expires_at: Option<DateTime<Utc>>,
    /// Stored with the payment; the billable, the reference and
    /// [`KIND_KEY`] are added.
    pub metadata: BTreeMap<String, String>,
    /// Makes a repeated request create one payment, where the provider
    /// supports keys.
    pub idempotency_key: String,
}

/// What the payer does next.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PaymentAction {
    /// Send the payer to this hosted page.
    Redirect {
        /// The page.
        url: String,
    },
    /// Open the provider's checkout script in your page with this token
    /// (a Razorpay order id with your key id, a Cashfree payment session
    /// id, a Stripe client secret).
    ClientToken {
        /// The token the script takes.
        token: String,
    },
    /// Open the payer's UPI app with this `upi://pay` link, or show it as
    /// a QR code.
    UpiIntent {
        /// The `upi://pay?...` link.
        url: String,
    },
}

/// Where a one-time payment is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PaymentStatus {
    /// Created; the payer has not paid yet.
    Pending,
    /// The payer acted and the provider is confirming (a UPI collect
    /// request, a bank transfer, a delayed card payment).
    Processing,
    /// Authorized, not captured yet.
    Authorized,
    /// Paid.
    Paid,
    /// The payment failed.
    Failed,
    /// Canceled before it was paid.
    Canceled,
    /// The payment page expired before it was paid.
    Expired,
}

impl PaymentStatus {
    /// `pending`, `processing`, `authorized`, `paid`, `failed`, `canceled`
    /// or `expired`.
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentStatus::Pending => "pending",
            PaymentStatus::Processing => "processing",
            PaymentStatus::Authorized => "authorized",
            PaymentStatus::Paid => "paid",
            PaymentStatus::Failed => "failed",
            PaymentStatus::Canceled => "canceled",
            PaymentStatus::Expired => "expired",
        }
    }
}

/// A one-time payment as the provider reports it: what
/// [`create_payment`](crate::BillingProvider::create_payment) returns and
/// what payment webhooks carry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPayment {
    /// The provider's id of what the payer opens: a Checkout Session, an
    /// order, a transaction.
    pub id: String,
    /// The id of the captured payment that refunds name (Stripe's
    /// PaymentIntent, Razorpay's `pay_...`), once there is one.
    pub payment_id: Option<String>,
    /// The payment link it came from, if any.
    pub link_id: Option<String>,
    /// Your reference.
    pub reference: Option<String>,
    /// Where it is.
    pub status: PaymentStatus,
    /// The amount.
    pub amount: Money,
    /// How it was paid, once the provider says.
    pub method: Option<PaymentMethod>,
    /// The provider's customer, if any.
    pub customer_id: Option<String>,
    /// What the payer does next (on creation).
    pub next_action: Option<PaymentAction>,
    /// The metadata (`billable_type`, `billable_id`, `reference`, ...).
    pub metadata: BTreeMap<String, String>,
}

impl ProviderPayment {
    /// The billable named by the metadata, if any.
    pub fn billable(&self) -> Option<(String, Uuid)> {
        billable_of(&self.metadata)
    }
}

/// A payment link to create: a page that takes one payment, which you send
/// by email, SMS or chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentLinkRequest {
    /// The billable paying, when there is one.
    pub billable: Option<(String, Uuid)>,
    /// The amount.
    pub amount: Money,
    /// What is paid for.
    pub description: String,
    /// Your own reference (see [`PaymentRequest::reference`]).
    pub reference: String,
    /// Who pays.
    pub customer: PaymentCustomer,
    /// The methods offered; empty offers the provider's defaults.
    pub methods: Vec<PaymentMethod>,
    /// When the link stops working, where the provider supports expiry
    /// (Stripe's links do not expire).
    pub expires_at: Option<DateTime<Utc>>,
    /// Asks the provider to send the link by email itself, where it can.
    pub notify_by_email: bool,
    /// Asks the provider to send the link by SMS itself, where it can.
    pub notify_by_sms: bool,
    /// Where the customer goes after paying.
    pub return_url: Option<String>,
    /// Stored with the link and its payment; the billable, the reference
    /// and [`KIND_KEY`] are added.
    pub metadata: BTreeMap<String, String>,
    /// Makes a repeated request create one link, where the provider
    /// supports keys.
    pub idempotency_key: String,
}

/// Where a payment link is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PaymentLinkStatus {
    /// It takes payments.
    Active,
    /// It has been paid.
    Paid,
    /// It expired unpaid.
    Expired,
    /// It was canceled.
    Canceled,
}

/// A created payment link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPaymentLink {
    /// The provider's link id.
    pub id: String,
    /// The page to send to the customer.
    pub url: String,
    /// Where it is.
    pub status: PaymentLinkStatus,
    /// The amount.
    pub amount: Money,
    /// Your reference.
    pub reference: Option<String>,
    /// When it stops working, if it expires.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Why a payment is refunded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RefundReason {
    /// The customer asked (the default).
    #[default]
    RequestedByCustomer,
    /// The customer was charged twice.
    Duplicate,
    /// The payment was fraudulent.
    Fraudulent,
}

impl RefundReason {
    /// `requested_by_customer`, `duplicate` or `fraudulent`.
    pub fn as_str(self) -> &'static str {
        match self {
            RefundReason::RequestedByCustomer => "requested_by_customer",
            RefundReason::Duplicate => "duplicate",
            RefundReason::Fraudulent => "fraudulent",
        }
    }
}

/// A refund to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundRequest {
    /// The payment to refund: a [`ProviderPayment::payment_id`] (or, for
    /// providers whose refunds name the order or transaction, its
    /// [`ProviderPayment::id`]).
    pub payment_id: String,
    /// How much; `None` refunds what is left of the payment.
    pub amount: Option<Money>,
    /// Why.
    pub reason: RefundReason,
    /// A note for the provider's records (and the customer, where the
    /// provider shows it).
    pub note: Option<String>,
    /// Your own refund reference (see [`PaymentRequest::reference`]);
    /// some providers require one.
    pub reference: String,
    /// The billable refunded, when known (stored in the metadata).
    pub billable: Option<(String, Uuid)>,
    /// Stored with the refund; the billable and the reference are added.
    pub metadata: BTreeMap<String, String>,
    /// Makes a repeated request refund once, where the provider supports
    /// keys. Refunding twice by mistake costs money: keep the key stable
    /// for one refund.
    pub idempotency_key: String,
}

/// Where a refund is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RefundStatus {
    /// Accepted; the money has not reached the customer yet (or the
    /// provider is reviewing it).
    Pending,
    /// Done.
    Succeeded,
    /// It failed.
    Failed,
    /// It was canceled.
    Canceled,
}

impl RefundStatus {
    /// `pending`, `succeeded`, `failed` or `canceled`.
    pub fn as_str(self) -> &'static str {
        match self {
            RefundStatus::Pending => "pending",
            RefundStatus::Succeeded => "succeeded",
            RefundStatus::Failed => "failed",
            RefundStatus::Canceled => "canceled",
        }
    }
}

/// A refund as the provider reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRefund {
    /// The provider's refund id.
    pub id: String,
    /// The payment refunded.
    pub payment_id: Option<String>,
    /// Your refund reference.
    pub reference: Option<String>,
    /// Where it is.
    pub status: RefundStatus,
    /// The amount refunded.
    pub amount: Money,
    /// The metadata.
    pub metadata: BTreeMap<String, String>,
}

impl ProviderRefund {
    /// The billable named by the metadata, if any.
    pub fn billable(&self) -> Option<(String, Uuid)> {
        billable_of(&self.metadata)
    }
}

/// Where a mandate is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MandateStatus {
    /// Waiting for the customer to approve it.
    Pending,
    /// Approved: the provider may debit against it.
    Active,
    /// Paused by the customer or the provider.
    Paused,
    /// Revoked by the customer (in their UPI app or with their bank) or
    /// by you.
    Revoked,
    /// Approval failed or the mandate expired.
    Failed,
}

impl MandateStatus {
    /// `pending`, `active`, `paused`, `revoked` or `failed`.
    pub fn as_str(self) -> &'static str {
        match self {
            MandateStatus::Pending => "pending",
            MandateStatus::Active => "active",
            MandateStatus::Paused => "paused",
            MandateStatus::Revoked => "revoked",
            MandateStatus::Failed => "failed",
        }
    }
}

/// A standing instruction the customer approved once, against which the
/// provider charges recurring payments: UPI AutoPay, an e-mandate on a
/// card, or eNACH on a bank account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMandate {
    /// The provider's mandate (or token) id.
    pub id: String,
    /// The provider's customer.
    pub customer_id: Option<String>,
    /// The provider's subscription it pays, if any.
    pub subscription_id: Option<String>,
    /// The method it debits.
    pub method: PaymentMethod,
    /// Where it is.
    pub status: MandateStatus,
    /// The most one debit may take.
    pub max_amount: Option<Money>,
    /// When it ends.
    pub expires_at: Option<DateTime<Utc>>,
    /// The metadata.
    pub metadata: BTreeMap<String, String>,
}

/// Whether `reference` is 1 to 40 letters, digits, `_` or `-`.
pub(crate) fn is_reference(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= 40
        && reference
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `metadata` with the billable, the reference and the kind added.
pub(crate) fn tagged_metadata(
    metadata: &BTreeMap<String, String>,
    billable: Option<&(String, Uuid)>,
    reference: &str,
    kind: Option<&str>,
) -> BTreeMap<String, String> {
    let mut tagged = metadata.clone();
    if let Some((billable_type, billable_id)) = billable {
        tagged.insert("billable_type".into(), billable_type.clone());
        tagged.insert("billable_id".into(), billable_id.to_string());
    }
    tagged.insert(REFERENCE_KEY.into(), reference.to_owned());
    if let Some(kind) = kind {
        tagged.insert(KIND_KEY.into(), kind.to_owned());
    }
    tagged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Rupees in lakh grouping, then the paise: `1_00_000_00` is ₹1,00,000.00.
    #[allow(clippy::inconsistent_digit_grouping)]
    fn amounts_are_integers_in_the_smallest_unit() {
        assert_eq!(Money::inr(149_900).major(), "1499.00");
        assert_eq!(Money::inr(5).major(), "0.05");
        assert_eq!(Money::inr(-250).major(), "-2.50");
        assert_eq!(Money::new(500, "JPY").major(), "500");
        assert_eq!(Money::new(500, "JPY").currency, "jpy");
        assert_eq!(Money::new(1_234, "kwd").major(), "1.234");
        assert_eq!(Money::inr(1).currency_code(), "INR");
        assert_eq!(Money::inr(1_00_000_00).to_string(), "INR 100000.00");
    }

    #[test]
    fn decimal_text_is_read_without_floating_point() {
        assert_eq!(Money::parse_major("1499", "inr"), Some(Money::inr(149_900)));
        assert_eq!(
            Money::parse_major("1499.5", "inr"),
            Some(Money::inr(149_950))
        );
        assert_eq!(Money::parse_major("0.01", "INR"), Some(Money::inr(1)));
        assert_eq!(Money::parse_major("-2.50", "inr"), Some(Money::inr(-250)));
        assert_eq!(
            Money::parse_major("500", "jpy"),
            Some(Money::new(500, "jpy"))
        );
        // 0.1 + 0.2 is exact here.
        assert_eq!(Money::parse_major("0.30", "usd").unwrap().amount, 30);
        for bad in [
            "",
            ".5",
            "1.234",
            "1,499.00",
            "1e3",
            "abc",
            "1.2.3",
            "99999999999999999999",
        ] {
            assert_eq!(Money::parse_major(bad, "inr"), None, "{bad}");
        }
        assert_eq!(Money::parse_major("1.5", "jpy"), None);
    }

    #[test]
    fn amounts_that_cannot_be_charged_are_named() {
        assert_eq!(Money::inr(100).problem(), None);
        assert!(Money::inr(0).problem().is_some());
        assert!(Money::inr(-1).problem().is_some());
        assert!(Money::new(100, "rupees").problem().is_some());
    }

    #[test]
    fn methods_have_stable_names() {
        for (method, name) in [
            (PaymentMethod::Card, "card"),
            (PaymentMethod::Upi, "upi"),
            (PaymentMethod::NetBanking, "netbanking"),
            (PaymentMethod::Wallet, "wallet"),
            (PaymentMethod::BankTransfer, "bank_transfer"),
            (PaymentMethod::BankDebit, "bank_debit"),
            (PaymentMethod::PayLater, "pay_later"),
            (PaymentMethod::Emi, "emi"),
        ] {
            assert_eq!(method.as_str(), name);
            assert_eq!(name.parse::<PaymentMethod>().unwrap(), method);
            assert_eq!(serde_json::to_value(&method).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<PaymentMethod>(name.into()).unwrap(),
                method
            );
        }
        assert_eq!(
            "cardless_emi".parse::<PaymentMethod>().unwrap(),
            PaymentMethod::Other("cardless_emi".into())
        );
    }

    #[test]
    fn references_follow_the_strictest_provider() {
        assert!(is_reference("INV-2026-0042"));
        assert!(is_reference(&"a".repeat(40)));
        assert!(!is_reference(""));
        assert!(!is_reference(&"a".repeat(41)));
        assert!(!is_reference("INV 42"));
        assert!(!is_reference("INV/42"));
    }

    #[test]
    fn metadata_names_the_billable_the_reference_and_the_kind() {
        let id = Uuid::nil();
        let tagged = tagged_metadata(
            &BTreeMap::from([("source".to_owned(), "invoice".to_owned())]),
            Some(&("workspace".to_owned(), id)),
            "INV-42",
            Some("payment"),
        );
        assert_eq!(tagged["billable_type"], "workspace");
        assert_eq!(tagged["billable_id"], id.to_string());
        assert_eq!(tagged[REFERENCE_KEY], "INV-42");
        assert_eq!(tagged[KIND_KEY], "payment");
        assert_eq!(tagged["source"], "invoice");
        assert_eq!(billable_of(&tagged), Some(("workspace".to_owned(), id)));
    }
}
