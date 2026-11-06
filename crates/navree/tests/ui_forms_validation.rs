//! A Navree UI form checks a post as the API checks a body (MOD-005): the app's
//! `validation.messages`, the reader's language through navree-i18n's translator, and
//! the database rules (`unique`, `exists`) in the request's unit of work, each answered
//! as a field error under its field, with what was posted kept.
//!
//! `cargo test -p navree --features sqlite,ui,orm,i18n,testing --test ui_forms_validation`

use std::sync::Arc;

use http::Method;
use navree::db::{Db, Executor, Statement};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder, TestResponse};
use navree::ui::form::Rejected;
use navree::ui::{Form, Submit, UiModule};
use navree::validation::{AsyncRule, AsyncRules};
use serde_json::{Value, json};

const FORM: &str = "application/x-www-form-urlencoded";

/// A sign-up form whose address must be new and whose team must exist.
#[derive(Debug, serde::Deserialize, Validate, Form)]
struct SignUp {
    /// Work email
    #[validate(email)]
    email: String,
    /// Password
    #[validate(length(min = 8))]
    password: String,
    /// Team
    #[validate(skip)]
    team_id: Option<i64>,
}

/// The same rules an API's `ValidAsync<Json<SignUp>>` runs: the derive hands them to
/// `Submit` with no more code.
impl AsyncRules for SignUp {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            AsyncRule::unique("email", "people", "email", self.email.clone(), None),
            AsyncRule::exists("team_id", "teams", "id", self.team_id),
        ]
    }
}

async fn sign_up(submit: Result<Submit<SignUp>, Rejected<SignUp>>) -> Response {
    match submit {
        Ok(Submit(person)) => format!("welcome {}", person.email).into_response(),
        Err(rejected) => rejected.into_response(),
    }
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
        vec![RouteDef::on(Method::POST, "!/sign-up", sign_up).public()]
    }
}

struct FormsModule;

impl Module for FormsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

/// English and French; no locale files of the app's own, so French comes from the
/// validation messages navree-i18n ships.
fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.locales", json!(["en", "fr"]))
        .config("i18n.dir", "target/ui-forms-validation-has-no-locale-files")
        // No sign-in in this app: the locale routes are not needed.
        .config("i18n.routes", false)
}

/// The app, with `people(email)` holding `taken@example.test` and `teams` holding
/// team 1.
async fn app(builder: TestAppBuilder) -> TestApp {
    let app = builder
        .module(DatabaseModule::from_config())
        .module(I18nModule::from_config())
        .module(ValidationModule)
        .module(UiModule)
        .module(FormsModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    for sql in [
        "CREATE TABLE people (id INTEGER PRIMARY KEY, email TEXT NOT NULL)",
        "CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "INSERT INTO people (id, email) VALUES (1, 'taken@example.test')",
        "INSERT INTO teams (id, name) VALUES (1, 'Ludhiana')",
    ] {
        db.execute(&Statement::text(sql)).await.expect(sql);
    }
    app
}

/// Posts `body`, from the runtime when `runtime`, in `language` when given.
async fn post(app: &TestApp, body: &str, runtime: bool, language: Option<&str>) -> TestResponse {
    let mut request = app.client().post("/sign-up").header("content-type", FORM);
    if runtime {
        request = request.header("x-navree-ui", "1");
    }
    if let Some(language) = language {
        request = request.header("accept-language", language);
    }
    request.body(body.to_owned()).send().await
}

/// The field errors of a runtime answer (`{"ops": [{"op": "errors", ...}]}`).
fn fields(response: &TestResponse) -> Value {
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.json_value();
    assert_eq!(body["ops"][0]["op"], "errors", "{body}");
    body["ops"][0]["fields"].clone()
}

// MOD-005: a reader who asks for French gets the rule's message in French, from the
// messages navree-i18n ships, with the form's label as the field's name.
#[tokio::test]
async fn a_french_reader_gets_french_field_errors() {
    let app = app(builder()).await;
    let response = post(
        &app,
        "email=nope&password=longenough&team_id=",
        true,
        Some("fr-FR,fr;q=0.9"),
    )
    .await;
    assert_eq!(
        fields(&response),
        json!({ "email": ["Work email doit être une adresse e-mail valide"] })
    );
    // English is the default.
    let response = post(&app, "email=nope&password=longenough&team_id=", true, None).await;
    assert_eq!(
        fields(&response),
        json!({ "email": ["Work email must be a valid email address"] })
    );
}

// MOD-005: `unique` runs in the request's unit of work: a taken address is a field
// error (422), not the database's answer, from the runtime and without JavaScript, where
// the form comes back with what was typed and never the password.
#[tokio::test]
async fn a_taken_address_is_a_field_error_and_the_post_is_kept() {
    let app = app(builder()).await;
    let body = "email=taken%40example.test&password=longenough&team_id=";
    let response = post(&app, body, true, None).await;
    assert_eq!(
        fields(&response),
        json!({ "email": ["Work email has already been taken"] })
    );

    let response = post(&app, body, false, None).await;
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("Work email has already been taken"), "{html}");
    assert!(html.contains(r#"value="taken@example.test""#), "{html}");
    assert!(
        !html.contains("longenough"),
        "a password never comes back: {html}"
    );

    // In the reader's language too.
    let response = post(&app, body, true, Some("fr")).await;
    assert_eq!(
        fields(&response),
        json!({ "email": ["Work email est déjà utilisé"] })
    );

    // A new address goes through.
    let response = post(
        &app,
        "email=new%40example.test&password=longenough&team_id=1",
        true,
        None,
    )
    .await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.text(), "welcome new@example.test");
}

// MOD-005: `exists` runs too, beside `unique`, and both failures land on their fields;
// a field left empty is no value, so its rule needs no lookup.
#[tokio::test]
async fn a_team_that_does_not_exist_is_a_field_error() {
    let app = app(builder()).await;
    let response = post(
        &app,
        "email=new%40example.test&password=longenough&team_id=99",
        true,
        None,
    )
    .await;
    assert_eq!(
        fields(&response),
        json!({ "team_id": ["Team does not exist"] })
    );
    let response = post(
        &app,
        "email=taken%40example.test&password=longenough&team_id=99",
        true,
        None,
    )
    .await;
    assert_eq!(
        fields(&response),
        json!({
            "email": ["Work email has already been taken"],
            "team_id": ["Team does not exist"],
        })
    );
    // The form's own rules first: a short password is said, and no query runs for it.
    let response = post(
        &app,
        "email=taken%40example.test&password=short&team_id=99",
        true,
        None,
    )
    .await;
    let errors = fields(&response);
    assert!(errors["password"].is_array(), "{errors}");
    assert!(errors.get("email").is_none(), "{errors}");
}

// MOD-005: the app's `validation.messages` override what a form says, and the form's
// label names the field over `validation.attributes`.
#[tokio::test]
async fn the_apps_messages_override_and_the_label_names_the_field() {
    let app = app(builder()
        .config("validation.messages.unique", "That :attribute is in use")
        .config(
            "validation.messages.team_id.exists",
            "Choose a team from the list",
        )
        .config("validation.attributes.email", "e-mail"))
    .await;
    let response = post(
        &app,
        "email=taken%40example.test&password=longenough&team_id=99",
        true,
        None,
    )
    .await;
    assert_eq!(
        fields(&response),
        json!({
            "email": ["That Work email is in use"],
            "team_id": ["Choose a team from the list"],
        })
    );
}
