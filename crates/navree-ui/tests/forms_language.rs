//! A form in the reader's language and in the app's shell: the date, time and slider
//! messages and the heading of a refused form come from the app's `Localizer`, the wizard
//! and the draft check in a language they are given, and a form refused without
//! JavaScript keeps the shell for a signed-in user.

use std::sync::Arc;

use http::{Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder, TestResponse, principal};
use navree_ui::flow::{Draft, Wizard, WizardOutcome, WizardStep};
use navree_ui::form::{Language, Rejected, decode, decode_for};
use navree_ui::localize::{Localizer, ReaderWords};
use navree_ui::{
    Form, FormView, NavGroup, NavItem, Shell, Submit, TestView, UiModule, Validate, icons,
};
use navree_validation::{AppMessages, ValidationModule};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const FORM: &str = "application/x-www-form-urlencoded";

#[derive(Debug, Deserialize, Validate, Form)]
struct Contact {
    /// Work email
    #[validate(email)]
    email: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Mix {
    /// Volume
    #[validate(range(min = 0, max = 100))]
    #[form(widget = slider)]
    volume: i64,
}

async fn contact(submit: Result<Submit<Contact>, Rejected<Contact>>) -> Response {
    match submit {
        Ok(Submit(contact)) => format!("saved {}", contact.email).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

async fn mix(submit: Result<Submit<Mix>, Rejected<Mix>>) -> Response {
    match submit {
        Ok(Submit(mix)) => format!("volume {}", mix.volume).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "!/members/contacts", contact).authenticated(),
            RouteDef::on(Method::POST, "!/contacts", contact).public(),
            RouteDef::on(Method::POST, "!/mixes", mix).public(),
        ]
    }
}

struct App {
    shell: bool,
}

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
        if self.shell {
            m.factory(Factory::<Shell>::new(|_| async {
                Ok(Arc::new(Shell::new("Agra").group(NavGroup::plain().item(
                    NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE),
                ))))
            }))
            .exports::<Shell>();
        }
    }
}

/// Says a few words in Hindi, as a translator would for a reader whose language it is.
struct Hindi;

impl Localizer for Hindi {
    fn words(&self) -> ReaderWords {
        let mut words = ReaderWords::english();
        words.check_form = "इस फ़ॉर्म को जाँचें".into();
        words.save = "सहेजें".into();
        words.slider.too_high = "{max} या उससे कम चुनें।".into();
        words
    }
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .fake_auth()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(HttpModule::for_root(Default::default()))
}

async fn boot(builder: TestAppBuilder, shell: bool) -> TestApp {
    builder
        .module(ValidationModule)
        .module(UiModule)
        .module(App { shell })
        .build()
        .await
}

async fn post_as_member(app: &TestApp, path: &str, body: &str) -> TestResponse {
    app.acting_as(principal(Uuid::from_u128(7)))
        .post(path)
        .header("content-type", FORM)
        .body(body.to_owned())
        .send()
        .await
}

async fn post_as_guest(app: &TestApp, path: &str, body: &str, runtime: bool) -> TestResponse {
    let mut request = app.client().post(path).header("content-type", FORM);
    if runtime {
        request = request.header("x-navree-ui", "1");
    }
    request.body(body.to_owned()).send().await
}

// A form refused without JavaScript draws inside the app's shell for a signed-in user.
#[tokio::test]
async fn a_refused_form_without_javascript_keeps_the_shell() {
    let app = boot(builder(), true).await;
    let response = post_as_member(&app, "/members/contacts", "email=nope").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("Check this form"), "{html}");
    assert!(
        html.contains("Agra") && html.contains("Deals"),
        "the shell's name and navigation: {html}"
    );
    assert!(html.contains(r#"value="nope""#), "what was typed: {html}");
    assert!(
        html.contains("Work email must be a valid email address"),
        "{html}"
    );
}

// With no shell in the app, or for a guest, the form is on a page of its own as before.
#[tokio::test]
async fn without_a_shell_or_for_a_guest_the_form_is_on_its_own() {
    let plain = boot(builder(), false).await;
    let response = post_as_member(&plain, "/members/contacts", "email=nope").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("<title>Check this form</title>"), "{html}");
    assert!(!html.contains("Agra"), "{html}");

    let app = boot(builder(), true).await;
    let response = post_as_guest(&app, "/contacts", "email=nope", false).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("<title>Check this form</title>"), "{html}");
    assert!(
        !html.contains("Agra") && !html.contains("Deals"),
        "a guest's page is not the app's: {html}"
    );
}

