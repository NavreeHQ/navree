//! Navree UI in the reader's language, through the facade: the toolkit's labels, money and
//! dates from the request's `Locale`, and a form that says its checks and the heading of
//! its page in the language the reader asked for.
//!
//! `cargo test -p navree --features ui,i18n,testing --test ui_i18n`
#![cfg(all(feature = "ui", feature = "i18n", feature = "testing"))]

use std::sync::Arc;

use http::Method;
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::i18n::ui::{FromLocale, LocaleUi, MoneyIn};
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder, TestResponse};
use navree::ui::form::Rejected;
use navree::ui::{Form, Money, Submit, TableLabels, UiModule};
use serde_json::{Value, json};

const FORM: &str = "application/x-www-form-urlencoded";

/// A form with a slider and a number: the two checks the toolkit makes on its own.
#[derive(Debug, serde::Deserialize, Validate, Form)]
struct Settings {
    /// Volume
    #[validate(range(min = 0, max = 100))]
    #[form(widget = slider)]
    volume: i64,
    /// Seats
    #[validate(range(min = 1, max = 500))]
    seats: i32,
}

async fn settings(submit: Result<Submit<Settings>, Rejected<Settings>>) -> Response {
    match submit {
        Ok(Submit(settings)) => format!("saved {}", settings.volume).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

/// A page that reads the request's locale and draws with it.
async fn words(locale: Locale) -> Json<Value> {
    let labels = TableLabels::from_locale(&locale);
    Json(json!({
        "locale": locale.tag(),
        "search": labels.search,
        "showing": labels.showing,
        "price": Money(10_000_000).display_in(&locale, "INR"),
        "october": locale.date_locale().month_name(10),
    }))
}

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<navree::kernel::ProviderKey> {
        Vec::new()
    }

    fn construct(
        _: Resolver,
    ) -> navree::contracts::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "!/settings", settings).public(),
            RouteDef::on(Method::GET, "!/words", words).public(),
        ]
    }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

/// English, Hindi and English (India); no locale files of the app's own, so every word
/// comes from what the framework ships.
fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.locales", json!(["en", "en-IN", "hi"]))
        .config("i18n.dir", "target/ui-i18n-has-no-locale-files")
        .config("i18n.routes", false)
}

async fn app() -> TestApp {
    builder()
        .module(I18nModule::from_config())
        .module(ValidationModule)
        .module(UiModule)
        .module(PagesModule)
        .build()
        .await
}

async fn post(app: &TestApp, body: &str, runtime: bool, language: Option<&str>) -> TestResponse {
    let mut request = app.client().post("/settings").header("content-type", FORM);
    if runtime {
        request = request.header("x-navree-ui", "1");
    }
    if let Some(language) = language {
        request = request.header("accept-language", language);
    }
    request.body(body.to_owned()).send().await
}

fn fields(response: &TestResponse) -> Value {
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    response.json_value()["ops"][0]["fields"].clone()
}

// The labels, the price and the month of a page follow the request's `Locale`.
#[tokio::test]
async fn a_page_draws_in_the_requests_locale() {
    let app = app().await;
    let get = |language: &'static str| {
        let app = &app;
        async move {
            app.client()
                .get("/words")
                .header("accept-language", language)
                .send()
                .await
                .json_value()
        }
    };
    let hindi = get("hi").await;
    assert_eq!(hindi["search"], "खोजें");
    assert_eq!(
        hindi["showing"],
        "{total} में से {from} से {to} परिणाम दिख रहे हैं"
    );
    assert_eq!(hindi["price"], "₹1,00,000.00");
    assert_eq!(hindi["october"], "अक्तूबर");
    let india = get("en-IN").await;
    assert_eq!(india["search"], "Search");
    assert_eq!(india["price"], "₹1,00,000.00");
    let english = get("en").await;
    assert_eq!(english["price"], "₹100,000.00");
    assert_eq!(english["october"], "October");
}

// A Hindi reader gets the form's own checks and the toolkit's slider words in Hindi; an
// English one is unchanged.
#[tokio::test]
async fn a_hindi_reader_gets_hindi_field_errors() {
    let app = app().await;
    let response = post(&app, "volume=500&seats=many", true, Some("hi")).await;
    let errors = fields(&response);
    assert_eq!(errors["seats"], json!(["Seats: संख्या लिखें।"]), "{errors}");
    let volume: Vec<&str> = errors["volume"]
        .as_array()
        .expect("volume has errors")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(volume.contains(&"100 या उससे कम चुनें।"), "{volume:?}");

    let response = post(&app, "volume=500&seats=many", true, None).await;
    let errors = fields(&response);
    assert_eq!(
        errors["seats"],
        json!(["Seats: enter a number."]),
        "{errors}"
    );
    assert!(
        errors["volume"]
            .as_array()
            .expect("volume has errors")
            .iter()
            .any(|text| text == "Choose 100 or less."),
        "{errors}"
    );
}

// Without JavaScript the page the form comes back on has its heading, its button and
// the month names in the reader's language.
#[tokio::test]
async fn the_page_of_a_refused_form_is_in_the_readers_language() {
    let app = app().await;
    let response = post(&app, "volume=500&seats=many", false, Some("hi")).await;
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("<title>इस फ़ॉर्म को जाँचें</title>"), "{html}");
    assert!(html.contains("सहेजें"), "the button: {html}");
    assert!(!html.contains("Check this form"), "{html}");

    let response = post(&app, "volume=500&seats=many", false, None).await;
    let html = response.text();
    assert!(html.contains("<title>Check this form</title>"), "{html}");
}
