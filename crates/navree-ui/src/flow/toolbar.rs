//! Toolbar: a row of tools with one tab stop, that moves what does not fit into a
//! menu.
//!
//! The server writes the tools in a labelled group. With no script they wrap onto
//! another line and every one works. The `toolbar` module makes it WAI-ARIA's
//! toolbar: one tab stop, the arrow keys, Home and End, and at a narrow width the
//! tools that do not fit go into a "More" menu, the shipped
//! [`DropdownMenu`]. Each tool that can move has a twin entry in that menu from the
//! start. A twin that stands for a toggle or a choice has no name and hands its
//! click to the real tool, which stays in the page, so a toggle that moved still
//! posts.

use std::borrow::Cow;

use crate::cmd::Commands;
use crate::components::{
    Button, ButtonVariant, DropdownMenu, MenuCheckbox, MenuItem, MenuRadios, Orientation, Size,
    icon,
};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Attr, AttrValue, Element, Node, Uses, el, into_node};
use crate::render::POST_FORM;

/// The words a toolbar writes. English by default.
#[derive(Clone, Debug)]
pub struct ToolbarLabels {
    /// The name of the button that opens the overflow menu.
    pub more: Cow<'static, str>,
}

impl Default for ToolbarLabels {
    fn default() -> Self {
        Self {
            more: "More".into(),
        }
    }
}

#[derive(Clone, Debug)]
enum Kind {
    Button {
        label: Node,
        glyph: Option<Icon>,
        icon_only: bool,
        aria: Option<String>,
        variant: ButtonVariant,
        commands: Option<Commands>,
        dialog: Option<String>,
        disabled: bool,
    },
    Link {
        label: Node,
        glyph: Option<Icon>,
        href: Href,
        disabled: bool,
        current: bool,
    },
    Post {
        label: Node,
        glyph: Option<Icon>,
        href: Href,
        disabled: bool,
    },
    Toggle {
        name: String,
        value: String,
        label: Node,
        glyph: Option<Icon>,
        icon_only: bool,
        pressed: bool,
        disabled: bool,
    },
    Radios {
        name: String,
        label: String,
        options: Vec<(String, Node)>,
        value: Option<String>,
        disabled: bool,
    },
    Menu {
        label: Node,
        menu: Box<DropdownMenu>,
    },
    Separator,
    Group {
        label: String,
        items: Vec<ToolbarItem>,
    },
}

/// One tool of a [`Toolbar`].
#[derive(Clone, Debug)]
#[must_use]
pub struct ToolbarItem {
    kind: Kind,
    keep: bool,
}

/// A group of tools: named for assistive technology, and moved into the overflow
/// menu together.
pub type ToolbarGroup = ToolbarItem;

impl ToolbarItem {
    fn of(kind: Kind) -> Self {
        Self { kind, keep: false }
    }

    /// A button with a label.
    pub fn button(label: impl Into<Node>) -> Self {
        Self::of(Kind::Button {
            label: label.into(),
            glyph: None,
            icon_only: false,
            aria: None,
            variant: ButtonVariant::Ghost,
            commands: None,
            dialog: None,
            disabled: false,
        })
    }

    /// A button that is only an icon. It must say what it is, so the accessible label
    /// is required.
    pub fn icon_button(glyph: Icon, label: impl Into<String>) -> Self {
        let label = label.into();
        Self::of(Kind::Button {
            label: Node::from(label.clone()),
            glyph: Some(glyph),
            icon_only: true,
            aria: Some(label),
            variant: ButtonVariant::Ghost,
            commands: None,
            dialog: None,
            disabled: false,
        })
    }

    /// A link to `href`.
    pub fn link(label: impl Into<Node>, href: impl Into<Href>) -> Self {
        Self::of(Kind::Link {
            label: label.into(),
            glyph: None,
            href: href.into(),
            disabled: false,
            current: false,
        })
    }

    /// A button that posts to `href` through the page's one post form, which carries
    /// the session's token. It works with no script.
    pub fn post(label: impl Into<Node>, href: impl Into<Href>) -> Self {
        Self::of(Kind::Post {
            label: label.into(),
            glyph: None,
            href: href.into(),
            disabled: false,
        })
    }

    /// A button that is on or off, posting `name` when it is on.
    pub fn toggle(name: impl Into<String>, label: impl Into<Node>) -> Self {
        Self::of(Kind::Toggle {
            name: name.into(),
            value: "on".to_owned(),
            label: label.into(),
            glyph: None,
            icon_only: false,
            pressed: false,
            disabled: false,
        })
    }