// The runtime still gets ops, whatever the shell.
#[tokio::test]
async fn the_runtime_still_gets_ops_for_a_refused_form() {
    let app = boot(builder(), true).await;
    let response = post_as_guest(&app, "/contacts", "email=nope", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json_value()["ops"][0]["fields"],
        json!({ "email": ["Work email must be a valid email address"] })
    );
}

// The heading of the page, its button and the slider's words come from the app's
// `Localizer`.
#[tokio::test]
async fn a_refused_form_says_the_readers_words() {
    let app = boot(
        builder().override_value::<dyn Localizer>(Arc::new(Hindi)),
        true,
    )
    .await;
    let response = post_as_guest(&app, "/contacts", "email=nope", false).await;
    let html = response.text();
    assert!(html.contains("<title>इस फ़ॉर्म को जाँचें</title>"), "{html}");
    assert!(html.contains("सहेजें"), "the submit button: {html}");
    assert!(!html.contains("Check this form"), "{html}");

    let response = post_as_guest(&app, "/mixes", "volume=500", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let fields = response.json_value()["ops"][0]["fields"].clone();
    let texts: Vec<&str> = fields["volume"]
        .as_array()
        .expect("volume has errors")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        texts.contains(&"100 या उससे कम चुनें।"),
        "the slider's words: {texts:?}"
    );

    // An app with no localizer keeps English.
    let plain = boot(builder(), false).await;
    let response = post_as_guest(&plain, "/mixes", "volume=500", true).await;
    let english = response.json_value()["ops"][0]["fields"]["volume"].clone();
    assert!(
        english
            .as_array()
            .expect("volume has errors")
            .iter()
            .any(|text| text == "Choose 100 or less."),
        "{english}"
    );
}

fn hindi_language() -> Language {
    Language::new(AppMessages::default(), Hindi.words())
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

// `decode` keeps English; `decode_for` says the reader's.
#[test]
fn decode_for_says_the_readers_words_and_decode_keeps_english() {
    let posted = pairs(&[("volume", "500")]);
    let english = decode::<Mix>(&posted).unwrap_err();
    assert!(
        english["volume"].iter().any(|t| t == "Choose 100 or less."),
        "{english:?}"
    );
    let hindi = decode_for::<Mix>(&posted, &hindi_language()).unwrap_err();
    assert!(
        hindi["volume"].iter().any(|t| t == "100 या उससे कम चुनें।"),
        "{hindi:?}"
    );
    assert!(decode_for::<Mix>(&pairs(&[("volume", "40")]), &hindi_language()).is_ok());
}

// A wizard checks its steps in the language it is given, and says so on the page and in
// the draft's secret errors alike.
#[test]
fn a_wizard_checks_in_the_language_it_is_given() {
    let wizard = |language: Language| {
        Wizard::new("mix")
            .base("/mix")
            .step(WizardStep::of::<Mix>("mix", "Mix"))
            .language(language)
    };
    let attempt = |wizard: &Wizard| {
        let mut draft = Draft::new("mix");
        let outcome = wizard.submit(
            &mut draft,
            "mix",
            &pairs(&[("_rev", "0"), ("_wizard", "finish"), ("volume", "500")]),
        );
        assert!(matches!(outcome, WizardOutcome::Invalid(_)), "{outcome:?}");
        let page = wizard.page(&draft, "mix").expect("the step opens");
        navree_ui::render(&navree_ui::Node::from(page))
    };
    let html = attempt(&wizard(Language::english()));
    assert!(html.contains("Choose 100 or less."), "{html}");
    let html = attempt(&wizard(hindi_language()));
    assert!(html.contains("100 या उससे कम चुनें।"), "{html}");
}

// A draft is read in a language too, and a test drives the page in one.
#[test]
fn a_draft_and_a_test_view_read_in_a_language() {
    let mut draft = Draft::new("mix");
    let wizard = Wizard::new("mix")
        .base("/mix")
        .step(WizardStep::of::<Mix>("mix", "Mix"));
    let outcome = wizard.submit(
        &mut draft,
        "mix",
        &pairs(&[("_rev", "0"), ("_wizard", "finish"), ("volume", "500")]),
    );
    assert!(matches!(outcome, WizardOutcome::Invalid(_)), "{outcome:?}");
    let english = draft.read::<Mix>("mix").unwrap_err();
    assert!(english["volume"].iter().any(|t| t == "Choose 100 or less."));
    let hindi = draft.read_for::<Mix>("mix", &hindi_language()).unwrap_err();
    assert!(hindi["volume"].iter().any(|t| t == "100 या उससे कम चुनें।"));

    let view = TestView::of_node(FormView::<Mix>::new()).fill("volume", "500");
    let errors = view.submit_as_for::<Mix>(&hindi_language()).unwrap_err();
    assert!(errors["volume"].iter().any(|t| t == "100 या उससे कम चुनें।"));
}
