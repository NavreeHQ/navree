//! Recorded-shape provider payloads: Stripe objects of API version
//! `2025-03-31.basil` and Paddle Billing entities (API version 1), as the
//! providers document them, trimmed to realistic fields.

#![allow(dead_code)]

use serde_json::{Value, json};
use uuid::Uuid;

/// 2026-10-26T09:00:00Z.
pub const NOW: i64 = 1_793_005_200;
/// 2026-11-09T09:00:00Z (a 14-day trial's end).
pub const TRIAL_END: i64 = 1_794_214_800;
/// 2026-11-26T09:00:00Z (a month later).
pub const PERIOD_END: i64 = 1_795_683_600;

pub const WORKSPACE: Uuid = Uuid::from_u128(0x0199_2f4e_7c1a_7000_8000_0000_0000_0001);

pub mod stripe {
    use super::*;

    pub fn customer() -> Value {
        json!({
            "id": "cus_QX1aBcDeF",
            "object": "customer",
            "created": NOW,
            "email": "ada@example.com",
            "livemode": false,
            "metadata": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string() },
            "name": "Acme"
        })
    }

    pub fn checkout_session() -> Value {
        json!({
            "id": "cs_test_a1B2c3D4",
            "object": "checkout.session",
            "created": NOW,
            "customer": "cus_QX1aBcDeF",
            "livemode": false,
            "mode": "subscription",
            "status": "open",
            "subscription": null,
            "url": "https://checkout.stripe.com/c/pay/cs_test_a1B2c3D4"
        })
    }

    fn item(id: &str, price: &str, quantity: Option<u64>, metered: bool, end: i64) -> Value {
        let mut item = json!({
            "id": id,
            "object": "subscription_item",
            "created": NOW,
            "current_period_end": end,
            "current_period_start": NOW,
            "metadata": {},
            "price": {
                "id": price,
                "object": "price",
                "active": true,
                "currency": "usd",
                "product": "prod_Q8Pro",
                "recurring": {
                    "interval": "month",
                    "interval_count": 1,
                    "meter": if metered { json!("mtr_test_61Q8AiSummaries") } else { Value::Null },
                    "usage_type": if metered { "metered" } else { "licensed" }
                },
                "type": "recurring",
                "unit_amount": if metered { json!(null) } else { json!(2900) }
            },
            "subscription": "sub_1Q8XyzAbC"
        });
        if let Some(quantity) = quantity {
            item["quantity"] = json!(quantity);
        }
        item
    }

    /// A subscription; `status` as Stripe sends it, `price` the base price.
    pub fn subscription(status: &str, price: &str, quantity: u64) -> Value {
        let trialing = status == "trialing";
        let end = if trialing { TRIAL_END } else { PERIOD_END };
        json!({
            "id": "sub_1Q8XyzAbC",
            "object": "subscription",
            "billing_cycle_anchor": end,
            "cancel_at": null,
            "cancel_at_period_end": false,
            "canceled_at": null,
            "collection_method": "charge_automatically",
            "created": NOW,
            "currency": "usd",
            "customer": "cus_QX1aBcDeF",
            "ended_at": null,
            "items": {
                "object": "list",
                "data": [
                    item("si_QX1base", price, Some(quantity), false, end),
                    item("si_QX1meter", "price_ai", None, true, end)
                ],
                "has_more": false,
                "total_count": 2,
                "url": "/v1/subscription_items?subscription=sub_1Q8XyzAbC"
            },
            "latest_invoice": "in_1Q8First",
            "livemode": false,
            "metadata": {
                "billable_type": "workspace",
                "billable_id": WORKSPACE.to_string(),
                "plan": "pro"
            },
            "pause_collection": null,
            "start_date": NOW,
            "status": status,
            "trial_end": if trialing { json!(TRIAL_END) } else { Value::Null },
            "trial_start": if trialing { json!(NOW) } else { Value::Null }
        })
    }

    pub fn canceling(price: &str) -> Value {
        let mut sub = subscription("active", price, 2);
        sub["cancel_at_period_end"] = json!(true);
        sub["cancel_at"] = json!(PERIOD_END);
        sub["canceled_at"] = json!(NOW);
        sub
    }

    pub fn canceled(price: &str) -> Value {
        let mut sub = subscription("canceled", price, 2);
        sub["canceled_at"] = json!(NOW);
        sub["ended_at"] = json!(NOW);
        sub
    }

    pub fn paused(price: &str) -> Value {
        let mut sub = subscription("active", price, 2);
        sub["pause_collection"] = json!({ "behavior": "void", "resumes_at": null });
        sub
    }

    pub fn invoice(status: &str, amount: i64) -> Value {
        json!({
            "id": "in_1Q8First",
            "object": "invoice",
            "amount_due": amount,
            "amount_paid": if status == "paid" { amount } else { 0 },
            "created": NOW,
            "currency": "usd",
            "customer": "cus_QX1aBcDeF",
            "hosted_invoice_url": "https://invoice.stripe.com/i/acct_1/test_YWNjdA",
            "invoice_pdf": "https://pay.stripe.com/invoice/acct_1/test_YWNjdA/pdf",
            "livemode": false,
            "number": "A1B2C3D4-0001",
            "parent": {
                "quote_details": null,
                "subscription_details": { "metadata": {}, "subscription": "sub_1Q8XyzAbC" },
                "type": "subscription_details"
            },
            "status": status,
            "total": amount
        })
    }

    pub fn list(data: Vec<Value>) -> Value {
        json!({ "object": "list", "data": data, "has_more": false, "url": "/v1/x" })
    }

    pub fn metered_price() -> Value {
        json!({
            "id": "price_ai",
            "object": "price",
            "active": true,
            "billing_scheme": "per_unit",
            "currency": "usd",
            "recurring": {
                "interval": "month",
                "interval_count": 1,
                "meter": "mtr_test_61Q8AiSummaries",
                "usage_type": "metered"
            },
            "type": "recurring",
            "unit_amount": 2
        })
    }

    pub fn meter() -> Value {
        json!({
            "id": "mtr_test_61Q8AiSummaries",
            "object": "billing.meter",
            "default_aggregation": { "formula": "sum" },
            "display_name": "AI summaries",
            "event_name": "ai_summary_generated",
            "status": "active"
        })
    }

    pub fn meter_event() -> Value {
        json!({
            "object": "billing.meter_event",
            "created": NOW,
            "event_name": "ai_summary_generated",
            "identifier": "navree-usage-x",
            "livemode": false,
            "payload": { "stripe_customer_id": "cus_QX1aBcDeF", "value": "3" },
            "timestamp": NOW
        })
    }

    pub fn portal_session() -> Value {
        json!({
            "id": "bps_1Q8",
            "object": "billing_portal.session",
            "customer": "cus_QX1aBcDeF",
            "return_url": "https://app.acme.test/settings/billing",
            "url": "https://billing.stripe.com/p/session/test_YWNjdA"
        })
    }

    pub fn webhook_endpoint() -> Value {
        json!({
            "id": "we_1Q8Endpoint",
            "object": "webhook_endpoint",
            "api_version": "2025-03-31.basil",
            "enabled_events": ["checkout.session.completed"],
            "secret": "whsec_created_by_api",
            "status": "enabled",
            "url": "https://api.acme.test/api/billing/webhook"
        })
    }

    pub fn error(message: &str) -> Value {
        json!({ "error": { "message": message, "type": "invalid_request_error", "code": "resource_missing" } })
    }

    pub fn event(id: &str, kind: &str, object: Value) -> Value {
        json!({
            "id": id,
            "object": "event",
            "api_version": "2025-03-31.basil",
            "created": NOW,
            "data": { "object": object },
            "livemode": false,
            "pending_webhooks": 1,
            "request": { "id": null, "idempotency_key": null },
            "type": kind
        })
    }
}

