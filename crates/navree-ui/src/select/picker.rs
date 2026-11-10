//! What Select, Combobox and MultiSelect share: the configuration, the markup around
//! the control, the popup with its listbox, and the native `<select>` that carries the
//! value.
//!
//! The structure, which the stylesheet and the runtime read:
//!
//! ```text
//! div[data-slot=select|combobox|multiselect][data-ui-picker]   the root, with the id
//!   button[select-trigger] | div[combobox-control] | div[multiselect-control]   the control
//!   select[select-native]            the value; hidden when scripting is on
//!   div[select-popup][popover]       placed by the overlay module
//!     div[select-list][role=listbox] the options
//!     div[select-footer]             a note: no results, searching, "N of M"
//!   div[select-status][role=status]  what a screen reader is told
//! ```

use std::collections::HashSet;

use super::choice::{Choice, Entries};
use super::labels::SelectLabels;
use super::list::{native_rows, rows};
use crate::components::{PopupAlign, Side, Size, icon};
use crate::html::Href;
use crate::icons;
use crate::node::{Attr, Element, Node, Uses, el};

/// Which of the three this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Select,
    Combobox,
    Multi,
}

impl Kind {
    const fn slot(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Combobox => "combobox",
            Self::Multi => "multiselect",
        }
    }

    const fn picker(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Combobox => "combobox",
            Self::Multi => "multi",
        }
    }
}

/// How a typed value that isn't an option becomes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Create {
    Off,
    /// The typed text is the value.
    Free,
    /// The server makes the record and answers the option.
    Url(Href),
}

/// A picker's configuration.
#[derive(Clone, Debug)]
pub(crate) struct Picker {
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) id: Option<String>,
    pub(crate) entries: Entries,
    /// Values chosen, in order.
    pub(crate) chosen: Vec<String>,
    /// Choices that are chosen but not in `entries`: the one a search found.
    pub(crate) extra: Vec<Choice>,
    pub(crate) placeholder: Option<String>,
    pub(crate) size: Size,
    pub(crate) disabled: bool,
    pub(crate) readonly: bool,
    pub(crate) required: bool,
    pub(crate) invalid: bool,
    pub(crate) clearable: bool,
    pub(crate) aria_label: Option<String>,
    pub(crate) labels: SelectLabels,
    pub(crate) side: Side,
    pub(crate) align: PopupAlign,
    pub(crate) search: Option<Href>,
    pub(crate) min_chars: Option<u32>,
    pub(crate) debounce: Option<u32>,
    pub(crate) create: Create,
    pub(crate) create_denied: Option<String>,
    pub(crate) max: Option<u32>,
    pub(crate) summary_after: Option<u32>,
    pub(crate) pick_all: bool,
    pub(crate) extra_attrs: Vec<Attr>,
}

