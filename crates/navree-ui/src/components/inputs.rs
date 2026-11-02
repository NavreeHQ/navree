//! Input, Textarea, NativeSelect, Checkbox, Switch, RadioGroup, Label, Field,
//! FieldSet, InputGroup.

use super::root_methods;
use crate::node::{Attr, AttrValue, Element, IntoNodes, Node, el, into_node};

/// The kind of an [`Input`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputKind {
    /// Text.
    #[default]
    Text,
    /// An email address.
    Email,
    /// A password.
    Password,
    /// A search.
    Search,
    /// A number.
    Number,
    /// A URL.
    Url,
    /// A phone number.
    Tel,
    /// A date.
    Date,
    /// A time.
    Time,
    /// A date and time.
    DateTime,
    /// A file.
    File,
    /// Not shown.
    Hidden,
}

impl InputKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Email => "email",
            Self::Password => "password",
            Self::Search => "search",
            Self::Number => "number",
            Self::Url => "url",
            Self::Tel => "tel",
            Self::Date => "date",
            Self::Time => "time",
            Self::DateTime => "datetime-local",
            Self::File => "file",
            Self::Hidden => "hidden",
        }
    }
}

/// A single-line input.
///
/// ```
/// use navree_ui::{Input, render};
///
/// let html = render(&Input::email("email").required().into());
/// assert_eq!(html, r#"<input data-slot="input" type="email" name="email" required>"#);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Input {
    el: Element,
}

impl Input {
    /// An input of `kind` posting as `name`.
    pub fn new(kind: InputKind, name: impl Into<String>) -> Self {
        Self {
            el: el("input")
                .slot("input")
                .attr("type", kind.as_str())
                .attr("name", name.into()),
        }
    }

    /// A text input.
    pub fn text(name: impl Into<String>) -> Self {
        Self::new(InputKind::Text, name)
    }

    /// An email input.
    pub fn email(name: impl Into<String>) -> Self {
        Self::new(InputKind::Email, name)
    }

    /// A password input.
    pub fn password(name: impl Into<String>) -> Self {
        Self::new(InputKind::Password, name)
    }

    /// A search input.
    pub fn search(name: impl Into<String>) -> Self {
        Self::new(InputKind::Search, name)
    }

    /// A number input.
    pub fn number(name: impl Into<String>) -> Self {
        Self::new(InputKind::Number, name)
    }

    /// A date input.
    pub fn date(name: impl Into<String>) -> Self {
        Self::new(InputKind::Date, name)
    }

    /// Sets the value. A password input takes none: a page never holds a password.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        if self.el.get("type") != Some(&AttrValue::Static("password")) {
            self.el = self.el.attr("value", value.into());
        }
        self
    }

    /// Sets the hint shown while empty.
    pub fn placeholder(mut self, placeholder: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr("placeholder", placeholder);
        self
    }

    /// Requires a value.
    pub fn required(mut self) -> Self {
        self.el = self.el.flag("required", true);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    /// Makes it read-only.
    pub fn readonly(mut self) -> Self {
        self.el = self.el.flag("readonly", true);
        self
    }

    /// Focuses it when the page loads.
    pub fn autofocus(mut self) -> Self {
        self.el = self.el.flag("autofocus", true);
        self
    }

    /// Sets `autocomplete`.
    pub fn autocomplete(mut self, hint: &'static str) -> Self {
        self.el = self.el.attr("autocomplete", hint);
        self
    }

    /// Sets `inputmode`.
    pub fn inputmode(mut self, mode: &'static str) -> Self {
        self.el = self.el.attr("inputmode", mode);
        self
    }

    /// Marks it invalid.
    pub fn invalid(mut self, invalid: bool) -> Self {
        if invalid {
            self.el = self.el.attr("aria-invalid", "true");
        }
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A multi-line input. It grows with its text where the browser can.
#[derive(Clone, Debug)]
#[must_use]
pub struct Textarea {
    el: Element,
    value: Option<String>,
}

impl Textarea {
    /// A textarea posting as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            el: el("textarea").slot("textarea").attr("name", name.into()),
            value: None,
        }
    }

    /// Sets the value.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets the number of rows.
    pub fn rows(mut self, rows: u32) -> Self {
        self.el = self.el.attr("rows", rows);
        self
    }

    /// Sets the hint shown while empty.
    pub fn placeholder(mut self, placeholder: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr("placeholder", placeholder);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    /// Requires a value.
    pub fn required(mut self) -> Self {
        self.el = self.el.flag("required", true);
        self
    }

    fn into_node(self) -> Node {
        // The HTML parser drops a newline right after `<textarea>`: a value that
        // starts with one needs a second to keep it.
        let lead = self
            .value
            .as_deref()
            .is_some_and(|value| value.starts_with('\n'))
            .then_some("\n");
        self.el.children((lead, self.value)).into()
    }
}

/// One choice in a [`NativeSelect`] or a [`RadioGroup`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionItem {
    /// What is posted.
    pub value: String,
    /// What is shown.
    pub label: String,
}

