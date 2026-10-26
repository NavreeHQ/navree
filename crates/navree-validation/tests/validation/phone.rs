//! `Phone` (feature `phone`): E.164 numbers from international or national
//! input written with any digits; the default region from
//! `validation.phone.default_region` or the app's `RegionSource`, asked only
//! when a number needs it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::{Form, Query};
use garde::Validate;
use http::Method;
use http::request::Parts;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use navree_validation::rules;
use navree_validation::{
    Country, ModuleBuilderValidationExt, Phone, RegionSource, Valid, ValidationModule,
    ValidationOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::support::{app_with, errors};

/// A contact with typed phone numbers.
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
pub struct Contact {
    pub phone: Phone,
    pub mobile: Option<Phone>,
}

/// A contact whose number stays a string, checked by `rules::phone`.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct RawContact {
    #[garde(custom(rules::phone))]
    pub phone: String,
}

/// No phone at all: never asks the region source.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Note {
    #[garde(length(min = 1))]
    pub text: String,
}

fn ok<T: Serialize>(input: &T) -> Json<Value> {
    Json(json!({ "ok": input }))
}

/// The routes of [`PhoneTestModule`].
pub struct PhoneController;

impl Injectable for PhoneController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(PhoneController) })
    }
}

impl Controller for PhoneController {
    fn prefix(&self) -> &'static str {
        "!/phone"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/contacts",
                |Valid(Json(input)): Valid<Json<Contact>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/contacts",
                |Valid(Query(input)): Valid<Query<Contact>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/contacts-form",
                |Valid(Form(input)): Valid<Form<Contact>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/raw",
                |Valid(Json(input)): Valid<Json<RawContact>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/notes",
                |Valid(Json(input)): Valid<Json<Note>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/details",
                |Valid(Json(input)): Valid<Json<Contact>>| async move {
                    let phone = &input.phone;
                    Json(json!({
                        "e164": phone.as_str(),
                        "country_code": phone.country_code(),
                        "national_number": phone.national_number(),
                        "region": phone.region(),
                        "international": phone.international(),
                        "national": phone.national(),
                        "extension": phone.extension(),
                    }))
                },
            )
            .public(),
        ]
    }
}

/// Registers [`PhoneController`].
pub struct PhoneTestModule;

impl Module for PhoneTestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<PhoneController>();
    }
}

/// The region in the request's `X-Country` header, counting its calls.
#[derive(Default)]
pub struct HeaderRegion {
    calls: AtomicUsize,
}

impl Injectable for HeaderRegion {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(HeaderRegion::default()) })
    }
}

impl RegionSource for HeaderRegion {
    fn region<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Country>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let region = parts
            .headers
            .get("x-country")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        Box::pin(async move { region })
    }
}

/// The phone routes plus [`HeaderRegion`] as the app's region source.
pub struct RegionTestModule;

impl Module for RegionTestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(PhoneTestModule);
        m.region_source::<HeaderRegion>();
    }
}

async fn post_phone(app: &TestApp, phone: &str) -> Value {
    app.client()
        .post("/phone/contacts")
        .json(&json!({ "phone": phone }))
        .send()
        .await
        .assert_status(200)
        .json_value()
}

#[tokio::test]
async fn international_numbers_become_e164_in_any_digits() {
    let app = app_with(PhoneTestModule).await;
    for (input, e164) in [
        ("+44 7400 123456", "+447400123456"),
        ("+1 (201) 555-0123", "+12015550123"),
        ("+971 50 123 4567", "+971501234567"),
        // Arabic-Indic digits.
        ("+٩٧١ ٥٠ ١٢٣ ٤٥٦٧", "+971501234567"),
        ("+٢٠ ١٠٠ ١٢٣ ٤٥٦٧", "+201001234567"),
        // Extended Arabic-Indic (Persian).
        ("+۹۸ ۹۱۲ ۳۴۵ ۶۷۸۹", "+989123456789"),
        // Devanagari.
        ("+९१ ८१२३४ ५६७८९", "+918123456789"),
        ("tel:+44-7400-123456", "+447400123456"),
    ] {
        let body = post_phone(&app, input).await;
        assert_eq!(body["ok"]["phone"], json!(e164), "{input}");
        assert_eq!(body["ok"]["mobile"], json!(null));
    }
}

#[tokio::test]
async fn invalid_numbers_are_field_errors() {
    let app = app_with(PhoneTestModule).await;
    for phone in [
        "+44 12",
        "hello",
        "",
        "+999 1234 5678",
        "+44 7400 123456 789 012",
    ] {
        let response = app
            .client()
            .post("/phone/contacts")
            .json(&json!({ "phone": phone }))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({ "phone": ["phone must be a valid phone number"] }),
            "{phone:?}"
        );
    }
    let response = app
        .client()
        .post("/phone/contacts")
        .json(&json!({ "phone": "+44 7400 123456", "mobile": 7_400_123_456_u64 }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "mobile": ["mobile must be a string"] })
    );
}