    /// A toggle that is only an icon, with the accessible label it needs.
    pub fn icon_toggle(name: impl Into<String>, glyph: Icon, label: impl Into<String>) -> Self {
        Self::of(Kind::Toggle {
            name: name.into(),
            value: "on".to_owned(),
            label: Node::from(label.into()),
            glyph: Some(glyph),
            icon_only: true,
            pressed: false,
            disabled: false,
        })
    }

    /// A set of buttons of which one is on, posting its value as `name`. `label` names
    /// the set. Add the choices with [`option`](Self::option).
    pub fn radios(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self::of(Kind::Radios {
            name: name.into(),
            label: label.into(),
            options: Vec::new(),
            value: None,
            disabled: false,
        })
    }

    /// Adds a choice to a set made with [`radios`](Self::radios).
    pub fn option(mut self, value: impl Into<String>, label: impl Into<Node>) -> Self {
        if let Kind::Radios { options, .. } = &mut self.kind {
            options.push((value.into(), label.into()));
        }
        self
    }

    /// The choice that is on in a set of radios.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        match &mut self.kind {
            Kind::Radios { value: on, .. } => *on = Some(value.into()),
            Kind::Toggle { value: posted, .. } => *posted = value.into(),
            _ => {}
        }
        self
    }

    /// A menu button. `label` is what the entry reads in the overflow menu; the
    /// button is the menu's own.
    pub fn menu(label: impl Into<Node>, menu: DropdownMenu) -> Self {
        Self::of(Kind::Menu {
            label: label.into(),
            menu: Box::new(menu),
        })
    }

    /// A line between tools.
    pub fn separator() -> Self {
        Self::of(Kind::Separator)
    }

    /// Tools in a group named `label`. A group inside a group is flattened.
    pub fn group(label: impl Into<String>, items: impl IntoIterator<Item = ToolbarItem>) -> Self {
        let items = items
            .into_iter()
            .flat_map(|item| match item.kind {
                Kind::Group { items, .. } => items,
                _ => vec![item],
            })
            .collect();
        Self::of(Kind::Group {
            label: label.into(),
            items,
        })
    }

    /// Adds an icon before the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        match &mut self.kind {
            Kind::Button { glyph: g, .. }
            | Kind::Link { glyph: g, .. }
            | Kind::Post { glyph: g, .. }
            | Kind::Toggle { glyph: g, .. } => *g = Some(glyph),
            _ => {}
        }
        self
    }

    /// Sets the look of a button.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        if let Kind::Button { variant: v, .. } = &mut self.kind {
            *v = variant;
        }
        self
    }

    /// Runs commands in the browser when a button is pressed.
    pub fn on_click(mut self, commands: impl Into<Commands>) -> Self {
        if let Kind::Button { commands: c, .. } = &mut self.kind {
            *c = Some(commands.into());
        }
        self
    }

    /// Opens the `<dialog>` with this id when a button is pressed.
    pub fn opens_dialog(mut self, dialog: impl Into<String>) -> Self {
        if let Kind::Button { dialog: d, .. } = &mut self.kind {
            *d = Some(dialog.into());
        }
        self
    }

    /// Turns a toggle on.
    pub fn pressed(mut self, pressed: bool) -> Self {
        if let Kind::Toggle { pressed: p, .. } = &mut self.kind {
            *p = pressed;
        }
        self
    }

    /// Marks a link as the page the person is on.
    pub fn current(mut self, current: bool) -> Self {
        if let Kind::Link { current: c, .. } = &mut self.kind {
            *c = current;
        }
        self
    }

    /// Disables the tool.
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self.kind {
            Kind::Button { disabled: d, .. }
            | Kind::Link { disabled: d, .. }
            | Kind::Post { disabled: d, .. }
            | Kind::Toggle { disabled: d, .. }
            | Kind::Radios { disabled: d, .. } => *d = disabled,
            _ => {}
        }
        self
    }

    /// Keeps the tool in the bar however narrow it gets: it is the last to move into
    /// the overflow menu, or never does.
    pub fn keep(mut self) -> Self {
        self.keep = true;
        self
    }
}