pub mod paddle {
    use super::*;

    fn iso(secs: i64) -> String {
        chrono::DateTime::from_timestamp(secs, 0)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    pub fn envelope(data: Value) -> Value {
        json!({ "data": data, "meta": { "request_id": "c8d1a2f0-5b6e-4a1b-9c3d-7e8f9a0b1c2d" } })
    }

    pub fn customer() -> Value {
        json!({
            "id": "ctm_01j8acme",
            "status": "active",
            "custom_data": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string() },
            "name": "Acme",
            "email": "ada@example.com",
            "marketing_consent": false,
            "locale": "en",
            "created_at": iso(NOW),
            "updated_at": iso(NOW)
        })
    }

    pub fn checkout_transaction() -> Value {
        json!({
            "id": "txn_01j8checkout",
            "status": "ready",
            "customer_id": "ctm_01j8acme",
            "currency_code": "USD",
            "origin": "api",
            "subscription_id": null,
            "collection_mode": "automatic",
            "custom_data": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string(), "plan": "pro" },
            "items": [{ "price": { "id": "pri_pro_m" }, "quantity": 2 }],
            "details": { "totals": { "subtotal": "5800", "tax": "0", "total": "5800", "grand_total": "5800", "currency_code": "USD" } },
            "checkout": { "url": "https://acme.test/pay?_ptxn=txn_01j8checkout" },
            "created_at": iso(NOW),
            "updated_at": iso(NOW)
        })
    }

    /// A subscription; `status` as Paddle sends it.
    pub fn subscription(status: &str, price: &str, quantity: u64) -> Value {
        let trialing = status == "trialing";
        let end = if trialing { TRIAL_END } else { PERIOD_END };
        let mut item = json!({
            "status": if trialing { "trialing" } else { "active" },
            "quantity": quantity,
            "recurring": true,
            "created_at": iso(NOW),
            "updated_at": iso(NOW),
            "previously_billed_at": null,
            "next_billed_at": iso(end),
            "trial_dates": null,
            "price": { "id": price, "product_id": "pro_01j8pro", "billing_cycle": { "interval": "month", "frequency": 1 } }
        });
        if trialing {
            item["trial_dates"] = json!({ "starts_at": iso(NOW), "ends_at": iso(TRIAL_END) });
        }
        json!({
            "id": "sub_01j8acme",
            "status": status,
            "customer_id": "ctm_01j8acme",
            "address_id": "add_01j8",
            "business_id": null,
            "currency_code": "USD",
            "created_at": iso(NOW),
            "updated_at": iso(NOW),
            "started_at": iso(NOW),
            "first_billed_at": if trialing { Value::Null } else { json!(iso(NOW)) },
            "next_billed_at": iso(end),
            "paused_at": null,
            "canceled_at": null,
            "collection_mode": "automatic",
            "billing_cycle": { "interval": "month", "frequency": 1 },
            "current_billing_period": { "starts_at": iso(NOW), "ends_at": iso(end) },
            "scheduled_change": null,
            "items": [item],
            "custom_data": {
                "billable_type": "workspace",
                "billable_id": WORKSPACE.to_string(),
                "plan": "pro"
            }
        })
    }

    pub fn canceling(price: &str) -> Value {
        let mut sub = subscription("active", price, 2);
        sub["scheduled_change"] =
            json!({ "action": "cancel", "effective_at": iso(PERIOD_END), "resume_at": null });
        sub
    }

    pub fn canceled(price: &str) -> Value {
        let mut sub = subscription("canceled", price, 2);
        sub["canceled_at"] = json!(iso(NOW));
        sub
    }

    pub fn paused(price: &str) -> Value {
        let mut sub = subscription("paused", price, 2);
        sub["paused_at"] = json!(iso(NOW));
        sub
    }

    pub fn transaction(status: &str, amount: &str) -> Value {
        json!({
            "id": "txn_01j8first",
            "status": status,
            "customer_id": "ctm_01j8acme",
            "subscription_id": "sub_01j8acme",
            "invoice_number": "325-10001",
            "currency_code": "USD",
            "origin": "subscription_recurring",
            "collection_mode": "automatic",
            "custom_data": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string() },
            "details": { "totals": { "subtotal": amount, "tax": "0", "total": amount, "grand_total": amount, "currency_code": "USD" } },
            "billed_at": iso(NOW),
            "created_at": iso(NOW),
            "updated_at": iso(NOW)
        })
    }

    pub fn portal_session() -> Value {
        json!({
            "id": "cpls_01j8",
            "customer_id": "ctm_01j8acme",
            "urls": {
                "general": { "overview": "https://customer-portal.paddle.com/cpl_01j8?action=overview&token=pga_x" },
                "subscriptions": []
            },
            "created_at": iso(NOW)
        })
    }

    pub fn notification_setting() -> Value {
        json!({
            "id": "ntfset_01j8",
            "description": "Navree billing",
            "type": "url",
            "destination": "https://api.acme.test/api/billing/webhook",
            "active": true,
            "api_version": 1,
            "include_sensitive_fields": false,
            "subscribed_events": [],
            "endpoint_secret_key": "pdl_ntfset_01j8_created_by_api"
        })
    }

    pub fn price() -> Value {
        json!({ "id": "pri_pro_m", "product_id": "pro_01j8pro", "status": "active", "unit_price": { "amount": "2900", "currency_code": "USD" } })
    }

    pub fn error(detail: &str) -> Value {
        json!({ "error": { "type": "request_error", "code": "not_found", "detail": detail, "documentation_url": "https://developer.paddle.com/v1/errors/shared/not_found" }, "meta": { "request_id": "x" } })
    }

    pub fn event(id: &str, kind: &str, data: Value) -> Value {
        json!({
            "event_id": id,
            "event_type": kind,
            "occurred_at": iso(NOW),
            "notification_id": "ntf_01j8",
            "data": data
        })
    }
}
