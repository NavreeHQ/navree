//! Two forms of one type on one page: an id prefix puts every id the form writes, and every
//! reference to one, in a scope of its own, and leaves what is posted alone.

use std::collections::BTreeSet;

use chrono::{NaiveDate, NaiveTime};
use navree_ui::form::{ControlField, FieldControl, FieldErrors, Rejected, decode};
use navree_ui::{Form, FormView, Money, Node, Options, Validate, View, el, render};
use serde::Deserialize;

/// A control of one's own whose group is named by the label's id and described by the help's.
struct Pin;

impl FieldControl for Pin {
    fn name(&self) -> &'static str {
        "app.pin"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        el("div")
            .slot("app:pin")
            .attr("role", "group")
            .attr_opt("aria-labelledby", field.label_id.map(str::to_owned))
            .attr_opt("aria-describedby", field.described_by.map(str::to_owned))
            .child(
                el("input")
                    .attr("type", "text")
                    .attr("name", field.name.to_owned())
                    .attr("inputmode", "numeric"),
            )
            .into()
    }
}

static PIN: Pin = Pin;

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    /// Sale
    Sale,
    /// Purchase
    Purchase,
}

/// One of every widget that writes ids of its own.
#[derive(Debug, Deserialize, Validate, Form)]
#[allow(dead_code, reason = "the form is drawn, its fields are not used")]
struct Entry {
    /// Party
    #[validate(length(min = 1))]
    #[form(help = "Who paid or was paid.")]
    party: String,
    /// Amount
    #[validate(skip)]
    amount: Money,
    /// Kind
    #[validate(skip)]
    #[form(widget = select)]
    kind: Kind,
    /// City
    #[validate(skip)]
    #[form(widget = combobox, placeholder = "Search cities")]
    city: Option<String>,
    /// Due
    #[validate(skip)]
    #[form(widget = date_picker)]
    due: NaiveDate,
    /// Opens at
    #[validate(skip)]
    #[form(widget = time_picker)]
    opens: NaiveTime,
    /// Mobile
    #[validate(skip)]
    #[form(widget = phone)]
    mobile: Option<String>,
    /// Notes
    #[validate(skip)]
    #[form(widget = textarea, help = "Shown on the receipt.")]
    notes: Option<String>,
    /// Paid
    #[validate(skip)]
    paid: bool,
    /// Pin
    #[validate(skip)]
    #[form(control = PIN, help = "Four digits")]
    pin: String,
    /// Receipt
    #[validate(skip)]
    #[form(widget = file)]
    receipt: String,
    /// Priority
    #[validate(range(min = 0, max = 100))]
    #[form(widget = slider)]
    priority: i64,
}

/// The smallest form there is.
#[derive(Debug, Deserialize, Validate, Form)]
#[allow(
    dead_code,
    reason = "the form is drawn and read back, its field is not used"
)]
struct Note {
    /// Title
    #[validate(length(min = 1))]
    title: String,
}

/// The text between each `<` of `html` and the `>` after it.
fn tags(html: &str) -> impl Iterator<Item = &str> {
    html.split('<')
        .skip(1)
        .filter_map(|rest| rest.split('>').next())
}

/// The value of attribute `name` in `tag`, when it has one.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')?;
    Some(&tag[start..start + end])
}

/// Every id the markup writes, in order.
fn ids(html: &str) -> Vec<&str> {
    tags(html).filter_map(|tag| attr(tag, "id")).collect()
}

/// Every name the markup posts, in order.
fn names(html: &str) -> Vec<&str> {
    tags(html).filter_map(|tag| attr(tag, "name")).collect()
}

/// What the markup's attributes name by id, as `(attribute, id)`: a list of ids is one pair each.
fn references(html: &str) -> Vec<(&'static str, String)> {
    const NAMES: [&str; 7] = [
        "for",
        "form",
        "aria-describedby",
        "aria-labelledby",
        "aria-controls",
        "popovertarget",
        "data-ui-anchor",
    ];
    let mut found = Vec::new();
    for tag in tags(html) {
        for name in NAMES {
            if let Some(value) = attr(tag, name) {
                found.extend(value.split_whitespace().map(|id| (name, id.to_owned())));
            }
        }
    }
    found
}

