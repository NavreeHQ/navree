//! A form's submit button in another look or none, a field in the mono face, fields at another
//! height, and a toggle group outside its form.

use navree_ui::form::{Form, FormView, Widget};
use navree_ui::{
    Button, ButtonVariant, Form, Node, Options, Size, ToggleGroup, ToggleItem, Uses, Validate,
    icons, measure, render,
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Plan {
    /// Starter
    Starter,
    /// Scale
    Scale,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Checkout {
    /// Plan
    #[validate(skip)]
    #[form(help = "You can change it later.")]
    plan: Plan,
    /// Company name
    #[validate(length(min = 1, max = 20))]
    company: String,
    /// GSTIN
    #[validate(length(min = 15, max = 15))]
    #[form(mono, placeholder = "22AAAAA0000A1Z5")]
    gstin: String,
}

fn cards(name: &str) -> ToggleGroup {
    ToggleGroup::single(name, "Plan")
        .item(ToggleItem::new("starter", "Starter"))
        .item(ToggleItem::new("scale", "Scale"))
}

fn form() -> FormView<Checkout> {
    FormView::<Checkout>::new().post("/checkout").submit("Pay")
}

#[test]
fn the_submit_button_takes_a_look_or_is_a_button_of_the_apps_own() {
    let plain = render(&form().into());
    assert!(
        plain.contains(
            r#"<button data-slot="button" data-variant="primary" type="submit">Pay</button>"#
        ),
        "{plain}"
    );
    let outline = render(&form().submit_variant(ButtonVariant::Outline).into());
    assert!(
        outline.contains(
            r#"<button data-slot="button" data-variant="outline" type="submit">Pay</button>"#
        ),
        "{outline}"
    );
    assert_eq!(
        outline.replace(r#"data-variant="outline""#, r#"data-variant="primary""#),
        plain,
        "the look is all that changes"
    );
    let own = render(
        &form()
            .submit_button(
                Button::outline("Continue with email")
                    .icon(icons::MAIL)
                    .size(Size::Lg)
                    .block()
                    .href("/elsewhere"),
            )
            .into(),
    );
    let actions = own.split(r#"data-slot="form-actions""#).nth(1).unwrap();
    assert!(actions.contains(r#"type="submit""#), "{actions}");
    assert!(actions.contains(r#"data-size="lg""#), "{actions}");
    assert!(actions.contains("Continue with email"), "{actions}");
    assert!(
        !actions.contains("/elsewhere") && actions.starts_with("><button"),
        "it submits the form: no link"
    );
    assert!(!actions.contains(">Pay<"), "{actions}");
}

#[test]
fn a_form_without_actions_draws_its_fields_and_no_buttons() {
    let html = render(&form().cancel("Back", "/plans").without_actions().into());
    assert!(
        html.contains(r#"<form data-slot="form" id="checkout""#),
        "{html}"
    );
    assert!(html.contains(r#"name="company""#), "{html}");
    assert!(
        !html.contains("form-actions") && !html.contains("<button"),
        "{html}"
    );
    // A button elsewhere names the form.
    assert_eq!(form().form_id(), "checkout");
}

#[test]
fn a_mono_field_is_drawn_in_the_mono_face_and_links_the_parts_sheet() {
    assert!(Checkout::FIELDS[2].mono);
    assert!(!Checkout::FIELDS[1].mono);
    assert_eq!(
        Checkout::FIELDS[2].widget,
        Widget::Input(navree_ui::InputKind::Text)
    );
    let html = render(&form().into());
    let gstin = html.split(r#"name="gstin""#).nth(1).unwrap();
    assert!(
        gstin.split('>').next().unwrap().contains("data-mono"),
        "{gstin}"
    );
    let company = html.split(r#"name="company""#).nth(1).unwrap();
    assert!(!company.split('>').next().unwrap().contains("data-mono"));
    let node: Node = form().into();
    assert!(measure(&node).uses.has(Uses::PARTS_STYLES));
    let sheet = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/extra/parts.css"),
    )
    .unwrap();
    assert!(sheet.contains("[data-slot=input][data-mono]{font-family:var(--font-mono)}"));
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
enum Currency {
    Inr,
    Usd,
}

#[derive(Debug, Deserialize, Validate, Form)]
#[form(size = lg)]
struct SignIn {
    /// Email
    #[validate(email)]
    email: String,
    #[validate(length(min = 8))]
    password: String,
    /// Workspace
    #[validate(skip)]
    #[form(widget = listbox)]
    workspace: Plan,
    /// Plan
    #[validate(skip)]
    plan: Plan,
    /// Budget
    #[validate(skip)]
    #[form(addon = "currency")]
    budget: navree_ui::Money,
    #[validate(skip)]
    currency: Currency,
    /// Code
    #[validate(skip)]
    #[form(size = sm)]
    code: Option<String>,
    /// Remember me
    #[validate(skip)]
    remember: bool,
}

#[test]
fn a_form_of_a_size_draws_its_fields_at_the_themes_step_of_that_size() {
    assert!(
        SignIn::FIELDS[..6]
            .iter()
            .all(|field| field.size == Size::Lg)
    );
    assert_eq!(SignIn::FIELDS[6].size, Size::Sm, "a field says its own");
    let html = render(&FormView::<SignIn>::new().post("/sign-in").into());
    let tag = |name: &str| -> String {
        let at = html
            .find(&format!(r#"name="{name}""#))
            .unwrap_or_else(|| panic!("{name}: {html}"));
        let open = html[..at].rfind('<').unwrap();
        html[open..at + html[at..].find('>').unwrap()].to_owned()
    };
    for name in ["email", "password", "plan"] {
        assert!(
            tag(name).contains(r#"data-size="lg""#),
            "{name}: {}",
            tag(name)
        );
    }
    assert!(tag("code").contains(r#"data-size="sm""#), "{}", tag("code"));
    // The listbox and the addon's group take it too.
    assert!(
        html.contains(r#"data-slot="select" data-size="lg""#)
            || html.contains(r#"data-size="lg" data-slot="select""#)
            || html
                .split(r#"data-slot="select""#)
                .nth(1)
                .is_some_and(|rest| rest
                    .split('>')
                    .next()
                    .unwrap()
                    .contains(r#"data-size="lg""#)),
        "{html}"
    );
    let group = html.split(r#"data-slot="input-group""#).nth(1).unwrap();
    assert!(
        group
            .split('>')
            .next()
            .unwrap()
            .contains(r#"data-size="lg""#),
        "{group}"
    );
    // A checkbox keeps its own.
    assert!(
        !tag("remember").contains("data-size"),
        "{}",
        tag("remember")
    );
    let node: Node = FormView::<SignIn>::new().into();
    assert!(measure(&node).uses.has(Uses::PARTS_STYLES));
    let sheet = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/extra/parts.css"),
    )
    .unwrap();
    assert!(
        sheet.contains("&[data-size=lg]{height:var(--field-lg,2.25rem)}"),
        "{sheet}"
    );
}

#[test]
fn a_toggle_group_outside_a_form_joins_it_by_its_id() {
    let html = render(&cards("plan").form("checkout").into());
    assert_eq!(html.matches(r#"form="checkout""#).count(), 2, "{html}");
    assert!(!render(&cards("plan").into()).contains("form="));
}