impl OptionItem {
    /// A choice.
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

/// The browser's `<select>`.
#[derive(Clone, Debug)]
#[must_use]
pub struct NativeSelect {
    el: Element,
    options: Vec<OptionItem>,
    selected: Option<String>,
    prompt: Option<String>,
}

impl NativeSelect {
    /// A select posting as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            el: el("select").slot("native-select").attr("name", name.into()),
            options: Vec::new(),
            selected: None,
            prompt: None,
        }
    }

    /// Adds a choice.
    pub fn option(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.options.push(OptionItem::new(value, label));
        self
    }

    /// Adds choices.
    pub fn options(mut self, options: impl IntoIterator<Item = OptionItem>) -> Self {
        self.options.extend(options);
        self
    }

    /// Sets the chosen value.
    pub fn selected(mut self, value: impl Into<String>) -> Self {
        self.selected = Some(value.into());
        self
    }

    /// Adds an empty first choice that says what to do.
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    /// Requires a choice.
    pub fn required(mut self) -> Self {
        self.el = self.el.flag("required", true);
        self
    }

    fn into_node(self) -> Node {
        let selected = self.selected;
        let mut children = Vec::with_capacity(self.options.len() + 1);
        if let Some(prompt) = self.prompt {
            children.push(el("option").attr("value", "").child(prompt));
        }
        for option in self.options {
            let chosen = selected.as_deref() == Some(option.value.as_str());
            children.push(
                el("option")
                    .attr("value", option.value)
                    .flag("selected", chosen)
                    .child(option.label),
            );
        }
        self.el.child(children).into()
    }
}

/// A checkbox.
#[derive(Clone, Debug)]
#[must_use]
pub struct Checkbox {
    el: Element,
    label: Option<Node>,
}