/// A toolbar.
///
/// ```
/// use navree_ui::{Toolbar, ToolbarItem, icons, render};
///
/// let bar = Toolbar::new("format", "Text formatting")
///     .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
///     .item(ToolbarItem::icon_toggle("italic", icons::ITALIC, "Italic"))
///     .item(ToolbarItem::separator())
///     .item(ToolbarItem::link("Help", "/help"));
/// let html = render(&bar.into());
/// assert!(html.contains(r#"aria-label="Text formatting""#));
/// assert!(html.contains(r#"name="bold""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Toolbar {
    id: String,
    label: String,
    items: Vec<ToolbarItem>,
    orientation: Orientation,
    wrap: bool,
    size: Size,
    labels: ToolbarLabels,
}

/// What a tool that can move has in the overflow menu.
enum Twin {
    Item(MenuItem),
    Check(MenuCheckbox),
    Radios(MenuRadios),
    Menu(crate::components::MenuSub),
}

impl Toolbar {
    /// A toolbar named `label` for assistive technology. `id` names its overflow
    /// menu, and must be unique on the page.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items: Vec::new(),
            orientation: Orientation::Horizontal,
            wrap: true,
            size: Size::Sm,
            labels: ToolbarLabels::default(),
        }
    }

    /// Adds a tool.
    pub fn item(mut self, item: ToolbarItem) -> Self {
        self.items.push(item);
        self
    }

    /// Adds tools.
    pub fn items(mut self, items: impl IntoIterator<Item = ToolbarItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Stacks the tools. A vertical toolbar uses the up and down arrows and has no
    /// overflow menu.
    pub fn vertical(mut self) -> Self {
        self.orientation = Orientation::Vertical;
        self
    }

    /// Stops the arrow keys at the first and last tool, where they would go round.
    pub fn no_loop(mut self) -> Self {
        self.wrap = false;
        self
    }

    /// Sets the size of the buttons.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: ToolbarLabels) -> Self {
        self.labels = labels;
        self
    }

    fn button(&self, kind: &Kind) -> Option<Element> {
        match kind {
            Kind::Button {
                label,
                glyph,
                icon_only,
                aria,
                variant,
                commands,
                dialog,
                disabled,
            } => {
                let mut button = match (icon_only, glyph) {
                    (true, Some(glyph)) => Button::icon_only(
                        *glyph,
                        aria.clone().unwrap_or_else(|| label.text_content()),
                    ),
                    _ => {
                        let mut button = Button::new(label.clone());
                        if let Some(glyph) = glyph {
                            button = button.icon(*glyph);
                        }
                        button
                    }
                }
                .variant(*variant)
                .size(self.size)
                .disabled(*disabled);
                if let Some(commands) = commands {
                    button = button.on_click(commands.clone());
                }
                if let Some(dialog) = dialog {
                    button = button.opens_dialog(dialog.clone());
                }
                Node::from(button).into_element()
            }
            Kind::Link {
                label,
                glyph,
                href,
                disabled,
                current,
            } => {
                let mut link = Button::ghost(label.clone()).size(self.size);
                if let Some(glyph) = glyph {
                    link = link.icon(*glyph);
                }
                if !disabled {
                    link = link.href(href.clone());
                }
                let link = link.disabled(*disabled);
                Node::from(link)
                    .into_element()
                    .map(|el| el.attr_opt("aria-current", current.then_some("page")))
            }
            Kind::Post {
                label,
                glyph,
                href,
                disabled,
            } => {
                let mut button = Button::ghost(label.clone()).size(self.size);
                if let Some(glyph) = glyph {
                    button = button.icon(*glyph);
                }
                let button = if *disabled {
                    button.disabled(true)
                } else {
                    button
                        .submit()
                        .attr("form", POST_FORM)
                        .attr("formaction", href.clone())
                        .attr("formmethod", "post")
                };
                Node::from(button).into_element()
            }
            _ => None,
        }
    }

    /// The real control of a tool, and the twin it has in the overflow menu.
    fn tool(&self, item: &ToolbarItem, index: usize, key: &mut u32) -> (Vec<Node>, Vec<Twin>) {
        let mut next_key = |input: Element| -> Element {
            *key += 1;
            input.attr("data-twin-key", format!("{index}.{key}"))
        };
        match &item.kind {
            Kind::Button {
                label,
                glyph,
                commands,
                dialog,
                disabled,
                ..
            } => {
                let mut twin = MenuItem::action(label.clone()).disabled(*disabled);
                if let Some(glyph) = glyph {
                    twin = twin.icon(*glyph);
                }
                if let Some(commands) = commands {
                    twin = twin.on_click(commands.clone());
                }
                if let Some(dialog) = dialog {
                    twin = twin.opens_dialog(dialog.clone());
                }
                (
                    self.button(&item.kind)
                        .map(Node::from)
                        .into_iter()
                        .collect(),
                    vec![Twin::Item(twin)],
                )
            }
            Kind::Link {
                label,
                glyph,
                href,
                disabled,
                ..
            } => {
                let mut twin = MenuItem::link(label.clone(), href.clone()).disabled(*disabled);
                if let Some(glyph) = glyph {
                    twin = twin.icon(*glyph);
                }
                (
                    self.button(&item.kind)
                        .map(Node::from)
                        .into_iter()
                        .collect(),
                    vec![Twin::Item(twin)],
                )
            }
            Kind::Post {
                label,
                glyph,
                href,
                disabled,
            } => {
                let mut twin = MenuItem::post(label.clone(), href.clone()).disabled(*disabled);
                if let Some(glyph) = glyph {
                    twin = twin.icon(*glyph);
                }
                (
                    self.button(&item.kind)
                        .map(Node::from)
                        .into_iter()
                        .collect(),
                    vec![Twin::Item(twin)],
                )
            }
            Kind::Toggle {
                name,
                value,
                label,
                glyph,
                icon_only,
                pressed,
                disabled,
            } => {
                let aria = icon_only.then(|| label.text_content());
                let input = next_key(
                    el("input")
                        .attr("type", "checkbox")
                        .attr("name", name.clone())
                        .attr("value", value.clone())
                        .flag("checked", *pressed)
                        .flag("disabled", *disabled)
                        .attr_opt("aria-label", aria),
                );
                let toggle = el("label")
                    .slot("toolbar-toggle")
                    .flag("data-icon-only", *icon_only)
                    .flag("data-size-sm", self.size == Size::Sm)
                    .children((input, glyph.map(icon), (!icon_only).then(|| label.clone())));
                let twin = MenuCheckbox::new("", label.clone())
                    .checked(*pressed)
                    .disabled(*disabled);
                (vec![toggle.into()], vec![Twin::Check(twin)])
            }
            Kind::Radios {
                name,
                label,
                options,
                value,
                disabled,
            } => {
                let inputs: Vec<Element> = options
                    .iter()
                    .map(|(each, text)| {
                        let input = next_key(
                            el("input")
                                .attr("type", "radio")
                                .attr("name", name.clone())
                                .attr("value", each.clone())
                                .flag("checked", value.as_deref() == Some(each.as_str()))
                                .flag("disabled", *disabled),
                        );
                        el("label")
                            .slot("toolbar-toggle")
                            .children((input, text.clone()))
                    })
                    .collect();
                let group = el("div")
                    .slot("toolbar-radios")
                    .attr("role", "radiogroup")
                    .attr("aria-label", label.clone())
                    .children(inputs);
                let mut twin = MenuRadios::new("", label.clone());
                for (each, text) in options {
                    twin = if *disabled {
                        twin.disabled_option(each.clone(), text.clone())
                    } else {
                        twin.option(each.clone(), text.clone())
                    };
                }
                if let Some(on) = value {
                    twin = twin.value(on.clone());
                }
                (vec![group.into()], vec![Twin::Radios(twin)])
            }
            Kind::Menu { label, menu } => (
                vec![(**menu).clone().into()],
                vec![Twin::Menu(menu.as_submenu(label.clone()))],
            ),
            Kind::Separator => (Vec::new(), Vec::new()),
            Kind::Group { .. } => (Vec::new(), Vec::new()),
        }
    }

    fn into_node(self) -> Node {
        let vertical = self.orientation == Orientation::Vertical;
        let mut tools: Vec<Element> = Vec::new();
        let mut owners: Vec<usize> = Vec::new();
        let mut menu = DropdownMenu::icon(
            format!("{}-more", self.id),
            icons::ELLIPSIS,
            self.labels.more.to_string(),
        )
        .size(self.size);
        let mut first_twin = true;
        // A line in the bar, or a group, is a line in the menu.
        let mut divide = false;
        for (index, item) in self.items.iter().enumerate() {
            let mut key = 0;
            let (own, twins, class): (Vec<Node>, Vec<Twin>, &'static str) = match &item.kind {
                Kind::Separator => (
                    vec![
                        el("div")
                            .slot("toolbar-separator")
                            .attr("role", "separator")
                            .attr(
                                "aria-orientation",
                                if vertical { "horizontal" } else { "vertical" },
                            )
                            .into(),
                    ],
                    Vec::new(),
                    "separator",
                ),
                Kind::Group { label, items } => {
                    let mut nodes = Vec::new();
                    let mut twins = Vec::new();
                    for inner in items {
                        let (own, mine) = self.tool(inner, index, &mut key);
                        nodes.extend(own);
                        twins.extend(mine);
                    }
                    let group = el("div")
                        .slot("toolbar-group")
                        .attr("role", "group")
                        .attr("aria-label", label.clone())
                        .children(nodes);
                    (vec![group.into()], twins, "group")
                }
                _ => {
                    let (own, twins) = self.tool(item, index, &mut key);
                    (own, twins, "tool")
                }
            };
            tools.push(
                el("div")
                    .slot("toolbar-tool")
                    .attr("data-tool", i64::try_from(index).unwrap_or(0))
                    .attr("data-kind", class)
                    .flag("data-keep", item.keep)
                    .children(own),
            );
            let is_group = matches!(item.kind, Kind::Group { .. });
            if matches!(item.kind, Kind::Separator) || is_group {
                divide = true;
            }
            if !twins.is_empty() && divide && !first_twin {
                menu = menu.separator();
                owners.push(index);
            }
            if !twins.is_empty() {
                divide = is_group;
            }
            for twin in twins {
                menu = match twin {
                    Twin::Item(each) => menu.item(each),
                    Twin::Check(each) => menu.checkbox(each),
                    Twin::Radios(each) => menu.radios(each),
                    Twin::Menu(each) => menu.submenu(each),
                };
                owners.push(index);
                first_twin = false;
            }
        }
        // A vertical bar doesn't give up tools: it has no overflow.
        let more = (!owners.is_empty() && !vertical).then(|| {
            let mut node: Node = menu.into();
            tag_overflow(&mut node, &owners);
            el("div").slot("toolbar-more").child(node)
        });
        let orientation = self.orientation.as_str();
        el("div")
            .slot("toolbar")
            .id(self.id.clone())
            .attr("role", "group")
            .attr("aria-label", self.label.clone())
            .attr("data-ui-toolbar", "")
            .attr("data-orientation", orientation)
            .flag("data-loop", self.wrap)
            .uses(Uses::TOOLBAR)
            .children((tools, more))
            .into()
    }
}

