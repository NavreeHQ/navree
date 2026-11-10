//! [`DescribedForm`]: a form's fields and their rules in the browser, given at run time.

use navree_validation::{FieldRules, Rule};

use super::{keep_options, keep_slice, keep_str};
use crate::form::{Form, FormField, FormShape, FormView, Widget};

/// The form type of a [`DescribedForm`]'s view: its fields are the description's, so the
/// type has none of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Described;

impl Form for Described {
    const ID: &'static str = "";
    const FIELDS: &'static [FormField] = &[];
    const RULES: &'static [FieldRules] = &[];

    fn values(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// One field of a [`DescribedForm`]: what a struct's field, its doc comment, its
/// `#[form(...)]` and its `#[validate(...)]` say, said at run time. The rules are the
/// ones the browser checks before anything is sent; the page checks the post again.
///
/// ```
/// use navree_ui::InputKind;
/// use navree_ui::described::FieldSpec;
/// use navree_ui::form::Widget;
///
/// let title = FieldSpec::new("title", "Title", Widget::Input(InputKind::Text))
///     .required()
///     .length(None, Some(200));
/// assert_eq!(title.field().name, "title");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct FieldSpec {
    field: FormField,
    rules: Vec<Rule>,
}

impl FieldSpec {
    /// The field `name` (as it posts), labelled `label` and drawn as `widget`. It must be
    /// filled in, as a struct's field that isn't an `Option` must, unless it is
    /// [`optional`](Self::optional).
    pub fn new(name: &str, label: &str, widget: Widget) -> Self {
        Self {
            field: FormField::new(keep_str(name), keep_str(label), widget),
            rules: Vec::new(),
        }
    }

    /// It may be left empty, as an `Option` may.
    pub fn optional(mut self) -> Self {
        self.field.optional = true;
        self
    }

    /// The hint while empty.
    pub fn placeholder(mut self, text: &str) -> Self {
        self.field.placeholder = Some(keep_str(text));
        self
    }

    /// The help below it.
    pub fn help(mut self, text: &str) -> Self {
        self.field.help = Some(keep_str(text));
        self
    }

    /// A select's empty first choice.
    pub fn prompt(mut self, text: &str) -> Self {
        self.field.prompt = Some(keep_str(text));
        self
    }

    /// The message for any failed rule, in place of the rule's own.
    pub fn error(mut self, text: &str) -> Self {
        self.field.error = Some(keep_str(text));
        self
    }

    /// Draws the field `name` inside this one's input group, as its end.
    pub fn addon(mut self, name: &str) -> Self {
        self.field.addon = Some(keep_str(name));
        self
    }

    /// Takes focus when the page loads.
    pub fn autofocus(mut self) -> Self {
        self.field.autofocus = true;
        self
    }

    /// Takes the whole width of the form.
    pub fn wide(mut self) -> Self {
        self.field.wide = true;
        self
    }

    /// A select's or radio group's fixed choices: `(value, label)`. Choices the page
    /// looks up go to [`FormView::options`].
    pub fn options<I, V, L>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = (V, L)>,
        V: AsRef<str>,
        L: AsRef<str>,
    {
        self.field.options = keep_options(options);
        self
    }

    /// The most values a multi-select field takes.
    pub fn max(mut self, values: u32) -> Self {
        self.field.max = Some(values);
        self
    }

    /// A date and time posted with no zone is read as UTC, and the field says so.
    pub fn instant(mut self) -> Self {
        self.field.instant = true;
        self
    }

    /// A number with more than 28 decimal places is refused, not rounded.
    pub fn decimal(mut self) -> Self {
        self.field.decimal = true;
        self
    }

    /// Must not be empty: `required` in the browser, whatever the widget.
    pub fn required(mut self) -> Self {
        self.rules.push(Rule::Required);
        self
    }

    /// The fewest and the most characters: `minlength` and `maxlength`.
    pub fn length(mut self, min: Option<usize>, max: Option<usize>) -> Self {
        self.rules.push(Rule::Length {
            min,
            max,
            bytes: false,
        });
        self
    }

