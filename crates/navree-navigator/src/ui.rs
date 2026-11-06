//! Navigator's view of the Navree UI: the component catalog behind
//! `list_components`, and the outline of a page behind `render_page`.
//!
//! The catalog is built into Navigator (see `build.rs`), so it is the catalog
//! of the Navree version Navigator ships with. A page is drawn by the app
//! itself: Navigator runs `<app> ui:render <path> --json`, which prints the
//! answer of the app's router to a guest's `GET` as one JSON object, and
//! [`outline`] reads the HTML in it. The outline is plain data: what a person
//! would see and can do on the page, without a browser.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::docs::EMBEDDED_VERSION;

/// The command of the app that draws one page: `ui:render <path> --json`.
pub const RENDER_COMMAND: &str = "ui:render";

/// What `render_page` says when the app doesn't have [`RENDER_COMMAND`].
pub const NO_RENDER_COMMAND: &str = "The app has no `ui:render` command, so Navigator cannot draw a page. \
    The command comes with Navree's UI module (`UiModule`), in a Navree version that has it: add the module to \
    the app, or update Navree, then ask again. `list_components` still lists what the toolkit can draw.";

/// What `list_components` says when this build of Navigator has no catalog.
pub const NO_CATALOG: &str = "This build of Navigator has no component catalog. The components are in the docs: \
    use search_docs for `components`, then read_doc.";

/// A group of components (`inputs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// The group's id, the `group` argument of `list_components`.
    pub id: String,
    /// Its title (`Date and time`).
    pub title: String,
}

/// One component of the catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    /// The id (`date-picker`).
    pub id: String,
    /// The name (`Date picker`).
    pub name: String,
    /// The id of its group.
    pub group: String,
    /// A one-line summary.
    pub summary: String,
    /// The docs page, as the slug `read_doc` takes (`components/button`);
    /// empty for a guide that has no docs page.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub docs: String,
    /// The Rust types that make it (`Button`, `ButtonVariant`).
    #[serde(default)]
    pub types: Vec<String>,
}

/// The components of one Navree version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Catalog {
    /// The groups, in the order the docs show them.
    #[serde(default)]
    pub groups: Vec<Group>,
    /// The components, in the order the docs show them.
    #[serde(default)]
    pub components: Vec<Component>,
}

impl Catalog {
    /// The catalog built into Navigator (empty when the build found none).
    pub fn embedded() -> &'static Catalog {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| Catalog::parse(crate::embedded::UI_CATALOG))
    }

    /// A catalog from its compact JSON; an empty one when `json` isn't.
    pub fn parse(json: &str) -> Catalog {
        serde_json::from_str(json).unwrap_or_default()
    }

    /// Whether there are no components.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    /// The component a `data-slot` belongs to: the one whose id is the slot
    /// (`card`) or its start (`card-title`), the longest id first.
    pub fn component_of_slot(&self, slot: &str) -> Option<&Component> {
        self.components
            .iter()
            .filter(|component| {
                slot == component.id
                    || slot
                        .strip_prefix(component.id.as_str())
                        .is_some_and(|rest| rest.starts_with('-'))
            })
            .max_by_key(|component| component.id.len())
    }

    /// The components of `group` (every group when `None`) that match the
    /// words of `query`, best first: an id or name that is the query, then
    /// one that contains it, then the rest in catalog order. A component
    /// matches when each word is in its id, name, group, summary or types.
    pub fn search(&self, query: &str, group: Option<&str>) -> Vec<&Component> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let squashed = squash(query);
        let mut found: Vec<(u8, &Component)> = Vec::new();
        for component in &self.components {
            if group.is_some_and(|group| !component.group.eq_ignore_ascii_case(group)) {
                continue;
            }
            if words.is_empty() {
                found.push((2, component));
                continue;
            }
            let title = self
                .groups
                .iter()
                .find(|group| group.id == component.group)
                .map_or("", |group| group.title.as_str());
            let haystack = format!(
                "{} {} {} {} {} {}",
                component.id,
                component.name,
                component.group,
                title,
                component.summary,
                component.types.join(" ")
            )
            .to_lowercase();
            let matches = words.iter().all(|word| haystack.contains(word.as_str()))
                || (!squashed.is_empty() && squash(&haystack).contains(&squashed));
            if !matches {
                continue;
            }
            let (id, name) = (squash(&component.id), squash(&component.name));
            let score = if !squashed.is_empty() && (id == squashed || name == squashed) {
                0
            } else if !squashed.is_empty() && (id.contains(&squashed) || name.contains(&squashed)) {
                1
            } else {
                2
            };
            found.push((score, component));
        }
        found.sort_by_key(|(score, _)| *score);
        found.into_iter().map(|(_, component)| component).collect()
    }
}

