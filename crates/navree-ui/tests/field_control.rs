//! A form field drawn by a control of one's own: drawn inside its field, and its server check run on every path a value arrives by, with the app's configuration.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use http::{Method, StatusCode};
use navree_http::body::Body;
use navree_http::extract::FromRequest;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Controller, Extension, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    AppContext, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder, TestResponse};
use navree_ui::flow::{Draft, Wizard, WizardOutcome, WizardStep};
use navree_ui::form::{
    Control, ControlCx, ControlField, FieldControl, FormField, Language, Rejected, Widget,
    check_controls, decode, decode_for, decode_with,
};
use navree_ui::table::EditCell;
use navree_ui::{
    EditedValue, Form, FormView, Node, Options, Reply, Submit, UiModule, Validate, assets, el, kit,
    render,
};
use navree_validation::{AppMessages, ValidationModule};
use serde::Deserialize;
use serde_json::{Value, json};

const FORM: &str = "application/x-www-form-urlencoded";

/// Six digits; and, when the app's configuration lists the pin codes it delivers to, one of
/// those. Without a list it can only hold the six digits.
struct PinCode;

impl FieldControl for PinCode {
    fn name(&self) -> &'static str {
        "app.pin-code"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:pin-code")
            .attr("role", "group")
            .attr_opt("aria-labelledby", field.label_id.map(str::to_owned))
            .child(
                el("input")
                    .attr("type", "text")
                    .attr("name", field.name.to_owned())
                    .attr("inputmode", "numeric")
                    .attr_opt("value", field.value.map(str::to_owned))
                    .flag("required", field.required)
                    .flag("disabled", field.disabled),
            )
            .into()
    }

    fn check(&self, value: &str, cx: &ControlCx<'_>) -> Result<(), String> {
        if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Enter a six-digit pin code".to_owned());
        }
        if let Some(config) = cx.config()
            && let Ok(served) = config.section::<Vec<String>>("delivery.pin_codes")
            && !served.iter().any(|code| code == value)
        {
            return Err("We deliver to Ludhiana and Jalandhar only".to_owned());
        }
        Ok(())
    }
}

static PIN_CODE: PinCode = PinCode;

/// Looks the pin code up in the app's configuration: with no configuration to look in it cannot
/// tell, and so it refuses.
struct DeliveryPin;

impl FieldControl for DeliveryPin {
    fn name(&self) -> &'static str {
        "app.delivery-pin"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("type", "text")
            .attr("name", field.name.to_owned())
            .flag("disabled", field.disabled)
            .into()
    }

    fn check(&self, value: &str, cx: &ControlCx<'_>) -> Result<(), String> {
        let Some(config) = cx.config() else {
            return Err("The delivery area cannot be checked right now".to_owned());
        };
        match config.section::<Vec<String>>("delivery.pin_codes") {
            Ok(served) if served.iter().any(|code| code == value) => Ok(()),
            _ => Err("We deliver to Ludhiana and Jalandhar only".to_owned()),
        }
    }
}

static DELIVERY_PIN: DeliveryPin = DeliveryPin;

/// What the last check of `Seen` was given: whether it had the app, and whether it had the
/// configuration.
static SAW: Mutex<Option<(bool, bool)>> = Mutex::new(None);

/// Accepts everything, and records what its check was given.
struct Seen;

impl FieldControl for Seen {
    fn name(&self) -> &'static str {
        "app.seen"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input").attr("name", field.name.to_owned()).into()
    }

    fn check(&self, _value: &str, cx: &ControlCx<'_>) -> Result<(), String> {
        *SAW.lock().expect("the record") = Some((cx.app().is_some(), cx.config().is_some()));
        Ok(())
    }
}

static SEEN: Seen = Seen;

/// A rating that checks nothing: the type reads it.
struct Stars;

impl FieldControl for Stars {
    fn name(&self) -> &'static str {
        "app.stars"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input").attr("name", field.name.to_owned()).into()
    }
}

static STARS: Stars = Stars;

/// A password with a meter of its strength. It writes back whatever value it is given to draw.
struct Strength;

impl FieldControl for Strength {
    fn name(&self) -> &'static str {
        "app.strength"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("type", "password")
            .attr("name", field.name.to_owned())
            .attr_opt("value", field.value.map(str::to_owned))
            .flag("disabled", field.disabled)
            .into()
    }

    fn check(&self, value: &str, _cx: &ControlCx<'_>) -> Result<(), String> {
        if value.chars().count() >= 12 {
            Ok(())
        } else {
            Err("Use at least 12 characters".to_owned())
        }
    }
}

static STRENGTH: Strength = Strength;

/// A one-time code in six boxes: a secret by its own word, under any name. It writes back
/// whatever value it is given to draw.
struct OneTimeCode;

impl FieldControl for OneTimeCode {
    fn name(&self) -> &'static str {
        "app.one-time-code"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("type", "text")
            .attr("name", field.name.to_owned())
            .attr("inputmode", "numeric")
            .attr_opt("value", field.value.map(str::to_owned))
            .flag("disabled", field.disabled)
            .into()
    }

    fn check(&self, value: &str, _cx: &ControlCx<'_>) -> Result<(), String> {
        if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(())
        } else {
            Err("Enter the six-digit code from the message".to_owned())
        }
    }

    fn secret(&self) -> bool {
        true
    }
}

static ONE_TIME_CODE: OneTimeCode = OneTimeCode;

