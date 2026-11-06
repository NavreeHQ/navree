//! [`StripeProvider`]: a narrow Stripe client on navree-http-client, and the
//! mapping of Stripe's objects and events to provider-neutral values.
//!
//! Requests are form-encoded, authenticated with the secret key and pinned
//! to [`STRIPE_API_VERSION`] with the `Stripe-Version` header. Every `POST`
//! carries an `Idempotency-Key`: the request's own key for what creates
//! something (a customer, a checkout, a payment, a link, a refund, usage),
//! a key made for the call for the rest, so the HTTP client retries every
//! write without Stripe acting twice.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use http::{HeaderMap, Method};
use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::invoice::{Invoice, InvoiceStatus};
use crate::provider::payments::{REFERENCE_KEY, tagged_metadata};
use crate::provider::signature::verify_stripe;
use crate::provider::{
    BillingProvider, Capability, CheckoutRequest, Money, NewCustomer, PaymentAction,
    PaymentLinkRequest, PaymentLinkStatus, PaymentMethod, PaymentRequest, PaymentStatus,
    ProviderCheckout, ProviderError, ProviderInvoice, ProviderItem, ProviderPayment,
    ProviderPaymentLink, ProviderRefund, ProviderSubscription, RefundRequest, RefundStatus,
    SubscriptionChange, UsageReport, WebhookAction, WebhookEndpoint, WebhookError, WebhookEvent,
};
use crate::subscription::{Proration, SubscriptionStatus};

/// The Stripe API version every request is pinned to (`Stripe-Version`).
/// Webhook endpoints created by `billing:webhook` use it too, so event
/// payloads have the shape this client reads. The tests' stripe-mock is the
/// release built from this version (`scripts/services/compose.yml`).
pub const STRIPE_API_VERSION: &str = "2026-09-30.endive";

const NAME: &str = "stripe";

/// The Stripe events Navree handles (what `billing:webhook` subscribes to).
pub const STRIPE_EVENTS: &[&str] = &[
    "checkout.session.completed",
    "checkout.session.async_payment_succeeded",
    "checkout.session.async_payment_failed",
    "checkout.session.expired",
    "customer.subscription.created",
    "customer.subscription.updated",
    "customer.subscription.deleted",
    "customer.subscription.paused",
    "customer.subscription.resumed",
    "customer.subscription.trial_will_end",
    "invoice.paid",
    "invoice.payment_failed",
    "refund.created",
    "refund.updated",
    "refund.failed",
];

/// What the Stripe driver does.
const CAPABILITIES: &[Capability] = &[
    Capability::Subscriptions,
    Capability::MeteredUsage,
    Capability::CustomerPortal,
    Capability::OneTimePayments,
    Capability::PaymentLinks,
    Capability::Refunds,
    Capability::PartialRefunds,
    Capability::Upi,
    Capability::IdempotencyKeys,
];

/// The longest idempotency key Stripe accepts.
const MAX_KEY_LEN: usize = 255;

/// A Stripe client: customers, Checkout Sessions, subscriptions, meter
/// events, invoices, portal sessions, prices and webhook endpoints.
pub struct StripeProvider {
    client: HttpClient,
    secret_key: Secret<String>,
    webhook_secret: Option<Secret<String>>,
    base: String,
    /// Metered price id → the event name of its Stripe meter.
    meter_events: Mutex<HashMap<String, String>>,
}

impl std::fmt::Debug for StripeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeProvider")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

/// Form fields of a Stripe request.
type Form = Vec<(String, String)>;

impl StripeProvider {
    /// A client for the API at `base` (`https://api.stripe.com`).
    pub fn new(
        client: HttpClient,
        secret_key: Secret<String>,
        webhook_secret: Option<Secret<String>>,
        base: impl Into<String>,
    ) -> StripeProvider {
        StripeProvider {
            client,
            secret_key,
            webhook_secret,
            base: base.into().trim_end_matches('/').to_owned(),
            meter_events: Mutex::new(HashMap::new()),
        }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        form: Option<&Form>,
        query: &[(&str, &str)],
        idempotency_key: Option<&str>,
    ) -> Result<Value, ProviderError> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer(self.secret_key.expose())
            .header("Stripe-Version", STRIPE_API_VERSION);
        if let Some(key) = idempotency_key {
            request = request.idempotency_key(key);
        }
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(form) = form {
            request = request.form(form);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ProviderError::Transport {
                provider: NAME,
                message: error.to_string(),
            })?;
        let status = response.status().as_u16();
        let body: Value = response.json().unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let message = body["error"]["message"]
                .as_str()
                .unwrap_or("no error message")
                .to_owned();
            return Err(ProviderError::Api {
                provider: NAME,
                status,
                message,
            });
        }
        Ok(body)
    }

    /// A `POST` with `Idempotency-Key: key`, retried safely.
    async fn post(&self, path: &str, form: &Form, key: &str) -> Result<Value, ProviderError> {
        self.call(Method::POST, path, Some(form), &[], Some(key))
            .await
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, ProviderError> {
        self.call(Method::GET, path, None, query, None).await
    }

    /// The event name of the meter behind a metered price (cached).
    async fn meter_event_name(&self, price_id: &str) -> Result<String, ProviderError> {
        if let Some(name) = self
            .meter_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(price_id)
        {
            return Ok(name.clone());
        }
        let price = self.get(&format!("/v1/prices/{price_id}"), &[]).await?;
        let meter = price["recurring"]["meter"].as_str().ok_or_else(|| {
            ProviderError::decode(
                NAME,
                format!("price `{price_id}` is not a metered price with a meter"),
            )
        })?;
        let meter = self
            .get(&format!("/v1/billing/meters/{meter}"), &[])
            .await?;
        let name = meter["event_name"]
            .as_str()
            .ok_or_else(|| ProviderError::decode(NAME, "a meter without `event_name`"))?
            .to_owned();
        self.meter_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(price_id.to_owned(), name.clone());
        Ok(name)
    }
}

fn push(form: &mut Form, key: impl Into<String>, value: impl Into<String>) {
    form.push((key.into(), value.into()));
}

fn metadata_fields(form: &mut Form, prefix: &str, metadata: &BTreeMap<String, String>) {
    for (key, value) in metadata {
        push(form, format!("{prefix}[{key}]"), value.clone());
    }
}

