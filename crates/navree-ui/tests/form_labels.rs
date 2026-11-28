//! A field's label chosen when the page is drawn (`FormView::label`): the form is drawn with
//! it, as text, and posts and reads back as before.

use navree_ui::form::{ControlField, FieldControl, decode};
use navree_ui::{Form, FormView, Node, Validate, el, render};
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate, Form)]
struct SignIn {
    /// Email
    #[validate(email)]
    #[form(widget = email)]
    email: String,
    /// Keep me signed in
    #[validate(skip)]
    #[serde(default)]
    remember: bool,
}

#[test]
fn a_field_takes_a_label_chosen_when_the_page_is_drawn() {
    let plain = render(&FormView::<SignIn>::new().into());
    let labelled = render(
        &FormView::<SignIn>::new()
            .label("email", "Work email")
            .label("remember", "Stay signed in")
            .into(),
    );
    assert!(
        labelled.contains(r#"<label data-slot="label" for="sign-in-email">Work email</label>"#),
        "{labelled}"
    );
    assert!(
        labelled.contains("<span>Stay signed in</span>"),
        "{labelled}"
    );
    // Nothing else changes: the names, the ids, the rules.
    assert_eq!(
        labelled
            .replace("Work email", "Email")
            .replace("Stay signed in", "Keep me signed in"),
        plain
    );
    // The label the type gives, or a field the form doesn't have, changes nothing.
    assert_eq!(
        render(
            &FormView::<SignIn>::new()
                .label("email", "Email")
                .label("password", "Password")
                .into()
        ),
        plain
    );
    // The last label given is the one drawn.
    let again = render(
        &FormView::<SignIn>::new()
            .label("email", "Work email")
            .label("email", "Office email")
            .into(),
    );
    assert!(again.contains(">Office email</label>") && !again.contains("Work email"));
}

#[test]
fn a_label_is_text_and_the_form_posts_as_before() {
    let hostile = render(
        &FormView::<SignIn>::new()
            .label("email", "<b onclick=alert(1)>Work</b> email")
            .into(),
    );
    assert!(!hostile.contains("<b onclick"), "{hostile}");
    assert!(
        hostile.contains("&lt;b onclick=alert(1)&gt;Work&lt;/b&gt; email"),
        "{hostile}"
    );
    let read: SignIn = decode(&[
        ("email".to_owned(), "anu@example.com".to_owned()),
        ("remember".to_owned(), "true".to_owned()),
    ])
    .expect("the post reads back");
    assert_eq!(read.email, "anu@example.com");
    assert!(read.remember);
}

/// A code drawn by a control of the app's own (`FieldControl`), which names its input by the
/// label the form gives the field.
struct Code;

impl FieldControl for Code {
    fn name(&self) -> &'static str {
        "app.code"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("input")
            .attr("type", "text")
            .attr("name", field.name.to_owned())
            .attr("data-label", field.label.to_owned())
            .into()
    }
}

static CODE: Code = Code;

#[derive(Debug, Deserialize, Validate, Form)]
struct Verify {
    /// Code
    #[validate(skip)]
    #[form(control = CODE)]
    code: String,
}

#[test]
fn a_label_chosen_when_the_page_is_drawn_also_names_a_field_a_control_draws() {
    let html = render(
        &FormView::<Verify>::new()
            .label("code", "Code from the email")
            .into(),
    );
    assert!(html.contains(">Code from the email</label>"), "{html}");
    assert!(
        html.contains(r#"data-label="Code from the email""#),
        "{html}"
    );
    assert!(!html.contains(">Code</label>"), "{html}");
    assert!(html.contains(r#"name="code""#), "{html}");
}
