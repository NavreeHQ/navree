//! Helpers for the provider tests.

#![allow(dead_code)]

use std::collections::BTreeMap;

use navree_billing::{CheckoutRequest, Interval, Proration, SubscriptionChange};
use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use wiremock::Request;

use crate::fixtures::WORKSPACE;

/// An HTTP client that may reach the local mock server (and does not
/// retry).
pub fn client() -> HttpClient {
    HttpClient::new(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".to_owned()],
        },
        ..HttpClientOptions::default()
    })
    .unwrap()
}

/// The form fields of a recorded request.
pub fn form(request: &Request) -> Vec<(String, String)> {
    form_urlencoded::parse(&request.body).into_owned().collect()
}

/// One form field.
pub fn field<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
    form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// The JSON body of a recorded request.
pub fn json(request: &Request) -> serde_json::Value {
    serde_json::from_slice(&request.body).unwrap()
}

/// A checkout of `price` (quantity 2, a 14-day trial) for the fixture
/// workspace.
pub fn checkout_request(customer: Option<&str>, price: &str) -> CheckoutRequest {
    CheckoutRequest {
        billable_type: "workspace".into(),
        billable_id: WORKSPACE,
        customer_id: customer.map(str::to_owned),
        customer_email: Some("ada@example.com".into()),
        plan: "pro".into(),
        interval: Interval::Monthly,
        price_id: price.into(),
        metered_prices: Vec::new(),
        quantity: 2,
        trial_days: Some(14),
        coupon: None,
        allow_promotion_codes: false,
        success_url: "https://app.acme.test/settings/billing?upgraded=1".into(),
        cancel_url: "https://app.acme.test/settings/billing".into(),
        metadata: BTreeMap::new(),
    }
}

/// A change to `price` with `quantity` seats.
pub fn change(base_item: Option<&str>, price: &str, quantity: u32) -> SubscriptionChange {
    SubscriptionChange {
        base_item_id: base_item.map(str::to_owned),
        price_id: price.into(),
        quantity,
        add_prices: Vec::new(),
        remove_item_ids: Vec::new(),
        proration: Proration::NextInvoice,
    }
}