#[tokio::test]
async fn national_numbers_need_a_default_region() {
    // Without one, a number with no country code is not a phone number.
    let app = app_with(PhoneTestModule).await;
    let response = app
        .client()
        .post("/phone/contacts")
        .json(&json!({ "phone": "07400 123456" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "phone": ["phone must be a valid phone number"] })
    );

    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("validation", json!({ "phone": { "default_region": "GB" } }))
        .module(ValidationModule::from_config())
        .module(PhoneTestModule)
        .build()
        .await;
    for (input, e164) in [
        ("07400 123456", "+447400123456"),
        ("٠٧٤٠٠ ١٢٣٤٥٦", "+447400123456"),
        // International numbers ignore the default region.
        ("+971 50 123 4567", "+971501234567"),
        // So does the international prefix dialled from the region.
        ("00 971 50 123 4567", "+971501234567"),
    ] {
        let body = post_phone(&app, input).await;
        assert_eq!(body["ok"]["phone"], json!(e164), "{input}");
    }
}

#[tokio::test]
async fn a_region_source_picks_the_region_per_request() {
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("validation", json!({ "phone": { "default_region": "GB" } }))
        .module(ValidationModule::from_config())
        .module(RegionTestModule)
        .build()
        .await;
    let source = app.get::<HeaderRegion>().await;

    // Input that needs no region never asks the source.
    app.client()
        .post("/phone/notes")
        .json(&json!({ "text": "hello" }))
        .send()
        .await
        .assert_status(200);
    post_phone(&app, "+44 7400 123456").await;
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);

    let body = app
        .client()
        .post("/phone/contacts")
        .header("x-country", "AE")
        .json(&json!({ "phone": "050 123 4567", "mobile": "٠٥٠ ١٢٣ ٤٥٦٧" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["ok"],
        json!({ "phone": "+971501234567", "mobile": "+971501234567" })
    );
    // Asked once per request, however many numbers need it.
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);

    // No region from the source: the configured one.
    let body = post_phone(&app, "07400 123456").await;
    assert_eq!(body["ok"]["phone"], json!("+447400123456"));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn query_strings_and_forms_use_the_region_source() {
    let app = app_with(RegionTestModule).await;
    app.client()
        .get("/phone/contacts")
        .query(&[("phone", "०८१२३४ ५६७८९")])
        .header("x-country", "IN")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.phone", json!("+918123456789"));
    app.client()
        .post("/phone/contacts-form")
        .header("x-country", "EG")
        .form(&[("phone", "٠١٠٠ ١٢٣ ٤٥٦٧")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.phone", json!("+201001234567"));
    let response = app
        .client()
        .get("/phone/contacts")
        .query(&[("phone", "0100 123 4567")])
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "phone": ["phone must be a valid phone number"] })
    );
}

#[tokio::test]
async fn phone_rule_checks_strings_with_the_same_region() {
    let app = app_with(RegionTestModule).await;
    app.client()
        .post("/phone/raw")
        .header("x-country", "GB")
        .json(&json!({ "phone": "07400 123456" }))
        .send()
        .await
        .assert_status(200)
        // The rule checks; the string stays as sent.
        .assert_json_path("ok.phone", json!("07400 123456"));
    for (header, phone) in [(None, "07400 123456"), (Some("GB"), "12")] {
        let mut request = app.client().post("/phone/raw");
        if let Some(header) = header {
            request = request.header("x-country", header);
        }
        let response = request
            .json(&json!({ "phone": phone }))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({ "phone": ["phone must be a valid phone number"] })
        );
    }
}

#[tokio::test]
async fn phone_details() {
    let app = app_with(PhoneTestModule).await;
    let body = app
        .client()
        .post("/phone/details")
        .json(&json!({ "phone": "+44 7400 123456 ext. 12" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "e164": "+447400123456",
            "country_code": 44,
            "national_number": 7_400_123_456_u64,
            "region": "GB",
            "international": "+44 7400 123456 x12",
            "national": "07400 123456 x12",
            "extension": "12",
        })
    );
}

#[tokio::test]
async fn the_phone_message_is_overridable() {
    let options = ValidationOptions {
        messages: BTreeMap::from([("phone".to_owned(), "Check :attribute.".to_owned())]),
        attributes: BTreeMap::from([("phone".to_owned(), "the number".to_owned())]),
        ..ValidationOptions::default()
    };
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(ValidationModule::with_options(options))
        .module(PhoneTestModule)
        .build()
        .await;
    let response = app
        .client()
        .post("/phone/contacts")
        .json(&json!({ "phone": "nope" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "phone": ["Check the number."] })
    );
}