/// The Checkout Session form of `request`.
pub(crate) fn checkout_form(request: &CheckoutRequest) -> Form {
    let mut form = Form::new();
    push(&mut form, "mode", "subscription");
    match (&request.customer_id, &request.customer_email) {
        (Some(customer), _) => push(&mut form, "customer", customer.clone()),
        (None, Some(email)) => push(&mut form, "customer_email", email.clone()),
        (None, None) => {}
    }
    push(&mut form, "line_items[0][price]", request.price_id.clone());
    push(
        &mut form,
        "line_items[0][quantity]",
        request.quantity.to_string(),
    );
    for (index, price) in request.metered_prices.iter().enumerate() {
        push(
            &mut form,
            format!("line_items[{}][price]", index + 1),
            price.clone(),
        );
    }
    push(&mut form, "success_url", request.success_url.clone());
    push(&mut form, "cancel_url", request.cancel_url.clone());
    let metadata = full_metadata(request);
    metadata_fields(&mut form, "metadata", &metadata);
    metadata_fields(&mut form, "subscription_data[metadata]", &metadata);
    if let Some(days) = request.trial_days {
        push(
            &mut form,
            "subscription_data[trial_period_days]",
            days.to_string(),
        );
    }
    match &request.coupon {
        Some(code) if code.starts_with("promo_") => {
            push(&mut form, "discounts[0][promotion_code]", code.clone());
        }
        Some(code) => push(&mut form, "discounts[0][coupon]", code.clone()),
        None if request.allow_promotion_codes => push(&mut form, "allow_promotion_codes", "true"),
        None => {}
    }
    form
}

/// The request's metadata plus the billable and the plan.
pub(crate) fn full_metadata(request: &CheckoutRequest) -> BTreeMap<String, String> {
    let mut metadata = request.metadata.clone();
    metadata.insert("billable_type".into(), request.billable_type.clone());
    metadata.insert("billable_id".into(), request.billable_id.to_string());
    metadata.insert("plan".into(), request.plan.clone());
    metadata
}

/// The subscription update form of `change`.
pub(crate) fn change_form(change: &SubscriptionChange) -> Form {
    let mut form = Form::new();
    if let Some(item) = &change.base_item_id {
        push(&mut form, "items[0][id]", item.clone());
    }
    push(&mut form, "items[0][price]", change.price_id.clone());
    push(&mut form, "items[0][quantity]", change.quantity.to_string());
    let mut index = 1;
    for item in &change.remove_item_ids {
        push(&mut form, format!("items[{index}][id]"), item.clone());
        push(&mut form, format!("items[{index}][deleted]"), "true");
        index += 1;
    }
    for price in &change.add_prices {
        push(&mut form, format!("items[{index}][price]"), price.clone());
        index += 1;
    }
    let proration = match change.proration {
        Proration::NextInvoice => "create_prorations",
        Proration::InvoiceNow => "always_invoice",
        Proration::None => "none",
    };
    push(&mut form, "proration_behavior", proration);
    form
}

/// A key for one call of an operation whose request carries none (a
/// subscription change, a portal session): the HTTP client's retries of
/// the call share it, and a later call gets a new one.
pub(crate) fn call_key(operation: &str) -> String {
    format!("navree-{operation}-{}", Uuid::now_v7().simple())
}

/// `key` for a second request of the same operation (a payment link's
/// price), kept within the 255 characters Stripe accepts.
pub(crate) fn sub_key(key: &str, suffix: &str) -> String {
    let joined = format!("{key}-{suffix}");
    if joined.len() <= MAX_KEY_LEN {
        return joined;
    }
    let digest = hex::encode(Sha256::digest(key.as_bytes()));
    format!("navree-{}-{suffix}", &digest[..32])
}

/// Stripe's names for `methods`: cards and UPI; Navree does not map
/// Stripe's other method types.
fn method_types(methods: &[PaymentMethod]) -> Result<Vec<&'static str>, ProviderError> {
    methods
        .iter()
        .map(|method| match method {
            PaymentMethod::Card => Ok("card"),
            PaymentMethod::Upi => Ok("upi"),
            _ => Err(ProviderError::Unsupported {
                provider: NAME,
                operation: "payment methods other than cards and UPI (leave `methods` empty to offer the methods set in the Stripe dashboard)",
            }),
        })
        .collect()
}

/// The method Stripe names `name`.
fn method_of(name: &str) -> PaymentMethod {
    match name {
        "card" => PaymentMethod::Card,
        "upi" => PaymentMethod::Upi,
        other => PaymentMethod::Other(other.to_owned()),
    }
}

/// The Checkout Session form of a one-time payment.
pub(crate) fn payment_form(request: &PaymentRequest) -> Result<Form, ProviderError> {
    let mut form = Form::new();
    push(&mut form, "mode", "payment");
    match (&request.customer.customer_id, &request.customer.email) {
        (Some(customer), _) => push(&mut form, "customer", customer.clone()),
        (None, Some(email)) => push(&mut form, "customer_email", email.clone()),
        (None, None) => {}
    }
    push(
        &mut form,
        "line_items[0][price_data][currency]",
        request.amount.currency.clone(),
    );
    push(
        &mut form,
        "line_items[0][price_data][unit_amount]",
        request.amount.amount.to_string(),
    );
    push(
        &mut form,
        "line_items[0][price_data][product_data][name]",
        request.description.clone(),
    );
    push(&mut form, "line_items[0][quantity]", "1");
    push(&mut form, "client_reference_id", request.reference.clone());
    if let Some(url) = &request.success_url {
        push(&mut form, "success_url", url.clone());
    }
    if let Some(url) = &request.cancel_url {
        push(&mut form, "cancel_url", url.clone());
    }
    if let Some(expires) = request.expires_at {
        push(&mut form, "expires_at", expires.timestamp().to_string());
    }
    // Since 2026-09-30 a Checkout Session takes the methods as a filter on
    // the ones Stripe finds eligible (payment links still take
    // `payment_method_types`).
    for (index, kind) in method_types(&request.methods)?.into_iter().enumerate() {
        push(
            &mut form,
            format!("allowed_payment_method_types[{index}]"),
            kind,
        );
    }
    let metadata = tagged_metadata(
        &request.metadata,
        request.billable.as_ref(),
        &request.reference,
        Some("payment"),
    );
    metadata_fields(&mut form, "metadata", &metadata);
    metadata_fields(&mut form, "payment_intent_data[metadata]", &metadata);
    Ok(form)
}

/// The price form of a payment link (Stripe's links sell a price).
pub(crate) fn link_price_form(request: &PaymentLinkRequest) -> Form {
    vec![
        ("currency".to_owned(), request.amount.currency.clone()),
        ("unit_amount".to_owned(), request.amount.amount.to_string()),
        ("product_data[name]".to_owned(), request.description.clone()),
    ]
}

