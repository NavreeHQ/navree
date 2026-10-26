//! [`BillingFake`]: the in-memory provider behind `billing.provider = "fake"`.
//!
//! It records checkouts, customers, subscriptions and reported usage instead
//! of calling Stripe or Paddle, and simulates the provider's webhooks. The
//! simulated webhooks are Stripe-shaped events (`customer.subscription.*`,
//! `invoice.*`, `checkout.session.completed`), signed with the fake's own
//! secret, and they go through the same verification, storage, job and
//! mapping as real ones.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Months, TimeDelta, Utc};
use http::{HeaderMap, HeaderValue};
use navree_contracts::Clock;
use navree_kernel::{BoxFuture, WeakAppContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::billable::AsBillable;
use crate::error::BillingError;
use crate::invoice::Invoice;
use crate::plan::{Interval, Plans, PriceRole};
use crate::provider::signature::{STRIPE_SIGNATURE_HEADER, sign_stripe_webhook, verify_stripe};
use crate::provider::stripe::{STRIPE_API_VERSION, STRIPE_EVENTS, full_metadata};
use crate::provider::{
    BillingProvider, CheckoutRequest, NewCustomer, ProviderCheckout, ProviderError,
    ProviderSubscription, SubscriptionChange, UsageReport, WebhookEndpoint, WebhookError,
    WebhookEvent, stripe,
};
use crate::webhook::WebhookReceipt;

const NAME: &str = "fake";

/// What the fake charges per seat for a period, in cents.
pub const FAKE_UNIT_AMOUNT_CENTS: i64 = 1000;

/// A checkout created through the fake provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeCheckout {
    /// The checkout id (`cs_fake_...`).
    pub id: String,
    /// The hosted page's URL.
    pub url: String,
    /// The billable type.
    pub billable_type: String,
    /// The billable id.
    pub billable_id: Uuid,
    /// The fake customer, when one existed.
    pub customer_id: Option<String>,
    /// The prefilled email.
    pub customer_email: Option<String>,
    /// The plan key.
    pub plan: String,
    /// Monthly or yearly.
    pub interval: Interval,
    /// The base price.
    pub price_id: String,
    /// The metered prices.
    pub metered_prices: Vec<String>,
    /// Seats.
    pub quantity: u32,
    /// The trial, in days.
    pub trial_days: Option<u32>,
    /// The coupon or promotion code.
    pub coupon: Option<String>,
    /// Whether customers may type a promotion code.
    pub allow_promotion_codes: bool,
    /// Where the customer returns after paying.
    pub success_url: String,
    /// Where the customer returns when they leave.
    pub cancel_url: String,
    /// The metadata (with `billable_type`, `billable_id` and `plan`).
    pub metadata: BTreeMap<String, String>,
    /// Whether [`BillingFake::complete_checkout`] completed it.
    pub completed: bool,
}

#[derive(Clone, Debug)]
struct FakeCustomer {
    id: String,
    billable_type: String,
    billable_id: Uuid,
    email: Option<String>,
}

#[derive(Clone, Debug)]
struct FakeItem {
    id: String,
    price_id: String,
    quantity: Option<u64>,
}

#[derive(Clone, Debug)]
struct FakeSubscription {
    id: String,
    customer_id: String,
    items: Vec<FakeItem>,
    /// Stripe's status string.
    status: &'static str,
    interval: Interval,
    trial_end: Option<DateTime<Utc>>,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    cancel_at_period_end: bool,
    canceled_at: Option<DateTime<Utc>>,
    paused: bool,
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct FakeState {
    next: u64,
    customers: BTreeMap<String, FakeCustomer>,
    checkouts: Vec<FakeCheckout>,
    subscriptions: BTreeMap<String, FakeSubscription>,
    fail_next: HashSet<(String, Uuid)>,
    usage: BTreeMap<(String, Uuid, String), u64>,
    invoices: BTreeMap<String, Vec<Value>>,
}

impl FakeState {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}_fake_{}", self.next)
    }

    fn customer_of(&self, billable_type: &str, billable_id: Uuid) -> Option<&FakeCustomer> {
        self.customers
            .values()
            .find(|c| c.billable_type == billable_type && c.billable_id == billable_id)
    }

    /// The billable's latest subscription that has not ended.
    fn subscription_of(&self, billable_type: &str, billable_id: Uuid) -> Option<String> {
        let customers: Vec<&str> = self
            .customers
            .values()
            .filter(|c| c.billable_type == billable_type && c.billable_id == billable_id)
            .map(|c| c.id.as_str())
            .collect();
        self.subscriptions
            .values()
            .filter(|s| customers.contains(&s.customer_id.as_str()) && s.status != "canceled")
            .map(|s| s.id.clone())
            .next_back()
    }
}