impl Checkbox {
    /// A checkbox posting as `name`. Its value is `true`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            el: el("input")
                .slot("checkbox")
                .attr("type", "checkbox")
                .attr("name", name.into())
                .attr("value", "true"),
            label: None,
        }
    }

    /// Starts checked.
    pub fn checked(mut self, checked: bool) -> Self {
        self.el = self.el.flag("checked", checked);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    /// Requires it.
    pub fn required(mut self) -> Self {
        self.el = self.el.flag("required", true);
        self
    }

    /// Puts it in a label with this text.
    pub fn label(mut self, label: impl Into<Node>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn into_node(self) -> Node {
        match self.label {
            Some(label) => el("label")
                .slot("choice")
                .children((self.el, el("span").child(label)))
                .into(),
            None => self.el.into(),
        }
    }
}

/// An on and off switch: a checkbox with the `switch` role.
#[derive(Clone, Debug)]
#[must_use]
pub struct Switch {
    el: Element,
    label: Option<Node>,
}

impl Switch {
    /// A switch posting as `name`. Its value is `true`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            el: el("input")
                .slot("switch")
                .attr("type", "checkbox")
                .attr("role", "switch")
                .attr("name", name.into())
                .attr("value", "true"),
            label: None,
        }
    }

    /// Starts on.
    pub fn checked(mut self, checked: bool) -> Self {
        self.el = self.el.flag("checked", checked);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    /// Puts it in a label with this text.
    pub fn label(mut self, label: impl Into<Node>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn into_node(self) -> Node {
        match self.label {
            Some(label) => el("label")
                .slot("choice")
                .children((self.el, el("span").child(label)))
                .into(),
            None => self.el.into(),
        }
    }
}

/// One choice out of several, shown: a `fieldset` of native radios, whose
/// keyboard is the browser's.
#[derive(Clone, Debug)]
#[must_use]
pub struct RadioGroup {
    el: Element,
    name: String,
    legend: Node,
    options: Vec<OptionItem>,
    selected: Option<String>,
    required: bool,
}

impl RadioGroup {
    /// A group posting as `name`, described by `legend`.
    pub fn new(name: impl Into<String>, legend: impl Into<Node>) -> Self {
        Self {
            el: el("fieldset").slot("radio-group"),
            name: name.into(),
            legend: legend.into(),
            options: Vec::new(),
            selected: None,
            required: false,
        }
    }

    /// Adds a choice.
    pub fn option(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.options.push(OptionItem::new(value, label));
        self
    }

    /// Adds choices.
    pub fn options(mut self, options: impl IntoIterator<Item = OptionItem>) -> Self {
        self.options.extend(options);
        self
    }

    /// Sets the chosen value.
    pub fn selected(mut self, value: impl Into<String>) -> Self {
        self.selected = Some(value.into());
        self
    }

    /// Requires a choice.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Disables the group.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    fn into_node(self) -> Node {
        let (name, selected, required) = (self.name, self.selected, self.required);
        let options: Vec<Element> = self
            .options
            .into_iter()
            .map(|option| {
                let chosen = selected.as_deref() == Some(option.value.as_str());
                el("label").slot("choice").children((
                    el("input")
                        .slot("radio")
                        .attr("type", "radio")
                        .attr("name", name.clone())
                        .attr("value", option.value)
                        .flag("checked", chosen)
                        .flag("required", required),
                    el("span").child(option.label),
                ))
            })
            .collect();
        self.el
            .children((
                el("legend").slot("field-legend").child(self.legend),
                options,
            ))
            .into()
    }
}

/// A label for the control with the id `control`.
#[derive(Clone, Debug)]
#[must_use]
pub struct Label {
    el: Element,
}

impl Label {
    /// A label for the control `control`.
    pub fn new(control: impl Into<String>, text: impl Into<Node>) -> Self {
        Self {
            el: el("label")
                .slot("label")
                .attr("for", control.into())
                .child(text.into()),
        }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A control with its label, help and error, tied together with `for`,
/// `aria-describedby` and `aria-invalid`.
///
/// ```
/// use navree_ui::{Field, Input, render};
///
/// let html = render(&Field::new("deal-title", "Deal name").error("Give the deal a name.").control(Input::text("title")).into());
/// assert!(html.contains(r#"<label data-slot="label" for="deal-title">Deal name</label>"#));
/// assert!(html.contains(r#"id="deal-title" aria-invalid="true" aria-describedby="deal-title-error""#));
/// assert!(html.contains(r#"<p data-slot="field-error" id="deal-title-error">Give the deal a name.</p>"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Field {
    el: Element,
    id: String,
    label: Node,
    required: bool,
    help: Option<Node>,
    error: Option<String>,
    control: Node,
}

impl Field {
    /// A field whose control gets the id `id`.
    pub fn new(id: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("field"),
            id: id.into(),
            label: label.into(),
            required: false,
            help: None,
            error: None,
            control: Node::Empty,
        }
    }