/// The payment link form for price `price_id`: one payment per link, the
/// metadata copied to the link's checkout sessions and payment.
pub(crate) fn link_form(
    request: &PaymentLinkRequest,
    price_id: &str,
) -> Result<Form, ProviderError> {
    let mut form = Form::new();
    push(&mut form, "line_items[0][price]", price_id);
    push(&mut form, "line_items[0][quantity]", "1");
    push(&mut form, "restrictions[completed_sessions][limit]", "1");
    if let Some(url) = &request.return_url {
        push(&mut form, "after_completion[type]", "redirect");
        push(&mut form, "after_completion[redirect][url]", url.clone());
    }
    for (index, kind) in method_types(&request.methods)?.into_iter().enumerate() {
        push(&mut form, format!("payment_method_types[{index}]"), kind);
    }
    let metadata = tagged_metadata(
        &request.metadata,
        request.billable.as_ref(),
        &request.reference,
        Some("payment_link"),
    );
    metadata_fields(&mut form, "metadata", &metadata);
    metadata_fields(&mut form, "payment_intent_data[metadata]", &metadata);
    Ok(form)
}

/// The refund form of `request`.
pub(crate) fn refund_form(request: &RefundRequest) -> Form {
    let mut form = Form::new();
    let target = if request.payment_id.starts_with("ch_") || request.payment_id.starts_with("py_") {
        "charge"
    } else {
        "payment_intent"
    };
    push(&mut form, target, request.payment_id.clone());
    if let Some(amount) = &request.amount {
        push(&mut form, "amount", amount.amount.to_string());
    }
    push(&mut form, "reason", request.reason.as_str());
    let mut metadata = tagged_metadata(
        &request.metadata,
        request.billable.as_ref(),
        &request.reference,
        None,
    );
    if let Some(note) = &request.note {
        metadata.insert("note".into(), note.clone());
    }
    metadata_fields(&mut form, "metadata", &metadata);
    form
}

impl BillingProvider for StripeProvider {
    fn name(&self) -> &'static str {
        NAME
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn create_customer<'a>(
        &'a self,
        customer: &'a NewCustomer,
    ) -> BoxFuture<'a, Result<Option<String>, ProviderError>> {
        Box::pin(async move {
            let mut form = Form::new();
            if !customer.name.is_empty() {
                push(&mut form, "name", customer.name.clone());
            }
            if let Some(email) = &customer.email {
                push(&mut form, "email", email.clone());
            }
            push(
                &mut form,
                "metadata[billable_type]",
                customer.billable_type.clone(),
            );
            push(
                &mut form,
                "metadata[billable_id]",
                customer.billable_id.to_string(),
            );
            let created = self
                .post("/v1/customers", &form, &customer.idempotency_key)
                .await?;
            Ok(Some(string(&created, "id")?))
        })
    }

    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<ProviderCheckout, ProviderError>> {
        Box::pin(async move {
            let session = self
                .post(
                    "/v1/checkout/sessions",
                    &checkout_form(request),
                    &request.idempotency_key,
                )
                .await?;
            Ok(ProviderCheckout {
                id: string(&session, "id")?,
                url: string(&session, "url")?,
            })
        })
    }

    fn update_subscription<'a>(
        &'a self,
        id: &'a str,
        change: &'a SubscriptionChange,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let updated = self
                .post(
                    &format!("/v1/subscriptions/{id}"),
                    &change_form(change),
                    &call_key("subscription-update"),
                )
                .await?;
            parse_subscription(&updated)
        })
    }

    fn cancel_subscription<'a>(
        &'a self,
        id: &'a str,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let updated = if at_period_end {
                let form = vec![("cancel_at_period_end".to_owned(), "true".to_owned())];
                self.post(
                    &format!("/v1/subscriptions/{id}"),
                    &form,
                    &call_key("subscription-cancel"),
                )
                .await?
            } else {
                self.call(
                    Method::DELETE,
                    &format!("/v1/subscriptions/{id}"),
                    None,
                    &[],
                    // DELETE is idempotent: Stripe ignores keys on it.
                    None,
                )
                .await?
            };
            parse_subscription(&updated)
        })
    }

    fn resume_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let form = vec![("cancel_at_period_end".to_owned(), "false".to_owned())];
            let updated = self
                .post(
                    &format!("/v1/subscriptions/{id}"),
                    &form,
                    &call_key("subscription-resume"),
                )
                .await?;
            parse_subscription(&updated)
        })
    }

    fn pause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let form = vec![("pause_collection[behavior]".to_owned(), "void".to_owned())];
            let updated = self
                .post(
                    &format!("/v1/subscriptions/{id}"),
                    &form,
                    &call_key("subscription-pause"),
                )
                .await?;
            parse_subscription(&updated)
        })
    }

    fn unpause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let form = vec![("pause_collection".to_owned(), String::new())];
            let updated = self
                .post(
                    &format!("/v1/subscriptions/{id}"),
                    &form,
                    &call_key("subscription-unpause"),
                )
                .await?;
            parse_subscription(&updated)
        })
    }

    fn subscriptions<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ProviderSubscription>, ProviderError>> {
        Box::pin(async move {
            let list = self
                .get(
                    "/v1/subscriptions",
                    &[
                        ("customer", customer_id),
                        ("status", "all"),
                        ("limit", "100"),
                    ],
                )
                .await?;
            data(&list)?.iter().map(parse_subscription).collect()
        })
    }

    fn report_usage<'a>(
        &'a self,
        report: &'a UsageReport,
    ) -> BoxFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            let event_name = self.meter_event_name(&report.price_id).await?;
            let form = vec![
                ("event_name".to_owned(), event_name),
                (
                    "payload[stripe_customer_id]".to_owned(),
                    report.customer_id.clone(),
                ),
                ("payload[value]".to_owned(), report.quantity.to_string()),
                ("identifier".to_owned(), report.idempotency_key.clone()),
                (
                    "timestamp".to_owned(),
                    report.timestamp.timestamp().to_string(),
                ),
            ];
            self.post("/v1/billing/meter_events", &form, &report.idempotency_key)
                .await?;
            Ok(())
        })
    }

    fn invoices<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Invoice>, ProviderError>> {
        Box::pin(async move {
            let list = self
                .get(
                    "/v1/invoices",
                    &[("customer", customer_id), ("limit", "100")],
                )
                .await?;
            data(&list)?.iter().map(parse_invoice).collect()
        })
    }

    fn portal_url<'a>(
        &'a self,
        customer_id: &'a str,
        return_url: &'a str,
        _subscription_ids: &'a [String],
    ) -> BoxFuture<'a, Result<String, ProviderError>> {
        Box::pin(async move {
            let form = vec![
                ("customer".to_owned(), customer_id.to_owned()),
                ("return_url".to_owned(), return_url.to_owned()),
            ];
            let session = self
                .post("/v1/billing_portal/sessions", &form, &call_key("portal"))
                .await?;
            string(&session, "url")
        })
    }

    fn price_exists<'a>(&'a self, price_id: &'a str) -> BoxFuture<'a, Result<bool, ProviderError>> {
        Box::pin(async move {
            match self.get(&format!("/v1/prices/{price_id}"), &[]).await {
                Ok(_) => Ok(true),
                Err(ProviderError::Api { status: 404, .. }) => Ok(false),
                Err(error) => Err(error),
            }
        })
    }

    fn create_webhook_endpoint<'a>(
        &'a self,
        url: &'a str,
    ) -> BoxFuture<'a, Result<WebhookEndpoint, ProviderError>> {
        Box::pin(async move {
            let mut form = vec![
                ("url".to_owned(), url.to_owned()),
                ("api_version".to_owned(), STRIPE_API_VERSION.to_owned()),
                ("description".to_owned(), "Navree billing".to_owned()),
            ];
            for (index, event) in STRIPE_EVENTS.iter().enumerate() {
                form.push((format!("enabled_events[{index}]"), (*event).to_owned()));
            }
            let endpoint = self
                .post(
                    "/v1/webhook_endpoints",
                    &form,
                    &call_key("webhook-endpoint"),
                )
                .await?;
            Ok(WebhookEndpoint {
                id: string(&endpoint, "id")?,
                secret: endpoint["secret"].as_str().map(str::to_owned),
                events: STRIPE_EVENTS.iter().map(|e| (*e).to_owned()).collect(),
            })
        })
    }

    fn create_payment<'a>(
        &'a self,
        request: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<ProviderPayment, ProviderError>> {
        Box::pin(async move {
            let form = payment_form(request)?;
            let session = self
                .post("/v1/checkout/sessions", &form, &request.idempotency_key)
                .await?;
            parse_session_payment(&session, None)
        })
    }

    fn create_payment_link<'a>(
        &'a self,
        request: &'a PaymentLinkRequest,
    ) -> BoxFuture<'a, Result<ProviderPaymentLink, ProviderError>> {
        Box::pin(async move {
            // Refuse an unsupported method before creating anything.
            method_types(&request.methods)?;
            let price = self
                .post(
                    "/v1/prices",
                    &link_price_form(request),
                    &sub_key(&request.idempotency_key, "price"),
                )
                .await?;
            let form = link_form(request, &string(&price, "id")?)?;
            let link = self
                .post(
                    "/v1/payment_links",
                    &form,
                    &sub_key(&request.idempotency_key, "link"),
                )
                .await?;
            // The reference rides on the link's URL, so every checkout of
            // the link carries it as `client_reference_id`.
            let url = string(&link, "url")?;
            let separator = if url.contains('?') { '&' } else { '?' };
            Ok(ProviderPaymentLink {
                id: string(&link, "id")?,
                url: format!("{url}{separator}client_reference_id={}", request.reference),
                status: if link["active"].as_bool() == Some(false) {
                    PaymentLinkStatus::Canceled
                } else {
                    PaymentLinkStatus::Active
                },
                amount: request.amount.clone(),
                reference: Some(request.reference.clone()),
                expires_at: None,
            })
        })
    }

    fn refund<'a>(
        &'a self,
        request: &'a RefundRequest,
    ) -> BoxFuture<'a, Result<ProviderRefund, ProviderError>> {
        Box::pin(async move {
            let refund = self
                .post(
                    "/v1/refunds",
                    &refund_form(request),
                    &request.idempotency_key,
                )
                .await?;
            parse_refund(&refund)
        })
    }

    fn verify_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: DateTime<Utc>,
        tolerance: Duration,
    ) -> Result<(), WebhookError> {
        let secret = self.webhook_secret.as_ref().ok_or(WebhookError::NoSecret)?;
        verify_stripe(headers, body, secret.expose(), now, tolerance)
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<WebhookEvent, ProviderError> {
        parse_event(body)
    }
}