/// The fake provider (`billing.provider = "fake"`): get it from
/// [`Billing::fake`](crate::Billing::fake).
///
/// | Method | |
/// |---|---|
/// | [`checkouts`](Self::checkouts), [`assert_checkout`](Self::assert_checkout) | The checkouts created; `assert_checkout` returns the match |
/// | [`complete_checkout`](Self::complete_checkout) | Sends the webhooks of a paid checkout: customer, subscription, first invoice |
/// | [`fail_next_payment`](Self::fail_next_payment) | The next renewal (or trial end) sends `invoice.payment_failed` |
/// | [`end_trial`](Self::end_trial), [`renew`](Self::renew) | Sends the webhooks of a trial ending or a period renewing |
/// | [`reported_usage`](Self::reported_usage) | The usage the reporter sent |
/// | [`send_webhook`](Self::send_webhook) | Any event, as a Stripe-shaped `data.object` |
///
/// Changes made through [`Billing`](crate::Billing) (swap, cancel, ...)
/// update the fake's state and the local copy at once; the fake sends no
/// webhook for them. Invoices cost [`FAKE_UNIT_AMOUNT_CENTS`] per seat and
/// period (nothing during a trial).
pub struct BillingFake {
    state: Mutex<FakeState>,
    clock: Arc<dyn Clock>,
    plans: Plans,
    currency: String,
    secret: String,
    app: WeakAppContext,
}

impl std::fmt::Debug for BillingFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BillingFake").finish_non_exhaustive()
    }
}

