//! A form says what the API says: the app's `validation.messages`, its
//! translator and its database rules reach `Submit`, with the form's labels as the
//! fields' names, and a refused form still keeps what was posted.

use std::sync::Arc;

use http::{Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::form::{Form as _, Rejected, decode, decode_with};
use navree_ui::{Form, Submit, UiModule, Validate};
use navree_validation::{
    AppMessages, AsyncRule, AsyncRules, MessageTranslator, Messages, ValidationModule,
};
use serde::Deserialize;
use serde_json::{Value, json};

const FORM: &str = "application/x-www-form-urlencoded";

#[derive(Debug, Deserialize, Validate, Form)]
struct Contact {
    /// Work email
    #[validate(email)]
    email: String,
    /// Seats
    #[validate(range(min = 1, max = 500))]
    seats: i32,
    /// Password
    #[validate(length(min = 8))]
    password: String,
}

/// A form with a database rule: `team_id` must name a team. No database is in these
/// tests, so a value that has to be looked up can't be, and a null needs no lookup.
#[derive(Debug, Deserialize, Validate, Form)]
struct Invite {
    /// Name
    #[validate(length(min = 1))]
    name: String,
    /// Team
    #[validate(skip)]
    team_id: Option<i64>,
}

impl AsyncRules for Invite {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![AsyncRule::exists("team_id", "teams", "id", self.team_id)]
    }
}

async fn contact(submit: Result<Submit<Contact>, Rejected<Contact>>) -> Response {
    match submit {
        Ok(Submit(contact)) => format!("saved {}", contact.email).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

async fn invite(submit: Result<Submit<Invite>, Rejected<Invite>>) -> Response {
    match submit {
        Ok(Submit(invite)) => format!("invited {}", invite.name).into_response(),
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
            RouteDef::on(Method::POST, "!/contacts", contact).public(),
            RouteDef::on(Method::POST, "!/invites", invite).public(),
        ]
    }
}

struct App;

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(HttpModule::for_root(Default::default()))
}

async fn boot(builder: TestAppBuilder) -> TestApp {
    builder
        .module(ValidationModule)
        .module(UiModule)
        .module(App)
        .build()
        .await
}

/// Posts `body` to `path`, from the runtime when `runtime`.
async fn post(
    app: &TestApp,
    path: &str,
    body: &str,
    runtime: bool,
) -> navree_testing::TestResponse {
    let mut request = app.client().post(path).header("content-type", FORM);
    if runtime {
        request = request.header("x-navree-ui", "1");
    }
    request.body(body.to_owned()).send().await
}

/// The field errors of a runtime answer (`{"ops": [{"op": "errors", ...}]}`).
fn fields(body: &Value) -> &Value {
    &body["ops"][0]["fields"]
}