impl Picker {
    pub(crate) fn new(kind: Kind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            id: None,
            entries: Entries::default(),
            chosen: Vec::new(),
            extra: Vec::new(),
            placeholder: None,
            size: Size::Md,
            disabled: false,
            readonly: false,
            required: false,
            invalid: false,
            clearable: false,
            aria_label: None,
            labels: SelectLabels::default(),
            side: Side::Bottom,
            align: PopupAlign::Start,
            search: None,
            min_chars: None,
            debounce: None,
            create: Create::Off,
            create_denied: None,
            max: None,
            summary_after: None,
            pick_all: false,
            extra_attrs: Vec::new(),
        }
    }

    fn base(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| format!("{}-{}", self.name, self.kind.slot()))
    }

    /// The choice that posts `value`, from the list or from what a search found.
    fn lookup(&self, value: &str) -> Option<&Choice> {
        self.entries
            .find(value)
            .or_else(|| self.extra.iter().find(|choice| choice.value == value))
    }

    /// The entries of the native `<select>`.
    ///
    /// A fixed list carries all its choices, so a browser without script can pick any.
    /// A searched list carries only what is chosen: the runtime adds an `<option>` when
    /// the user picks one it found.
    fn carrier_entries(&self) -> Entries {
        let mut carrier = if self.search.is_some() {
            Entries::default()
        } else {
            self.entries.clone()
        };
        let mut seen: HashSet<String> = carrier.choices().map(|c| c.value.clone()).collect();
        for value in &self.chosen {
            if seen.contains(value) {
                continue;
            }
            if let Some(choice) = self.lookup(value) {
                seen.insert(value.clone());
                carrier.choice(choice.clone());
            }
        }
        carrier
    }

    fn side_name(&self) -> &'static str {
        match self.side {
            Side::Top => "top",
            Side::Left => "left",
            Side::Right => "right",
            _ => "bottom",
        }
    }

    fn align_name(&self) -> Option<&'static str> {
        match self.align {
            PopupAlign::Start => Some("start"),
            PopupAlign::End => Some("end"),
            _ => None,
        }
    }

    fn remove_label(&self, label: &str) -> String {
        self.labels.remove.replace("{label}", label)
    }

    fn root(&self, base: &str) -> Element {
        let mut root = el("div")
            .slot(self.kind.slot())
            .attr("id", base.to_owned())
            .attr("data-ui-picker", self.kind.picker())
            // A morph from the server leaves an open list as it is.
            .attr("data-ui-keep-attrs", "data-open")
            .attr_opt("data-size", self.size.as_str())
            .flag("data-invalid", self.invalid)
            .flag("data-disabled", self.disabled)
            .flag("data-readonly", self.readonly)
            .flag("data-ui-clearable", self.clearable)
            .attr_opt("data-l", self.labels.changed())
            .attr_opt("data-ui-search", self.search.clone())
            .attr_opt("data-ui-min", self.min_chars)
            .attr_opt("data-ui-debounce", self.debounce)
            .attr_opt("data-ui-max", self.max)
            .attr_opt("data-ui-summary", self.summary_after)
            .flag("data-ui-pick-all", self.pick_all)
            .uses(Uses::OVERLAY.with(Uses::SELECT));
        match &self.create {
            Create::Off => {}
            Create::Free => root = root.attr("data-ui-create", "free"),
            Create::Url(url) => root = root.attr("data-ui-create", url.clone()),
        }
        root = root.attr_opt("data-ui-create-denied", self.create_denied.clone());
        for attr in &self.extra_attrs {
            root.attrs.push(attr.clone());
        }
        root
    }

    /// The text the control shows for the chosen value of a single picker.
    fn shown(&self) -> Option<String> {
        let value = self.chosen.first()?;
        if value.is_empty() {
            return None;
        }
        Some(
            self.lookup(value)
                .map_or_else(|| value.clone(), |choice| choice.shown().to_owned()),
        )
    }

    fn trigger(&self, base: &str, popup_id: &str, list_id: &str) -> Element {
        let shown = self.shown();
        let placeholder = shown.is_none();
        let text = shown
            .or_else(|| self.placeholder.clone())
            .unwrap_or_default();
        let chosen = self.chosen.first().and_then(|value| self.lookup(value));
        el("button")
            .slot("select-trigger")
            .attr("type", "button")
            .attr("id", format!("{base}-trigger"))
            .attr("role", "combobox")
            .attr("aria-haspopup", "listbox")
            .attr("aria-expanded", "false")
            .attr("data-ui-keep-attrs", "aria-expanded aria-activedescendant")
            .attr("aria-controls", list_id.to_owned())
            .attr_opt("aria-required", self.required.then_some("true"))
            .attr_opt("aria-readonly", self.readonly.then_some("true"))
            .attr_opt("aria-invalid", self.invalid.then_some("true"))
            .attr_opt("aria-label", self.aria_label.clone())
            .flag("disabled", self.disabled)
            .flag("data-field-control", true)
            .attr("data-ui-anchor", popup_id.to_owned())
            .flag("data-placeholder", placeholder)
            .children((
                chosen
                    .and_then(Choice::lead_slot)
                    .map(Node::from)
                    .or_else(|| chosen.and_then(|choice| choice.icon).map(icon)),
                el("span")
                    .slot("select-value")
                    .attr("dir", "auto")
                    .child(text),
                icon(icons::CHEVRONS_UP_DOWN),
            ))
    }

    fn clear_button(&self, slot: &'static str, visible: bool) -> Element {
        el("button")
            .slot(slot)
            .attr("type", "button")
            .attr("tabindex", -1_i64)
            .attr("aria-label", self.labels.clear.to_string())
            .flag("hidden", !visible)
            .child(icon(icons::X))
    }

    fn input(&self, base: &str, list_id: &str, slot: &'static str) -> Element {
        let value = if self.kind == Kind::Combobox {
            self.shown()
        } else {
            None
        };
        el("input")
            .slot(slot)
            .attr("type", "text")
            .attr("id", format!("{base}-input"))
            .attr("role", "combobox")
            .attr("aria-autocomplete", "list")
            .attr("aria-haspopup", "listbox")
            .attr("aria-expanded", "false")
            .attr("data-ui-keep-attrs", "aria-expanded aria-activedescendant")
            .attr("aria-controls", list_id.to_owned())
            .attr("autocomplete", "off")
            .attr("autocapitalize", "none")
            .attr("spellcheck", "false")
            .attr("dir", "auto")
            .attr_opt("value", value)
            .attr_opt("placeholder", self.placeholder.clone())
            .attr_opt("aria-required", self.required.then_some("true"))
            .attr_opt("aria-invalid", self.invalid.then_some("true"))
            .attr_opt("aria-label", self.aria_label.clone())
            .flag("readonly", self.readonly)
            .flag("disabled", self.disabled)
            .flag("data-field-control", true)
    }

    fn toggle_button(&self) -> Element {
        el("button")
            .slot("combobox-toggle")
            .attr("type", "button")
            .attr("tabindex", -1_i64)
            .attr("aria-hidden", "true")
            .flag("disabled", self.disabled)
            .child(icon(icons::CHEVRON_DOWN))
    }

    fn chip(&self, value: &str, label: &str) -> Element {
        el("span")
            .slot("chip")
            .attr("role", "listitem")
            .attr("tabindex", -1_i64)
            .attr("data-value", value.to_owned())
            .children((
                el("span")
                    .slot("chip-text")
                    .attr("dir", "auto")
                    .child(label.to_owned()),
                el("button")
                    .slot("chip-remove")
                    .attr("type", "button")
                    .attr("tabindex", -1_i64)
                    .attr("aria-label", self.remove_label(label))
                    .flag("disabled", self.disabled)
                    .child(icon(icons::X)),
            ))
    }

    /// The control of a combobox or multi-select: a box holding the input.
    fn typed_control(&self, base: &str, popup_id: &str, list_id: &str) -> Element {
        let multi = self.kind == Kind::Multi;
        let has_value = !self.chosen.is_empty();
        let slot = if multi {
            "multiselect-control"
        } else {
            "combobox-control"
        };
        let input_slot = if multi {
            "multiselect-input"
        } else {
            "combobox-input"
        };
        let chips = multi.then(|| {
            let chips: Vec<Element> = self
                .chosen
                .iter()
                .map(|value| {
                    let label = self.lookup(value).map_or(value.as_str(), |c| c.shown());
                    self.chip(value, label)
                })
                .collect();
            el("div")
                .slot("chips")
                .attr("role", "list")
                .attr_opt("aria-label", self.aria_label.clone())
                // Inactive, so the dimmed text is exempt from the contrast rule.
                .attr_opt("aria-disabled", self.disabled.then_some("true"))
                .child(chips)
        });
        el("div")
            .slot(slot)
            .attr("data-ui-anchor", popup_id.to_owned())
            .children((
                chips,
                self.input(base, list_id, input_slot),
                self.clearable
                    .then(|| self.clear_button("combobox-clear", has_value)),
                self.toggle_button(),
            ))
    }

    fn carrier(&self) -> Element {
        let multi = self.kind == Kind::Multi;
        let empty = (!multi).then(|| self.placeholder.as_deref().unwrap_or(""));
        let entries = self.carrier_entries();
        el("select")
            .slot("select-native")
            .attr("name", self.name.clone())
            .flag("multiple", multi)
            .flag("required", self.required)
            .flag("disabled", self.disabled)
            .child(native_rows(&entries, &self.chosen, empty))
    }

    fn popup(&self, popup_id: &str, list_id: &str) -> Element {
        let chosen: HashSet<&str> = self.chosen.iter().map(String::as_str).collect();
        let multi = self.kind == Kind::Multi;
        el("div")
            .slot("select-popup")
            .attr("id", popup_id.to_owned())
            .attr("popover", "manual")
            .flag("data-ui-place", true)
            .attr("data-side", self.side_name())
            .attr_opt("data-align", self.align_name())
            .flag("data-ui-fit", true)
            .flag("data-ui-grow", self.search.is_some())
            .flag("data-ui-match-width", true)
            .children((
                el("div")
                    .slot("select-list")
                    .attr("id", list_id.to_owned())
                    .attr("role", "listbox")
                    .attr_opt("aria-multiselectable", multi.then_some("true"))
                    .attr_opt("aria-label", self.aria_label.clone())
                    .child(rows(
                        &self.entries,
                        &chosen,
                        multi,
                        self.kind != Kind::Select,
                    )),
                el("div").slot("select-footer").child(
                    el("p")
                        .slot("select-note")
                        .attr("aria-hidden", "true")
                        .flag("hidden", true),
                ),
            ))
    }

    pub(crate) fn into_node(self) -> Node {
        let base = self.base();
        let popup_id = format!("{base}-popup");
        let list_id = format!("{base}-list");
        let control: Element = match self.kind {
            Kind::Select => {
                // The trigger first, so a field finds it and not the carrier.
                let has_value = self.shown().is_some();
                el("div").slot("select-control").children((
                    self.trigger(&base, &popup_id, &list_id),
                    self.clearable
                        .then(|| self.clear_button("select-clear", has_value)),
                ))
            }
            _ => self.typed_control(&base, &popup_id, &list_id),
        };
        let template = (self.kind == Kind::Multi).then(|| {
            el("template")
                .slot("chip-template")
                .child(self.chip("", ""))
        });
        self.root(&base)
            .children((
                control,
                self.carrier(),
                self.popup(&popup_id, &list_id),
                template,
                el("div")
                    .slot("select-status")
                    .attr("role", "status")
                    .attr("aria-live", "polite")
                    .attr("aria-atomic", "true"),
            ))
            .into()
    }
}