fn entry(prefix: Option<&str>) -> String {
    let mut view = FormView::<Entry>::new().post("/entries");
    if let Some(prefix) = prefix {
        view = view.id_prefix(prefix);
    }
    render(&view.into())
}

fn note(view: FormView<Note>) -> String {
    render(&view.post("/notes").into())
}

#[test]
fn a_form_with_no_prefix_is_drawn_as_it_was() {
    let plain = note(FormView::new());
    assert_eq!(
        plain,
        concat!(
            r#"<form data-slot="form" id="note" method="post" action="/notes" data-ui-form>"#,
            r#"<div data-slot="form-grid"><div data-slot="field">"#,
            r#"<label data-slot="label" for="note-title">Title</label>"#,
            r#"<input data-slot="input" type="text" name="title" required minlength="1" id="note-title">"#,
            r#"</div></div>"#,
            r#"<div data-slot="form-actions">"#,
            r#"<button data-slot="button" data-variant="primary" type="submit">Save</button>"#,
            r#"</div></form>"#
        )
    );
    // A prefix with nothing in it is no prefix.
    for empty in ["", " ", "-", " - "] {
        assert_eq!(note(FormView::new().id_prefix(empty)), plain, "{empty:?}");
    }
    // The bigger form, too: no prefix and an empty prefix write the same bytes.
    assert_eq!(entry(None), entry(Some("")));
    assert_eq!(FormView::<Note>::new().form_id(), "note");
}

#[test]
fn a_prefix_goes_before_every_id_and_every_reference_of_a_form() {
    let view = FormView::<Note>::new().id_prefix("quick");
    assert_eq!(view.form_id(), "quick-note");
    assert_eq!(
        note(view),
        concat!(
            r#"<form data-slot="form" id="quick-note" method="post" action="/notes" data-ui-form>"#,
            r#"<div data-slot="form-grid"><div data-slot="field">"#,
            r#"<label data-slot="label" for="quick-note-title">Title</label>"#,
            r#"<input data-slot="input" type="text" name="title" required minlength="1" id="quick-note-title">"#,
            r#"</div></div>"#,
            r#"<div data-slot="form-actions">"#,
            r#"<button data-slot="button" data-variant="primary" type="submit">Save</button>"#,
            r#"</div></form>"#
        )
    );
}

