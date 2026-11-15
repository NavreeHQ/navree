//! A form field drawn by a control of one's own: drawn inside its field, and its server check run on every path a value arrives by, with the app's configuration.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use http::{Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, Extension, HttpModule, ModuleBuilderHttpExt, RouteDef};
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
#[allow(dead_code, reason = "the form is read and its check run; its field is not used")]
struct Dropoff {
    /// Pin code
    #[validate(skip)]
    #[form(control = DELIVERY_PIN)]
    pin_code: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
#[allow(dead_code, reason = "the form is read and its check run; its field is not used")]
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
    let errors = draft
        .read_for::<Parcel>("parcel", &language)
        .unwrap_err();
    assert_eq!(errors["pin_code"], ["We deliver to Ludhiana and Jalandhar only"]);
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
    assert_eq!(errors["pin_code"], ["We deliver to Ludhiana and Jalandhar only"]);
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

/// What `kit::assert_control` says is wrong with `control`.
fn said_of(control: &dyn FieldControl) -> String {
    let result = catch_unwind(AssertUnwindSafe(|| kit::assert_control(control)));
    match result {
        Ok(()) => panic!("{} broke no rule", control.name()),
        Err(payload) => payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_owned()))
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
    assert_eq!(said, "control app.misnamed: draws no form field named drop_at");

    kit::assert_control(&PIN_CODE);
    kit::assert_control(&DELIVERY_PIN);
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

// A value that is not of the field's type is refused in the form's own words, with no check of
// a control's to say so.
#[tokio::test]
async fn an_edited_value_the_type_cannot_read_is_refused_in_the_forms_words() {
    let app = boot().await;
    let response = post(&app, "/deliveries/42/seats", "value=x", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.json_value()["ops"][0]["text"], "Value is not valid.");
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