/// Adds `id`, `attr`, `span`... to a picker type whose `picker` field is a [`Picker`].
macro_rules! picker_methods {
    ($type:ident) => {
        impl $type {
            /// Sets the root element's `id`. The popup and list ids follow from it.
            pub fn id(mut self, id: impl Into<String>) -> Self {
                self.picker.id = Some(id.into());
                self
            }

            /// Sets an attribute on the root element.
            pub fn attr(
                mut self,
                name: &'static str,
                value: impl Into<$crate::node::AttrValue>,
            ) -> Self {
                self.picker.extra_attrs.push($crate::node::Attr {
                    name,
                    value: value.into(),
                });
                self
            }

            /// Adds a choice that posts `value` and shows `label`.
            pub fn option(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
                self.picker
                    .entries
                    .choice($crate::select::Choice::new(value, label));
                self
            }

            /// Adds a choice.
            pub fn choice(mut self, choice: $crate::select::Choice) -> Self {
                self.picker.entries.choice(choice);
                self
            }

            /// Adds choices: `Choice`s, `(value, label)` pairs or `OptionItem`s.
            pub fn options<C: Into<$crate::select::Choice>>(
                mut self,
                options: impl IntoIterator<Item = C>,
            ) -> Self {
                for option in options {
                    self.picker.entries.choice(option.into());
                }
                self
            }

            /// Adds a labelled group of choices.
            pub fn group<C: Into<$crate::select::Choice>>(
                mut self,
                label: impl Into<String>,
                choices: impl IntoIterator<Item = C>,
            ) -> Self {
                self.picker
                    .entries
                    .group(label.into(), choices.into_iter().map(Into::into).collect());
                self
            }

            /// Adds a divider.
            pub fn separator(mut self) -> Self {
                self.picker.entries.separator();
                self
            }

            /// Sets the hint shown while nothing is chosen.
            pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
                self.picker.placeholder = Some(placeholder.into());
                self
            }

            /// Sets the size.
            pub fn size(mut self, size: $crate::components::Size) -> Self {
                self.picker.size = size;
                self
            }

            /// Disables it. A disabled control posts nothing.
            pub fn disabled(mut self, disabled: bool) -> Self {
                self.picker.disabled = disabled;
                self
            }

            /// Shows the value and refuses changes, but still posts it.
            pub fn readonly(mut self) -> Self {
                self.picker.readonly = true;
                self
            }

            /// Requires a choice.
            pub fn required(mut self) -> Self {
                self.picker.required = true;
                self
            }

            /// Requires a choice when `required` is true.
            pub fn required_if(mut self, required: bool) -> Self {
                self.picker.required = required;
                self
            }

            /// Marks it invalid.
            pub fn invalid(mut self, invalid: bool) -> Self {
                self.picker.invalid = invalid;
                self
            }

            /// Names the control for assistive technology, when no `Field` label does.
            pub fn aria_label(mut self, label: impl Into<String>) -> Self {
                self.picker.aria_label = Some(label.into());
                self
            }

            /// Sets the words, for another language.
            pub fn labels(mut self, labels: &$crate::select::SelectLabels) -> Self {
                self.picker.labels = labels.clone();
                self
            }

            /// Sets the side the popup opens on.
            pub fn side(mut self, side: $crate::components::Side) -> Self {
                self.picker.side = side;
                self
            }

            /// Sets how the popup lines up along that side.
            pub fn align(mut self, align: $crate::components::PopupAlign) -> Self {
                self.picker.align = align;
                self
            }

            fn into_node(self) -> $crate::node::Node {
                self.picker.into_node()
            }
        }
    };
}
pub(crate) use picker_methods;

