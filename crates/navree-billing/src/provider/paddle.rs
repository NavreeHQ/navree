//! [`PaddleProvider`]: a narrow Paddle Billing client on navree-http-client,
//! and the mapping of Paddle's entities and notifications to
//! provider-neutral values.
//!
//! Requests are JSON, authenticated with the API key and pinned with
//! `Paddle-Version: 1`. Paddle is the merchant of record: checkout is a
//! transaction whose hosted page is the account's default payment link;
//! metered usage is a one-time charge added to the next renewal.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use http::{HeaderMap, Method};
use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde_json::{Value, json};

use crate::invoice::{Invoice, InvoiceStatus};
use crate::provider::signature::verify_paddle;
use crate::provider::{
    BillingProvider, CheckoutRequest, NewCustomer, ProviderCheckout, ProviderError,
    ProviderInvoice, ProviderItem, ProviderSubscription, SubscriptionChange, UsageReport,
    WebhookAction, WebhookEndpoint, WebhookError, WebhookEvent, billable_of,
};
use crate::subscription::{Proration, SubscriptionStatus};

const NAME: &str = "paddle";

/// The Paddle API version every request is pinned to (`Paddle-Version`).
pub const PADDLE_API_VERSION: &str = "1";

/// The Paddle events Navree handles (what `billing:webhook` subscribes to).
pub const PADDLE_EVENTS: &[&str] = &[
    "subscription.created",
    "subscription.updated",
    "subscription.activated",
    "subscription.trialing",
    "subscription.past_due",
    "subscription.paused",
    "subscription.resumed",
    "subscription.canceled",
    "transaction.completed",
    "transaction.payment_failed",
];

/// A Paddle Billing client: customers, transactions (checkout and
/// invoices), subscriptions, one-time charges, portal sessions, prices and
/// notification settings.
pub struct PaddleProvider {
    client: HttpClient,
    api_key: Secret<String>,
    webhook_secret: Option<Secret<String>>,
    base: String,
}

impl std::fmt::Debug for PaddleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaddleProvider")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl PaddleProvider {
    /// A client for the API at `base` (`https://sandbox-api.paddle.com` or
    /// `https://api.paddle.com`).
    pub fn new(
        client: HttpClient,
        api_key: Secret<String>,
        webhook_secret: Option<Secret<String>>,
        base: impl Into<String>,
    ) -> PaddleProvider {
        PaddleProvider {
            client,
            api_key,
            webhook_secret,
            base: base.into().trim_end_matches('/').to_owned(),
        }
    }

    /// Sends a request and returns the envelope's `data`.
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        query: &[(&str, &str)],
    ) -> Result<Value, ProviderError> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer(self.api_key.expose())
            .header("Paddle-Version", PADDLE_API_VERSION);
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ProviderError::Transport {
                provider: NAME,
                message: error.to_string(),
            })?;
        let status = response.status().as_u16();
        let mut envelope: Value = response.json().unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let message = envelope["error"]["detail"]
                .as_str()
                .unwrap_or("no error detail")
                .to_owned();
            return Err(ProviderError::Api {
                provider: NAME,
                status,
                message,
            });
        }
        Ok(envelope["data"].take())
    }

    async fn subscription_call(
        &self,
        method: Method,
        path: &str,
        body: &Value,
    ) -> Result<ProviderSubscription, ProviderError> {
        let data = self.call(method, path, Some(body), &[]).await?;
        parse_subscription(&data)
    }
}

fn proration(proration: Proration) -> &'static str {
    match proration {
        Proration::NextInvoice => "prorated_next_billing_period",
        Proration::InvoiceNow => "prorated_immediately",
        Proration::None => "do_not_bill",
    }
}

/// The transaction body of a checkout.
pub(crate) fn checkout_body(request: &CheckoutRequest) -> Value {
    let mut custom_data = request.metadata.clone();
    custom_data.insert("billable_type".into(), request.billable_type.clone());
    custom_data.insert("billable_id".into(), request.billable_id.to_string());
    custom_data.insert("plan".into(), request.plan.clone());
    let mut body = json!({
        "items": [{ "price_id": request.price_id, "quantity": request.quantity }],
        "custom_data": custom_data,
        "collection_mode": "automatic",
    });
    if let Some(customer) = &request.customer_id {
        body["customer_id"] = json!(customer);
    }
    if let Some(discount) = &request.coupon {
        body["discount_id"] = json!(discount);
    }
    body
}