into_node!(Toolbar);

fn set_attr(element: &mut Element, name: &'static str, value: AttrValue) {
    element.attrs.push(Attr { name, value });
}

/// Marks the overflow menu's entries with the tool each stands for, hides them and
/// hides the button that opens the menu, until the module shows what is needed.
fn tag_overflow(node: &mut Node, owners: &[usize]) {
    let mut counts: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let Node::Fragment(parts) = node else { return };
    if let Some(Node::Element(trigger)) = parts.first_mut() {
        set_attr(trigger, "hidden", AttrValue::Flag);
        set_attr(trigger, "data-slot-more", AttrValue::Flag);
    }
    let Some(Node::Element(menu)) = parts.get_mut(1) else {
        return;
    };
    for (entry, owner) in menu.children.iter_mut().zip(owners) {
        tag_twin(entry, *owner, &mut counts);
    }
}

fn tag_twin(node: &mut Node, owner: usize, counts: &mut std::collections::HashMap<usize, u32>) {
    match node {
        Node::Element(element) => {
            set_attr(
                element,
                "data-twin",
                AttrValue::Int(i64::try_from(owner).unwrap_or(0)),
            );
            set_attr(element, "hidden", AttrValue::Flag);
            // A twin of a toggle or a choice shares a key with the real input.
            number_inputs(node, owner, counts);
        }
        // A submenu is its button and its menu: the button is the twin.
        Node::Fragment(parts) => {
            if let Some(first) = parts.first_mut() {
                tag_twin(first, owner, counts);
            }
        }
        _ => {}
    }
}