    /// The smallest and the largest number, written as the browser reads them: `min`
    /// and `max`.
    pub fn range(mut self, min: Option<&str>, max: Option<&str>) -> Self {
        self.rules.push(Rule::Range {
            min: min.map(keep_str),
            max: max.map(keep_str),
        });
        self
    }

    /// A format the browser knows, on a text field: `email`, `url` or `phone` make the
    /// input that kind.
    pub fn format(mut self, format: &str) -> Self {
        self.rules.push(Rule::Format(keep_str(format)));
        self
    }

    /// A pattern the browser checks: `pattern`. Only a pattern that is the same in a
    /// browser's dialect belongs here; the page checks the post with its own.
    pub fn browser_pattern(mut self, pattern: &str) -> Self {
        self.rules.push(Rule::BrowserPattern(keep_str(pattern)));
        self
    }

    /// The field as the form reads it.
    pub fn field(&self) -> &FormField {
        &self.field
    }
}

/// A form's fields, given at run time: what `#[derive(Form)]` says of a struct.
/// [`view`](Self::view) is its [`FormView`], drawn by the same code as a derived form's,
/// with every setter a typed view has: where it posts, its values and errors, the choices
/// a page looks up, the uploads.
///
/// ```
/// use navree_ui::InputKind;
/// use navree_ui::described::{DescribedForm, FieldSpec};
/// use navree_ui::form::Widget;
/// use navree_ui::render;
///
/// let form = DescribedForm::new("deal")
///     .field(FieldSpec::new("title", "Title", Widget::Input(InputKind::Text)).required())
///     .field(FieldSpec::new("notes", "Notes", Widget::Textarea { rows: 5 }).optional());
/// let html = render(&form.view().post("/deals").submit("Create deal").into());
/// assert!(html.contains(r#"<form data-slot="form" id="deal" method="post" action="/deals""#));
/// assert!(html.contains(r#"name="title""#) && html.contains("required"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DescribedForm {
    id: &'static str,
    fields: Vec<FormField>,
    rules: Vec<FieldRules>,
}

impl DescribedForm {
    /// A form with the id `id` and no fields yet.
    pub fn new(id: &str) -> Self {
        Self {
            id: keep_str(id),
            fields: Vec::new(),
            rules: Vec::new(),
        }
    }

    /// Adds a field, after the others.
    pub fn field(mut self, spec: FieldSpec) -> Self {
        if !spec.rules.is_empty() {
            self.rules.push(FieldRules {
                name: spec.field.name,
                rules: keep_slice(spec.rules),
            });
        }
        self.fields.push(spec.field);
        self
    }

    /// The fields, in order.
    pub fn fields(&self) -> &[FormField] {
        &self.fields
    }

