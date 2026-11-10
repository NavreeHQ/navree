//! International input without optional features: `rules::unicode_name`,
//! `Folded` digits, `Country` and `Locale` (types and string rules), and
//! their messages in the app's message table.

use std::collections::BTreeMap;
use std::sync::Arc;

use http::Method;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use navree_validation::Validate;
use navree_validation::rules;
use navree_validation::{Country, Folded, Locale, Valid, ValidationModule, ValidationOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::support::{app_with, errors};

/// A person, named in any script.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Person {
    #[validate(length(min = 1, max = 100), custom(rules::unicode_name))]
    pub name: String,
    #[validate(custom(rules::unicode_name))]
    pub nickname: Option<String>,
}

/// A one-time code and a quantity, typed with any digits.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Code {
    #[validate(length(min = 6, max = 6), pattern(r"^[0-9]+$"))]
    pub code: Folded<String>,
    #[validate(range(min = 1, max = 99))]
    pub quantity: Option<Folded<u32>>,
}

/// Typed country and locale fields, normalized while deserializing.
#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(skip_unmarked)]
pub struct Profile {
    pub country: Country,
    pub locale: Option<Locale>,
}

/// The same checks on plain strings, which keep the input as sent.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct RawProfile {
    #[validate(custom(rules::country))]
    pub country: String,
    #[validate(custom(rules::locale))]
    pub locale: Option<String>,
}

fn ok<T: Serialize>(input: &T) -> Json<Value> {
    Json(json!({ "ok": input }))
}

/// The routes of [`IntlTestModule`].
pub struct IntlController;

impl Injectable for IntlController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(IntlController) })
    }
}

impl Controller for IntlController {
    fn prefix(&self) -> &'static str {
        "!/intl"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/people",
                |Valid(Json(input)): Valid<Json<Person>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/codes",
                |Valid(Json(input)): Valid<Json<Code>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/codes",
                |Valid(Query(input)): Valid<Query<Code>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/profiles",
                |Valid(Json(input)): Valid<Json<Profile>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/raw-profiles",
                |Valid(Json(input)): Valid<Json<RawProfile>>| async move { ok(&input) },
            )
            .public(),
        ]
    }
}

/// Registers [`IntlController`].
pub struct IntlTestModule;

impl Module for IntlTestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<IntlController>();
    }
}