/// `text` in lower case with only letters and digits (`Date picker` and
/// `date-picker` are both `datepicker`).
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The answer of `list_components`: the matching components, with the
/// version of the catalog. `app_version` is the Navree version of the app,
/// when it is known; a different one is said in a `note`.
pub fn components_answer(
    catalog: &Catalog,
    query: Option<&str>,
    group: Option<&str>,
    app_version: Option<&str>,
) -> Result<Value, String> {
    if catalog.is_empty() {
        return Err(NO_CATALOG.to_owned());
    }
    let query = query.map(str::trim).filter(|query| !query.is_empty());
    let group = group.map(str::trim).filter(|group| !group.is_empty());
    if let Some(group) = group
        && !catalog
            .groups
            .iter()
            .any(|known| known.id.eq_ignore_ascii_case(group))
    {
        return Err(format!(
            "no component group named `{group}`; the groups are {}",
            catalog
                .groups
                .iter()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let found = catalog.search(query.unwrap_or_default(), group);
    let mut answer = json!({
        "version": EMBEDDED_VERSION,
        "total": catalog.components.len(),
        "groups": catalog.groups,
        "count": found.len(),
        "components": found,
    });
    if let Some(app) = app_version
        && app != EMBEDDED_VERSION
    {
        answer["app_version"] = json!(app);
        answer["note"] = json!(format!(
            "These are the components of Navree {EMBEDDED_VERSION}, which Navigator ships with; the app uses \
             Navree {app}. Check a component's API against the code when the versions differ."
        ));
    }
    if found.is_empty() {
        answer["hint"] = json!(
            "No component matches. Call list_components without a query to see all of them, or search_docs for the word."
        );
    }
    Ok(answer)
}

/// The path to draw, checked: one path of the app (`/deals?page=2`), never
/// a URL, so the app is asked for its own pages only.
pub fn check_path(path: &str) -> Result<String, String> {
    let path = path.trim().split('#').next().unwrap_or_default();
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(format!(
            "`path` must be a path of the app that starts with one `/` (`/deals`), not `{path}`"
        ));
    }
    if path.len() > 2048 || path.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(
            "`path` must be at most 2048 characters, with no spaces or control characters".to_owned(),
        );
    }
    Ok(path.to_owned())
}

/// Whether `message` is the app saying it has no [`RENDER_COMMAND`].
pub fn lacks_render_command(message: &str) -> bool {
    message.contains("unknown command") && message.contains(RENDER_COMMAND)
}

/// Whether `printed` has the shape `ui:render --json` prints: the path, the
/// status and the HTML (null when the answer isn't a page).
pub fn is_rendered(printed: &Value) -> bool {
    printed["path"].is_string() && printed["status"].is_u64() && printed.get("html").is_some()
}

/// The answer of `render_page` from what the app printed for `path`:
/// `{path, status, redirect?, note?}` and, when the answer is a page, its
/// [`outline`].
pub fn render_answer(path: &str, printed: &Value, catalog: &Catalog) -> Value {
    let status = printed["status"].as_u64().unwrap_or_default();
    let mut answer = json!({ "path": path, "status": status });
    let location = printed["location"].as_str().filter(|l| !l.is_empty());
    if let Some(location) = location {
        answer["redirect"] = json!(location);
    }
    match printed["html"].as_str() {
        Some(html) => {
            if let (Value::Object(drawn), Some(object)) =
                (outline(html, catalog), answer.as_object_mut())
            {
                object.extend(drawn);
            }
        }
        None => {
            let note = match location {
                Some(location) => format!(
                    "The route redirects to {location}; ask for that path to see the page it leads to."
                ),
                None => format!(
                    "The route answered {status} with {}, which isn't an HTML page, so there is nothing to outline.",
                    printed["content_type"].as_str().unwrap_or("no content type")
                ),
            };
            answer["note"] = json!(note);
        }
    }
    answer
}

/// The most of each list an outline holds; `truncated` says when one was cut.
const MAX_HEADINGS: usize = 100;
const MAX_LINKS: usize = 200;
const MAX_BUTTONS: usize = 100;
const MAX_FORMS: usize = 30;
const MAX_FIELDS: usize = 300;
const MAX_OPTIONS: usize = 30;
/// The longest text kept of a heading, link, button or label.
const MAX_TEXT: usize = 200;

/// What a person sees and can do on the page `html`: `{title, headings,
/// links, buttons, forms, fields, components, other_slots, truncated}`.
///
/// - `headings`: `{level, text}` in page order.
/// - `links`: `{text, href}`, without repeats; fragment-only links left out.
/// - `buttons`: the text of the buttons outside any form.
/// - `forms`: `{action, method, id, fields, buttons}`; a field is `{type,
///   name, label, required, placeholder, value, options}` with what the
///   control has. Hidden inputs are left out, and no value is read.
/// - `fields`: controls that are in no form (only when there are any).
/// - `components`: the catalog components the page uses, by the `data-slot`
///   its elements carry: `{id, name, docs, count}`, where `count` is the
///   number of elements that are the component itself, and `docs` is the
///   slug for `read_doc`.
/// - `other_slots`: the `data-slot` names that are no catalog component
///   (layout and parts, `row`, `icon`).
pub fn outline(html: &str, catalog: &Catalog) -> Value {
    let tokenizer = Tokenizer::new(Sink::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.state.into_inner().into_json(catalog)
}

/// One control of the page.
struct Field {
    kind: String,
    name: Option<String>,
    id: Option<String>,
    label: Option<String>,
    aria_label: Option<String>,
    required: bool,
    placeholder: Option<String>,
    value: Option<String>,
    options: Vec<(String, String)>,
    form: Option<usize>,
}

/// One `<form>`.
struct Form {
    action: Option<String>,
    method: String,
    id: Option<String>,
    buttons: Vec<String>,
}

/// An `<a href>` being read.
struct OpenLink {
    href: String,
    fallback: Option<String>,
    text: String,
}

/// A `<button>` being read.
struct OpenButton {
    fallback: Option<String>,
    text: String,
    form: Option<usize>,
}

/// A `<label>` being read, and the controls inside it.
struct OpenLabel {
    target: Option<String>,
    text: String,
    fields: Vec<usize>,
}

/// Everything the tokens say, so far.
#[derive(Default)]
struct State {
    title: Option<String>,
    in_title: bool,
    /// Depth inside `<svg>`, whose own `<title>` is not the page's.
    svg: u32,
    /// The raw-text element being skipped (`script`, `style`, `textarea`...).
    skipping: Option<String>,
    headings: Vec<(u8, String)>,
    links: Vec<(String, String)>,
    buttons: Vec<String>,
    forms: Vec<Form>,
    fields: Vec<Field>,
    slots: Vec<(String, usize)>,
    /// Label text by the id of the control it is `for`.
    labels: HashMap<String, String>,
    truncated: bool,
    heading: Option<(u8, String)>,
    link: Option<OpenLink>,
    button: Option<OpenButton>,
    label: Option<OpenLabel>,
    /// Inside a `<form>`, and its place in `forms` (none past the cap).
    form_open: bool,
    form: Option<usize>,
    /// The `<select>` being read, and the `<option>` in it.
    select: Option<usize>,
    option: Option<(Option<String>, String)>,
}

/// The value of attribute `name`.
fn attr<'a>(tag: &'a Tag, name: &str) -> Option<&'a str> {
    tag.attrs
        .iter()
        .find(|attribute| &*attribute.name.local == name)
        .map(|attribute| &*attribute.value)
}