// ------------------------------------------------------------------ parsing

fn string(value: &Value, key: &str) -> Result<String, ProviderError> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ProviderError::decode(NAME, format!("missing `{key}`")))
}

/// An id that may be expanded into an object (`"cus_1"` or `{"id": "cus_1"}`).
fn id_of(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value["id"].as_str())
        .map(str::to_owned)
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_i64()
        .and_then(|secs| Utc.timestamp_opt(secs, 0).single())
}

fn data(list: &Value) -> Result<&Vec<Value>, ProviderError> {
    list["data"]
        .as_array()
        .ok_or_else(|| ProviderError::decode(NAME, "a list without `data`"))
}

fn metadata(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Maps a Stripe Subscription object.
pub(crate) fn parse_subscription(value: &Value) -> Result<ProviderSubscription, ProviderError> {
    let id = string(value, "id")?;
    let customer_id = id_of(&value["customer"])
        .ok_or_else(|| ProviderError::decode(NAME, "a subscription without `customer`"))?;
    let raw = value["status"].as_str().unwrap_or_default();
    let paused = !value["pause_collection"].is_null();
    let status = match raw {
        "trialing" => SubscriptionStatus::Trialing,
        "active" if paused => SubscriptionStatus::Paused,
        "active" => SubscriptionStatus::Active,
        "past_due" | "unpaid" => SubscriptionStatus::PastDue,
        "paused" => SubscriptionStatus::Paused,
        "canceled" | "incomplete_expired" => SubscriptionStatus::Canceled,
        "incomplete" => SubscriptionStatus::Incomplete,
        other => {
            return Err(ProviderError::decode(
                NAME,
                format!("unknown subscription status `{other}`"),
            ));
        }
    };
    let raw_items = value["items"]["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::with_capacity(raw_items.len());
    for item in &raw_items {
        let price = &item["price"];
        let price_id = id_of(price)
            .ok_or_else(|| ProviderError::decode(NAME, "a subscription item without a price"))?;
        let metered = price["recurring"]["usage_type"].as_str() == Some("metered");
        items.push(ProviderItem {
            id: item["id"].as_str().map(str::to_owned),
            price_id,
            quantity: if metered {
                None
            } else {
                item["quantity"].as_u64().or(Some(1))
            },
        });
    }
    // Since 2025-03-31 the period lives on the items; older versions have it
    // on the subscription.
    let first_item = raw_items.first().cloned().unwrap_or(Value::Null);
    let current_period_start = timestamp(&value["current_period_start"])
        .or_else(|| timestamp(&first_item["current_period_start"]));
    let current_period_end = timestamp(&value["current_period_end"])
        .or_else(|| timestamp(&first_item["current_period_end"]));
    let ends_at = if status == SubscriptionStatus::Canceled {
        timestamp(&value["ended_at"]).or_else(|| timestamp(&value["canceled_at"]))
    } else if value["cancel_at_period_end"].as_bool() == Some(true) {
        current_period_end.or_else(|| timestamp(&value["cancel_at"]))
    } else {
        timestamp(&value["cancel_at"])
    };
    Ok(ProviderSubscription {
        id,
        customer_id,
        status,
        items,
        trial_ends_at: timestamp(&value["trial_end"]),
        current_period_start,
        current_period_end,
        ends_at,
        metadata: metadata(&value["metadata"]),
    })
}

/// Maps a Stripe Invoice object for listing.
pub(crate) fn parse_invoice(value: &Value) -> Result<Invoice, ProviderError> {
    let status = match value["status"].as_str().unwrap_or("draft") {
        "open" => InvoiceStatus::Open,
        "paid" => InvoiceStatus::Paid,
        "void" => InvoiceStatus::Void,
        "uncollectible" => InvoiceStatus::Uncollectible,
        _ => InvoiceStatus::Draft,
    };
    Ok(Invoice {
        id: string(value, "id")?,
        number: value["number"].as_str().unwrap_or_default().to_owned(),
        status,
        total_cents: value["total"].as_i64().unwrap_or_default(),
        currency: value["currency"].as_str().unwrap_or_default().to_owned(),
        pdf_url: value["invoice_pdf"].as_str().map(str::to_owned),
        hosted_url: value["hosted_invoice_url"].as_str().map(str::to_owned),
        created_at: timestamp(&value["created"]).unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
    })
}

/// Maps a Stripe Invoice object of a payment event.
fn parse_payment(value: &Value, paid: bool) -> Result<ProviderInvoice, ProviderError> {
    let subscription = id_of(&value["subscription"])
        .or_else(|| id_of(&value["parent"]["subscription_details"]["subscription"]));
    let amount = if paid {
        value["amount_paid"].as_i64()
    } else {
        value["amount_due"].as_i64()
    };
    Ok(ProviderInvoice {
        id: string(value, "id")?,
        customer_id: id_of(&value["customer"]),
        subscription_id: subscription,
        amount_cents: amount
            .or_else(|| value["total"].as_i64())
            .unwrap_or_default(),
        currency: value["currency"].as_str().unwrap_or_default().to_owned(),
    })
}

/// Maps a Checkout Session in `payment` mode; `status` overrides the
/// session's own (the event says what happened).
pub(crate) fn parse_session_payment(
    value: &Value,
    status: Option<PaymentStatus>,
) -> Result<ProviderPayment, ProviderError> {
    let metadata = metadata(&value["metadata"]);
    let status = status.unwrap_or_else(|| {
        match (
            value["status"].as_str().unwrap_or_default(),
            value["payment_status"].as_str().unwrap_or_default(),
        ) {
            ("complete", "unpaid") => PaymentStatus::Processing,
            ("complete", _) => PaymentStatus::Paid,
            ("expired", _) => PaymentStatus::Expired,
            _ => PaymentStatus::Pending,
        }
    });
    let methods = value["payment_method_types"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let method = match methods.as_slice() {
        [only] => only.as_str().map(method_of),
        _ => None,
    };
    let next_action = match (status, value["url"].as_str()) {
        (PaymentStatus::Pending, Some(url)) => Some(PaymentAction::Redirect {
            url: url.to_owned(),
        }),
        _ => None,
    };
    Ok(ProviderPayment {
        id: string(value, "id")?,
        payment_id: id_of(&value["payment_intent"]),
        link_id: id_of(&value["payment_link"]),
        reference: value["client_reference_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| metadata.get(REFERENCE_KEY).cloned()),
        status,
        amount: Money::new(
            value["amount_total"].as_i64().unwrap_or_default(),
            value["currency"].as_str().unwrap_or_default(),
        ),
        method,
        customer_id: id_of(&value["customer"]),
        next_action,
        metadata,
    })
}

/// Maps a Stripe Refund object.
pub(crate) fn parse_refund(value: &Value) -> Result<ProviderRefund, ProviderError> {
    let metadata = metadata(&value["metadata"]);
    let status = match value["status"].as_str().unwrap_or_default() {
        "succeeded" => RefundStatus::Succeeded,
        "failed" => RefundStatus::Failed,
        "canceled" => RefundStatus::Canceled,
        // `pending` and `requires_action`.
        _ => RefundStatus::Pending,
    };
    Ok(ProviderRefund {
        id: string(value, "id")?,
        payment_id: id_of(&value["payment_intent"]).or_else(|| id_of(&value["charge"])),
        reference: metadata.get(REFERENCE_KEY).cloned(),
        status,
        amount: Money::new(
            value["amount"].as_i64().unwrap_or_default(),
            value["currency"].as_str().unwrap_or_default(),
        ),
        metadata,
    })
}

/// Whether a Checkout Session takes a one-time payment (not a
/// subscription).
fn is_payment_session(object: &Value) -> bool {
    object["mode"].as_str() == Some("payment")
}

/// Parses a Stripe event body.
pub(crate) fn parse_event(body: &[u8]) -> Result<WebhookEvent, ProviderError> {
    let event: Value = serde_json::from_slice(body)
        .map_err(|error| ProviderError::decode(NAME, format!("invalid JSON: {error}")))?;
    let id = string(&event, "id")?;
    let event_type = string(&event, "type")?;
    let occurred_at = timestamp(&event["created"])
        .ok_or_else(|| ProviderError::decode(NAME, "an event without `created`"))?;
    let object = &event["data"]["object"];
    let actions = match event_type.as_str() {
        "checkout.session.completed" => {
            let mut actions = match id_of(&object["customer"]) {
                Some(customer_id) => vec![WebhookAction::CustomerLinked {
                    customer_id,
                    billable: crate::provider::billable_of(&metadata(&object["metadata"])),
                    email: object["customer_details"]["email"]
                        .as_str()
                        .or_else(|| object["customer_email"].as_str())
                        .map(str::to_owned),
                }],
                None => Vec::new(),
            };
            if is_payment_session(object) {
                actions.push(WebhookAction::Payment(parse_session_payment(object, None)?));
            }
            actions
        }
        "checkout.session.async_payment_succeeded"
        | "checkout.session.async_payment_failed"
        | "checkout.session.expired"
            if is_payment_session(object) =>
        {
            let status = match event_type.as_str() {
                "checkout.session.async_payment_succeeded" => PaymentStatus::Paid,
                "checkout.session.async_payment_failed" => PaymentStatus::Failed,
                _ => PaymentStatus::Expired,
            };
            vec![WebhookAction::Payment(parse_session_payment(
                object,
                Some(status),
            )?)]
        }
        "refund.created" | "refund.updated" | "refund.failed" => {
            vec![WebhookAction::Refund(parse_refund(object)?)]
        }
        "customer.subscription.created"
        | "customer.subscription.updated"
        | "customer.subscription.deleted"
        | "customer.subscription.paused"
        | "customer.subscription.resumed"
        | "customer.subscription.pending_update_applied"
        | "customer.subscription.pending_update_expired" => {
            vec![WebhookAction::Subscription(parse_subscription(object)?)]
        }
        "customer.subscription.trial_will_end" => {
            vec![WebhookAction::TrialEnding(parse_subscription(object)?)]
        }
        "invoice.paid" => vec![WebhookAction::InvoicePaid(parse_payment(object, true)?)],
        "invoice.payment_failed" => {
            vec![WebhookAction::PaymentFailed(parse_payment(object, false)?)]
        }
        _ => Vec::new(),
    };
    Ok(WebhookEvent {
        id,
        event_type,
        occurred_at,
        actions,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn request() -> CheckoutRequest {
        CheckoutRequest {
            billable_type: "workspace".into(),
            billable_id: Uuid::nil(),
            customer_id: Some("cus_1".into()),
            customer_email: Some("ada@example.com".into()),
            plan: "pro".into(),
            interval: crate::plan::Interval::Monthly,
            price_id: "price_pro".into(),
            metered_prices: vec!["price_ai".into()],
            quantity: 3,
            trial_days: Some(14),
            coupon: None,
            allow_promotion_codes: true,
            success_url: "https://app.test/ok".into(),
            cancel_url: "https://app.test/back".into(),
            metadata: BTreeMap::from([("source".to_owned(), "upgrade".to_owned())]),
            idempotency_key: "checkout-key".into(),
        }
    }

    fn field<'a>(form: &'a Form, key: &str) -> Option<&'a str> {
        form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn payment() -> PaymentRequest {
        PaymentRequest {
            billable: Some(("workspace".into(), Uuid::nil())),
            amount: Money::inr(149_900),
            description: "Annual support, Ludhiana office".into(),
            reference: "INV-2026-0042".into(),
            customer: crate::provider::PaymentCustomer {
                customer_id: None,
                name: Some("Harpreet Kaur".into()),
                email: Some("harpreet@example.in".into()),
                phone: Some("+919876543210".into()),
            },
            methods: Vec::new(),
            success_url: Some("https://app.test/paid".into()),
            cancel_url: None,
            expires_at: Some(DateTime::from_timestamp(1_800_000_000, 0).unwrap()),
            metadata: BTreeMap::from([("order".to_owned(), "42".to_owned())]),
            idempotency_key: "payment-INV-2026-0042".into(),
        }
    }

    #[test]
    fn one_time_payments_are_payment_mode_sessions_in_the_smallest_unit() {
        let form = payment_form(&payment()).unwrap();
        assert_eq!(field(&form, "mode"), Some("payment"));
        assert_eq!(
            field(&form, "line_items[0][price_data][unit_amount]"),
            Some("149900")
        );
        assert_eq!(
            field(&form, "line_items[0][price_data][currency]"),
            Some("inr")
        );
        assert_eq!(
            field(&form, "line_items[0][price_data][product_data][name]"),
            Some("Annual support, Ludhiana office")
        );
        assert_eq!(field(&form, "client_reference_id"), Some("INV-2026-0042"));
        assert_eq!(field(&form, "customer_email"), Some("harpreet@example.in"));
        assert_eq!(field(&form, "expires_at"), Some("1800000000"));
        assert_eq!(field(&form, "cancel_url"), None);
        for prefix in ["metadata", "payment_intent_data[metadata]"] {
            assert_eq!(
                field(&form, &format!("{prefix}[reference]")),
                Some("INV-2026-0042")
            );
            assert_eq!(
                field(&form, &format!("{prefix}[billable_type]")),
                Some("workspace")
            );
            assert_eq!(
                field(&form, &format!("{prefix}[navree_kind]")),
                Some("payment")
            );
            assert_eq!(field(&form, &format!("{prefix}[order]")), Some("42"));
        }
        assert_eq!(field(&form, "allowed_payment_method_types[0]"), None);

        let mut cards = payment();
        cards.methods = vec![PaymentMethod::Card, PaymentMethod::Upi];
        let form = payment_form(&cards).unwrap();
        assert_eq!(
            field(&form, "allowed_payment_method_types[0]"),
            Some("card")
        );
        assert_eq!(field(&form, "allowed_payment_method_types[1]"), Some("upi"));
        assert_eq!(field(&form, "payment_method_types[0]"), None);
        cards.methods = vec![PaymentMethod::NetBanking];
        assert!(matches!(
            payment_form(&cards),
            Err(ProviderError::Unsupported { .. })
        ));
    }

    #[test]
    fn payment_links_sell_one_price_once() {
        let request = PaymentLinkRequest {
            billable: None,
            amount: Money::inr(50_000),
            description: "Workshop seat, Amritsar".into(),
            reference: "SEAT-7".into(),
            customer: Default::default(),
            methods: Vec::new(),
            expires_at: None,
            notify_by_email: false,
            notify_by_sms: false,
            return_url: Some("https://app.test/thanks".into()),
            metadata: BTreeMap::new(),
            idempotency_key: "link-SEAT-7".into(),
        };
        let price = link_price_form(&request);
        assert_eq!(field(&price, "unit_amount"), Some("50000"));
        assert_eq!(field(&price, "currency"), Some("inr"));
        assert_eq!(
            field(&price, "product_data[name]"),
            Some("Workshop seat, Amritsar")
        );
        let form = link_form(&request, "price_1").unwrap();
        assert_eq!(field(&form, "line_items[0][price]"), Some("price_1"));
        assert_eq!(field(&form, "line_items[0][quantity]"), Some("1"));
        assert_eq!(
            field(&form, "restrictions[completed_sessions][limit]"),
            Some("1")
        );
        assert_eq!(field(&form, "after_completion[type]"), Some("redirect"));
        assert_eq!(field(&form, "metadata[reference]"), Some("SEAT-7"));
        assert_eq!(field(&form, "metadata[navree_kind]"), Some("payment_link"));
        let upi = PaymentLinkRequest {
            methods: vec![PaymentMethod::Upi],
            ..request
        };
        let form = link_form(&upi, "price_1").unwrap();
        assert_eq!(field(&form, "payment_method_types[0]"), Some("upi"));
    }

    #[test]
    fn stripe_method_names_map_both_ways() {
        assert_eq!(
            method_types(&[PaymentMethod::Card, PaymentMethod::Upi]).unwrap(),
            ["card", "upi"]
        );
        assert_eq!(method_of("upi"), PaymentMethod::Upi);
        assert_eq!(method_of("card"), PaymentMethod::Card);
        assert_eq!(
            method_of("us_bank_account"),
            PaymentMethod::Other("us_bank_account".into())
        );
        let session = serde_json::json!({
            "id": "cs_test_upi",
            "status": "complete",
            "payment_status": "paid",
            "payment_method_types": ["upi"],
            "amount_total": 149_900,
            "currency": "inr",
        });
        assert_eq!(
            parse_session_payment(&session, None).unwrap().method,
            Some(PaymentMethod::Upi)
        );
    }

    #[test]
    fn refunds_name_the_payment_intent_or_the_charge() {
        let mut request = RefundRequest {
            payment_id: "pi_3Q8".into(),
            amount: Some(Money::inr(20_000)),
            reason: crate::provider::RefundReason::Duplicate,
            note: Some("charged twice".into()),
            reference: "RF-9".into(),
            billable: None,
            metadata: BTreeMap::new(),
            idempotency_key: "refund-RF-9".into(),
        };
        let form = refund_form(&request);
        assert_eq!(field(&form, "payment_intent"), Some("pi_3Q8"));
        assert_eq!(field(&form, "amount"), Some("20000"));
        assert_eq!(field(&form, "reason"), Some("duplicate"));
        assert_eq!(field(&form, "metadata[reference]"), Some("RF-9"));
        assert_eq!(field(&form, "metadata[note]"), Some("charged twice"));
        request.payment_id = "ch_3Q8".into();
        request.amount = None;
        let form = refund_form(&request);
        assert_eq!(field(&form, "charge"), Some("ch_3Q8"));
        assert_eq!(field(&form, "amount"), None, "the rest of the payment");
    }

    #[test]
    fn keys_are_per_call_or_derived_within_the_limit() {
        let (a, b) = (call_key("portal"), call_key("portal"));
        assert_ne!(a, b, "each call gets its own key");
        assert!(a.starts_with("navree-portal-") && a.len() <= MAX_KEY_LEN);
        assert_eq!(sub_key("link-SEAT-7", "price"), "link-SEAT-7-price");
        let long = "k".repeat(255);
        let derived = sub_key(&long, "price");
        assert!(derived.len() <= MAX_KEY_LEN, "{derived}");
        assert_eq!(derived, sub_key(&long, "price"), "stable");
        assert_ne!(derived, sub_key(&long, "link"));
    }

    #[test]
    fn payment_and_refund_events_map_to_actions() {
        let session = json!({
            "id": "cs_test_pay1", "object": "checkout.session", "mode": "payment",
            "status": "complete", "payment_status": "paid", "amount_total": 149_900,
            "currency": "inr", "customer": "cus_1", "payment_intent": "pi_1",
            "client_reference_id": "INV-2026-0042",
            "metadata": { "billable_type": "workspace", "billable_id": Uuid::nil(), "reference": "INV-2026-0042" }
        });
        let event = json!({ "id": "evt_p", "type": "checkout.session.completed", "created": 1, "data": { "object": session } });
        let parsed = parse_event(event.to_string().as_bytes()).unwrap();
        let [
            WebhookAction::CustomerLinked { .. },
            WebhookAction::Payment(paid),
        ] = parsed.actions.as_slice()
        else {
            panic!("{:?}", parsed.actions);
        };
        assert_eq!(paid.status, PaymentStatus::Paid);
        assert_eq!(paid.amount, Money::inr(149_900));
        assert_eq!(paid.payment_id.as_deref(), Some("pi_1"));
        assert_eq!(paid.reference.as_deref(), Some("INV-2026-0042"));
        assert_eq!(paid.billable(), Some(("workspace".to_owned(), Uuid::nil())));

        for (kind, status) in [
            (
                "checkout.session.async_payment_succeeded",
                PaymentStatus::Paid,
            ),
            (
                "checkout.session.async_payment_failed",
                PaymentStatus::Failed,
            ),
            ("checkout.session.expired", PaymentStatus::Expired),
        ] {
            let event =
                json!({ "id": "evt_q", "type": kind, "created": 1, "data": { "object": session } });
            let parsed = parse_event(event.to_string().as_bytes()).unwrap();
            let [WebhookAction::Payment(payment)] = parsed.actions.as_slice() else {
                panic!("{kind}: {:?}", parsed.actions);
            };
            assert_eq!(payment.status, status, "{kind}");
        }
        // A subscription checkout is not a one-time payment.
        let mut subscription = session.clone();
        subscription["mode"] = json!("subscription");
        let event = json!({ "id": "evt_s", "type": "checkout.session.expired", "created": 1, "data": { "object": subscription } });
        assert!(
            parse_event(event.to_string().as_bytes())
                .unwrap()
                .actions
                .is_empty()
        );

        let refund = json!({
            "id": "re_1", "object": "refund", "amount": 20_000, "currency": "inr",
            "status": "succeeded", "payment_intent": "pi_1",
            "metadata": { "reference": "RF-9" }
        });
        let event = json!({ "id": "evt_r", "type": "refund.updated", "created": 1, "data": { "object": refund } });
        let parsed = parse_event(event.to_string().as_bytes()).unwrap();
        let [WebhookAction::Refund(refund)] = parsed.actions.as_slice() else {
            panic!("{:?}", parsed.actions);
        };
        assert_eq!(refund.status, RefundStatus::Succeeded);
        assert_eq!(refund.amount, Money::inr(20_000));
        assert_eq!(refund.payment_id.as_deref(), Some("pi_1"));
        assert_eq!(refund.reference.as_deref(), Some("RF-9"));
    }

    #[test]
    fn checkout_sessions_carry_the_billable_and_the_options() {
        let form = checkout_form(&request());
        assert_eq!(field(&form, "mode"), Some("subscription"));
        assert_eq!(field(&form, "customer"), Some("cus_1"));
        assert_eq!(field(&form, "customer_email"), None, "customer wins");
        assert_eq!(field(&form, "line_items[0][price]"), Some("price_pro"));
        assert_eq!(field(&form, "line_items[0][quantity]"), Some("3"));
        assert_eq!(field(&form, "line_items[1][price]"), Some("price_ai"));
        assert_eq!(field(&form, "line_items[1][quantity]"), None);
        assert_eq!(
            field(&form, "subscription_data[trial_period_days]"),
            Some("14")
        );
        assert_eq!(
            field(&form, "subscription_data[metadata][billable_type]"),
            Some("workspace")
        );
        assert_eq!(field(&form, "metadata[plan]"), Some("pro"));
        assert_eq!(field(&form, "metadata[source]"), Some("upgrade"));
        assert_eq!(field(&form, "allow_promotion_codes"), Some("true"));

        let mut coupon = request();
        coupon.coupon = Some("LAUNCH".into());
        coupon.customer_id = None;
        let form = checkout_form(&coupon);
        assert_eq!(field(&form, "discounts[0][coupon]"), Some("LAUNCH"));
        assert_eq!(field(&form, "allow_promotion_codes"), None);
        assert_eq!(field(&form, "customer_email"), Some("ada@example.com"));
        coupon.coupon = Some("promo_123".into());
        let form = checkout_form(&coupon);
        assert_eq!(
            field(&form, "discounts[0][promotion_code]"),
            Some("promo_123")
        );
    }

    #[test]
    fn changes_replace_the_base_item_and_the_meters() {
        let form = change_form(&SubscriptionChange {
            base_item_id: Some("si_base".into()),
            price_id: "price_business".into(),
            quantity: 4,
            add_prices: vec!["price_new_meter".into()],
            remove_item_ids: vec!["si_old_meter".into()],
            proration: Proration::InvoiceNow,
        });
        assert_eq!(field(&form, "items[0][id]"), Some("si_base"));
        assert_eq!(field(&form, "items[0][price]"), Some("price_business"));
        assert_eq!(field(&form, "items[0][quantity]"), Some("4"));
        assert_eq!(field(&form, "items[1][id]"), Some("si_old_meter"));
        assert_eq!(field(&form, "items[1][deleted]"), Some("true"));
        assert_eq!(field(&form, "items[2][price]"), Some("price_new_meter"));
        assert_eq!(field(&form, "proration_behavior"), Some("always_invoice"));
    }

    #[test]
    fn subscriptions_map_status_items_periods_and_cancellation() {
        let sub = parse_subscription(&json!({
            "id": "sub_1",
            "customer": "cus_1",
            "status": "trialing",
            "trial_end": 1_700_000_000,
            "cancel_at_period_end": true,
            "pause_collection": null,
            "metadata": { "billable_type": "workspace", "billable_id": Uuid::nil(), "n": 1 },
            "items": { "data": [
                { "id": "si_1", "quantity": 2, "price": { "id": "price_pro", "recurring": { "usage_type": "licensed" } },
                  "current_period_start": 1_699_000_000, "current_period_end": 1_701_000_000 },
                { "id": "si_2", "price": { "id": "price_ai", "recurring": { "usage_type": "metered" } } }
            ] }
        }))
        .unwrap();
        assert_eq!(sub.status, SubscriptionStatus::Trialing);
        assert_eq!(sub.items[0].quantity, Some(2));
        assert_eq!(sub.items[1].quantity, None);
        assert_eq!(sub.current_period_end.unwrap().timestamp(), 1_701_000_000);
        assert_eq!(sub.ends_at, sub.current_period_end, "cancel at period end");
        assert_eq!(sub.trial_ends_at.unwrap().timestamp(), 1_700_000_000);
        assert_eq!(sub.billable(), Some(("workspace".to_owned(), Uuid::nil())));
        assert!(!sub.metadata.contains_key("n"), "only string metadata");

        let paused = parse_subscription(&json!({
            "id": "sub_1", "customer": { "id": "cus_1" }, "status": "active",
            "pause_collection": { "behavior": "void" }, "items": { "data": [] }
        }))
        .unwrap();
        assert_eq!(paused.status, SubscriptionStatus::Paused);
        assert_eq!(paused.customer_id, "cus_1");

        let ended = parse_subscription(&json!({
            "id": "sub_1", "customer": "cus_1", "status": "canceled",
            "ended_at": 1_700_000_500, "canceled_at": 1_700_000_400,
            "current_period_end": 1_701_000_000, "items": { "data": [] }
        }))
        .unwrap();
        assert_eq!(ended.ends_at.unwrap().timestamp(), 1_700_000_500);
        assert_eq!(ended.current_period_end.unwrap().timestamp(), 1_701_000_000);

        assert!(
            parse_subscription(&json!({ "id": "sub_1", "customer": "cus_1", "status": "weird" }))
                .is_err()
        );
    }

    #[test]
    fn events_map_to_actions() {
        let body = json!({
            "id": "evt_1", "type": "invoice.payment_failed", "created": 1_700_000_000,
            "data": { "object": {
                "id": "in_1", "customer": "cus_1", "amount_due": 2900, "amount_paid": 0,
                "currency": "usd",
                "parent": { "subscription_details": { "subscription": "sub_1" } }
            } }
        });
        let event = parse_event(body.to_string().as_bytes()).unwrap();
        assert_eq!(event.id, "evt_1");
        assert_eq!(event.occurred_at.timestamp(), 1_700_000_000);
        assert_eq!(
            event.actions,
            [WebhookAction::PaymentFailed(ProviderInvoice {
                id: "in_1".into(),
                customer_id: Some("cus_1".into()),
                subscription_id: Some("sub_1".into()),
                amount_cents: 2900,
                currency: "usd".into(),
            })]
        );
        let ignored = json!({ "id": "evt_2", "type": "charge.refunded", "created": 1, "data": { "object": {} } });
        assert!(
            parse_event(ignored.to_string().as_bytes())
                .unwrap()
                .actions
                .is_empty()
        );
        assert!(parse_event(b"not json").is_err());
        assert!(parse_event(br#"{"id":"evt_3","type":"x"}"#).is_err());
    }
}