    /// Sets the help below the control.
    pub fn help(mut self, help: impl Into<Node>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Sets the error message.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Sets the error message when there is one.
    pub fn error_opt(mut self, error: Option<String>) -> Self {
        self.error = error;
        self
    }

    /// Marks the field required in its label.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Sets the control: an input, or something holding one (an [`InputGroup`]).
    pub fn control(mut self, control: impl Into<Node>) -> Self {
        self.control = control.into();
        self
    }

    fn into_node(self) -> Node {
        let help_id = self.help.as_ref().map(|_| format!("{}-help", self.id));
        let error_id = self.error.as_ref().map(|_| format!("{}-error", self.id));
        let described = match (&help_id, &error_id) {
            (Some(help), Some(error)) => Some(format!("{help} {error}")),
            (Some(id), None) | (None, Some(id)) => Some(id.clone()),
            (None, None) => None,
        };
        let mut control = self.control;
        tie_control(&mut control, &self.id, self.error.is_some(), described);
        let label = el("label")
            .slot("label")
            .attr("for", self.id)
            .child(self.label);
        let label = if self.required {
            label.child(
                el("span")
                    .slot("field-required")
                    .attr("aria-hidden", "true")
                    .child(" *"),
            )
        } else {
            label
        };
        self.el
            .flag("data-invalid", self.error.is_some())
            .children((
                label,
                control,
                self.help.map(|help| {
                    el("p")
                        .slot("field-description")
                        .attr_opt("id", help_id)
                        .child(help)
                }),
                self.error.map(|error| {
                    el("p")
                        .slot("field-error")
                        .attr_opt("id", error_id)
                        .child(error)
                }),
            ))
            .into()
    }
}

/// Finds the input in `node` and gives it the id and ARIA ties of its field.
fn tie_control(node: &mut Node, id: &str, invalid: bool, described: Option<String>) -> bool {
    match node {
        Node::Element(element) => {
            if matches!(element.tag, "input" | "select" | "textarea") {
                element.attrs.retain(|attr| attr.name != "id");
                element.attrs.push(Attr {
                    name: "id",
                    value: AttrValue::Text(id.into()),
                });
                if invalid && element.get("aria-invalid").is_none() {
                    element.attrs.push(Attr {
                        name: "aria-invalid",
                        value: AttrValue::Static("true"),
                    });
                }
                if let Some(described) = described {
                    element.attrs.push(Attr {
                        name: "aria-describedby",
                        value: AttrValue::Text(described.into()),
                    });
                }
                return true;
            }
            for child in &mut element.children {
                if tie_control(child, id, invalid, described.clone()) {
                    return true;
                }
            }
            false
        }
        Node::Fragment(nodes) => {
            for child in nodes {
                if tie_control(child, id, invalid, described.clone()) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// Fields that belong together, under one legend.
#[derive(Clone, Debug)]
#[must_use]
pub struct FieldSet {
    el: Element,
}

impl FieldSet {
    /// A set described by `legend`.
    pub fn new(legend: impl Into<Node>) -> Self {
        Self {
            el: el("fieldset")
                .slot("field-set")
                .child(el("legend").slot("field-legend").child(legend.into())),
        }
    }

    /// Adds fields.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// An input with something fixed before or after it.
#[derive(Clone, Debug)]
#[must_use]
pub struct InputGroup {
    el: Element,
    start: Option<Node>,
    input: Node,
    end: Option<Node>,
}

impl InputGroup {
    /// A group around `input`.
    pub fn new(input: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("input-group"),
            start: None,
            input: input.into(),
            end: None,
        }
    }

    /// Adds something before the input.
    pub fn start(mut self, addon: impl Into<Node>) -> Self {
        self.start = Some(addon.into());
        self
    }

    /// Adds something after the input.
    pub fn end(mut self, addon: impl Into<Node>) -> Self {
        self.end = Some(addon.into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                self.start
                    .map(|addon| el("div").slot("input-group-addon").child(addon)),
                self.input,
                self.end
                    .map(|addon| el("div").slot("input-group-addon").child(addon)),
            ))
            .into()
    }
}

root_methods!(
    Input,
    Textarea,
    NativeSelect,
    Checkbox,
    Switch,
    RadioGroup,
    Label,
    Field,
    FieldSet,
    InputGroup
);
into_node!(
    Input,
    Textarea,
    NativeSelect,
    Checkbox,
    Switch,
    RadioGroup,
    Label,
    Field,
    FieldSet,
    InputGroup
);