/// Methods of the two typed pickers: a search endpoint and created options.
macro_rules! typed_methods {
    ($type:ident) => {
        impl $type {
            /// Searches on the server. The runtime asks `url` with `?q=text` and shows
            /// the answer, an [`ChoiceList`](crate::select::ChoiceList), as the options.
            /// Requests are debounced, a slower earlier answer is dropped, and a failed
            /// one shows a way to try again.
            pub fn search(mut self, url: impl Into<$crate::html::Href>) -> Self {
                self.picker.search = Some(url.into());
                self
            }

            /// Waits for this many characters before searching (0 by default: the
            /// list opens with the server's first options).
            pub fn min_chars(mut self, n: u32) -> Self {
                self.picker.min_chars = Some(n);
                self
            }

            /// Waits this many milliseconds after the last key before searching: 100 by
            /// default, 0 for no wait. Choosing an option never waits.
            pub fn debounce(mut self, milliseconds: u32) -> Self {
                self.picker.debounce = Some(milliseconds.min(5000));
                self
            }

            /// Offers to use the typed text as a new value when no option matches it.
            /// The text, trimmed, is posted as the value.
            pub fn creatable(mut self) -> Self {
                self.picker.create = $crate::select::picker::Create::Free;
                self
            }

            /// Offers to create the typed text on the server: the runtime posts
            /// `name=text` to `url` and the answer, an
            /// [`ChoiceList`](crate::select::ChoiceList) holding the new option, is chosen.
            /// The route must check that the caller may create, and what the text is, as
            /// any post: a browser can send to it whatever the row shows.
            pub fn create_url(mut self, url: impl Into<$crate::html::Href>) -> Self {
                self.picker.create = $crate::select::picker::Create::Url(url.into());
                self
            }

            /// Shows the create row switched off, with the reason, for someone who may
            /// not create. It is a hint to the person and stops nobody: the route behind
            /// [`create_url`](Self::create_url) must check permission itself.
            pub fn create_denied(mut self, reason: impl Into<String>) -> Self {
                self.picker.create_denied = Some(reason.into());
                self
            }

            /// Adds a clear button, and clears on `Esc` when the list is closed.
            pub fn clearable(mut self) -> Self {
                self.picker.clearable = true;
                self
            }
        }
    };
}
pub(crate) use typed_methods;