impl BillingFake {
    pub(crate) fn new(
        clock: Arc<dyn Clock>,
        plans: Plans,
        currency: String,
        app: WeakAppContext,
    ) -> BillingFake {
        BillingFake {
            state: Mutex::new(FakeState::default()),
            clock,
            plans,
            currency,
            secret: format!("whsec_fake_{}", Uuid::now_v7().simple()),
            app,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// The secret the fake signs its webhooks with (`Stripe-Signature`), for
    /// tests that post to `/api/billing/webhook` themselves.
    pub fn webhook_secret(&self) -> &str {
        &self.secret
    }

    /// Every checkout created so far.
    pub fn checkouts(&self) -> Vec<FakeCheckout> {
        self.state().checkouts.clone()
    }

    /// The first checkout matching `predicate`.
    ///
    /// # Panics
    ///
    /// When none matches, listing the checkouts.
    pub fn assert_checkout(&self, predicate: impl Fn(&FakeCheckout) -> bool) -> FakeCheckout {
        let checkouts = self.checkouts();
        checkouts
            .iter()
            .find(|checkout| predicate(checkout))
            .cloned()
            .unwrap_or_else(|| panic!("no checkout matches; checkouts: {checkouts:#?}"))
    }

    /// Makes the billable's next payment (renewal or trial end) fail.
    pub fn fail_next_payment(&self, billable: &impl AsBillable) {
        let billable = billable.billable_ref();
        self.state()
            .fail_next
            .insert((billable.billable_type, billable.billable_id));
    }

    /// The total usage the reporter sent for the billable and meter.
    pub fn reported_usage(&self, billable: &impl AsBillable, meter: &str) -> u64 {
        let billable = billable.billable_ref();
        self.state()
            .usage
            .get(&(
                billable.billable_type,
                billable.billable_id,
                meter.to_owned(),
            ))
            .copied()
            .unwrap_or(0)
    }

    /// Completes checkout `id` as if the customer paid: sends
    /// `checkout.session.completed`, `customer.subscription.created` and
    /// `invoice.paid` (the first invoice; nothing to pay during a trial).
    pub async fn complete_checkout(&self, id: &str) -> Result<(), BillingError> {
        let now = self.now();
        let events = {
            let mut state = self.state();
            let Some(index) = state.checkouts.iter().position(|c| c.id == id) else {
                return Err(BillingError::Fake(format!("no checkout `{id}`")));
            };
            if state.checkouts[index].completed {
                return Err(BillingError::Fake(format!(
                    "checkout `{id}` is already completed"
                )));
            }
            state.checkouts[index].completed = true;
            let checkout = state.checkouts[index].clone();
            let customer_id = match &checkout.customer_id {
                Some(customer) => customer.clone(),
                None => {
                    let customer = state.id("cus");
                    state.customers.insert(
                        customer.clone(),
                        FakeCustomer {
                            id: customer.clone(),
                            billable_type: checkout.billable_type.clone(),
                            billable_id: checkout.billable_id,
                            email: checkout.customer_email.clone(),
                        },
                    );
                    customer
                }
            };
            let mut items = vec![FakeItem {
                id: state.id("si"),
                price_id: checkout.price_id.clone(),
                quantity: Some(u64::from(checkout.quantity)),
            }];
            for price in &checkout.metered_prices {
                items.push(FakeItem {
                    id: state.id("si"),
                    price_id: price.clone(),
                    quantity: None,
                });
            }
            let trial_end = checkout
                .trial_days
                .map(|days| now + TimeDelta::days(i64::from(days)));
            let subscription = FakeSubscription {
                id: state.id("sub"),
                customer_id: customer_id.clone(),
                items,
                status: if trial_end.is_some() {
                    "trialing"
                } else {
                    "active"
                },
                interval: checkout.interval,
                trial_end,
                period_start: now,
                period_end: trial_end.unwrap_or_else(|| advance(now, checkout.interval)),
                cancel_at_period_end: false,
                canceled_at: None,
                paused: false,
                metadata: checkout.metadata.clone(),
            };
            state
                .subscriptions
                .insert(subscription.id.clone(), subscription.clone());
            let email = state
                .customers
                .get(&customer_id)
                .and_then(|c| c.email.clone())
                .or(checkout.customer_email.clone());
            let session = json!({
                "id": checkout.id,
                "object": "checkout.session",
                "mode": "subscription",
                "status": "complete",
                "customer": customer_id,
                "subscription": subscription.id,
                "metadata": checkout.metadata,
                "customer_details": { "email": email },
            });
            let amount = if trial_end.is_some() {
                0
            } else {
                FAKE_UNIT_AMOUNT_CENTS * i64::from(checkout.quantity)
            };
            let invoice = self.invoice(&mut state, &subscription, amount, true, now);
            vec![
                ("checkout.session.completed", session),
                (
                    "customer.subscription.created",
                    subscription_json(&subscription),
                ),
                ("invoice.paid", invoice),
            ]
        };
        self.deliver_all(events).await
    }

    /// Ends the billable's trial now: sends
    /// `customer.subscription.trial_will_end`, then
    /// `customer.subscription.updated` (active, or past due after
    /// [`fail_next_payment`](Self::fail_next_payment)) and the first paid
    /// (or failed) invoice.
    pub async fn end_trial(&self, billable: &impl AsBillable) -> Result<(), BillingError> {
        let billable = billable.billable_ref();
        let now = self.now();
        let events = {
            let mut state = self.state();
            let id = state
                .subscription_of(&billable.billable_type, billable.billable_id)
                .ok_or_else(|| {
                    BillingError::Fake("the billable has no fake subscription".into())
                })?;
            let before = state.subscriptions[&id].clone();
            if before.status != "trialing" {
                return Err(BillingError::Fake(
                    "the subscription is not on trial".into(),
                ));
            }
            let warning = subscription_json(&before);
            let failed = state
                .fail_next
                .remove(&(billable.billable_type.clone(), billable.billable_id));
            let subscription = {
                let sub = state.subscriptions.get_mut(&id).expect("listed");
                sub.trial_end = Some(now);
                sub.period_start = now;
                sub.period_end = advance(now, sub.interval);
                sub.status = if failed { "past_due" } else { "active" };
                sub.clone()
            };
            let amount = FAKE_UNIT_AMOUNT_CENTS * quantity_of(&subscription);
            let invoice = self.invoice(&mut state, &subscription, amount, !failed, now);
            vec![
                ("customer.subscription.trial_will_end", warning),
                (
                    "customer.subscription.updated",
                    subscription_json(&subscription),
                ),
                (
                    if failed {
                        "invoice.payment_failed"
                    } else {
                        "invoice.paid"
                    },
                    invoice,
                ),
            ]
        };
        self.deliver_all(events).await
    }

    /// Renews the billable's subscription for another period: sends
    /// `customer.subscription.updated` and `invoice.paid` (or
    /// `invoice.payment_failed` after
    /// [`fail_next_payment`](Self::fail_next_payment), leaving it past
    /// due). A subscription canceled at the period's end ends instead, with
    /// `customer.subscription.deleted`.
    pub async fn renew(&self, billable: &impl AsBillable) -> Result<(), BillingError> {
        let billable = billable.billable_ref();
        let now = self.now();
        let events = {
            let mut state = self.state();
            let id = state
                .subscription_of(&billable.billable_type, billable.billable_id)
                .ok_or_else(|| {
                    BillingError::Fake("the billable has no fake subscription".into())
                })?;
            if state.subscriptions[&id].cancel_at_period_end {
                let sub = state.subscriptions.get_mut(&id).expect("listed");
                sub.status = "canceled";
                sub.canceled_at = Some(sub.period_end.min(now));
                let ended = sub.clone();
                vec![("customer.subscription.deleted", subscription_json(&ended))]
            } else {
                let failed = state
                    .fail_next
                    .remove(&(billable.billable_type.clone(), billable.billable_id));
                let subscription = {
                    let sub = state.subscriptions.get_mut(&id).expect("listed");
                    let start = sub.period_end;
                    sub.period_start = start;
                    sub.period_end = advance(start, sub.interval);
                    if sub.status == "trialing" {
                        sub.trial_end = Some(start);
                    }
                    sub.status = if failed { "past_due" } else { "active" };
                    sub.clone()
                };
                let amount = FAKE_UNIT_AMOUNT_CENTS * quantity_of(&subscription);
                let invoice = self.invoice(&mut state, &subscription, amount, !failed, now);
                vec![
                    (
                        "customer.subscription.updated",
                        subscription_json(&subscription),
                    ),
                    (
                        if failed {
                            "invoice.payment_failed"
                        } else {
                            "invoice.paid"
                        },
                        invoice,
                    ),
                ]
            }
        };
        self.deliver_all(events).await
    }

    /// Sends any event: `object` becomes the Stripe-shaped `data.object` of
    /// an event of type `event_type`, signed and posted through the webhook
    /// pipeline.
    pub async fn send_webhook(
        &self,
        event_type: &str,
        object: Value,
    ) -> Result<WebhookReceipt, BillingError> {
        let event = self.event(event_type, object);
        self.deliver(&event).await
    }

    fn event(&self, event_type: &str, object: Value) -> Value {
        let id = self.state().id("evt");
        json!({
            "id": id,
            "object": "event",
            "api_version": STRIPE_API_VERSION,
            "type": event_type,
            "created": self.now().timestamp(),
            "data": { "object": object },
        })
    }

    async fn deliver_all(&self, events: Vec<(&'static str, Value)>) -> Result<(), BillingError> {
        for (event_type, object) in events {
            let event = self.event(event_type, object);
            self.deliver(&event).await?;
        }
        Ok(())
    }

    async fn deliver(&self, event: &Value) -> Result<WebhookReceipt, BillingError> {
        let body = serde_json::to_vec(event).expect("JSON values serialize");
        let signature = sign_stripe_webhook(&self.secret, self.now().timestamp(), &body);
        let mut headers = HeaderMap::new();
        headers.insert(
            STRIPE_SIGNATURE_HEADER,
            HeaderValue::from_str(&signature).expect("a signature is a valid header value"),
        );
        let app = self
            .app
            .upgrade()
            .ok_or_else(|| BillingError::Fake("the app has shut down".into()))?;
        let billing = app
            .get::<crate::Billing>()
            .await
            .map_err(|error| BillingError::Fake(error.to_string()))?;
        billing.receive_webhook(&headers, &body).await
    }

    /// Records an invoice for `subscription` and returns its Stripe JSON.
    fn invoice(
        &self,
        state: &mut FakeState,
        subscription: &FakeSubscription,
        amount: i64,
        paid: bool,
        now: DateTime<Utc>,
    ) -> Value {
        let id = state.id("in");
        let number = format!("FAKE-{:04}", state.next);
        let invoice = json!({
            "id": id,
            "object": "invoice",
            "customer": subscription.customer_id,
            "number": number,
            "status": if paid { "paid" } else { "open" },
            "total": amount,
            "amount_due": amount,
            "amount_paid": if paid { amount } else { 0 },
            "currency": self.currency,
            "invoice_pdf": format!("https://billing.fake.test/invoices/{id}.pdf"),
            "hosted_invoice_url": format!("https://billing.fake.test/invoices/{id}"),
            "created": now.timestamp(),
            "parent": { "subscription_details": { "subscription": subscription.id } },
        });
        state
            .invoices
            .entry(subscription.customer_id.clone())
            .or_default()
            .push(invoice.clone());
        invoice
    }

    fn snapshot(&self, sub: &FakeSubscription) -> Result<ProviderSubscription, ProviderError> {
        stripe::parse_subscription(&subscription_json(sub))
    }

    fn with_subscription(
        &self,
        id: &str,
        change: impl FnOnce(&mut FakeSubscription, &Plans, DateTime<Utc>),
    ) -> Result<ProviderSubscription, ProviderError> {
        let now = self.now();
        let mut state = self.state();
        let sub = state
            .subscriptions
            .get_mut(id)
            .ok_or_else(|| not_found(format!("No such subscription: '{id}'")))?;
        change(sub, &self.plans, now);
        let sub = sub.clone();
        drop(state);
        self.snapshot(&sub)
    }
}

fn not_found(message: String) -> ProviderError {
    ProviderError::Api {
        provider: NAME,
        status: 404,
        message,
    }
}

/// `start` plus one period.
fn advance(start: DateTime<Utc>, interval: Interval) -> DateTime<Utc> {
    let months = match interval {
        Interval::Monthly => Months::new(1),
        Interval::Yearly => Months::new(12),
    };
    start.checked_add_months(months).unwrap_or(start)
}

fn quantity_of(sub: &FakeSubscription) -> i64 {
    sub.items
        .iter()
        .find_map(|item| item.quantity)
        .map_or(1, |q| i64::try_from(q).unwrap_or(i64::MAX))
}

/// The Stripe JSON of a fake subscription.
fn subscription_json(sub: &FakeSubscription) -> Value {
    let items: Vec<Value> = sub
        .items
        .iter()
        .map(|item| {
            let usage = if item.quantity.is_some() {
                "licensed"
            } else {
                "metered"
            };
            let mut value = json!({
                "id": item.id,
                "object": "subscription_item",
                "price": { "id": item.price_id, "recurring": { "usage_type": usage } },
                "current_period_start": sub.period_start.timestamp(),
                "current_period_end": sub.period_end.timestamp(),
            });
            if let Some(quantity) = item.quantity {
                value["quantity"] = json!(quantity);
            }
            value
        })
        .collect();
    json!({
        "id": sub.id,
        "object": "subscription",
        "customer": sub.customer_id,
        "status": sub.status,
        "trial_end": sub.trial_end.map(|t| t.timestamp()),
        "cancel_at_period_end": sub.cancel_at_period_end,
        "cancel_at": if sub.cancel_at_period_end { Some(sub.period_end.timestamp()) } else { None },
        "canceled_at": sub.canceled_at.map(|t| t.timestamp()),
        "ended_at": if sub.status == "canceled" { sub.canceled_at.map(|t| t.timestamp()) } else { None },
        "pause_collection": if sub.paused { json!({ "behavior": "void" }) } else { Value::Null },
        "metadata": sub.metadata,
        "items": { "object": "list", "data": items },
    })
}

impl BillingProvider for BillingFake {
    fn name(&self) -> &'static str {
        NAME
    }

    fn create_customer<'a>(
        &'a self,
        customer: &'a NewCustomer,
    ) -> BoxFuture<'a, Result<Option<String>, ProviderError>> {
        Box::pin(async move {
            let mut state = self.state();
            if let Some(existing) = state.customer_of(&customer.billable_type, customer.billable_id)
            {
                return Ok(Some(existing.id.clone()));
            }
            let id = state.id("cus");
            state.customers.insert(
                id.clone(),
                FakeCustomer {
                    id: id.clone(),
                    billable_type: customer.billable_type.clone(),
                    billable_id: customer.billable_id,
                    email: customer.email.clone(),
                },
            );
            Ok(Some(id))
        })
    }

    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<ProviderCheckout, ProviderError>> {
        Box::pin(async move {
            let mut state = self.state();
            let id = state.id("cs");
            let url = format!("https://checkout.fake.test/c/{id}");
            state.checkouts.push(FakeCheckout {
                id: id.clone(),
                url: url.clone(),
                billable_type: request.billable_type.clone(),
                billable_id: request.billable_id,
                customer_id: request.customer_id.clone(),
                customer_email: request.customer_email.clone(),
                plan: request.plan.clone(),
                interval: request.interval,
                price_id: request.price_id.clone(),
                metered_prices: request.metered_prices.clone(),
                quantity: request.quantity,
                trial_days: request.trial_days,
                coupon: request.coupon.clone(),
                allow_promotion_codes: request.allow_promotion_codes,
                success_url: request.success_url.clone(),
                cancel_url: request.cancel_url.clone(),
                metadata: full_metadata(request),
                completed: false,
            });
            Ok(ProviderCheckout { id, url })
        })
    }