impl BillingProvider for PaddleProvider {
    fn name(&self) -> &'static str {
        NAME
    }

    fn create_customer<'a>(
        &'a self,
        customer: &'a NewCustomer,
    ) -> BoxFuture<'a, Result<Option<String>, ProviderError>> {
        Box::pin(async move {
            // Paddle customers need an email; without one the checkout
            // collects it and the transaction's webhook links the customer.
            let Some(email) = &customer.email else {
                return Ok(None);
            };
            let mut body = json!({
                "email": email,
                "custom_data": {
                    "billable_type": customer.billable_type,
                    "billable_id": customer.billable_id.to_string(),
                },
            });
            if !customer.name.is_empty() {
                body["name"] = json!(customer.name);
            }
            let created = self
                .call(Method::POST, "/customers", Some(&body), &[])
                .await?;
            Ok(Some(string(&created, "id")?))
        })
    }

    fn create_checkout<'a>(
        &'a self,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<ProviderCheckout, ProviderError>> {
        Box::pin(async move {
            let transaction = self
                .call(
                    Method::POST,
                    "/transactions",
                    Some(&checkout_body(request)),
                    &[],
                )
                .await?;
            let url = transaction["checkout"]["url"].as_str().ok_or_else(|| {
                ProviderError::decode(
                    NAME,
                    "the transaction has no checkout URL: set a default payment link in Paddle",
                )
            })?;
            Ok(ProviderCheckout {
                id: string(&transaction, "id")?,
                url: url.to_owned(),
            })
        })
    }

    fn update_subscription<'a>(
        &'a self,
        id: &'a str,
        change: &'a SubscriptionChange,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let body = json!({
                "items": [{ "price_id": change.price_id, "quantity": change.quantity }],
                "proration_billing_mode": proration(change.proration),
            });
            self.subscription_call(Method::PATCH, &format!("/subscriptions/{id}"), &body)
                .await
        })
    }

    fn cancel_subscription<'a>(
        &'a self,
        id: &'a str,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let effective = if at_period_end {
                "next_billing_period"
            } else {
                "immediately"
            };
            let body = json!({ "effective_from": effective });
            self.subscription_call(Method::POST, &format!("/subscriptions/{id}/cancel"), &body)
                .await
        })
    }

    fn resume_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let body = json!({ "scheduled_change": null });
            self.subscription_call(Method::PATCH, &format!("/subscriptions/{id}"), &body)
                .await
        })
    }

    fn pause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let body = json!({ "effective_from": "immediately" });
            self.subscription_call(Method::POST, &format!("/subscriptions/{id}/pause"), &body)
                .await
        })
    }

    fn unpause_subscription<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<ProviderSubscription, ProviderError>> {
        Box::pin(async move {
            let body = json!({ "effective_from": "immediately" });
            self.subscription_call(Method::POST, &format!("/subscriptions/{id}/resume"), &body)
                .await
        })
    }

    fn subscriptions<'a>(
        &'a self,
        customer_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ProviderSubscription>, ProviderError>> {
        Box::pin(async move {
            let list = self
                .call(
                    Method::GET,
                    "/subscriptions",
                    None,
                    &[("customer_id", customer_id), ("per_page", "200")],
                )
                .await?;
            list.as_array()
                .ok_or_else(|| ProviderError::decode(NAME, "a list that is not an array"))?
                .iter()
                .map(parse_subscription)
                .collect()
        })
    }

    fn report_usage<'a>(
        &'a self,
        report: &'a UsageReport,
    ) -> BoxFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            let body = json!({
                "effective_from": "next_billing_period",
                "items": [{ "price_id": report.price_id, "quantity": report.quantity }],
            });
            self.call(
                Method::POST,
                &format!("/subscriptions/{}/charge", report.subscription_id),
                Some(&body),
                &[],
            )
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
                .call(
                    Method::GET,
                    "/transactions",
                    None,
                    &[
                        ("customer_id", customer_id),
                        ("status", "billed,paid,completed,past_due,canceled"),
                        ("order_by", "created_at[DESC]"),
                        ("per_page", "100"),
                    ],
                )
                .await?;
            list.as_array()
                .ok_or_else(|| ProviderError::decode(NAME, "a list that is not an array"))?
                .iter()
                .map(parse_invoice)
                .collect()
        })
    }

    fn portal_url<'a>(
        &'a self,
        customer_id: &'a str,
        _return_url: &'a str,
        subscription_ids: &'a [String],
    ) -> BoxFuture<'a, Result<String, ProviderError>> {
        Box::pin(async move {
            let body = json!({ "subscription_ids": subscription_ids });
            let session = self
                .call(
                    Method::POST,
                    &format!("/customers/{customer_id}/portal-sessions"),
                    Some(&body),
                    &[],
                )
                .await?;
            session["urls"]["general"]["overview"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| ProviderError::decode(NAME, "a portal session without a URL"))
        })
    }

    fn price_exists<'a>(&'a self, price_id: &'a str) -> BoxFuture<'a, Result<bool, ProviderError>> {
        Box::pin(async move {
            match self
                .call(Method::GET, &format!("/prices/{price_id}"), None, &[])
                .await
            {
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
            let body = json!({
                "description": "Navree billing",
                "destination": url,
                "type": "url",
                "api_version": 1,
                "subscribed_events": PADDLE_EVENTS,
            });
            let setting = self
                .call(Method::POST, "/notification-settings", Some(&body), &[])
                .await?;
            Ok(WebhookEndpoint {
                id: string(&setting, "id")?,
                secret: setting["endpoint_secret_key"].as_str().map(str::to_owned),
                events: PADDLE_EVENTS.iter().map(|e| (*e).to_owned()).collect(),
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
        verify_paddle(headers, body, secret.expose(), now, tolerance)
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

fn time(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&Utc))
}

fn custom_data(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// An amount in the smallest unit, which Paddle sends as a string.
fn amount(value: &Value) -> i64 {
    value
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| value.as_i64())
        .unwrap_or_default()
}

/// Maps a Paddle subscription entity.
pub(crate) fn parse_subscription(value: &Value) -> Result<ProviderSubscription, ProviderError> {
    let id = string(value, "id")?;
    let customer_id = string(value, "customer_id")?;
    let status = match value["status"].as_str().unwrap_or_default() {
        "trialing" => SubscriptionStatus::Trialing,
        "active" => SubscriptionStatus::Active,
        "past_due" => SubscriptionStatus::PastDue,
        "paused" => SubscriptionStatus::Paused,
        "canceled" => SubscriptionStatus::Canceled,
        other => {
            return Err(ProviderError::decode(
                NAME,
                format!("unknown subscription status `{other}`"),
            ));
        }
    };
    let raw_items = value["items"].as_array().cloned().unwrap_or_default();
    let items = raw_items
        .iter()
        .map(|item| {
            Ok(ProviderItem {
                id: None,
                price_id: item["price"]["id"]
                    .as_str()
                    .or_else(|| item["price_id"].as_str())
                    .ok_or_else(|| ProviderError::decode(NAME, "an item without a price"))?
                    .to_owned(),
                quantity: item["quantity"].as_u64().or(Some(1)),
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    let trial_ends_at = raw_items
        .iter()
        .find_map(|item| time(&item["trial_dates"]["ends_at"]));
    let current_period_end = time(&value["current_billing_period"]["ends_at"]);
    let scheduled = &value["scheduled_change"];
    let ends_at = if status == SubscriptionStatus::Canceled {
        time(&value["canceled_at"])
    } else if scheduled["action"].as_str() == Some("cancel") {
        time(&scheduled["effective_at"]).or(current_period_end)
    } else {
        None
    };
    Ok(ProviderSubscription {
        id,
        customer_id,
        status,
        items,
        trial_ends_at,
        current_period_start: time(&value["current_billing_period"]["starts_at"]),
        current_period_end,
        ends_at,
        metadata: custom_data(&value["custom_data"]),
    })
}

/// Maps a Paddle transaction for listing.
pub(crate) fn parse_invoice(value: &Value) -> Result<Invoice, ProviderError> {
    let status = match value["status"].as_str().unwrap_or_default() {
        "completed" | "paid" => InvoiceStatus::Paid,
        "billed" | "past_due" => InvoiceStatus::Open,
        "canceled" => InvoiceStatus::Void,
        _ => InvoiceStatus::Draft,
    };
    Ok(Invoice {
        id: string(value, "id")?,
        number: value["invoice_number"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        status,
        total_cents: amount(&value["details"]["totals"]["grand_total"]),
        currency: value["currency_code"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase(),
        pdf_url: None,
        hosted_url: None,
        created_at: time(&value["created_at"]).unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
    })
}

fn parse_payment(value: &Value) -> Result<ProviderInvoice, ProviderError> {
    Ok(ProviderInvoice {
        id: string(value, "id")?,
        customer_id: value["customer_id"].as_str().map(str::to_owned),
        subscription_id: value["subscription_id"].as_str().map(str::to_owned),
        amount_cents: amount(&value["details"]["totals"]["grand_total"]),
        currency: value["currency_code"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase(),
    })
}

/// Parses a Paddle notification body.
pub(crate) fn parse_event(body: &[u8]) -> Result<WebhookEvent, ProviderError> {
    let event: Value = serde_json::from_slice(body)
        .map_err(|error| ProviderError::decode(NAME, format!("invalid JSON: {error}")))?;
    let id = string(&event, "event_id")?;
    let event_type = string(&event, "event_type")?;
    let occurred_at = time(&event["occurred_at"])
        .ok_or_else(|| ProviderError::decode(NAME, "a notification without `occurred_at`"))?;
    let data = &event["data"];
    let actions = match event_type.as_str() {
        kind if kind.starts_with("subscription.") => {
            vec![WebhookAction::Subscription(parse_subscription(data)?)]
        }
        "transaction.completed" => {
            let mut actions = Vec::new();
            if let Some(customer_id) = data["customer_id"].as_str() {
                actions.push(WebhookAction::CustomerLinked {
                    customer_id: customer_id.to_owned(),
                    billable: billable_of(&custom_data(&data["custom_data"])),
                    email: None,
                });
            }
            actions.push(WebhookAction::InvoicePaid(parse_payment(data)?));
            actions
        }
        "transaction.payment_failed" => vec![WebhookAction::PaymentFailed(parse_payment(data)?)],
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
    use uuid::Uuid;

    use super::*;

    #[test]
    fn checkout_transactions_carry_the_billable() {
        let body = checkout_body(&CheckoutRequest {
            billable_type: "user".into(),
            billable_id: Uuid::nil(),
            customer_id: Some("ctm_1".into()),
            customer_email: None,
            plan: "pro".into(),
            interval: crate::plan::Interval::Monthly,
            price_id: "pri_pro".into(),
            metered_prices: vec!["pri_ai".into()],
            quantity: 2,
            trial_days: Some(14),
            coupon: Some("dsc_1".into()),
            allow_promotion_codes: false,
            success_url: "https://app.test/ok".into(),
            cancel_url: "https://app.test/back".into(),
            metadata: BTreeMap::new(),
        });
        assert_eq!(
            body["items"],
            json!([{ "price_id": "pri_pro", "quantity": 2 }])
        );
        assert_eq!(body["customer_id"], "ctm_1");
        assert_eq!(body["discount_id"], "dsc_1");
        assert_eq!(body["custom_data"]["billable_type"], "user");
        assert_eq!(body["custom_data"]["plan"], "pro");
    }

    #[test]
    fn subscriptions_map_trials_periods_and_scheduled_cancels() {
        let sub = parse_subscription(&json!({
            "id": "sub_01", "customer_id": "ctm_01", "status": "trialing",
            "items": [{ "price": { "id": "pri_pro" }, "quantity": 3,
                        "trial_dates": { "starts_at": "2026-10-26T09:00:00Z", "ends_at": "2026-11-09T09:00:00Z" } }],
            "current_billing_period": { "starts_at": "2026-10-26T09:00:00Z", "ends_at": "2026-11-09T09:00:00Z" },
            "scheduled_change": { "action": "cancel", "effective_at": "2026-11-09T09:00:00Z" },
            "custom_data": { "billable_type": "workspace", "billable_id": Uuid::nil() }
        }))
        .unwrap();
        assert_eq!(sub.status, SubscriptionStatus::Trialing);
        assert_eq!(sub.items[0].quantity, Some(3));
        assert_eq!(sub.trial_ends_at, sub.current_period_end);
        assert_eq!(sub.ends_at, sub.current_period_end);
        assert_eq!(sub.billable(), Some(("workspace".to_owned(), Uuid::nil())));
        assert!(
            parse_subscription(&json!({ "id": "s", "customer_id": "c", "status": "x" })).is_err()
        );
    }

    #[test]
    fn transactions_link_customers_and_report_payments() {
        let body = json!({
            "event_id": "evt_01", "event_type": "transaction.completed",
            "occurred_at": "2026-10-26T09:00:00.123Z",
            "data": { "id": "txn_01", "customer_id": "ctm_01", "subscription_id": "sub_01",
                      "currency_code": "EUR", "details": { "totals": { "grand_total": "2900" } },
                      "custom_data": { "billable_type": "user", "billable_id": Uuid::nil() } }
        });
        let event = parse_event(body.to_string().as_bytes()).unwrap();
        assert_eq!(event.actions.len(), 2);
        assert_eq!(
            event.actions[1],
            WebhookAction::InvoicePaid(ProviderInvoice {
                id: "txn_01".into(),
                customer_id: Some("ctm_01".into()),
                subscription_id: Some("sub_01".into()),
                amount_cents: 2900,
                currency: "eur".into(),
            })
        );
        let invoice = parse_invoice(&body["data"]).unwrap();
        assert_eq!(invoice.total_cents, 2900);
        assert_eq!(invoice.status, InvoiceStatus::Draft);
    }
}