#[derive(Debug, Deserialize, Validate, Form)]
#[allow(
    dead_code,
    reason = "the form is drawn, refused and kept in a draft; its fields are not read"
)]
struct SignUp {
    /// Full name
    #[validate(skip)]
    full_name: String,
    /// Password
    #[validate(skip)]
    #[form(control = STRENGTH)]
    password: String,
    /// Code from the message
    #[validate(skip)]
    #[form(control = ONE_TIME_CODE)]
    code: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Parcel {
    /// Who receives it
    #[validate(skip)]
    recipient: String,
    /// Pin code
    #[validate(skip)]
    #[form(control = PIN_CODE, help = "Six digits")]
    pin_code: String,
    /// A second address
    #[validate(skip)]
    #[form(control = PIN_CODE)]
    spare: Option<String>,
}

#[derive(Debug, Deserialize, Validate, Form)]
#[allow(
    dead_code,
    reason = "the form is read and its check run; its field is not used"
)]
struct Dropoff {
    /// Pin code
    #[validate(skip)]
    #[form(control = DELIVERY_PIN)]
    pin_code: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
#[allow(
    dead_code,
    reason = "the form is read and its check run; its field is not used"
)]
struct Watched {
    /// Code
    #[validate(skip)]
    #[form(control = SEEN)]
    code: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Feedback {
    /// Stars
    #[validate(skip)]
    #[form(control = STARS)]
    stars: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    /// By road
    Road,
    /// By rail
    Rail,
}

#[derive(Debug, Deserialize, Validate, Form)]
#[allow(dead_code, reason = "the form is drawn, not read")]
struct Deal {
    /// Title
    #[validate(skip)]
    title: String,
    /// Amount
    #[validate(skip)]
    amount: navree_ui::Money,
    /// Closes
    #[validate(skip)]
    closes: chrono::NaiveDate,
    /// Mode
    #[validate(skip)]
    mode: Mode,
    /// Urgent
    #[validate(skip)]
    urgent: bool,
}

async fn parcels(submit: Result<Submit<Parcel>, Rejected<Parcel>>) -> Response {
    match submit {
        Ok(Submit(parcel)) => {
            let spare = parcel.spare.unwrap_or_default();
            format!("saved {} {} {spare}", parcel.recipient, parcel.pin_code).into_response()
        }
        Err(rejected) => rejected.into_response(),
    }
}

/// A sign-up: refused, the form is drawn again.
async fn sign_ups(submit: Result<Submit<SignUp>, Rejected<SignUp>>) -> Response {
    match submit {
        Ok(Submit(_)) => "saved".into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

/// What a refused sign-up gives its handler back: each field and its text, a line each, then
/// what `value` finds for the password and for the code.
async fn sign_up_values(submit: Result<Submit<SignUp>, Rejected<SignUp>>) -> Response {
    match submit {
        Ok(Submit(_)) => "saved".into_response(),
        Err(rejected) => {
            let mut lines: Vec<String> = rejected
                .values()
                .map(|(name, text)| format!("{name}={text}"))
                .collect();
            lines.push(format!("password: {:?}", rejected.value("password")));
            lines.push(format!("code: {:?}", rejected.value("code")));
            lines.join("\n").into_response()
        }
    }
}

/// An inline edit of a delivery's pin code: the value is held to the pin code control's check.
async fn pin_edit(Extension(ctx): Extension<AppContext>, edit: EditedValue<String>) -> Reply {
    let language = Language::of(Some(&ctx)).await;
    match edit.read(Control::new(&PIN_CODE), &language) {
        Ok(_) => Reply::ops(vec![]),
        Err(refused) => refused.reply("/deliveries"),
    }
}

/// An inline edit of a delivery's note, in a built-in editor: nothing to check but the type.
async fn note_edit(Extension(ctx): Extension<AppContext>, edit: EditedValue<String>) -> Reply {
    let language = Language::of(Some(&ctx)).await;
    match edit.read_built_in(&language) {
        Ok(_) => Reply::ops(vec![]),
        Err(refused) => refused.reply("/deliveries"),
    }
}

/// An inline edit of a rating: a control that checks nothing, so only the type can refuse.
async fn seats_edit(Extension(ctx): Extension<AppContext>, edit: EditedValue<u8>) -> Reply {
    let language = Language::of(Some(&ctx)).await;
    match edit.read(Control::new(&STARS), &language) {
        Ok(_) => Reply::ops(vec![]),
        Err(refused) => refused.reply("/deliveries"),
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
            RouteDef::on(Method::POST, "!/parcels", parcels).public(),
            RouteDef::on(Method::POST, "!/sign-ups", sign_ups).public(),
            RouteDef::on(Method::POST, "!/sign-ups/values", sign_up_values).public(),
            RouteDef::on(Method::POST, "!/deliveries/{id}/pin", pin_edit).public(),
            RouteDef::on(Method::POST, "!/deliveries/{id}/note", note_edit).public(),
            RouteDef::on(Method::POST, "!/deliveries/{id}/seats", seats_edit).public(),
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
        .config("delivery.pin_codes", json!(["141001", "144001"]))
        .module(HttpModule::for_root(Default::default()))
}

async fn boot() -> TestApp {
    builder()
        .module(ValidationModule)
        .module(UiModule)
        .module(App)
        .build()
        .await
}

/// A post, as the runtime sends it when `runtime` is set and as a browser without JavaScript
/// does when it is not.
async fn post(app: &TestApp, path: &str, body: &str, runtime: bool) -> TestResponse {
    let mut request = app.client().post(path).header("content-type", FORM);
    if runtime {
        request = request.header("x-navree-ui", "1");
    }
    request.body(body.to_owned()).send().await
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

// A derived form holds a control's field to the control's check, and what the type reads is
// the text the control posted. With no app, the check has no configuration to read.
#[tokio::test]
async fn a_derived_form_runs_the_controls_check() {
    let errors = decode::<Parcel>(&pairs(&[
        ("recipient", "Harpreet Sandhu"),
        ("pin_code", "14100"),
    ]))
    .unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors["pin_code"], ["Enter a six-digit pin code"]);

    let app = boot().await;
    let response = post(
        &app,
        "/parcels",
        "recipient=Harpreet+Sandhu&pin_code=110001",
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json_value()["ops"][0]["fields"],
        json!({ "pin_code": ["We deliver to Ludhiana and Jalandhar only"] })
    );

    let response = post(
        &app,
        "/parcels",
        "recipient=Harpreet+Sandhu&pin_code=141001",
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.text().contains("saved Harpreet Sandhu 141001"),
        "{}",
        response.text()
    );

    // With JavaScript off the refused form is drawn again, and says why.
    let response = post(
        &app,
        "/parcels",
        "recipient=Harpreet+Sandhu&pin_code=110001",
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(
        html.contains("We deliver to Ludhiana and Jalandhar only"),
        "{html}"
    );
    assert!(html.contains(r#"value="110001""#), "what was typed: {html}");
}

// A step of a wizard is read through the same decode, in the language the wizard is given.
#[tokio::test]
async fn a_wizard_step_runs_the_controls_check() {
    let app = boot().await;
    let language = Language::of(Some(app.ctx())).await;
    let wizard = Wizard::new("parcel")
        .base("/parcel")
        .step(WizardStep::of::<Parcel>("parcel", "Parcel"))
        .language(language);

    let mut draft = Draft::new("parcel");
    let outcome = wizard.submit(
        &mut draft,
        "parcel",
        &pairs(&[
            ("_rev", "0"),
            ("_wizard", "finish"),
            ("recipient", "Meera Sharma"),
            ("pin_code", "110001"),
        ]),
    );
    assert!(matches!(outcome, WizardOutcome::Invalid(_)), "{outcome:?}");
    let page = wizard.page(&draft, "parcel").expect("the step opens");
    let html = render(&Node::from(page));
    assert!(
        html.contains("We deliver to Ludhiana and Jalandhar only"),
        "{html}"
    );

    let mut draft = Draft::new("parcel");
    let outcome = wizard.submit(
        &mut draft,
        "parcel",
        &pairs(&[
            ("_rev", "0"),
            ("_wizard", "finish"),
            ("recipient", "Meera Sharma"),
            ("pin_code", "141001"),
        ]),
    );
    assert_eq!(outcome, WizardOutcome::Done);
}

// A draft saved on a wizard with no app is read back with the app's language, and the control's
// check is run again, with the configuration.
#[tokio::test]
async fn a_draft_read_back_runs_the_controls_check() {
    let wizard = Wizard::new("parcel")
        .base("/parcel")
        .step(WizardStep::of::<Parcel>("parcel", "Parcel"));
    let mut draft = Draft::new("parcel");
    let outcome = wizard.submit(
        &mut draft,
        "parcel",
        &pairs(&[
            ("_rev", "0"),
            ("_wizard", "finish"),
            ("recipient", "Simran Kaur"),
            ("pin_code", "110001"),
        ]),
    );
    assert_eq!(outcome, WizardOutcome::Done);

    let app = boot().await;
    let language = Language::of(Some(app.ctx())).await;
    let errors = draft.read_for::<Parcel>("parcel", &language).unwrap_err();
    assert_eq!(
        errors["pin_code"],
        ["We deliver to Ludhiana and Jalandhar only"]
    );
    // Read with no language, the check has no configuration to read.
    assert!(draft.read::<Parcel>("parcel").is_ok());
}

// A form described at run time has no type to decode into: its reader runs the checks.
#[test]
fn check_controls_refuses_a_described_forms_control_value() {
    let fields = [FormField::new(
        "pin_code",
        "Pin code",
        Widget::Control(Control::new(&PIN_CODE)),
    )];
    let english = Language::english();
    let errors = check_controls(&fields, &pairs(&[("pin_code", "12")]), &english);
    assert_eq!(errors["pin_code"], ["Enter a six-digit pin code"]);
    let errors = check_controls(&fields, &pairs(&[("pin_code", "141001")]), &english);
    assert!(errors.is_empty(), "{errors:?}");

    // A field that is not optional and was left out reads as empty, and is checked as that.
    let errors = check_controls(&fields, &[], &english);
    assert_eq!(errors["pin_code"], ["Enter a six-digit pin code"]);

    // An optional field left empty is not checked.
    let optional = [fields[0].optional(true)];
    let errors = check_controls(&optional, &pairs(&[("pin_code", "")]), &english);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_controls(&optional, &[], &english);
    assert!(errors.is_empty(), "{errors:?}");

    // The field's own error says it in place of the control's.
    let own = [fields[0].error(Some("Check the pin code"))];
    let errors = check_controls(&own, &pairs(&[("pin_code", "12")]), &english);
    assert_eq!(errors["pin_code"], ["Check the pin code"]);
}

// The check reads the plugin's configuration through the app, and only through an app.
#[tokio::test]
async fn a_controls_check_reads_its_plugins_configuration_through_the_app() {
    let app = boot().await;
    let language = Language::of(Some(app.ctx())).await;
    let errors = decode_for::<Parcel>(
        &pairs(&[("recipient", "Kuldeep Gill"), ("pin_code", "110001")]),
        &language,
    )
    .unwrap_err();
    assert_eq!(
        errors["pin_code"],
        ["We deliver to Ludhiana and Jalandhar only"]
    );
    let taken = decode_for::<Parcel>(
        &pairs(&[("recipient", "Kuldeep Gill"), ("pin_code", "141001")]),
        &language,
    )
    .expect("a pin code the app delivers to");
    assert_eq!(taken.pin_code, "141001");

    // With no configuration this control can only hold the six digits, and accepts.
    let taken = decode_for::<Parcel>(
        &pairs(&[("recipient", "Kuldeep Gill"), ("pin_code", "110001")]),
        &Language::english(),
    )
    .expect("no configuration to hold it to");
    assert_eq!(taken.pin_code, "110001");
}

// A check that cannot do its work without the app or the configuration refuses the value; the
// decodes that have neither give it neither.
#[tokio::test]
async fn a_check_that_cannot_work_without_the_app_or_configuration_refuses() {
    let posted = pairs(&[("pin_code", "141001")]);
    let cannot = ["The delivery area cannot be checked right now"];

    assert_eq!(decode::<Dropoff>(&posted).unwrap_err()["pin_code"], cannot);
    assert_eq!(
        decode_with::<Dropoff>(&posted, &AppMessages::default()).unwrap_err()["pin_code"],
        cannot
    );
    assert_eq!(
        decode_for::<Dropoff>(&posted, &Language::english()).unwrap_err()["pin_code"],
        cannot
    );

    let app = boot().await;
    let language = Language::of(Some(app.ctx())).await;
    assert!(decode_for::<Dropoff>(&posted, &language).is_ok());
    let elsewhere = decode_for::<Dropoff>(&pairs(&[("pin_code", "110001")]), &language);
    assert_eq!(
        elsewhere.unwrap_err()["pin_code"],
        ["We deliver to Ludhiana and Jalandhar only"]
    );
}

// `decode` and `decode_with` give the check no app and no configuration; `decode_for` with the
// language of an app gives both, and a language made with `english` none.
#[tokio::test]
async fn decode_without_a_language_gives_the_check_no_app() {
    let posted = pairs(&[("code", "x")]);

    *SAW.lock().expect("the record") = None;
    decode::<Watched>(&posted).expect("decodes");
    assert_eq!(*SAW.lock().expect("the record"), Some((false, false)));

    *SAW.lock().expect("the record") = None;
    decode_with::<Watched>(&posted, &AppMessages::default()).expect("decodes");
    assert_eq!(*SAW.lock().expect("the record"), Some((false, false)));

    *SAW.lock().expect("the record") = None;
    decode_for::<Watched>(&posted, &Language::english()).expect("decodes");
    assert_eq!(*SAW.lock().expect("the record"), Some((false, false)));

    let app = boot().await;
    let language = Language::of(Some(app.ctx())).await;
    *SAW.lock().expect("the record") = None;
    decode_for::<Watched>(&posted, &language).expect("decodes");
    assert_eq!(*SAW.lock().expect("the record"), Some((true, true)));
}

// A control is drawn inside its field: the label names it, the help and the error describe the
// part the field ties, and the label has an id for the control's other parts.
#[test]
fn a_field_control_is_drawn_inside_the_field_with_its_label_help_error_and_label_id() {
    let errors = BTreeMap::from([(
        "pin_code".to_owned(),
        vec!["We deliver to Ludhiana and Jalandhar only".to_owned()],
    )]);
    let html = render(
        &FormView::<Parcel>::new()
            .with_values(pairs(&[
                ("recipient", "Meera Sharma"),
                ("pin_code", "110001"),
            ]))
            .with_errors(errors)
            .into(),
    );
    assert!(
        html.contains(
            r#"<label data-slot="label" for="parcel-pin_code" id="parcel-pin_code-label">Pin code</label>"#
        ),
        "{html}"
    );
    assert!(
        html.contains(r#"aria-labelledby="parcel-pin_code-label""#),
        "{html}"
    );
    assert!(html.contains(r#"id="parcel-pin_code""#), "{html}");
    assert!(html.contains(r#"aria-invalid="true""#), "{html}");
    assert!(
        html.contains(r#"aria-describedby="parcel-pin_code-help parcel-pin_code-error""#),
        "{html}"
    );
    assert!(html.contains("Six digits"), "{html}");
    assert!(
        html.contains("We deliver to Ludhiana and Jalandhar only"),
        "{html}"
    );
    assert!(html.contains(r#"value="110001""#), "{html}");
    assert!(
        html.contains(r#"<label data-slot="label" for="parcel-recipient">Who receives it</label>"#),
        "the other labels have no id: {html}"
    );
}

// A form with no control is drawn as it was: a label gets an id only for a control.
#[test]
fn a_form_without_a_control_is_drawn_byte_for_byte_as_before() {
    let html = render(&FormView::<Deal>::new().post("/deals").into());
    assert!(!html.contains(r#"-label""#), "no label has an id: {html}");
    for (name, label) in [
        ("title", "Title"),
        ("amount", "Amount"),
        ("closes", "Closes"),
        ("mode", "Mode"),
    ] {
        let expected = format!(r#"<label data-slot="label" for="deal-{name}">{label}</label>"#);
        assert!(html.contains(&expected), "{expected}: {html}");
    }
}

// An optional control left empty is no value: it is not checked, and a value in it is.
#[test]
fn an_empty_optional_field_control_is_not_checked() {
    let posted = |spare: &str| {
        pairs(&[
            ("recipient", "Gurpreet Singh"),
            ("pin_code", "141001"),
            ("spare", spare),
        ])
    };
    let parcel = decode::<Parcel>(&posted("")).expect("an empty optional field");
    assert_eq!(parcel.spare, None);
    let errors = decode::<Parcel>(&posted("12")).unwrap_err();
    assert_eq!(errors["spare"], ["Enter a six-digit pin code"]);
    // Left out of the post altogether, it is as empty.
    assert!(
        decode::<Parcel>(&pairs(&[
            ("recipient", "Gurpreet Singh"),
            ("pin_code", "141001")
        ]))
        .is_ok()
    );
}

// A required control that the post leaves out reads as empty, as an input does, and its check
// holds that value too.
#[test]
fn a_required_control_left_out_of_the_post_is_checked_as_empty() {
    let errors = decode::<Parcel>(&pairs(&[("recipient", "Harpreet Sandhu")])).unwrap_err();
    assert_eq!(errors["pin_code"], ["Enter a six-digit pin code"]);
    assert!(!errors.contains_key("spare"), "{errors:?}");
}

// What a control posts is read as the field's type reads text, and a value that is not of the
// type is the form's own `form.invalid`.
#[test]
fn a_field_control_value_decodes_into_the_fields_type() {
    let feedback = decode::<Feedback>(&pairs(&[("stars", "4")])).expect("a number of stars");
    assert_eq!(feedback.stars, 4);
    let errors = decode::<Feedback>(&pairs(&[("stars", "x")])).unwrap_err();
    assert_eq!(errors["stars"], ["Stars is not valid."]);
}

// A secret drawn by a control, a password by its field's name and a one-time code by its
// control's own word, is kept as a password input's value is: a refused form never shows it
// again, the handler is never given it back, a wizard never keeps it, and the control is never
// given it to draw.
#[tokio::test]
async fn a_secret_drawn_by_a_control_is_never_shown_again_given_back_or_kept() {
    let (password, code) = ("Kesar-12", "482913");
    let fields = <SignUp as navree_ui::form::Form>::FIELDS;
    let field = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name)
            .expect("the field")
    };
    assert!(
        field("password").secret,
        "a control under a password's name"
    );
    assert!(!field("code").secret, "a secret by its control's word only");
    assert!(!field("full_name").secret);

    // The password is too short: with JavaScript off the form is drawn again, with the name and
    // without either secret.
    let app = boot().await;
    let body = format!("full_name=Simran+Kaur&password={password}&code={code}");
    let response = post(&app, "/sign-ups", &body, false).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = response.text();
    assert!(html.contains("Use at least 12 characters"), "{html}");
    assert!(
        html.contains(r#"value="Simran Kaur""#),
        "what was typed: {html}"
    );
    assert!(
        !html.contains(password),
        "the password is shown again: {html}"
    );
    assert!(!html.contains(code), "the code is shown again: {html}");

    // The handler of a refused form is given the name back, and neither secret.
    let response = post(&app, "/sign-ups/values", &body, false).await;
    assert_eq!(
        response.text(),
        "full_name=Simran Kaur\npassword: None\ncode: None"
    );

    // A form given the secrets to draw hands neither to its control.
    let html = render(
        &FormView::<SignUp>::new()
            .with_values(pairs(&[
                ("full_name", "Simran Kaur"),
                ("password", password),
                ("code", code),
            ]))
            .into(),
    );
    assert!(html.contains(r#"value="Simran Kaur""#), "{html}");
    assert!(!html.contains(password), "{html}");
    assert!(!html.contains(code), "{html}");

    // Nor does a table cell.
    let cell = EditCell::control(code, "/accounts/7/code", Control::new(&ONE_TIME_CODE));
    let html = render(&cell.into());
    assert!(html.contains(r#"name="value""#), "{html}");
    assert!(!html.contains(code), "{html}");

    // A step saved keeps what was typed, and neither secret.
    let wizard = Wizard::new("sign-up")
        .base("/sign-up")
        .step(WizardStep::of::<SignUp>("account", "Account"));
    let mut draft = Draft::new("account");
    let outcome = wizard.submit(
        &mut draft,
        "account",
        &pairs(&[
            ("_rev", "0"),
            ("_wizard", "save"),
            ("full_name", "Simran Kaur"),
            ("password", password),
            ("code", code),
        ]),
    );
    assert!(!matches!(outcome, WizardOutcome::Refused(_)), "{outcome:?}");
    assert_eq!(draft.value("account", "full_name"), Some("Simran Kaur"));
    assert_eq!(draft.value("account", "password"), None);
    assert_eq!(draft.value("account", "code"), None);
    let kept = serde_json::to_string(&draft).expect("a draft is data");
    assert!(!kept.contains(password), "{kept}");
    assert!(!kept.contains(code), "{kept}");
}

// A form, a field and an editable cell can hold a control, and are as safe to carry across a
// caught panic as they were before they could. A failure here is a compile error naming the type.
#[test]
fn what_can_hold_a_control_keeps_its_unwind_safety() {
    fn unwind_safe<T: std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
    unwind_safe::<Control>();
    unwind_safe::<Widget>();
    unwind_safe::<FormField>();
    unwind_safe::<FormView<Parcel>>();
    unwind_safe::<EditCell>();
    unwind_safe::<navree_ui::described::FieldSpec>();
    unwind_safe::<navree_ui::described::DescribedForm>();
}

/// A count of seats whose check says what a count looks like.
struct Seats;

impl FieldControl for Seats {
    fn name(&self) -> &'static str {
        "app.seats"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input").attr("name", field.name.to_owned()).into()
    }

    fn check(&self, value: &str, _cx: &ControlCx<'_>) -> Result<(), String> {
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(())
        } else {
            Err("Enter the seats as a number, such as 4".to_owned())
        }
    }
}

static SEATS: Seats = Seats;

#[derive(Debug, Deserialize, Validate, Form)]
#[allow(
    dead_code,
    reason = "the form is read and its check run; its fields are not used"
)]
struct Booking {
    /// Seats
    #[validate(skip)]
    #[form(control = SEATS)]
    seats: u8,
    /// Coach
    #[validate(skip)]
    coach: u8,
}

// A text the control refuses and the type cannot read is one fault, said once, in the
// control's words. The form's own words stay for a text the control accepts.
#[test]
fn a_text_the_control_refuses_and_the_type_cannot_read_is_said_once_in_the_controls_words() {
    let errors = decode::<Booking>(&pairs(&[("seats", "four"), ("coach", "2")])).unwrap_err();
    assert_eq!(errors["seats"], ["Enter the seats as a number, such as 4"]);
    // 300 is a number to the control and too large for the type.
    let errors = decode::<Booking>(&pairs(&[("seats", "300"), ("coach", "2")])).unwrap_err();
    assert_eq!(errors["seats"], ["Seats is not valid."]);
    // Another field's wrong type is still named beside the control's refusal.
    let errors = decode::<Booking>(&pairs(&[("seats", "four"), ("coach", "B")])).unwrap_err();
    assert_eq!(errors["seats"], ["Enter the seats as a number, such as 4"]);
    assert_eq!(errors["coach"], ["Coach: enter a number."]);
    let booking = decode::<Booking>(&pairs(&[("seats", "4"), ("coach", "2")])).expect("a booking");
    assert_eq!((booking.seats, booking.coach), (4, 2));
}

/// A control that posts through a hidden input, and marks a part of its own that no label names.
struct HiddenUnlabelled;

impl FieldControl for HiddenUnlabelled {
    fn name(&self) -> &'static str {
        "app.swatch-hidden"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:swatch-hidden")
            .children((
                el("input")
                    .attr("type", "hidden")
                    .attr("name", field.name.to_owned())
                    .flag("disabled", field.disabled),
                el("span")
                    .flag("data-field-control", true)
                    .attr("tabindex", "0"),
            ))
            .into()
    }
}

/// A control whose button does not follow `disabled`.
struct IgnoresDisabled;

impl FieldControl for IgnoresDisabled {
    fn name(&self) -> &'static str {
        "app.swatch"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:swatch")
            .children((
                el("input")
                    .attr("type", "text")
                    .attr("name", field.name.to_owned())
                    .flag("disabled", field.disabled),
                el("button").attr("type", "button").child("Pick"),
            ))
            .into()
    }
}

/// A control that describes the input the field ties, as the field does.
struct TiesTwice;

impl FieldControl for TiesTwice {
    fn name(&self) -> &'static str {
        "app.pin-twice"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("type", "text")
            .attr("name", field.name.to_owned())
            .attr("aria-describedby", "drop_at-hint")
            .flag("disabled", field.disabled)
            .into()
    }
}

/// A control that draws a field named otherwise.
struct Misnamed;

impl FieldControl for Misnamed {
    fn name(&self) -> &'static str {
        "app.misnamed"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("name", "somewhere_else")
            .flag("disabled", field.disabled)
            .into()
    }
}

/// A control that draws its form field inside an owned body, where the seal takes it out of the
/// form.
struct SealedField;

impl FieldControl for SealedField {
    fn name(&self) -> &'static str {
        "app.sealed"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:sealed")
            .child(
                el("div")
                    .slot("app:sealed-body")
                    .flag("data-ui-keep", true)
                    .child(
                        el("input")
                            .attr("type", "text")
                            .attr("name", field.name.to_owned())
                            .flag("disabled", field.disabled),
                    ),
            )
            .into()
    }
}

/// A control that posts through a hidden input, and labels the part that stands in for it with
/// an element that is not the field's label.
struct LabelledElsewhere;

impl FieldControl for LabelledElsewhere {
    fn name(&self) -> &'static str {
        "app.swatch-elsewhere"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:swatch-elsewhere")
            .children((
                el("input")
                    .attr("type", "hidden")
                    .attr("name", field.name.to_owned())
                    .flag("disabled", field.disabled),
                el("span")
                    .flag("data-field-control", true)
                    .attr("tabindex", "0")
                    .attr_opt(
                        "aria-labelledby",
                        field.label_id.map(|_| "drop_at-help".to_owned()),
                    )
                    .attr_opt(
                        "aria-label",
                        field.label_id.is_none().then(|| field.label.to_owned()),
                    ),
            ))
            .into()
    }
}

/// A control that posts through a hidden input, and labels the part that stands in for it with
/// the field's label among the ids of `aria-labelledby`.
struct HiddenLabelled;

impl FieldControl for HiddenLabelled {
    fn name(&self) -> &'static str {
        "app.swatch-labelled"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:swatch-labelled")
            .children((
                el("input")
                    .attr("type", "hidden")
                    .attr("name", field.name.to_owned())
                    .flag("disabled", field.disabled),
                el("span")
                    .flag("data-field-control", true)
                    .attr("tabindex", "0")
                    .attr_opt(
                        "aria-labelledby",
                        field.label_id.map(|id| format!("{id} swatch-name")),
                    )
                    .attr_opt(
                        "aria-label",
                        field.label_id.is_none().then(|| field.label.to_owned()),
                    ),
            ))
            .into()
    }
}

/// A control whose fieldset does not follow `disabled`, beside an output, which has no `disabled`
/// to follow.
struct IgnoresDisabledSet;

impl FieldControl for IgnoresDisabledSet {
    fn name(&self) -> &'static str {
        "app.swatch-set"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:swatch-set")
            .children((
                el("fieldset").child(
                    el("input")
                        .attr("type", "text")
                        .attr("name", field.name.to_owned())
                        .flag("disabled", field.disabled),
                ),
                el("output").child("#e8b4b8"),
            ))
            .into()
    }
}

/// What `kit::assert_control` says is wrong with `control`.
fn said_of(control: &dyn FieldControl) -> String {
    let result = catch_unwind(AssertUnwindSafe(|| kit::assert_control(control)));
    match result {
        Ok(()) => panic!("{} broke no rule", control.name()),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
            })
            .unwrap_or_default(),
    }
}

#[test]
fn assert_control_refuses_an_unlabelled_hidden_control_one_that_ignores_disabled_and_one_that_ties_twice()
 {
    let said = said_of(&HiddenUnlabelled);
    assert_eq!(
        said,
        "control app.swatch-hidden: drop_at is hidden, and no part marked data-field-control is labelled"
    );

    let said = said_of(&IgnoresDisabled);
    assert_eq!(
        said,
        "control app.swatch: with disabled set, div[app:swatch] > button is not disabled"
    );

    let said = said_of(&TiesTwice);
    assert_eq!(
        said,
        "control app.pin-twice: input sets aria-describedby on the part Field ties; use described_by on its other parts"
    );

    let said = said_of(&Misnamed);
    assert_eq!(
        said,
        "control app.misnamed: draws no form field named drop_at"
    );

    kit::assert_control(&PIN_CODE);
    kit::assert_control(&DELIVERY_PIN);
}

// A field that sits only inside an owned body never posts; a stand-in labelled by another element
// is not labelled by the field's label; and a fieldset is disabled with the rest, while an output,
// which HTML gives no `disabled`, is not asked for one. The label's id among the ids of
// `aria-labelledby` is a label.
#[test]
fn assert_control_refuses_a_sealed_field_a_label_elsewhere_and_an_enabled_fieldset() {
    assert_eq!(
        said_of(&SealedField),
        "control app.sealed: drop_at is drawn inside an owned body (data-ui-keep), where it never posts; draw it outside the control's owned bodies"
    );

    assert_eq!(
        said_of(&LabelledElsewhere),
        "control app.swatch-elsewhere: drop_at is hidden, and no part marked data-field-control is labelled"
    );

    assert_eq!(
        said_of(&IgnoresDisabledSet),
        "control app.swatch-set: with disabled set, div[app:swatch-set] > fieldset is not disabled"
    );

    kit::assert_control(&HiddenLabelled);
}

// An inline edit is held to the control's check before the app stores anything: refused with a
// toast the cell shows and no navigation, or with a redirect when JavaScript is off.
#[tokio::test]
async fn an_inline_edit_through_a_control_is_refused_by_the_controls_check() {
    let app = boot().await;

    let response = post(&app, "/deliveries/42/pin", "value=110001", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.json_value();
    let ops = body["ops"].as_array().expect("the ops");
    assert_eq!(ops.len(), 1, "{ops:?}");
    assert_eq!(ops[0]["op"], "toast");
    assert_eq!(ops[0]["tone"], "danger");
    assert_eq!(ops[0]["text"], "We deliver to Ludhiana and Jalandhar only");
    assert!(
        ops.iter().all(|op| op["op"] != "navigate"),
        "the runtime only puts the cell back: {ops:?}"
    );

    // With JavaScript off the person goes back with the message as a flash.
    let response = post(&app, "/deliveries/42/pin", "value=110001", false).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.header("location"), Some("/deliveries"));
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .any(|cookie| cookie
                .to_str()
                .unwrap_or_default()
                .starts_with("navree_ui_flash=")),
        "the flash cookie: {:?}",
        response.headers()
    );

    let response = post(&app, "/deliveries/42/pin", "value=141001", true).await;
    assert_eq!(response.status(), StatusCode::OK);

    // A post with no `value` is no edit.
    let response = post(&app, "/deliveries/42/pin", "other=141001", true).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The built-in editors have no control to check, and read the value as its type only.
    let response = post(&app, "/deliveries/42/note", "value=110001", true).await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// An inline edit's post of `value=141001` to `/deliveries/42/pin`, with `headers`.
fn edit_post(method: Method, headers: &[(&str, &str)]) -> http::Request<Body> {
    let mut request = http::Request::builder()
        .method(method)
        .uri("/deliveries/42/pin");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::from("value=141001")).expect("a post")
}

/// What an inline edit's post is refused with, or `None` when its value is read.
async fn edit_refused(request: http::Request<Body>) -> Option<AppError> {
    EditedValue::<String>::from_request(request).await.err()
}

// An inline edit has the doors a form's post has, with or without a session: a post from another
// site, a body that is not a form, and a GET or a HEAD are refused before the value is read.
#[tokio::test]
async fn an_edited_value_is_refused_from_another_site_in_another_body_and_by_a_get() {
    let form = ("content-type", FORM);
    let request = edit_post(Method::POST, &[form]);
    let Ok(edit) = EditedValue::<String>::from_request(request).await else {
        panic!("a post from this site is read");
    };
    assert_eq!(edit.text(), "141001");

    // A post from a sibling subdomain or another site, one whose origin is another site's, and
    // one that carries a cookie and says nothing of where it comes from.
    let elsewhere: [&[(&str, &str)]; 4] = [
        &[form, ("sec-fetch-site", "same-site")],
        &[form, ("sec-fetch-site", "cross-site")],
        &[
            form,
            ("host", "app.example"),
            ("origin", "https://evil.example"),
        ],
        &[form, ("cookie", "sid=abc")],
    ];
    for headers in elsewhere {
        let refused = edit_refused(edit_post(Method::POST, headers)).await;
        assert_eq!(
            refused.map(|refused| refused.status),
            Some(StatusCode::FORBIDDEN),
            "{headers:?}"
        );
    }

    // A body any page can send without asking first, text that reads as a form would.
    let request = edit_post(Method::POST, &[("content-type", "text/plain")]);
    let refused = edit_refused(request).await;
    assert_eq!(
        refused.map(|refused| refused.status),
        Some(StatusCode::UNSUPPORTED_MEDIA_TYPE)
    );

    // A link or a prefetch edits nothing.
    for method in [Method::GET, Method::HEAD] {
        let refused = edit_refused(edit_post(method.clone(), &[form]))
            .await
            .expect("refused");
        assert_eq!(refused.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(refused.into_response().headers()["allow"], "POST");
    }
}

// A value that is not of the field's type is refused in the form's own words, with no check of
// a control's to say so.
#[tokio::test]
async fn an_edited_value_the_type_cannot_read_is_refused_in_the_forms_words() {
    let app = boot().await;
    let response = post(&app, "/deliveries/42/seats", "value=x", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json_value()["ops"][0]["text"],
        "Value is not valid."
    );
    let response = post(&app, "/deliveries/42/seats", "value=4", true).await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// What `Recording` was last drawn for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Drawn {
    name: String,
    id: String,
    label: String,
    label_id: Option<String>,
    value: Option<String>,
    disabled: bool,
}

static DRAWN: Mutex<Option<Drawn>> = Mutex::new(None);

/// Accepts everything, and records what it was drawn for.
struct Recording;

impl FieldControl for Recording {
    fn name(&self) -> &'static str {
        "app.recording"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        *DRAWN.lock().expect("the record") = Some(Drawn {
            name: field.name.to_owned(),
            id: field.id.to_owned(),
            label: field.label.to_owned(),
            label_id: field.label_id.map(str::to_owned),
            value: field.value.map(str::to_owned),
            disabled: field.disabled,
        });
        el("input")
            .attr("name", field.name.to_owned())
            .attr("aria-label", field.label.to_owned())
            .flag("disabled", field.disabled)
            .into()
    }
}

static RECORDING: Recording = Recording;

/// What the control of `cell` was drawn for.
fn drawn_for(cell: EditCell) -> Drawn {
    *DRAWN.lock().expect("the record") = None;
    let _ = Node::from(cell);
    DRAWN
        .lock()
        .expect("the record")
        .clone()
        .expect("the control was drawn")
}

// A cell's control is drawn for the field `value`, with an id of its own for each row, the
// label as its `aria-label` and no label id; the editors of today are drawn as before.
#[test]
fn an_edit_cell_control_gets_an_id_of_its_own_no_label_id_and_its_disabled() {
    let cell = EditCell::control("141001", "/deliveries/42/pin", Control::new(&RECORDING))
        .label("Pin code")
        .disabled(true);
    let drawn = drawn_for(cell);
    let id = format!("edit-{}", assets::content_hash(b"/deliveries/42/pin"));
    assert_eq!(
        drawn,
        Drawn {
            name: "value".to_owned(),
            id: id.clone(),
            label: "Pin code".to_owned(),
            label_id: None,
            value: Some("141001".to_owned()),
            disabled: true,
        }
    );

    let other = drawn_for(EditCell::control(
        "144001",
        "/deliveries/43/pin",
        Control::new(&RECORDING),
    ));
    assert_ne!(other.id, id, "no two rows share an id");
    assert_eq!(
        other.id,
        format!("edit-{}", assets::content_hash(b"/deliveries/43/pin"))
    );
    assert_eq!(other.label, "Edit");
    assert!(!other.disabled);

    // The control sits inside the same cell form as the built-in editors.
    let html = render(
        &EditCell::control("141001", "/deliveries/42/pin", Control::new(&PIN_CODE))
            .label("Pin code")
            .into(),
    );
    assert!(
        html.starts_with(
            r#"<form data-slot="edit-cell" method="post" action="/deliveries/42/pin" data-ui-form data-ui-edit>"#
        ),
        "{html}"
    );
    assert!(html.contains(r#"name="value""#), "{html}");
    assert!(html.contains(r#"value="141001""#), "{html}");
    assert!(html.ends_with("</noscript></form>"), "{html}");

    // The editors of today are unchanged.
    assert_eq!(
        render(&EditCell::text("Kesar", "/deals/1/title").into()),
        r#"<form data-slot="edit-cell" method="post" action="/deals/1/title" data-ui-form data-ui-edit><input data-slot="input" data-size="sm" type="text" name="value" value="Kesar" aria-label="Edit"><noscript><button data-slot="button" data-variant="outline" data-size="xs" type="submit">Save</button></noscript></form>"#
    );
}

#[tokio::test]
async fn a_refused_cell_value_is_said_in_the_language_of_the_page() {
    let invalid_request = || {
        http::Request::builder()
            .method(Method::POST)
            .uri("/deliveries/42/seats")
            .header("content-type", FORM)
            .body(Body::from("value=x"))
            .expect("a post")
    };

    // Hindi via DateLocale tag (`hi`) and Hindi template.
    let hindi_messages = AppMessages::new(
        Arc::new(
            navree_validation::Messages::default()
                .with_message("form.invalid", ":attribute सही नहीं है।"),
        ),
        None,
    );
    let mut hindi_words = navree_ui::localize::ReaderWords::english();
    hindi_words.dates = navree_ui::dates::DateLocale::of("hi");
    let hindi = Language::new(hindi_messages.clone(), hindi_words);

    let edit = EditedValue::<u8>::from_request(invalid_request())
        .await
        .expect("a posted edit");
    let refused = edit.read(Control::new(&STARS), &hindi).unwrap_err();
    assert_eq!(refused.message(), "मान सही नहीं है।");

    // An explicit `words.value` set by a localizer overrides the tag fallback.
    let mut custom_words = navree_ui::localize::ReaderWords::english();
    custom_words.value = "सीट संख्या".into();
    let custom = Language::new(hindi_messages.clone(), custom_words);
    let edit = EditedValue::<u8>::from_request(invalid_request())
        .await
        .expect("a posted edit");
    let refused = edit.read_built_in(&custom).unwrap_err();
    assert_eq!(refused.message(), "सीट संख्या सही नहीं है।");

    // A configured `validation.attributes` display name for `value` wins first.
    let named = Language::new(
        hindi_messages.with_attributes([("value", "कुल सीटें")]),
        navree_ui::localize::ReaderWords::english(),
    );
    let edit = EditedValue::<u8>::from_request(invalid_request())
        .await
        .expect("a posted edit");
    let refused = edit.read(Control::new(&STARS), &named).unwrap_err();
    assert_eq!(refused.message(), "कुल सीटें सही नहीं है।");
}

#[test]
fn a_control_field_takes_an_id_a_label_a_placeholder_and_a_description() {
    let field = ControlField::new("drop_at")
        .id("parcel-drop-42")
        .label("Drop point in Ludhiana")
        .label_id(Some("parcel-drop-42-label"))
        .value("30.9010, 75.8573")
        .placeholder("30.9010, 75.8573")
        .described_by("parcel-drop-42-help parcel-drop-42-error")
        .required(true)
        .invalid(true)
        .disabled(false);

    assert_eq!(field.name, "drop_at");
    assert_eq!(field.id, "parcel-drop-42");
    assert_eq!(field.label, "Drop point in Ludhiana");
    assert_eq!(field.label_id, Some("parcel-drop-42-label"));
    assert_eq!(field.value, Some("30.9010, 75.8573"));
    assert_eq!(field.placeholder, Some("30.9010, 75.8573"));
    assert_eq!(
        field.described_by,
        Some("parcel-drop-42-help parcel-drop-42-error")
    );
    assert!(field.required);
    assert!(field.invalid);
    assert!(!field.disabled);
}

/// Accepts only digits in `check`, and explains in `parse_error` why digits outside `u8` failed
/// to parse.
struct CrateCount;

impl FieldControl for CrateCount {
    fn name(&self) -> &'static str {
        "app.crate-count"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("name", field.name.to_owned())
            .flag("disabled", field.disabled)
            .into()
    }

    fn check(&self, value: &str, _cx: &ControlCx<'_>) -> Result<(), String> {
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(())
        } else {
            Err("Enter digits only for crates".to_owned())
        }
    }

    fn parse_error(&self, value: &str, _cx: &ControlCx<'_>) -> Option<String> {
        Some(format!("{value} is above the 255-crate limit"))
    }
}

static CRATE_COUNT: CrateCount = CrateCount;

#[derive(Debug, Deserialize, Validate, Form)]
struct ShipmentCrates {
    /// Crates
    #[validate(skip)]
    #[form(control = CRATE_COUNT)]
    crates: u8,
    /// Spare crates
    #[validate(skip)]
    #[form(control = CRATE_COUNT, error = "Enter 0 to 255 spare crates")]
    spare: Option<u8>,
}

#[tokio::test]
async fn a_controls_check_can_say_why_a_value_did_not_parse() {
    // When `check` refuses, its message is the field's one error and `parse_error` is not used.
    let refused_by_check =
        decode_for::<ShipmentCrates>(&pairs(&[("crates", "dozen")]), &Language::english())
            .unwrap_err();
    assert_eq!(refused_by_check["crates"], ["Enter digits only for crates"]);

    // When `check` accepts `"400"` and `u8` cannot parse it, `parse_error` supplies the words,
    // while a field with its own `error` keeps that override.
    let parse_failed = decode_for::<ShipmentCrates>(
        &pairs(&[("crates", "400"), ("spare", "300")]),
        &Language::english(),
    )
    .unwrap_err();
    assert_eq!(parse_failed["crates"], ["400 is above the 255-crate limit"]);
    assert_eq!(parse_failed["spare"], ["Enter 0 to 255 spare crates"]);

    // `EditedValue::read` uses `parse_error` on a cell too.
    let post_body = |body: &'static str| {
        http::Request::builder()
            .method(Method::POST)
            .uri("/deliveries/42/crates")
            .header("content-type", FORM)
            .body(Body::from(body))
            .expect("a post")
    };
    let edit = EditedValue::<u8>::from_request(post_body("value=400"))
        .await
        .expect("a posted edit");
    let refused = edit
        .read(Control::new(&CRATE_COUNT), &Language::english())
        .unwrap_err();
    assert_eq!(refused.message(), "400 is above the 255-crate limit");

    let edit = EditedValue::<u8>::from_request(post_body("value=dozen"))
        .await
        .expect("a posted edit");
    let refused = edit
        .read(Control::new(&CRATE_COUNT), &Language::english())
        .unwrap_err();
    assert_eq!(refused.message(), "Enter digits only for crates");
}