#[test]
fn a_fields_help_and_error_are_tied_to_it_by_prefixed_ids() {
    let errors = [("title".to_owned(), vec!["Give it a title.".to_owned()])];
    let html = render(
        &FormView::<Note>::new()
            .id_prefix("edit")
            .with_errors(errors.into_iter().collect())
            .into(),
    );
    assert!(html.contains(r#"id="edit-note-title""#), "{html}");
    assert!(
        html.contains(r#"aria-describedby="edit-note-title-error""#),
        "{html}"
    );
    assert!(
        html.contains(
            r#"<p data-slot="field-error" id="edit-note-title-error">Give it a title.</p>"#
        ),
        "{html}"
    );
}

#[test]
fn two_forms_of_one_type_share_no_id_and_every_reference_finds_its_id() {
    let add = entry(Some("add"));
    let edit = entry(Some("edit"));
    let page = format!("{add}{edit}");
    let all = ids(&page);
    assert!(all.len() > 20, "{} ids in {page}", all.len());
    let mut seen = BTreeSet::new();
    for id in &all {
        assert!(seen.insert(*id), "{id} is written twice in {page}");
        assert!(
            id.starts_with("add-") || id.starts_with("edit-"),
            "{id} is in no scope"
        );
    }
    // Every reference points at an id the page writes, and one of its own form's.
    for (html, scope) in [(&add, "add-"), (&edit, "edit-")] {
        let named = references(html);
        assert!(named.len() > 10, "{} references", named.len());
        for (attribute, id) in named {
            assert!(seen.contains(id.as_str()), "{attribute}={id} names no id");
            assert!(id.starts_with(scope), "{attribute}={id} leaves its scope");
        }
    }
    // The ids of the widgets that write their own are in the scope too.
    for fragment in [
        r#"id="add-entry-due-picker""#,
        r#"id="add-entry-opens-picker""#,
        r#"id="add-entry-city-picker""#,
        r#"id="add-entry-upload-receipt""#,
        r#"id="add-entry-pin-label""#,
        r#"aria-labelledby="add-entry-pin-label""#,
        r#"aria-describedby="add-entry-pin-help""#,
    ] {
        assert!(add.contains(fragment), "{fragment} in {add}");
    }
    #[cfg(feature = "phone")]
    assert!(add.contains(r#"id="add-entry-mobile-countries""#), "{add}");
}

#[test]
fn without_a_prefix_two_forms_of_one_type_repeat_their_ids() {
    let page = format!("{}{}", entry(None), entry(None));
    let all = ids(&page);
    let unique: BTreeSet<&str> = all.iter().copied().collect();
    assert!(unique.len() < all.len(), "{page}");
}

#[test]
fn a_prefix_changes_no_posted_name_and_the_form_still_decodes() {
    let plain = entry(None);
    let scoped = entry(Some("add"));
    assert_eq!(names(&scoped), names(&plain));
    assert!(names(&scoped).contains(&"party"), "{scoped}");
    assert!(names(&scoped).contains(&"mobile"), "{scoped}");
    // The names are what the handler reads.
    let small = note(FormView::new().id_prefix("add"));
    assert_eq!(names(&small), ["title"]);
    let posted = [("title".to_owned(), "Meera Sharma".to_owned())];
    let read: Note = decode(&posted).unwrap();
    assert_eq!(read.title, "Meera Sharma");
}

#[test]
fn a_prefix_is_made_safe_to_be_part_of_an_id() {
    let view = FormView::<Note>::new().id_prefix("a b\"c<d>");
    assert_eq!(view.form_id(), "a-b-c-d-note");
    let html = note(view);
    assert!(
        html.contains(r#"<form data-slot="form" id="a-b-c-d-note""#),
        "{html}"
    );
    assert!(!html.contains(r#"c<d"#), "{html}");
    // Underscores and digits stay.
    assert_eq!(
        FormView::<Note>::new().id_prefix("dialog_2").form_id(),
        "dialog_2-note"
    );
}

async fn body_text(response: navree_http::response::Response) -> String {
    use http_body_util::BodyExt;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// The parts of a post the form's page sent, with or without the runtime.
fn post(runtime: bool) -> http::request::Parts {
    let request = http::Request::builder().method("POST").uri("/notes");
    let request = if runtime {
        request.header("x-navree-ui", "1")
    } else {
        request
    };
    request.body(()).unwrap().into_parts().0
}

#[tokio::test]
async fn a_refused_form_names_the_form_with_the_prefix_in_its_errors_and_in_its_page() {
    let note = Note {
        title: String::new(),
    };
    let errors = FieldErrors::from([("title".to_owned(), vec!["Give it a title.".to_owned()])]);
    let page = |form: FormView<Note>| View::new("Note").body(Node::from(form));
    // The runtime is told which form the errors are for.
    let plain = Rejected::<Note>::after_decode(&note, errors.clone(), &post(true)).respond(page);
    assert!(body_text(plain).await.contains(r#""form":"note""#));
    let scoped = Rejected::<Note>::after_decode(&note, errors.clone(), &post(true))
        .id_prefix("quick")
        .respond(page);
    let ops = body_text(scoped).await;
    assert!(ops.contains(r#""form":"quick-note""#), "{ops}");
    assert!(!ops.contains(r#""form":"note""#), "{ops}");
    // With no script the form comes back in its scope.
    let scoped = Rejected::<Note>::after_decode(&note, errors, &post(false))
        .id_prefix("quick")
        .respond(page);
    let html = body_text(scoped).await;
    assert!(
        html.contains(r#"<form data-slot="form" id="quick-note""#),
        "{html}"
    );
    assert!(html.contains(r#"for="quick-note-title""#), "{html}");
}