/// Gives the inputs inside `node` the keys of the real inputs they follow, in order.
fn number_inputs(
    node: &mut Node,
    owner: usize,
    counts: &mut std::collections::HashMap<usize, u32>,
) {
    fn walk(node: &mut Node, owner: usize, n: &mut u32) {
        match node {
            Node::Element(element) => {
                if element.tag == "input" {
                    *n += 1;
                    set_attr(
                        element,
                        "data-twin-key",
                        AttrValue::Text(format!("{owner}.{n}").into()),
                    );
                }
                for child in &mut element.children {
                    walk(child, owner, n);
                }
            }
            Node::Fragment(parts) => {
                for part in parts {
                    walk(part, owner, n);
                }
            }
            _ => {}
        }
    }
    // The count carries across the entries of one tool.
    let n = counts.entry(owner).or_insert(0);
    walk(node, owner, n);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::MenuItem;
    use crate::render::{measure, render};

    fn html(bar: Toolbar) -> String {
        render(&bar.into())
    }

    fn format_bar() -> Toolbar {
        Toolbar::new("fmt", "Text formatting")
            .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
            .item(ToolbarItem::icon_toggle("italic", icons::ITALIC, "Italic").pressed(true))
            .item(ToolbarItem::separator())
            .item(ToolbarItem::group(
                "Alignment",
                [ToolbarItem::radios("align", "Alignment")
                    .option("left", "Left")
                    .option("center", "Center")
                    .value("left")],
            ))
            .item(ToolbarItem::link("Help", "/help"))
            .item(
                ToolbarItem::button("Copy")
                    .icon(icons::COPY)
                    .on_click(crate::Cmd::copy("x")),
            )
    }

    #[test]
    fn it_is_a_labelled_group_of_tools_until_the_module_runs() {
        let out = html(format_bar());
        assert!(
            out.contains(r#"role="group" aria-label="Text formatting""#),
            "{out}"
        );
        assert!(!out.contains(r#"role="toolbar""#), "{out}");
        assert!(out.contains("data-ui-toolbar"), "{out}");
        assert!(out.contains(r#"data-orientation="horizontal""#), "{out}");
        assert!(out.contains("data-loop"), "{out}");
        assert_eq!(
            out.matches(r#"data-slot="toolbar-tool""#).count(),
            6,
            "{out}"
        );
    }

    #[test]
    fn toggles_and_choices_are_native_inputs_that_post() {
        let out = html(format_bar());
        assert!(
            out.contains(r#"type="checkbox" name="bold" value="on""#),
            "{out}"
        );
        assert!(out.contains(r#"name="italic" value="on" checked"#), "{out}");
        assert!(out.contains(r#"aria-label="Bold""#), "{out}");
        assert!(
            out.contains(r#"role="radiogroup" aria-label="Alignment""#),
            "{out}"
        );
        assert!(
            out.contains(r#"type="radio" name="align" value="left" checked"#),
            "{out}"
        );
        assert!(
            out.contains(r#"role="group" aria-label="Alignment""#),
            "a group of its own: {out}"
        );
        assert!(
            out.contains(r#"role="separator" aria-orientation="vertical""#),
            "{out}"
        );
    }

    #[test]
    fn every_tool_that_can_move_has_a_hidden_twin_in_the_menu() {
        let out = html(format_bar());
        assert!(out.contains(r#"id="fmt-more""#), "{out}");
        assert!(out.contains(r#"aria-label="More""#), "{out}");
        // The button that opens it is hidden, and so is each twin.
        assert!(out.contains("hidden data-slot-more"), "{out}");
        assert_eq!(out.matches("data-twin=").count(), 7, "{out}");
        for owner in [0, 1, 3, 4, 5] {
            assert!(
                out.contains(&format!(r#"data-twin="{owner}""#)),
                "tool {owner}: {out}"
            );
        }
        assert!(
            !out.contains(r#"data-twin="2""#),
            "a separator has no twin: {out}"
        );
    }

    #[test]
    fn a_twin_of_a_choice_has_no_name_and_shares_a_key_with_the_real_input() {
        let out = html(format_bar());
        let real = out.matches(r#"data-twin-key="0.1""#).count();
        assert_eq!(real, 2, "the toggle and its twin: {out}");
        // The radios: two real, two twins, keys 3.1 and 3.2.
        assert_eq!(out.matches(r#"data-twin-key="3.1""#).count(), 2, "{out}");
        assert_eq!(out.matches(r#"data-twin-key="3.2""#).count(), 2, "{out}");
        // The twins carry empty names.
        assert!(out.contains(r#"name="""#), "{out}");
    }

    #[test]
    fn a_menu_button_has_a_submenu_twin_with_its_entries() {
        let out = html(
            Toolbar::new("t", "Tools")
                .item(ToolbarItem::button("A"))
                .item(ToolbarItem::menu(
                    "Share",
                    DropdownMenu::new("share", "Share").item(MenuItem::link("Email", "/mail")),
                )),
        );
        assert!(out.contains(r#"id="share""#), "the real menu: {out}");
        assert!(out.contains(r#"href="/mail""#), "{out}");
        assert!(
            out.matches(r#"href="/mail""#).count() == 2,
            "in the menu and in its twin: {out}"
        );
        assert!(out.contains(r#"data-twin="1""#), "{out}");
    }

    #[test]
    fn a_post_tool_posts_through_the_page_form_and_the_page_gets_one() {
        let bar = Toolbar::new("t", "T").item(ToolbarItem::post("Archive", "/archive"));
        let out = html(bar.clone());
        assert!(
            out.contains(r#"form="ui-post" formaction="/archive" formmethod="post""#),
            "{out}"
        );
        let node: Node = bar.into();
        assert!(measure(&node).posts);
        assert!(measure(&node).uses.has(Uses::CORE) || measure(&node).posts);
    }

    #[test]
    fn disabled_and_kept_tools() {
        let out = html(
            Toolbar::new("t", "T")
                .item(ToolbarItem::button("A").disabled(true))
                .item(ToolbarItem::toggle("b", "B").disabled(true))
                .item(ToolbarItem::link("C", "/c").keep().disabled(true)),
        );
        assert!(out.matches("disabled").count() >= 3, "{out}");
        assert!(out.contains("data-keep"), "{out}");
        assert!(!out.contains(r#"href="/c" data-slot=button"#), "{out}");
    }

    #[test]
    fn vertical_and_no_loop() {
        let out = html(format_bar().vertical().no_loop());
        assert!(
            !out.contains("toolbar-more") && !out.contains("data-twin="),
            "a vertical bar has no overflow: {out}"
        );
        assert!(out.contains(r#"data-orientation="vertical""#), "{out}");
        assert!(!out.contains("data-loop"), "{out}");
        assert!(
            out.contains(r#"role="separator" aria-orientation="horizontal""#),
            "{out}"
        );
    }

    #[test]
    fn nothing_to_show_and_many_tools() {
        let empty = html(Toolbar::new("e", "Empty"));
        assert!(empty.contains("data-ui-toolbar"), "{empty}");
        assert!(!empty.contains("toolbar-more"), "{empty}");
        let only_separator = html(Toolbar::new("s", "S").item(ToolbarItem::separator()));
        assert!(!only_separator.contains("toolbar-more"), "{only_separator}");
        let many = html(
            Toolbar::new("m", "Many")
                .items((0..200).map(|n| ToolbarItem::button(format!("Tool {n}")))),
        );
        assert_eq!(many.matches(r#"data-slot="toolbar-tool""#).count(), 200);
        assert_eq!(many.matches("data-twin=").count(), 200);
    }

    #[test]
    fn hostile_text_is_text_and_a_bad_link_is_defused() {
        let out = html(
            Toolbar::new("a\"><script>", "<b>x</b>")
                .item(ToolbarItem::button("<img src=x onerror=y>"))
                .item(ToolbarItem::link("l", "javascript:alert(1)"))
                .item(ToolbarItem::toggle("n\"><i>", "<u>"))
                .item(ToolbarItem::group(
                    "</div><script>",
                    [ToolbarItem::radios("r", "<i>").option("\"><script>", "<s>")],
                )),
        );
        assert!(
            !out.contains("<script>")
                && !out.contains("<img")
                && !out.contains("<b>")
                && !out.contains("<u>")
                && !out.contains("<s>"),
            "{out}"
        );
        assert!(!out.contains("javascript:"), "{out}");
    }

    #[test]
    fn it_asks_for_the_toolbar_module_and_the_menus_it_holds() {
        let node: Node = format_bar().into();
        let uses = measure(&node).uses;
        assert!(
            uses.has(Uses::TOOLBAR) && uses.has(Uses::MENUS) && uses.has(Uses::OVERLAY),
            "{uses:?}"
        );
        assert!(!uses.has(Uses::SLIDER) && !uses.has(Uses::TREE));
    }
}