    fn update_subscription<'a>(
        &'a self,
        id: &'a str,
        change: &'a SubscriptionChange,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let mut new_ids: Vec<String> = Vec::new();
            {
                let mut state = self.state();
                for _ in &change.add_prices {
                    let item = state.id("si");
                    new_ids.push(item);
                }
            }
            self.with_subscription(id, |sub, plans, _now| {
                let base = sub
                    .items
                    .iter_mut()
                    .find(|item| match &change.base_item_id {
                        Some(base) => &item.id == base,
                        None => item.quantity.is_some(),
                    });
                if let Some(base) = base {
                    base.price_id = change.price_id.clone();
                    base.quantity = Some(u64::from(change.quantity));
                }
                if let Some(PriceRole::Base { interval, .. }) = plans.role_of(&change.price_id) {
                    sub.interval = interval;
                }
                sub.items
                    .retain(|item| !change.remove_item_ids.contains(&item.id));
                for (price, item) in change.add_prices.iter().zip(new_ids) {
                    sub.items.push(FakeItem {
                        id: item,
                        price_id: price.clone(),
                        quantity: None,
                    });
                }
            })
        })
    }

    fn cancel_subscription<'a>(
        &'a self,
        id: &'a str,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            self.with_subscription(id, |sub, _, now| {
                if at_period_end {
                    sub.cancel_at_period_end = true;
                } else {
                    sub.status = "canceled";
                    sub.canceled_at = Some(now);
                }
            })
        })
    }

    fn resume_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(
            async move { self.with_subscription(id, |sub, _, _| sub.cancel_at_period_end = false) },
        )
    }

    fn pause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move { self.with_subscription(id, |sub, _, _| sub.paused = true) })
    }

    fn unpause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move { self.with_subscription(id, |sub, _, _| sub.paused = false) })
    }

    fn subscriptions<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ProviderSubscription>, ProviderError>> {
        Box::pin(async move {
            let subs: Vec<FakeSubscription> = self
                .state()
                .subscriptions
                .values()
                .filter(|sub| sub.customer_id == customer_id)
                .cloned()
                .collect();
            subs.iter().map(|sub| self.snapshot(sub)).collect()
        })
    }

    fn report_usage<'a>(
        &'a self,
        report: &'a UsageReport,
    ) -> BoxFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            let mut state = self.state();
            let customer = state
                .customers
                .get(&report.customer_id)
                .cloned()
                .ok_or_else(|| not_found(format!("No such customer: '{}'", report.customer_id)))?;
            *state
                .usage
                .entry((
                    customer.billable_type,
                    customer.billable_id,
                    report.meter.clone(),
                ))
                .or_default() += report.quantity;
            Ok(())
        })
    }

    fn invoices<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Invoice>, ProviderError>> {
        Box::pin(async move {
            let raw = self
                .state()
                .invoices
                .get(customer_id)
                .cloned()
                .unwrap_or_default();
            let mut invoices = raw
                .iter()
                .map(stripe::parse_invoice)
                .collect::<Result<Vec<_>, _>>()?;
            invoices.reverse();
            Ok(invoices)
        })
    }

    fn portal_url<'a>(
        &'a self,
        customer_id: &'a str,
        return_url: &'a str,
        _subscription_ids: &'a [String],
    ) -> BoxFuture<'a, Result<String, ProviderError>> {
        Box::pin(async move {
            Ok(format!(
                "https://billing.fake.test/portal/{customer_id}?return_url={return_url}"
            ))
        })
    }

    fn price_exists<'a>(&'a self, price_id: &'a str) -> BoxFuture<'a, Result<bool, ProviderError>> {
        Box::pin(async move { Ok(self.plans.price_ids().iter().any(|p| p == price_id)) })
    }

    fn create_webhook_endpoint<'a>(
        &'a self,
        url: &'a str,
    ) -> BoxFuture<'a, Result<WebhookEndpoint, ProviderError>> {
        Box::pin(async move {
            let _ = url;
            Ok(WebhookEndpoint {
                id: self.state().id("we"),
                secret: Some(self.secret.clone()),
                events: STRIPE_EVENTS.iter().map(|e| (*e).to_owned()).collect(),
            })
        })
    }

    fn verify_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: DateTime<Utc>,
        tolerance: Duration,
    ) -> Result<(), WebhookError> {
        verify_stripe(headers, body, &self.secret, now, tolerance)
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<WebhookEvent, ProviderError> {
        stripe::parse_event(body)
    }
}