/// Whether the tag has attribute `name`, whatever its value.
fn has(tag: &Tag, name: &str) -> bool {
    attr(tag, name).is_some()
}

fn owned(value: &str) -> String {
    value.to_owned()
}

/// `text` on one line, trimmed and cut to [`MAX_TEXT`] characters.
fn clean(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > MAX_TEXT {
        let mut cut: String = line.chars().take(MAX_TEXT).collect();
        cut.push_str("...");
        cut
    } else {
        line
    }
}

impl State {
    fn tag(&mut self, tag: &Tag) -> TokenSinkResult<()> {
        let name: &str = &tag.name;
        let start = tag.kind == TagKind::StartTag;
        if let Some(skipped) = &self.skipping {
            if !start && skipped == name {
                self.skipping = None;
            }
            return TokenSinkResult::Continue;
        }
        if start {
            self.count_slot(tag);
        }
        match (name, start) {
            ("title", true) if self.svg == 0 => {
                self.in_title = true;
                return TokenSinkResult::RawData(RawKind::Rcdata);
            }
            ("title", false) if self.svg == 0 => self.in_title = false,
            ("script", true) => {
                self.skipping = Some(owned(name));
                return TokenSinkResult::RawData(RawKind::ScriptData);
            }
            ("style" | "noscript", true) => {
                self.skipping = Some(owned(name));
                return TokenSinkResult::RawData(RawKind::Rawtext);
            }
            ("textarea", true) => {
                self.add_field(tag, "textarea");
                self.skipping = Some(owned(name));
                return TokenSinkResult::RawData(RawKind::Rcdata);
            }
            ("svg", true) if !tag.self_closing => self.svg += 1,
            ("svg", false) => self.svg = self.svg.saturating_sub(1),
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => {
                let level = name.as_bytes()[1] - b'0';
                self.heading = Some((level, String::new()));
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => self.end_heading(),
            ("a", true) => self.start_link(tag),
            ("a", false) => self.end_link(),
            ("button", true) => {
                self.button = Some(OpenButton {
                    fallback: attr(tag, "aria-label").map(owned),
                    text: String::new(),
                    form: self.form,
                });
            }
            ("button", false) => self.end_button(),
            ("form", true) => self.start_form(tag),
            ("form", false) => {
                self.form_open = false;
                self.form = None;
            }
            ("label", true) => {
                self.label = Some(OpenLabel {
                    target: attr(tag, "for").map(owned),
                    text: String::new(),
                    fields: Vec::new(),
                });
            }
            ("label", false) => self.end_label(),
            ("input", true) => self.input(tag),
            ("select", true) => self.select = self.add_field(tag, "select"),
            ("select", false) => {
                self.end_option();
                self.select = None;
            }
            ("option", true) => {
                self.end_option();
                if self.select.is_some() {
                    self.option = Some((attr(tag, "value").map(owned), String::new()));
                }
            }
            ("option", false) => self.end_option(),
            _ => {}
        }
        TokenSinkResult::Continue
    }

    fn text(&mut self, text: &str) {
        // Nothing inside an `<svg>` is text a person reads.
        if self.skipping.is_some() || self.svg > 0 {
            return;
        }
        if self.in_title {
            self.title.get_or_insert_with(String::new).push_str(text);
        } else if let Some((_, option)) = &mut self.option {
            option.push_str(text);
        } else {
            if let Some((_, heading)) = &mut self.heading {
                heading.push_str(text);
            }
            if let Some(link) = &mut self.link {
                link.text.push_str(text);
            }
            if let Some(button) = &mut self.button {
                button.text.push_str(text);
            }
            if let Some(label) = &mut self.label {
                label.text.push_str(text);
            }
        }
    }

    fn count_slot(&mut self, tag: &Tag) {
        let Some(slot) = attr(tag, "data-slot") else {
            return;
        };
        match self.slots.iter_mut().find(|(known, _)| known == slot) {
            Some((_, count)) => *count += 1,
            None => self.slots.push((owned(slot), 1)),
        }
    }

    fn end_heading(&mut self) {
        let Some((level, text)) = self.heading.take() else {
            return;
        };
        let text = clean(&text);
        if text.is_empty() {
            return;
        }
        if self.headings.len() >= MAX_HEADINGS {
            self.truncated = true;
        } else {
            self.headings.push((level, text));
        }
    }

    /// An `<a>` without `href` is not a link.
    fn start_link(&mut self, tag: &Tag) {
        self.link = attr(tag, "href").map(|href| OpenLink {
            href: href.trim().to_owned(),
            fallback: attr(tag, "aria-label")
                .or_else(|| attr(tag, "title"))
                .map(owned),
            text: String::new(),
        });
    }

    fn end_link(&mut self) {
        let Some(link) = self.link.take() else {
            return;
        };
        let lower = link.href.to_ascii_lowercase();
        if link.href.is_empty() || link.href.starts_with('#') || lower.starts_with("javascript:") {
            return;
        }
        let mut text = clean(&link.text);
        if text.is_empty() {
            text = link.fallback.as_deref().map(clean).unwrap_or_default();
        }
        let entry = (text, link.href);
        if self.links.contains(&entry) {
            return;
        }
        if self.links.len() >= MAX_LINKS {
            self.truncated = true;
        } else {
            self.links.push(entry);
        }
    }

    fn end_button(&mut self) {
        let Some(button) = self.button.take() else {
            return;
        };
        let mut text = clean(&button.text);
        if text.is_empty() {
            text = button.fallback.as_deref().map(clean).unwrap_or_default();
        }
        self.add_button(text, button.form);
    }

    /// Lists a button in its form, or on the page. A form past the cap, and
    /// a button with no text, are left out.
    fn add_button(&mut self, text: String, form: Option<usize>) {
        if text.is_empty() || (self.form_open && form.is_none()) {
            return;
        }
        let list = match form {
            Some(index) => &mut self.forms[index].buttons,
            None => &mut self.buttons,
        };
        if list.contains(&text) {
            return;
        }
        if list.len() >= MAX_BUTTONS {
            self.truncated = true;
        } else {
            list.push(text);
        }
    }

    fn start_form(&mut self, tag: &Tag) {
        self.form_open = true;
        if self.forms.len() >= MAX_FORMS {
            self.truncated = true;
            self.form = None;
            return;
        }
        self.forms.push(Form {
            action: attr(tag, "action").map(owned),
            method: attr(tag, "method").map_or_else(|| "get".to_owned(), str::to_lowercase),
            id: attr(tag, "id").map(owned),
            buttons: Vec::new(),
        });
        self.form = Some(self.forms.len() - 1);
    }

    fn end_label(&mut self) {
        let Some(label) = self.label.take() else {
            return;
        };
        let text = clean(&label.text);
        if text.is_empty() {
            return;
        }
        for index in label.fields {
            self.fields[index].label.get_or_insert_with(|| text.clone());
        }
        if let Some(target) = label.target {
            self.labels.entry(target).or_insert(text);
        }
    }

    /// An `<input>`: a button for the kinds that are buttons, nothing for a
    /// hidden one, a field for the rest.
    fn input(&mut self, tag: &Tag) {
        let kind = attr(tag, "type").unwrap_or("text").to_ascii_lowercase();
        match kind.as_str() {
            "hidden" => {}
            "submit" | "button" | "reset" | "image" => {
                let text = attr(tag, "value")
                    .or_else(|| attr(tag, "alt"))
                    .or_else(|| attr(tag, "aria-label"))
                    .map(clean)
                    .unwrap_or_default();
                self.add_button(text, self.form);
            }
            _ => {
                self.add_field(tag, &kind);
            }
        }
    }

    /// A control, in the open form (or on the page); its place in `fields`.
    /// A control in a form past the cap, and any past [`MAX_FIELDS`], are
    /// left out.
    fn add_field(&mut self, tag: &Tag, kind: &str) -> Option<usize> {
        if self.form_open && self.form.is_none() {
            return None;
        }
        if self.fields.len() >= MAX_FIELDS {
            self.truncated = true;
            return None;
        }
        let has_value = matches!(kind, "radio" | "checkbox");
        self.fields.push(Field {
            kind: owned(kind),
            name: attr(tag, "name").map(owned),
            id: attr(tag, "id").map(owned),
            label: None,
            aria_label: attr(tag, "aria-label").map(clean),
            required: has(tag, "required"),
            placeholder: attr(tag, "placeholder").map(clean),
            value: attr(tag, "value").filter(|_| has_value).map(clean),
            options: Vec::new(),
            form: self.form,
        });
        let index = self.fields.len() - 1;
        if let Some(label) = &mut self.label {
            label.fields.push(index);
        }
        Some(index)
    }

    fn end_option(&mut self) {
        let Some((value, text)) = self.option.take() else {
            return;
        };
        let Some(select) = self.select else {
            return;
        };
        let text = clean(&text);
        let value = value.unwrap_or_else(|| text.clone());
        let options = &mut self.fields[select].options;
        if options.len() >= MAX_OPTIONS {
            self.truncated = true;
        } else {
            options.push((value, text));
        }
    }

    fn into_json(mut self, catalog: &Catalog) -> Value {
        let labels = std::mem::take(&mut self.labels);
        let fields = std::mem::take(&mut self.fields);
        let field_json = |field: &Field| -> Value {
            let label = field
                .label
                .clone()
                .or_else(|| field.id.as_ref().and_then(|id| labels.get(id).cloned()))
                .or_else(|| field.aria_label.clone());
            let mut object = Map::new();
            object.insert("type".to_owned(), json!(field.kind));
            if let Some(name) = &field.name {
                object.insert("name".to_owned(), json!(name));
            }
            if let Some(label) = label {
                object.insert("label".to_owned(), json!(label));
            }
            if field.required {
                object.insert("required".to_owned(), json!(true));
            }
            if let Some(placeholder) = &field.placeholder {
                object.insert("placeholder".to_owned(), json!(placeholder));
            }
            if let Some(value) = &field.value {
                object.insert("value".to_owned(), json!(value));
            }
            if !field.options.is_empty() {
                let options: Vec<Value> = field
                    .options
                    .iter()
                    .map(|(value, text)| json!({ "value": value, "text": text }))
                    .collect();
                object.insert("options".to_owned(), Value::Array(options));
            }
            Value::Object(object)
        };

        let forms: Vec<Value> = self
            .forms
            .iter()
            .enumerate()
            .map(|(index, form)| {
                let mut object = Map::new();
                if let Some(action) = &form.action {
                    object.insert("action".to_owned(), json!(action));
                }
                object.insert("method".to_owned(), json!(form.method));
                if let Some(id) = &form.id {
                    object.insert("id".to_owned(), json!(id));
                }
                let in_form: Vec<Value> = fields
                    .iter()
                    .filter(|field| field.form == Some(index))
                    .map(&field_json)
                    .collect();
                object.insert("fields".to_owned(), Value::Array(in_form));
                object.insert("buttons".to_owned(), json!(form.buttons));
                Value::Object(object)
            })
            .collect();
        let loose: Vec<Value> = fields
            .iter()
            .filter(|field| field.form.is_none())
            .map(&field_json)
            .collect();

        let mut used: Vec<(&Component, usize)> = Vec::new();
        let mut other: Vec<&str> = Vec::new();
        for (slot, count) in &self.slots {
            match catalog.component_of_slot(slot) {
                Some(component) => {
                    let own = if *slot == component.id { *count } else { 0 };
                    match used.iter_mut().find(|(known, _)| known.id == component.id) {
                        Some((_, total)) => *total += own,
                        None => used.push((component, own)),
                    }
                }
                None => other.push(slot),
            }
        }
        let components: Vec<Value> = used
            .iter()
            .map(|(component, count)| {
                json!({
                    "id": component.id,
                    "name": component.name,
                    "docs": component.docs,
                    "count": count,
                })
            })
            .collect();

        let title = self
            .title
            .as_deref()
            .map(clean)
            .filter(|title| !title.is_empty());
        let headings: Vec<Value> = self
            .headings
            .iter()
            .map(|(level, text)| json!({ "level": level, "text": text }))
            .collect();
        let links: Vec<Value> = self
            .links
            .iter()
            .map(|(text, href)| json!({ "text": text, "href": href }))
            .collect();
        let mut outline = json!({
            "title": title,
            "headings": headings,
            "links": links,
            "buttons": self.buttons,
            "forms": forms,
            "components": components,
            "other_slots": other,
            "truncated": self.truncated,
        });
        if !loose.is_empty() {
            outline["fields"] = Value::Array(loose);
        }
        outline
    }
}

/// The tokenizer's sink: the tokens, read into a [`State`].
#[derive(Default)]
struct Sink {
    state: RefCell<State>,
}

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut state = self.state.borrow_mut();
        match token {
            Token::TagToken(tag) => state.tag(&tag),
            Token::CharacterTokens(text) => {
                state.text(&text);
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::parse(
            &json!({
                "groups": [
                    {"id": "actions", "title": "Actions"},
                    {"id": "dates", "title": "Date and time"},
                    {"id": "display", "title": "Display"},
                ],
                "components": [
                    {"id": "button", "name": "Button", "group": "actions", "summary": "A button, or a link that looks like one.", "docs": "components/button", "types": ["Button", "ButtonVariant"]},
                    {"id": "date-picker", "name": "Date picker", "group": "dates", "summary": "Picks one day.", "docs": "components/date-picker", "types": ["DatePicker"]},
                    {"id": "date-range-picker", "name": "Date range picker", "group": "dates", "summary": "Picks a first and a last day.", "docs": "components/date-range-picker", "types": ["DateRangePicker"]},
                    {"id": "card", "name": "Card", "group": "display", "summary": "A titled box.", "docs": "components/card", "types": ["Card"]},
                ],
            })
            .to_string(),
        )
    }

    fn ids(found: Vec<&Component>) -> Vec<&str> {
        found.into_iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn a_catalog_that_is_not_json_is_empty() {
        assert!(Catalog::parse("").is_empty());
        assert!(Catalog::parse("{oops").is_empty());
        assert!(!catalog().is_empty());
    }

    #[test]
    fn components_are_found_by_words_in_any_field_and_ranked() {
        let catalog = catalog();
        assert_eq!(
            ids(catalog.search("", None)),
            ["button", "date-picker", "date-range-picker", "card"]
        );
        // The name is the query: first, then the ones that contain it.
        assert_eq!(
            ids(catalog.search("date picker", None)),
            ["date-picker", "date-range-picker"]
        );
        assert_eq!(ids(catalog.search("datepicker", None)), ["date-picker"]);
        assert_eq!(ids(catalog.search("ButtonVariant", None)), ["button"]);
        assert_eq!(ids(catalog.search("last day", None)), ["date-range-picker"]);
        assert_eq!(ids(catalog.search("date and time", None)).len(), 2);
        assert_eq!(ids(catalog.search("nothing like it", None)).len(), 0);
        assert_eq!(
            ids(catalog.search("", Some("DATES"))),
            ["date-picker", "date-range-picker"]
        );
        assert_eq!(ids(catalog.search("picker", Some("actions"))).len(), 0);
    }

    #[test]
    fn a_slot_belongs_to_the_longest_component_id_it_starts_with() {
        let catalog = catalog();
        let id = |slot: &str| catalog.component_of_slot(slot).map(|c| c.id.as_str());
        assert_eq!(id("card"), Some("card"));
        assert_eq!(id("card-title"), Some("card"));
        assert_eq!(id("date-range-picker-day"), Some("date-range-picker"));
        assert_eq!(id("date-picker-input"), Some("date-picker"));
        assert_eq!(id("cardigan"), None, "only a whole word before the dash");
        assert_eq!(id("row"), None);
    }

    #[test]
    fn the_components_answer_names_the_catalog_version() {
        let catalog = catalog();
        let answer = components_answer(&catalog, Some("card"), None, None).unwrap();
        assert_eq!(answer["version"], EMBEDDED_VERSION);
        assert_eq!(answer["total"], 4);
        assert_eq!(answer["count"], 1);
        assert_eq!(answer["components"][0]["docs"], "components/card");
        assert!(answer.get("note").is_none());

        let other = components_answer(&catalog, None, None, Some("0.0.9")).unwrap();
        assert_eq!(other["app_version"], "0.0.9");
        assert!(other["note"].as_str().unwrap().contains("0.0.9"));
        let same = components_answer(&catalog, None, None, Some(EMBEDDED_VERSION)).unwrap();
        assert!(same.get("note").is_none());

        let none = components_answer(&catalog, Some("zzz"), None, None).unwrap();
        assert_eq!(none["count"], 0);
        assert!(none["hint"].as_str().unwrap().contains("without a query"));

        let error = components_answer(&catalog, None, Some("nope"), None).unwrap_err();
        assert!(error.contains("actions, dates, display"), "{error}");
        assert_eq!(
            components_answer(&Catalog::default(), None, None, None).unwrap_err(),
            NO_CATALOG
        );
    }

    #[test]
    fn only_a_path_of_the_app_is_drawn() {
        assert_eq!(check_path(" /deals?page=2#top "), Ok("/deals?page=2".into()));
        assert_eq!(check_path("/"), Ok("/".into()));
        for bad in ["deals", "//evil.example/x", "http://evil.example/", "/a b", "/a\nb", ""] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
        assert!(check_path(&format!("/{}", "a".repeat(2100))).is_err());
    }

    const PAGE: &str = r##"<!doctype html><html lang="en"><head>
<meta charset="utf-8"><title>Deals &amp; pipeline</title>
<style>h1 { color: red }</style><script>var x = "<h1>not a heading</h1>";</script>
</head><body>
<a href="#main">Skip</a>
<nav data-slot="navigation-menu"><a href="/deals" data-slot="button">Deals</a><a href="/companies">Companies</a><a href="/deals">Deals</a></nav>
<main>
<h1 data-slot="typography">Deals <span>(12)</span></h1>
<div data-slot="card"><div data-slot="card-header"><h2 data-slot="card-title">Sutlej Supply renewal</h2></div>
<button data-slot="button" type="button">Export</button>
<button data-slot="button" type="button" aria-label="Close"><svg data-slot="icon"><title>x</title></svg></button></div>
<form data-slot="form" id="new-deal" method="POST" action="/deals">
<input type="hidden" name="_token" value="secret">
<div data-slot="field"><label for="new-deal-title">Deal name</label>
<input data-slot="input" type="text" name="title" id="new-deal-title" placeholder="Annual license" required></div>
<div data-slot="field"><label for="new-deal-stage">Stage</label>
<select name="stage" id="new-deal-stage"><option value="lead">Lead</option><option>Won (100%)</option></select></div>
<label><input type="checkbox" name="hot" value="true"><span>Hot deal</span></label>
<textarea name="notes" aria-label="Notes">default <b>text</b></textarea>
<a data-slot="button" href="/deals">Cancel</a>
<button data-slot="button" type="submit">Create deal</button>
<input type="submit" value="Create and add another">
</form>
<input type="search" name="q" aria-label="Search deals">
<a href="/export.csv"><svg aria-hidden="true"></svg></a>
<a href="javascript:void(0)">Nothing</a><a>No href</a>
</main></body></html>"##;

    #[test]
    fn an_outline_has_the_title_headings_links_and_buttons() {
        let outline = outline(PAGE, &catalog());
        assert_eq!(outline["title"], "Deals & pipeline");
        assert_eq!(
            outline["headings"],
            json!([
                {"level": 1, "text": "Deals (12)"},
                {"level": 2, "text": "Sutlej Supply renewal"},
            ]),
            "the script's <h1> is no heading"
        );
        assert_eq!(
            outline["links"],
            json!([
                {"text": "Deals", "href": "/deals"},
                {"text": "Companies", "href": "/companies"},
                {"text": "Cancel", "href": "/deals"},
                {"text": "", "href": "/export.csv"},
            ]),
            "no fragment, javascript: or href-less links, and no repeats"
        );
        assert_eq!(outline["buttons"], json!(["Export", "Close"]));
        assert_eq!(outline["truncated"], false);
    }

    #[test]
    fn a_form_lists_its_fields_with_their_labels() {
        let outline = outline(PAGE, &catalog());
        let forms = outline["forms"].as_array().unwrap();
        assert_eq!(forms.len(), 1);
        let form = &forms[0];
        assert_eq!(form["action"], "/deals");
        assert_eq!(form["method"], "post");
        assert_eq!(form["id"], "new-deal");
        assert_eq!(
            form["fields"],
            json!([
                {"type": "text", "name": "title", "label": "Deal name", "required": true, "placeholder": "Annual license"},
                {"type": "select", "name": "stage", "label": "Stage", "options": [
                    {"value": "lead", "text": "Lead"},
                    {"value": "Won (100%)", "text": "Won (100%)"},
                ]},
                {"type": "checkbox", "name": "hot", "label": "Hot deal", "value": "true"},
                {"type": "textarea", "name": "notes", "label": "Notes"},
            ]),
            "hidden inputs and default text are not read"
        );
        assert_eq!(
            form["buttons"],
            json!(["Create deal", "Create and add another"])
        );
        assert_eq!(
            outline["fields"],
            json!([{"type": "search", "name": "q", "label": "Search deals"}]),
            "a control outside any form is listed on its own"
        );
    }

    #[test]
    fn an_outline_names_the_catalog_components_the_page_uses() {
        let outline = outline(PAGE, &catalog());
        assert_eq!(
            outline["components"],
            json!([
                {"id": "button", "name": "Button", "docs": "components/button", "count": 5},
                {"id": "card", "name": "Card", "docs": "components/card", "count": 1},
            ]),
            "in the order they first appear; card-header and card-title are card's, but not in its count"
        );
        let other: Vec<&str> = outline["other_slots"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(other.contains(&"typography") && other.contains(&"icon"), "{other:?}");
        assert!(!other.contains(&"card"), "{other:?}");
    }

    #[test]
    fn long_lists_are_cut_and_say_so() {
        let mut html = String::from("<title>Many</title>");
        for n in 0..(MAX_LINKS + 5) {
            html.push_str(&format!("<a href=\"/p/{n}\">Page {n}</a>"));
        }
        html.push_str(&format!("<h1>{}</h1>", "word ".repeat(100)));
        let outline = outline(&html, &Catalog::default());
        assert_eq!(outline["links"].as_array().unwrap().len(), MAX_LINKS);
        assert_eq!(outline["truncated"], true);
        let heading = outline["headings"][0]["text"].as_str().unwrap();
        assert!(heading.ends_with("..."), "{heading}");
        assert_eq!(heading.chars().count(), MAX_TEXT + 3);
    }

    #[test]
    fn the_render_answer_covers_pages_redirects_and_other_content() {
        let catalog = catalog();
        let page = render_answer(
            "/deals",
            &json!({"path": "/deals", "status": 200, "content_type": "text/html", "location": null,
                    "html": "<title>Deals</title><h1>Deals</h1>"}),
            &catalog,
        );
        assert_eq!(page["path"], "/deals");
        assert_eq!(page["status"], 200);
        assert_eq!(page["title"], "Deals");
        assert!(page.get("redirect").is_none() && page.get("note").is_none());

        let redirect = render_answer(
            "/deals",
            &json!({"path": "/deals", "status": 302, "content_type": null, "location": "/sign-in", "html": null}),
            &catalog,
        );
        assert_eq!(redirect["redirect"], "/sign-in");
        assert!(redirect["note"].as_str().unwrap().contains("/sign-in"));
        assert!(redirect.get("title").is_none());

        let data = render_answer(
            "/api/deals",
            &json!({"path": "/api/deals", "status": 200, "content_type": "application/json", "location": null, "html": null}),
            &catalog,
        );
        assert!(
            data["note"].as_str().unwrap().contains("application/json"),
            "{data}"
        );
    }

    #[test]
    fn what_the_app_prints_is_told_from_a_log_line() {
        assert!(is_rendered(&json!({"path": "/", "status": 200, "html": null})));
        assert!(!is_rendered(&json!({"path": "/", "status": 200, "level": "ERROR"})));
        assert!(!is_rendered(&json!({"status": 200, "html": "<p>"})));
        assert!(lacks_render_command(
            "`crm ui:render / --json` failed:\n\nError: unknown command `ui:render`; run `help` to list the available commands"
        ));
        assert!(!lacks_render_command("unknown command `queue:work`"));
    }
}
