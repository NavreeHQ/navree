//! ToggleGroup: a row of buttons that are on or off together. Underneath they are
//! radios (one choice) or checkboxes (many), so the group posts with its form,
//! keeps its state and answers the keyboard with no script. The `menus` module
//! gives it one tab stop with the arrow keys, and lets the chosen item be unchosen.

use super::{Orientation, Size, icon};
use crate::cmd::{Commands, Trigger};
use crate::icons::Icon;
use crate::node::{Element, Node, Uses, el, into_node};

/// How a toggle group looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToggleVariant {
    /// No line round the items; the chosen one is filled.
    #[default]
    Default,
    /// A line round each item.
    Outline,
}

/// One item of a [`ToggleGroup`].
#[derive(Clone, Debug)]
#[must_use]
pub struct ToggleItem {
    value: String,
    label: Option<Node>,
    aria_label: Option<String>,
    start: Option<Icon>,
    disabled: bool,
}

impl ToggleItem {
    /// An item that posts `value` and shows `label`.
    pub fn new(value: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            value: value.into(),
            label: Some(label.into()),
            aria_label: None,
            start: None,
            disabled: false,
        }
    }

    /// An item with an icon and no text. It must say what it is, so the accessible
    /// label is required.
    pub fn icon_only(value: impl Into<String>, glyph: Icon, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: None,
            aria_label: Some(label.into()),
            start: Some(glyph),
            disabled: false,
        }
    }

    /// Adds an icon before the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// A group of toggles that post as one field.
///
/// ```
/// use navree_ui::{ToggleGroup, ToggleItem, render};
///
/// let group = ToggleGroup::single("view", "Show as")
///     .item(ToggleItem::new("list", "List"))
///     .item(ToggleItem::new("grid", "Grid"))
///     .value("grid");
/// let html = render(&group.into());
/// assert!(html.contains(r#"role="radiogroup""#));
/// assert!(html.contains(r#"value="grid" checked"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ToggleGroup {
    el: Element,
    name: String,
    multiple: bool,
    items: Vec<ToggleItem>,
    values: Vec<String>,
    variant: ToggleVariant,
    size: Size,
    orientation: Orientation,
    joined: bool,
    disabled: bool,
    deselectable: bool,
}

impl ToggleGroup {
    fn with(name: String, label: String, multiple: bool) -> Self {
        Self {
            el: el("div")
                .slot("toggle-group")
                .attr("role", if multiple { "group" } else { "radiogroup" })
                .attr("aria-label", label),
            name,
            multiple,
            items: Vec::new(),
            values: Vec::new(),
            variant: ToggleVariant::Default,
            size: Size::Md,
            orientation: Orientation::Horizontal,
            joined: false,
            disabled: false,
            deselectable: false,
        }
    }

    /// A group where one item is on at a time, posting as `name`. `label` names the
    /// group for assistive technology.
    pub fn single(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self::with(name.into(), label.into(), false)
    }

    /// A group where any number of items are on, each posting as `name`.
    pub fn multiple(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self::with(name.into(), label.into(), true)
    }

    /// Adds an item.
    pub fn item(mut self, item: ToggleItem) -> Self {
        self.items.push(item);
        self
    }

    /// Turns one item on (the first one named, for a single group).
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.values.push(value.into());
        self
    }

    /// Turns these items on.
    pub fn values<S: Into<String>>(mut self, values: impl IntoIterator<Item = S>) -> Self {
        self.values.extend(values.into_iter().map(Into::into));
        self
    }

    /// Sets the look.
    pub fn variant(mut self, variant: ToggleVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Stacks the items.
    pub fn vertical(mut self) -> Self {
        self.orientation = Orientation::Vertical;
        self
    }

    /// Joins the items into one block, with no gap between them.
    pub fn joined(mut self) -> Self {
        self.joined = true;
        self
    }

    /// Disables every item.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Lets a click on the chosen item of a single group turn it off. It needs the
    /// script: with none, a radio stays on once chosen.
    pub fn deselectable(mut self) -> Self {
        self.deselectable = true;
        self
    }

    /// Runs commands when the choice changes.
    pub fn on_change(mut self, commands: impl Into<Commands>) -> Self {
        self.el = self.el.on(Trigger::Change, commands);
        self
    }

    fn into_node(self) -> Node {
        let kind = if self.multiple { "checkbox" } else { "radio" };
        let chosen = |value: &str, seen: &mut bool| {
            if self.multiple {
                self.values.iter().any(|v| v == value)
            } else if !*seen && self.values.iter().any(|v| v == value) {
                *seen = true;
                true
            } else {
                false
            }
        };
        let mut seen = false;
        let items: Vec<Element> = self
            .items
            .iter()
            .map(|item| {
                let on = chosen(&item.value, &mut seen);
                let off = self.disabled || item.disabled;
                el("label")
                    .slot("toggle-group-item")
                    .flag("data-icon-only", item.label.is_none())
                    .children((
                        el("input")
                            .attr("type", kind)
                            .attr("name", self.name.clone())
                            .attr("value", item.value.clone())
                            .flag("checked", on)
                            .flag("disabled", off)
                            .attr_opt("aria-label", item.aria_label.clone()),
                        item.start.map(icon),
                        item.label.clone(),
                    ))
            })
            .collect();
        self.el
            .attr(
                "data-variant",
                match self.variant {
                    ToggleVariant::Default => "default",
                    ToggleVariant::Outline => "outline",
                },
            )
            .attr_opt("data-size", self.size.as_str())
            .attr_opt(
                "data-orientation",
                (self.orientation == Orientation::Vertical).then_some("vertical"),
            )
            .attr_opt(
                "aria-orientation",
                (self.orientation == Orientation::Vertical).then_some("vertical"),
            )
            .flag("data-joined", self.joined)
            .flag("data-ui-deselect", self.deselectable && !self.multiple)
            .flag("data-ui-toggle-group", true)
            .uses(Uses::MENUS)
            .child(items)
            .into()
    }
}

