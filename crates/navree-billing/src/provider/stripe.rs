//! [`StripeProvider`]: a narrow Stripe client on navree-http-client, and the
//! mapping of Stripe's objects and events to provider-neutral values.
//!
//! Requests are form-encoded, authenticated with the secret key and pinned
//! to [`STRIPE_API_VERSION`] with the `Stripe-Version` header.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use http::{HeaderMap, Method};
use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde_json::Value;

use crate::invoice::{Invoice, InvoiceStatus};
use crate::provider::signature::verify_stripe;
use crate::provider::{
    BillingProvider, CheckoutRequest, NewCustomer, ProviderCheckout, ProviderError,
    ProviderInvoice, ProviderItem, ProviderSubscription, SubscriptionChange, UsageReport,
    WebhookAction, WebhookEndpoint, WebhookError, WebhookEvent,
};
use crate::subscription::{Proration, SubscriptionStatus};

/// The Stripe API version every request is pinned to (`Stripe-Version`).
/// Webhook endpoints created by `billing:webhook` use it too, so event
/// payloads have the shape this client reads.
pub const STRIPE_API_VERSION: &str = "2025-03-31.basil";

const NAME: &str = "stripe";

/// The Stripe events Navree handles (what `billing:webhook` subscribes to).
pub const STRIPE_EVENTS: &[&str] = &[
    "checkout.session.completed",
    "customer.subscription.created",
    "customer.subscription.updated",
    "customer.subscription.deleted",
    "customer.subscription.paused",
    "customer.subscription.resumed",
    "customer.subscription.trial_will_end",
    "invoice.paid",
    "invoice.payment_failed",
];

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
    ) -> Result<Value, ProviderError> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer(self.secret_key.expose())
            .header("Stripe-Version", STRIPE_API_VERSION);
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

    async fn post(&self, path: &str, form: &Form) -> Result<Value, ProviderError> {
        self.call(Method::POST, path, Some(form), &[]).await
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, ProviderError> {
        self.call(Method::GET, path, None, query).await
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

impl BillingProvider for StripeProvider {
    fn name(&self) -> &'static str {
        NAME
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
            let created = self.post("/v1/customers", &form).await?;
            Ok(Some(string(&created, "id")?))
        })
    }

    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<ProviderCheckout, ProviderError>> {
        Box::pin(async move {
            let session = self
                .post("/v1/checkout/sessions", &checkout_form(request))
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
                .post(&format!("/v1/subscriptions/{id}"), &change_form(change))
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
                self.post(&format!("/v1/subscriptions/{id}"), &form).await?
            } else {
                self.call(
                    Method::DELETE,
                    &format!("/v1/subscriptions/{id}"),
                    None,
                    &[],
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
            let updated = self.post(&format!("/v1/subscriptions/{id}"), &form).await?;
            parse_subscription(&updated)
        })
    }

    fn pause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let form = vec![("pause_collection[behavior]".to_owned(), "void".to_owned())];
            let updated = self.post(&format!("/v1/subscriptions/{id}"), &form).await?;
            parse_subscription(&updated)
        })
    }

    fn unpause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let form = vec![("pause_collection".to_owned(), String::new())];
            let updated = self.post(&format!("/v1/subscriptions/{id}"), &form).await?;
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
            self.post("/v1/billing/meter_events", &form).await?;
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
            let session = self.post("/v1/billing_portal/sessions", &form).await?;
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
            let endpoint = self.post("/v1/webhook_endpoints", &form).await?;
            Ok(WebhookEndpoint {
                id: string(&endpoint, "id")?,
                secret: endpoint["secret"].as_str().map(str::to_owned),
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
        "checkout.session.completed" => match id_of(&object["customer"]) {
            Some(customer_id) => vec![WebhookAction::CustomerLinked {
                customer_id,
                billable: crate::provider::billable_of(&metadata(&object["metadata"])),
                email: object["customer_details"]["email"]
                    .as_str()
                    .or_else(|| object["customer_email"].as_str())
                    .map(str::to_owned),
            }],
            None => Vec::new(),
        },
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
        }
    }

    fn field<'a>(form: &'a Form, key: &str) -> Option<&'a str> {
        form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
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