    /// The form's view, empty: fill it, point it and draw it as a typed one.
    pub fn view(&self) -> FormView<Described> {
        FormView::shaped(FormShape {
            id: self.id,
            fields: keep_slice(self.fields.clone()),
            rules: keep_slice(self.rules.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::components::InputKind;
    use crate::node::Node;
    use crate::render::{measure, render};

    /// A form typed the way the derive types one.
    struct Deal;

    impl Form for Deal {
        const ID: &'static str = "deal";
        const FIELDS: &'static [FormField] = &[
            FormField::new("title", "Title", Widget::Input(InputKind::Text))
                .placeholder(Some("Gulati renewal"))
                .autofocus(true),
            FormField::new("amount", "Amount", Widget::Input(InputKind::Number))
                .help(Some("In rupees")),
            FormField::new("stage", "Stage", Widget::Select)
                .prompt(Some("Pick one"))
                .options(&[("lead", "Lead"), ("won", "Won")]),
            FormField::new("closes", "Closes", Widget::Input(InputKind::Date)).optional(true),
            FormField::new("at", "At", Widget::Input(InputKind::DateTime))
                .optional(true)
                .instant(true),
            FormField::new("notes", "Notes", Widget::Textarea { rows: 5 })
                .optional(true)
                .wide(true),
            FormField::new("hot", "Hot", Widget::Switch),
            FormField::new("contract", "Contract", Widget::File).optional(true),
        ];
        const RULES: &'static [FieldRules] = &[
            FieldRules {
                name: "title",
                rules: &[
                    Rule::Required,
                    Rule::Length {
                        min: None,
                        max: Some(200),
                        bytes: false,
                    },
                ],
            },
            FieldRules {
                name: "amount",
                rules: &[Rule::Range {
                    min: Some("0"),
                    max: None,
                }],
            },
        ];

        fn values(&self) -> Vec<(&'static str, String)> {
            Vec::new()
        }
    }

    /// The same form, described.
    fn described() -> DescribedForm {
        DescribedForm::new("deal")
            .field(
                FieldSpec::new("title", "Title", Widget::Input(InputKind::Text))
                    .placeholder("Gulati renewal")
                    .autofocus()
                    .required()
                    .length(None, Some(200)),
            )
            .field(
                FieldSpec::new("amount", "Amount", Widget::Input(InputKind::Number))
                    .help("In rupees")
                    .range(Some("0"), None),
            )
            .field(
                FieldSpec::new("stage", "Stage", Widget::Select)
                    .prompt("Pick one")
                    .options([("lead", "Lead"), ("won", "Won")]),
            )
            .field(FieldSpec::new("closes", "Closes", Widget::Input(InputKind::Date)).optional())
            .field(
                FieldSpec::new("at", "At", Widget::Input(InputKind::DateTime))
                    .optional()
                    .instant(),
            )
            .field(
                FieldSpec::new("notes", "Notes", Widget::Textarea { rows: 5 })
                    .optional()
                    .wide(),
            )
            .field(FieldSpec::new("hot", "Hot", Widget::Switch))
            .field(FieldSpec::new("contract", "Contract", Widget::File).optional())
    }

    #[test]
    fn a_described_form_is_drawn_exactly_as_the_typed_one() {
        let values = vec![
            ("title".to_owned(), "Sharma <and> sons".to_owned()),
            ("amount".to_owned(), "12500".to_owned()),
            ("stage".to_owned(), "won".to_owned()),
            ("hot".to_owned(), "true".to_owned()),
        ];
        let mut errors = BTreeMap::new();
        errors.insert(
            "title".to_owned(),
            vec!["title must be shorter than or equal to 200 characters".to_owned()],
        );
        errors.insert(
            "deal".to_owned(),
            vec!["Someone changed it first.".to_owned()],
        );
        let typed: Node = FormView::<Deal>::new()
            .post("/deals")
            .csrf("token")
            .hidden("_return", "/deals?page=2")
            .with_values(values.clone())
            .with_errors(errors.clone())
            .cancel("Cancel", "/deals")
            .submit("Create deal")
            .into();
        let drawn: Node = described()
            .view()
            .post("/deals")
            .csrf("token")
            .hidden("_return", "/deals?page=2")
            .with_values(values)
            .with_errors(errors)
            .cancel("Cancel", "/deals")
            .submit("Create deal")
            .into();
        let html = render(&typed);
        assert_eq!(html, render(&drawn));
        assert_eq!(measure(&typed).uses, measure(&drawn).uses);
        // What the rules say reaches the browser.
        assert!(html.contains(r#"maxlength="200""#), "{html}");
        assert!(html.contains(r#"min="0""#), "{html}");
        assert!(html.contains(r#"enctype="multipart/form-data""#), "{html}");
        assert!(
            html.contains(r#"<input type="hidden" name="_return" value="/deals?page=2">"#),
            "{html}"
        );
    }

    #[test]
    fn a_field_without_rules_adds_none() {
        let form = DescribedForm::new("x")
            .field(FieldSpec::new("a", "A", Widget::Input(InputKind::Text)))
            .field(FieldSpec::new("b", "B", Widget::Input(InputKind::Text)).required());
        assert_eq!(form.fields().len(), 2);
        assert_eq!(form.rules.len(), 1);
        assert_eq!(form.rules[0].name, "b");
    }
}