#[tokio::test]
async fn names_in_any_script_pass_unicode_name() {
    let app = app_with(IntlTestModule).await;
    for name in [
        "Harpreet Kaur",
        // Devanagari and Gurmukhi: vowel signs and the virama are marks.
        "हरप्रीत कौर",
        "ਹਰਪ੍ਰੀਤ ਕੌਰ",
        // Arabic, with and without its vowel marks.
        "محمد عبد الله",
        "مُحَمَّد",
        // Persian writes some names with a zero-width non-joiner.
        "فاطمه\u{200c}زهرا",
        "Zoë O’Brien-Smith",
        "J. R. R. Tolkien",
        // Decomposed: e, then two combining marks.
        "Nguye\u{0302}\u{0303}n Văn An",
        "李小龙",
        "Ελένη Παπαδοπούλου",
        "Łukasz Gołębiowski",
        "Louis 14",
    ] {
        app.client()
            .post("/intl/people")
            .json(&json!({ "name": name, "nickname": null }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("ok.name", json!(name));
    }
}

#[tokio::test]
async fn unicode_name_rejects_symbols_markup_and_digit_only_input() {
    let app = app_with(IntlTestModule).await;
    for name in [
        "<script>alert(1)</script>",
        "Robert'); DROP TABLE students;--",
        "Anu 😀",
        "@anu",
        "   ",
        // Digits alone (Arabic-Indic here) name nobody.
        "١٢٣",
        "Anu\tLakhotia",
        // "Zalgo" text: a run of more than four combining marks.
        "Z\u{0301}\u{0302}\u{0303}\u{0304}\u{0305}algo",
    ] {
        let response = app
            .client()
            .post("/intl/people")
            .json(&json!({ "name": name, "nickname": "ok" }))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({
                "name": ["name must contain only letters, numbers, spaces, hyphens, apostrophes and periods"],
            }),
            "{name:?}"
        );
    }

    // `None` passes; a present nickname is checked.
    let response = app
        .client()
        .post("/intl/people")
        .json(&json!({ "name": "Anu", "nickname": "<b>" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "nickname": ["nickname must contain only letters, numbers, spaces, hyphens, apostrophes and periods"],
        })
    );
}

#[tokio::test]
async fn digits_are_folded_before_length_and_pattern_checks() {
    let app = app_with(IntlTestModule).await;
    for (code, folded) in [
        ("٦٥٤٣٢١", "654321"), // Arabic-Indic
        ("۱۲۳۴۵۶", "123456"), // Extended Arabic-Indic (Persian, Urdu)
        ("१२३४५६", "123456"), // Devanagari
        ("੧੨੩੪੫੬", "123456"), // Gurmukhi
        ("12٣٤5६", "123456"), // mixed
        ("123456", "123456"),
    ] {
        app.client()
            .post("/intl/codes")
            .json(&json!({ "code": code }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("ok.code", json!(folded));
    }

    // Six Arabic-Indic digits are 12 bytes, but the check sees the folded
    // code: three folded digits are too short.
    let response = app
        .client()
        .post("/intl/codes")
        .json(&json!({ "code": "١٢٣" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "code": ["code must be longer than or equal to 6 characters"] })
    );
    let response = app
        .client()
        .post("/intl/codes")
        .json(&json!({ "code": "١٢٣abc" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "code": ["code must match ^[0-9]+$ regular expression"] })
    );
}

#[tokio::test]
async fn folded_numbers_parse_native_digits() {
    let app = app_with(IntlTestModule).await;
    for quantity in [json!("٤٢"), json!("४२"), json!(42)] {
        app.client()
            .post("/intl/codes")
            .json(&json!({ "code": "123456", "quantity": quantity }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("ok.quantity", json!(42));
    }

    let response = app
        .client()
        .post("/intl/codes")
        .json(&json!({ "code": "123456", "quantity": "١٠٠" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "quantity": ["quantity must not be greater than 99"] })
    );
    let response = app
        .client()
        .post("/intl/codes")
        .json(&json!({ "code": "123456", "quantity": "lots" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "quantity": ["quantity must be an integer number"] })
    );
}

#[tokio::test]
async fn query_strings_fold_digits_too() {
    let app = app_with(IntlTestModule).await;
    app.client()
        .get("/intl/codes")
        .query(&[("code", "٦٥٤٣٢١"), ("quantity", "٧")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.code", json!("654321"))
        .assert_json_path("ok.quantity", json!(7));
}

#[tokio::test]
async fn countries_and_locales_are_normalized() {
    let app = app_with(IntlTestModule).await;
    for (country, locale, want_country, want_locale) in [
        ("gb", json!("en-gb"), "GB", json!("en-GB")),
        ("AE", json!("ar-AE"), "AE", json!("ar-AE")),
        ("In", json!("hi-Deva-IN"), "IN", json!("hi-Deva-IN")),
        ("tw", json!("ZH-hant-tw"), "TW", json!("zh-Hant-TW")),
        (
            "eg",
            json!("ar-EG-u-nu-arab"),
            "EG",
            json!("ar-EG-u-nu-arab"),
        ),
        ("us", json!(null), "US", json!(null)),
    ] {
        app.client()
            .post("/intl/profiles")
            .json(&json!({ "country": country, "locale": locale }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("ok.country", json!(want_country))
            .assert_json_path("ok.locale", want_locale);
    }

    for country in ["XX", "GBR", "uk", "", "G1"] {
        let response = app
            .client()
            .post("/intl/profiles")
            .json(&json!({ "country": country }))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({ "country": ["country must be a valid ISO 3166-1 alpha-2 country code"] }),
            "{country:?}"
        );
    }
    for locale in ["en_US", "e", "en-", "123", ""] {
        let response = app
            .client()
            .post("/intl/profiles")
            .json(&json!({ "country": "GB", "locale": locale }))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({ "locale": ["locale must be a valid locale"] }),
            "{locale:?}"
        );
    }

    let response = app
        .client()
        .post("/intl/profiles")
        .json(&json!({ "country": 44 }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "country": ["country must be a string"] })
    );
}

#[tokio::test]
async fn country_and_locale_rules_check_strings() {
    let app = app_with(IntlTestModule).await;
    app.client()
        .post("/intl/raw-profiles")
        .json(&json!({ "country": "gb", "locale": "en-gb" }))
        .send()
        .await
        .assert_status(200)
        // The rules check; they don't rewrite the string.
        .assert_json_path("ok.country", json!("gb"))
        .assert_json_path("ok.locale", json!("en-gb"));

    let response = app
        .client()
        .post("/intl/raw-profiles")
        .json(&json!({ "country": "Narnia", "locale": "en_GB" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "country": ["country must be a valid ISO 3166-1 alpha-2 country code"],
            "locale": ["locale must be a valid locale"],
        })
    );
}

#[tokio::test]
async fn an_unknown_default_region_fails_at_boot() {
    let built = TestApp::builder()
        .config("validation", json!({ "phone": { "default_region": "UK" } }))
        .module(ValidationModule::from_config())
        .module(IntlTestModule)
        .try_build()
        .await;
    let Err(error) = built else {
        panic!("`UK` is not a country, so the app must not boot");
    };
    assert!(
        error
            .to_string()
            .contains("`UK` is not an ISO 3166-1 alpha-2 country code"),
        "{error}"
    );
}

#[tokio::test]
async fn international_messages_come_from_the_message_table() {
    let options = ValidationOptions {
        messages: BTreeMap::from([
            (
                "unicode_name".to_owned(),
                ":Attribute may only use letters.".to_owned(),
            ),
            ("country".to_owned(), "Pick a country.".to_owned()),
        ]),
        attributes: BTreeMap::from([("name".to_owned(), "full name".to_owned())]),
        ..ValidationOptions::default()
    };
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(ValidationModule::with_options(options))
        .module(IntlTestModule)
        .build()
        .await;

    let response = app
        .client()
        .post("/intl/people")
        .json(&json!({ "name": "R2-D2 🤖" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "name": ["Full name may only use letters."] })
    );
    let response = app
        .client()
        .post("/intl/profiles")
        .json(&json!({ "country": "ZZ" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "country": ["Pick a country."] })
    );
}