// The app's `validation.messages` reach a form, with the form's label as the
// field's name (over `validation.attributes`), from the runtime and without it.
#[tokio::test]
async fn a_form_says_the_apps_messages_with_its_own_labels() {
    let app = boot(
        builder()
            .config("validation.messages.email", "Check the :attribute")
            .config("validation.attributes.email", "e-mail"),
    )
    .await;
    let response = post(
        &app,
        "/contacts",
        "email=nope&seats=3&password=longenough",
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        fields(&response.json_value()),
        &json!({ "email": ["Check the Work email"] })
    );

    // Without JavaScript the form comes back with the same words and what was typed.
    let response = post(
        &app,
        "/contacts",
        "email=nope&seats=3&password=longenough",
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("Check the Work email"), "{html}");
    assert!(
        html.contains(r#"value="nope""#),
        "what was typed stays: {html}"
    );
    assert!(
        !html.contains("longenough"),
        "a password never comes back: {html}"
    );
}

// A value of the wrong type has a rule key the app can override, for every
// field (`form.number`) or for one (`seats.form.number`).
#[tokio::test]
async fn a_type_error_takes_the_apps_words() {
    let app =
        boot(builder().config("validation.messages.seats.form.number", "How many seats?")).await;
    let response = post(
        &app,
        "/contacts",
        "email=a%40b.example&seats=many&password=longenough",
        true,
    )
    .await;
    assert_eq!(
        fields(&response.json_value()),
        &json!({ "seats": ["How many seats?"] })
    );
    // An app that says nothing keeps the form's own words.
    let plain = boot(builder()).await;
    let response = post(
        &plain,
        "/contacts",
        "email=a%40b.example&seats=many&password=longenough",
        true,
    )
    .await;
    assert_eq!(
        fields(&response.json_value()),
        &json!({ "seats": ["Seats: enter a number."] })
    );
}

/// Renders every message in capitals, with names of its own, to show a form goes
/// through the app's translator as the API does.
struct Shouting;

impl MessageTranslator for Shouting {
    fn message(
        &self,
        messages: &Messages,
        field: &str,
        rule: &str,
        params: &[(&str, String)],
    ) -> String {
        let mut values = vec![("attribute", self.attribute(messages, field))];
        values.extend(params.iter().map(|(name, value)| (*name, value.clone())));
        let template = messages
            .lookup(field, rule)
            .unwrap_or(":attribute is invalid");
        Messages::fill(template, &values).to_uppercase()
    }

    fn attribute(&self, messages: &Messages, field: &str) -> String {
        match field {
            "seats" => "places".to_owned(),
            _ => messages.attribute(field),
        }
    }
}

// A registered translator renders a form's messages: the rules', a type
// error's, and its own names for fields win over the labels.
#[tokio::test]
async fn a_form_goes_through_the_apps_translator() {
    let app = boot(builder().override_value::<dyn MessageTranslator>(Arc::new(Shouting))).await;
    let response = post(
        &app,
        "/contacts",
        "email=nope&seats=0&password=longenough",
        true,
    )
    .await;
    assert_eq!(
        fields(&response.json_value()),
        &json!({
            "email": ["WORK EMAIL MUST BE A VALID EMAIL ADDRESS"],
            "seats": ["PLACES MUST NOT BE LESS THAN 1"],
        })
    );
    let response = post(
        &app,
        "/contacts",
        "email=a%40b.example&seats=lots&password=longenough",
        true,
    )
    .await;
    assert_eq!(
        fields(&response.json_value()),
        &json!({ "seats": ["PLACES: ENTER A NUMBER."] })
    );
}

// The derive hands a type's `AsyncRules` to `Submit`; a type without them has
// none.
#[test]
fn the_derive_gives_a_forms_database_rules() {
    let invite = Invite {
        name: "Navreet".to_owned(),
        team_id: Some(4),
    };
    let rules = invite.database_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!((rules[0].field(), rules[0].rule()), ("team_id", "exists"));
    let contact = Contact {
        email: "a@b.example".to_owned(),
        seats: 1,
        password: "longenough".to_owned(),
    };
    assert!(contact.database_rules().is_empty());
}

// A database rule that can't run (here, no unit of work: the app has no
// database) is the app's error answer, not a field error; a null value needs no lookup.
#[tokio::test]
async fn a_database_rule_that_cannot_run_is_an_error_not_a_field_error() {
    let app = boot(builder()).await;
    let response = post(&app, "/invites", "name=Navreet&team_id=4", true).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !response.text().contains("does not exist"),
        "not a field error: {}",
        response.text()
    );
    let response = post(&app, "/invites", "name=Navreet&team_id=", true).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "invited Navreet");
    // The form's own rules run first: a refused name never reaches the database rule.
    let response = post(&app, "/invites", "name=&team_id=4", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(fields(&response.json_value())["name"].is_array());
}

// `decode` keeps its English; `decode_with` says the app's words.
#[test]
fn decode_with_takes_the_apps_messages() {
    let pairs = |seats: &str| {
        vec![
            ("email".to_owned(), "nope".to_owned()),
            ("seats".to_owned(), seats.to_owned()),
            ("password".to_owned(), "longenough".to_owned()),
        ]
    };
    let english = decode::<Contact>(&pairs("3")).unwrap_err();
    assert_eq!(
        english["email"],
        ["Work email must be a valid email address"]
    );
    let messages = AppMessages::new(
        Arc::new(
            Messages::default()
                .with_message("email", ":attribute: that is not an address")
                .with_message("form.number", ":attribute: digits only"),
        ),
        None,
    );
    let errors = decode_with::<Contact>(&pairs("3"), &messages).unwrap_err();
    assert_eq!(errors["email"], ["Work email: that is not an address"]);
    let errors = decode_with::<Contact>(&pairs("three"), &messages).unwrap_err();
    assert_eq!(errors["seats"], ["Seats: digits only"]);
    assert_eq!(
        decode::<Contact>(&pairs("three")).unwrap_err()["seats"],
        ["Seats: enter a number."]
    );
}