into_node!(ToggleGroup);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(group: ToggleGroup) -> String {
        render(&group.into())
    }

    fn view() -> ToggleGroup {
        ToggleGroup::single("view", "Show as")
            .item(ToggleItem::new("list", "List"))
            .item(ToggleItem::new("grid", "Grid"))
    }

    #[test]
    fn a_single_group_is_radios_in_a_radiogroup() {
        let out = html(view().value("grid"));
        assert!(
            out.contains(r#"role="radiogroup" aria-label="Show as""#),
            "{out}"
        );
        assert_eq!(out.matches(r#"type="radio""#).count(), 2, "{out}");
        assert!(out.contains(r#"name="view""#), "{out}");
        assert!(out.contains(r#"value="grid" checked"#), "{out}");
        assert!(!out.contains(r#"value="list" checked"#), "{out}");
    }

    #[test]
    fn only_one_radio_is_on_even_if_two_values_are_given() {
        let out = html(view().values(["list", "grid"]));
        assert_eq!(out.matches("checked").count(), 1, "{out}");
    }

    #[test]
    fn a_multiple_group_is_checkboxes_in_a_group() {
        let out = html(
            ToggleGroup::multiple("days", "Working days")
                .item(ToggleItem::new("mon", "Mon"))
                .item(ToggleItem::new("tue", "Tue"))
                .item(ToggleItem::new("wed", "Wed"))
                .values(["mon", "wed"]),
        );
        assert!(
            out.contains(r#"role="group" aria-label="Working days""#),
            "{out}"
        );
        assert_eq!(out.matches(r#"type="checkbox""#).count(), 3, "{out}");
        assert_eq!(out.matches("checked").count(), 2, "{out}");
    }

    #[test]
    fn items_can_be_icons_with_text_or_icons_alone() {
        let out = html(
            view()
                .item(ToggleItem::new("map", "Map").icon(crate::icons::STAR))
                .item(ToggleItem::icon_only("chart", crate::icons::STAR, "Chart")),
        );
        assert!(out.contains(r#"aria-label="Chart""#), "{out}");
        assert_eq!(out.matches("data-icon-only").count(), 1, "{out}");
        assert_eq!(out.matches("data-slot=\"icon\"").count(), 2, "{out}");
    }

    #[test]
    fn disabling_the_group_or_an_item() {
        let out = html(view().item(ToggleItem::new("x", "X").disabled(true)));
        assert_eq!(out.matches("disabled").count(), 1, "{out}");
        let all = html(view().disabled(true));
        assert_eq!(all.matches("disabled").count(), 2, "{all}");
    }

    #[test]
    fn looks_sizes_and_direction_are_attributes() {
        let out = html(
            view()
                .variant(ToggleVariant::Outline)
                .size(Size::Sm)
                .vertical()
                .joined()
                .deselectable(),
        );
        assert!(out.contains(r#"data-variant="outline""#), "{out}");
        assert!(out.contains(r#"data-size="sm""#), "{out}");
        assert!(out.contains(r#"aria-orientation="vertical""#), "{out}");
        assert!(out.contains("data-joined"), "{out}");
        assert!(out.contains("data-ui-deselect"), "{out}");
        // Many items can be on at once, so there is nothing to turn off.
        let many = html(ToggleGroup::multiple("a", "A").deselectable());
        assert!(!many.contains("data-ui-deselect"), "{many}");
    }

    #[test]
    fn labels_and_values_are_text() {
        let out =
            html(ToggleGroup::single("v", "<b>").item(ToggleItem::new("\"><script>", "<i>x</i>")));
        assert!(!out.contains("<script>"), "{out}");
        assert!(!out.contains("<i>"), "{out}");
    }

    #[test]
    fn the_module_loads_and_a_change_command_asks_for_core() {
        let node: Node = view().into();
        assert!(crate::render::measure(&node).uses.has(Uses::MENUS));
        let node: Node = view().on_change(crate::Cmd::submit("filters")).into();
        assert!(crate::render::measure(&node).uses.has(Uses::CORE));
    }
}
