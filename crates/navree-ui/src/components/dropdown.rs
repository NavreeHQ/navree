//! DropdownMenu: a button that opens a menu of actions, links, choices and submenus.
//! The popup is the browser's `popover`, so light dismiss, Esc and the top layer are
//! its own; the `menus` module adds what makes it a menu: the arrow keys, typeahead,
//! submenus, and closing after a choice. The `overlay` module places it.

use std::borrow::Cow;

use super::{Button, ButtonVariant, PopupAlign, Side, Size, icon};
use crate::cmd::{Commands, Trigger};
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, Node, Uses, el, into_node};

#[derive(Clone, Debug)]
enum Kind {
    Link(Href),
    Action,
    Post(Href),
}

/// One thing a menu does: a link, an action in the browser, or a post.
#[derive(Clone, Debug)]
#[must_use]
pub struct MenuItem {
    label: Node,
    kind: Kind,
    start: Option<Icon>,
    end: Option<Icon>,
    shortcut: Option<Cow<'static, str>>,
    destructive: bool,
    disabled: bool,
    keep_open: bool,
    commands: Option<Commands>,
    dialog: Option<String>,
}

impl MenuItem {
    fn with(label: Node, kind: Kind) -> Self {
        Self {
            label,
            kind,
            start: None,
            end: None,
            shortcut: None,
            destructive: false,
            disabled: false,
            keep_open: false,
            commands: None,
            dialog: None,
        }
    }

    /// A link to `href`.
    pub fn link(label: impl Into<Node>, href: impl Into<Href>) -> Self {
        Self::with(label.into(), Kind::Link(href.into()))
    }

    /// A button with no destination of its own: give it [`on_click`](Self::on_click)
    /// commands, or [`opens_dialog`](Self::opens_dialog).
    pub fn action(label: impl Into<Node>) -> Self {
        Self::with(label.into(), Kind::Action)
    }

    /// A button that posts to `href`. Give the menu the session's token with
    /// [`DropdownMenu::csrf`] for a page that works with no script.
    pub fn post(label: impl Into<Node>, href: impl Into<Href>) -> Self {
        Self::with(label.into(), Kind::Post(href.into()))
    }

    /// Adds an icon before the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Adds an icon after the label.
    pub fn icon_end(mut self, glyph: Icon) -> Self {
        self.end = Some(glyph);
        self
    }

    /// Shows a hint of the shortcut that does the same thing. It binds nothing.
    pub fn shortcut(mut self, keys: impl Into<Cow<'static, str>>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// Marks it as destroying something.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Disables it: the arrow keys still reach it, as they should, but it does nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Leaves the menu open when this is chosen.
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// Runs commands in the browser when chosen.
    pub fn on_click(mut self, commands: impl Into<Commands>) -> Self {
        self.commands = Some(commands.into());
        self
    }

    /// Opens the `<dialog>` with this id, natively, when chosen.
    pub fn opens_dialog(mut self, dialog: impl Into<String>) -> Self {
        self.dialog = Some(dialog.into());
        self
    }
}

/// A choice that is on or off, posting `name` when it is on.
#[derive(Clone, Debug)]
#[must_use]
pub struct MenuCheckbox {
    name: String,
    value: String,
    label: Node,
    checked: bool,
    disabled: bool,
    keep_open: bool,
    commands: Option<Commands>,
}

impl MenuCheckbox {
    /// A checkbox item posting as `name`.
    pub fn new(name: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            name: name.into(),
            value: String::from("on"),
            label: label.into(),
            checked: false,
            disabled: false,
            keep_open: false,
            commands: None,
        }
    }

    /// Sets the value it posts when on.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// Starts on.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Leaves the menu open when this is chosen.
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// Runs commands when it changes.
    pub fn on_change(mut self, commands: impl Into<Commands>) -> Self {
        self.commands = Some(commands.into());
        self
    }
}

/// A set of choices of which one is on, under a heading.
#[derive(Clone, Debug)]
#[must_use]
pub struct MenuRadios {
    name: String,
    label: Node,
    options: Vec<(String, Node, bool)>,
    value: Option<String>,
    keep_open: bool,
    commands: Option<Commands>,
}

impl MenuRadios {
    /// A set posting as `name`, headed `label`.
    pub fn new(name: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            options: Vec::new(),
            value: None,
            keep_open: false,
            commands: None,
        }
    }

    /// Adds a choice.
    pub fn option(mut self, value: impl Into<String>, label: impl Into<Node>) -> Self {
        self.options.push((value.into(), label.into(), false));
        self
    }

    /// Adds a choice that can't be taken.
    pub fn disabled_option(mut self, value: impl Into<String>, label: impl Into<Node>) -> Self {
        self.options.push((value.into(), label.into(), true));
        self
    }

    /// Turns one on.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Leaves the menu open when a choice is made.
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// Runs commands when the choice changes.
    pub fn on_change(mut self, commands: impl Into<Commands>) -> Self {
        self.commands = Some(commands.into());
        self
    }
}

#[derive(Clone, Debug)]
enum Entry {
    Item(MenuItem),
    Separator,
    Header(Node),
    Group(MenuGroup),
    Checkbox(MenuCheckbox),
    Radios(MenuRadios),
    Sub(MenuSub),
}

/// Adds the entries every menu-like thing holds.
macro_rules! entries {
    ($($type:ident),+) => {
        $(impl $type {
            /// Adds an item.
            pub fn item(mut self, item: MenuItem) -> Self {
                self.entries.push(Entry::Item(item));
                self
            }

            /// Adds a line between items.
            pub fn separator(mut self) -> Self {
                self.entries.push(Entry::Separator);
                self
            }

            /// Adds a group of items under a heading.
            pub fn group(mut self, group: MenuGroup) -> Self {
                self.entries.push(Entry::Group(group));
                self
            }

            /// Adds a choice that is on or off.
            pub fn checkbox(mut self, checkbox: MenuCheckbox) -> Self {
                self.entries.push(Entry::Checkbox(checkbox));
                self
            }

            /// Adds a set of choices of which one is on.
            pub fn radios(mut self, radios: MenuRadios) -> Self {
                self.entries.push(Entry::Radios(radios));
                self
            }

            /// Adds a submenu.
            pub fn submenu(mut self, sub: MenuSub) -> Self {
                self.entries.push(Entry::Sub(sub));
                self
            }
        })+
    };
}

/// Items under a heading.
#[derive(Clone, Debug)]
#[must_use]
pub struct MenuGroup {
    label: Node,
    entries: Vec<Entry>,
}

impl MenuGroup {
    /// A group headed `label`.
    pub fn new(label: impl Into<Node>) -> Self {
        Self {
            label: label.into(),
            entries: Vec::new(),
        }
    }
}

/// An item that opens a menu of its own.
#[derive(Clone, Debug)]
#[must_use]
pub struct MenuSub {
    label: Node,
    start: Option<Icon>,
    disabled: bool,
    entries: Vec<Entry>,
}

impl MenuSub {
    /// A submenu opened by an item reading `label`.
    pub fn new(label: impl Into<Node>) -> Self {
        Self {
            label: label.into(),
            start: None,
            disabled: false,
            entries: Vec::new(),
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

/// A button and the menu it opens.
///
/// ```
/// use navree_ui::{DropdownMenu, MenuItem, render};
///
/// let menu = DropdownMenu::new("deal-menu", "Actions")
///     .item(MenuItem::link("Edit", "/deals/1/edit"))
///     .separator()
///     .item(MenuItem::post("Delete", "/deals/1/delete").destructive());
/// let html = render(&menu.into());
/// assert!(html.contains(r#"role="menu""#));
/// assert!(html.contains(r#"aria-haspopup="menu""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DropdownMenu {
    id: String,
    trigger: Button,
    side: Side,
    align: PopupAlign,
    entries: Vec<Entry>,
    keep_open: bool,
    csrf: Option<String>,
    label: Option<String>,
}

entries!(MenuGroup, MenuSub);

impl DropdownMenu {
    /// A menu with the id `id`, opened by an outline button reading `label`.
    pub fn new(id: impl Into<String>, label: impl Into<Node>) -> Self {
        Self::button(id, Button::outline(label))
    }

    /// A menu opened by an icon button. The accessible label is required.
    pub fn icon(id: impl Into<String>, glyph: Icon, label: impl Into<String>) -> Self {
        Self::button(
            id,
            Button::icon_only(glyph, label).variant(ButtonVariant::Ghost),
        )
    }

    /// A menu opened by `trigger`, which this makes the menu's button.
    pub fn button(id: impl Into<String>, trigger: Button) -> Self {
        Self {
            id: id.into(),
            trigger,
            side: Side::Bottom,
            align: PopupAlign::Start,
            entries: Vec::new(),
            keep_open: false,
            csrf: None,
            label: None,
        }
    }

    /// Sets the button's look.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.trigger = self.trigger.variant(variant);
        self
    }

    /// Sets the button's size.
    pub fn size(mut self, size: Size) -> Self {
        self.trigger = self.trigger.size(size);
        self
    }

    /// Adds a chevron to the button.
    pub fn caret(mut self) -> Self {
        self.trigger = self.trigger.icon_end(icons::CHEVRON_DOWN);
        self
    }

    /// Sets the side it opens on (below by default).
    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    /// Sets how it lines up along that side (the start by default; a menu on the
    /// last column of a table wants the end).
    pub fn align(mut self, align: PopupAlign) -> Self {
        self.align = align;
        self
    }

    /// Names the menu when the button's text doesn't.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Leaves the menu open when any item is chosen.
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// Puts the session's CSRF token in each post item's form.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Adds something before the items that is not one: a name and an address.
    pub fn header(mut self, header: impl Into<Node>) -> Self {
        self.entries.push(Entry::Header(header.into()));
        self
    }

    /// Adds an item.
    pub fn item(mut self, item: MenuItem) -> Self {
        self.entries.push(Entry::Item(item));
        self
    }

    /// Adds a line between items.
    pub fn separator(mut self) -> Self {
        self.entries.push(Entry::Separator);
        self
    }

    /// Adds a group of items under a heading.
    pub fn group(mut self, group: MenuGroup) -> Self {
        self.entries.push(Entry::Group(group));
        self
    }

    /// Adds a choice that is on or off.
    pub fn checkbox(mut self, checkbox: MenuCheckbox) -> Self {
        self.entries.push(Entry::Checkbox(checkbox));
        self
    }

    /// Adds a set of choices of which one is on.
    pub fn radios(mut self, radios: MenuRadios) -> Self {
        self.entries.push(Entry::Radios(radios));
        self
    }

    /// Adds a submenu.
    pub fn submenu(mut self, sub: MenuSub) -> Self {
        self.entries.push(Entry::Sub(sub));
        self
    }

    fn into_node(self) -> Node {
        let trigger_id = format!("{}-trigger", self.id);
        let mut cx = Cx {
            menu: &self.id,
            next: 0,
            csrf: self.csrf.as_deref(),
            keep_open: self.keep_open,
            posts: false,
        };
        let items: Vec<Node> = self
            .entries
            .iter()
            .map(|entry| entry_node(entry, &mut cx))
            .collect();
        let posts = cx.posts;
        let trigger = self
            .trigger
            .attr("id", trigger_id.clone())
            .attr("popovertarget", self.id.clone())
            .attr("aria-haspopup", "menu")
            .attr("aria-expanded", "false");
        let menu = menu_element(
            &self.id,
            &trigger_id,
            self.label,
            self.side,
            self.align,
            false,
        )
        .flag("data-keep-open", self.keep_open)
        .child(items);
        let menu = if posts { menu.uses(Uses::CORE) } else { menu };
        Node::list((trigger, menu))
    }
}

/// What numbering and settings an entry needs from the menu around it.
struct Cx<'a> {
    menu: &'a str,
    next: usize,
    csrf: Option<&'a str>,
    keep_open: bool,
    posts: bool,
}

impl Cx<'_> {
    fn id(&mut self, kind: char) -> String {
        self.next += 1;
        format!("{}-{kind}{}", self.menu, self.next)
    }
}

fn menu_element(
    id: &str,
    labelled_by: &str,
    label: Option<String>,
    side: Side,
    align: PopupAlign,
    sub: bool,
) -> Element {
    el("div")
        .slot("dropdown-menu")
        .attr("id", id.to_owned())
        .attr("popover", "auto")
        .attr("role", "menu")
        .attr("tabindex", -1_i64)
        .attr("aria-labelledby", labelled_by.to_owned())
        .attr_opt("aria-label", label)
        .flag("data-sub", sub)
        .flag("data-ui-menu", true)
        .flag("data-ui-place", true)
        .attr("data-side", side.as_str())
        .attr_opt("data-align", align.as_str())
        .uses(Uses::MENUS.with(Uses::OVERLAY))
}

fn entry_node(entry: &Entry, cx: &mut Cx<'_>) -> Node {
    match entry {
        Entry::Item(item) => item_node(item, cx),
        Entry::Separator => el("div")
            .slot("menu-separator")
            .attr("role", "separator")
            .into(),
        Entry::Header(node) => el("div")
            .slot("menu-header")
            .attr("role", "presentation")
            .child(node.clone())
            .into(),
        Entry::Group(group) => {
            let label_id = cx.id('g');
            let inner: Vec<Node> = group
                .entries
                .iter()
                .map(|entry| entry_node(entry, cx))
                .collect();
            el("div")
                .attr("role", "group")
                .attr("aria-labelledby", label_id.clone())
                .slot("menu-group")
                .children((
                    el("div")
                        .slot("menu-label")
                        .attr("id", label_id)
                        .child(group.label.clone()),
                    inner,
                ))
                .into()
        }
        Entry::Checkbox(checkbox) => {
            let mut input = el("input")
                .attr("type", "checkbox")
                .attr("role", "menuitemcheckbox")
                .attr("name", checkbox.name.clone())
                .attr("value", checkbox.value.clone())
                .flag("checked", checkbox.checked)
                .flag("disabled", checkbox.disabled);
            if let Some(commands) = &checkbox.commands {
                input = input.on(Trigger::Change, commands.clone());
            }
            choice(
                input,
                checkbox.label.clone(),
                "check",
                cx.keep_open || checkbox.keep_open,
            )
        }
        Entry::Radios(radios) => {
            let label_id = cx.id('g');
            let keep = cx.keep_open || radios.keep_open;
            let options: Vec<Node> = radios
                .options
                .iter()
                .map(|(value, label, disabled)| {
                    let mut input = el("input")
                        .attr("type", "radio")
                        .attr("role", "menuitemradio")
                        .attr("name", radios.name.clone())
                        .attr("value", value.clone())
                        .flag("checked", radios.value.as_deref() == Some(value.as_str()))
                        .flag("disabled", *disabled);
                    if let Some(commands) = &radios.commands {
                        input = input.on(Trigger::Change, commands.clone());
                    }
                    choice(input, label.clone(), "radio", keep)
                })
                .collect();
            el("div")
                .attr("role", "group")
                .attr("aria-labelledby", label_id.clone())
                .slot("menu-group")
                .children((
                    el("div")
                        .slot("menu-label")
                        .attr("id", label_id)
                        .child(radios.label.clone()),
                    options,
                ))
                .into()
        }
        Entry::Sub(sub) => {
            let sub_id = cx.id('s');
            let trigger_id = format!("{sub_id}-trigger");
            let inner: Vec<Node> = sub
                .entries
                .iter()
                .map(|entry| entry_node(entry, cx))
                .collect();
            let trigger = el("button")
                .slot("menu-item")
                .attr("type", "button")
                .attr("role", "menuitem")
                .attr("id", trigger_id.clone())
                .attr("popovertarget", sub_id.clone())
                .attr("aria-haspopup", "menu")
                .attr("aria-expanded", "false")
                .flag("data-sub-trigger", true)
                .children((
                    sub.start.map(icon),
                    el("span").slot("menu-text").child(sub.label.clone()),
                    icon(icons::CHEVRON_RIGHT),
                ));
            let trigger = if sub.disabled {
                trigger
                    .attr("aria-disabled", "true")
                    .flag("data-disabled", true)
            } else {
                trigger
            };
            let menu = menu_element(
                &sub_id,
                &trigger_id,
                None,
                Side::Right,
                PopupAlign::Start,
                true,
            )
            .flag("data-keep-open", cx.keep_open)
            .child(inner);
            Node::list((trigger, menu))
        }
    }
}

/// A checkbox or radio item: the real input, with its mark and text, in a label.
fn choice(input: Element, label: Node, mark: &'static str, keep_open: bool) -> Node {
    el("label")
        .slot("menu-item")
        .attr("data-choice", mark)
        .flag("data-keep-open", keep_open)
        .children((
            input,
            el("span").slot("menu-mark").child(if mark == "check" {
                icon(icons::CHECK)
            } else {
                icon(icons::CIRCLE)
            }),
            el("span").slot("menu-text").child(label),
        ))
        .into()
}

fn item_node(item: &MenuItem, cx: &mut Cx<'_>) -> Node {
    let content = (
        item.start.map(icon),
        el("span").slot("menu-text").child(item.label.clone()),
        item.shortcut
            .clone()
            .map(|keys| el("span").slot("menu-shortcut").child(keys)),
        item.end.map(icon),
    );
    let keep = item.keep_open || cx.keep_open;
    let common = |base: Element| -> Element {
        let mut base = base
            .slot("menu-item")
            .attr("role", "menuitem")
            .flag("data-destructive", item.destructive)
            .flag("data-keep-open", keep);
        if item.disabled {
            base = base
                .attr("aria-disabled", "true")
                .flag("data-disabled", true);
        }
        if let Some(dialog) = &item.dialog
            && !item.disabled
        {
            base = base
                .attr("command", "show-modal")
                .attr("commandfor", dialog.clone());
        }
        if let Some(commands) = &item.commands
            && !item.disabled
        {
            base = base.on(Trigger::Click, commands.clone());
        }
        base
    };
    match &item.kind {
        Kind::Link(href) => {
            let link = common(el("a"));
            let link = if item.disabled {
                link
            } else {
                link.href(href.clone())
            };
            link.children(content).into()
        }
        Kind::Action => common(el("button"))
            .attr("type", "button")
            .children(content)
            .into(),
        Kind::Post(href) => {
            cx.posts = true;
            let button = common(el("button"))
                .attr("type", if item.disabled { "button" } else { "submit" })
                .children(content);
            el("form")
                .attr("role", "none")
                .attr("method", "post")
                .attr("action", href.clone())
                .flag("data-ui-form", true)
                .uses(Uses::CORE)
                .children((cx.csrf.map(|token| csrf_field(token.to_owned())), button))
                .into()
        }
    }
}

into_node!(DropdownMenu);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(menu: DropdownMenu) -> String {
        render(&menu.into())
    }

    fn basic() -> DropdownMenu {
        DropdownMenu::new("m", "Actions")
            .item(MenuItem::link("Edit", "/e").icon(icons::STAR).shortcut("E"))
            .item(MenuItem::action("Copy").on_click(crate::Cmd::copy("x")))
            .separator()
            .item(MenuItem::post("Delete", "/d").destructive())
    }

    #[test]
    fn the_button_and_the_menu_are_tied_together() {
        let out = html(basic());
        assert!(out.contains(r#"id="m-trigger""#), "{out}");
        assert!(out.contains(r#"popovertarget="m""#), "{out}");
        assert!(out.contains(r#"aria-haspopup="menu""#), "{out}");
        assert!(out.contains(r#"aria-expanded="false""#), "{out}");
        assert!(
            out.contains(r#"id="m" popover="auto" role="menu""#),
            "{out}"
        );
        assert!(out.contains(r#"aria-labelledby="m-trigger""#), "{out}");
    }

    #[test]
    fn items_have_the_menuitem_role_whatever_they_are() {
        let out = html(basic());
        assert_eq!(out.matches(r#"role="menuitem""#).count(), 3, "{out}");
        assert!(
            out.contains(r#"<a data-slot="menu-item" role="menuitem" href="/e""#),
            "{out}"
        );
        assert!(out.contains(r#"role="separator""#), "{out}");
        assert!(out.contains(r#"data-slot="menu-shortcut""#), "{out}");
        assert!(out.contains("data-destructive"), "{out}");
    }

    #[test]
    fn a_post_item_is_a_form_and_wants_core() {
        let out = html(basic().csrf("tok"));
        assert!(
            out.contains(r#"<form role="none" method="post" action="/d" data-ui-form>"#),
            "{out}"
        );
        assert!(out.contains(r#"name="_csrf" value="tok""#), "{out}");
        let node: Node = basic().into();
        assert!(crate::render::measure(&node).uses.has(Uses::CORE));
        let plain: Node = DropdownMenu::new("m", "A")
            .item(MenuItem::link("Edit", "/e"))
            .into();
        let uses = crate::render::measure(&plain).uses;
        assert!(uses.has(Uses::MENUS) && uses.has(Uses::OVERLAY));
        assert!(!uses.has(Uses::CORE));
    }

    #[test]
    fn a_disabled_item_stays_in_the_list_and_cannot_act() {
        let out = html(
            DropdownMenu::new("m", "A")
                .item(MenuItem::link("Edit", "/e").disabled(true))
                .item(
                    MenuItem::action("Copy")
                        .on_click(crate::Cmd::copy("x"))
                        .disabled(true),
                )
                .item(MenuItem::post("Del", "/d").disabled(true)),
        );
        assert_eq!(out.matches(r#"aria-disabled="true""#).count(), 3, "{out}");
        assert!(!out.contains("href=\"/e\""), "{out}");
        assert!(!out.contains("data-ui-click"), "{out}");
        assert!(!out.contains("type=\"submit\""), "{out}");
    }

    #[test]
    fn an_item_can_open_a_dialog() {
        let out = html(
            DropdownMenu::new("m", "A").item(MenuItem::action("Delete…").opens_dialog("confirm")),
        );
        assert!(
            out.contains(r#"command="show-modal" commandfor="confirm""#),
            "{out}"
        );
    }

    #[test]
    fn checkbox_and_radio_items_are_real_inputs() {
        let out = html(
            DropdownMenu::new("m", "View")
                .checkbox(MenuCheckbox::new("archived", "Show archived").checked(true))
                .radios(
                    MenuRadios::new("sort", "Sort by")
                        .option("name", "Name")
                        .option("date", "Date")
                        .disabled_option("size", "Size")
                        .value("date"),
                ),
        );
        assert!(
            out.contains(r#"role="menuitemcheckbox" name="archived" value="on" checked"#),
            "{out}"
        );
        assert_eq!(out.matches(r#"role="menuitemradio""#).count(), 3, "{out}");
        assert!(out.contains(r#"value="date" checked"#), "{out}");
        assert_eq!(out.matches("checked").count(), 2, "{out}");
        assert!(
            out.contains(r#"role="group" aria-labelledby="m-g1""#),
            "{out}"
        );
        assert!(out.contains(r#"id="m-g1""#), "{out}");
    }

    #[test]
    fn groups_are_named_by_their_heading() {
        let out = html(
            DropdownMenu::new("m", "A")
                .group(MenuGroup::new("Share").item(MenuItem::action("Link")))
                .group(MenuGroup::new("Danger").item(MenuItem::action("Delete"))),
        );
        assert!(out.contains(r#"aria-labelledby="m-g1""#), "{out}");
        assert!(out.contains(r#"aria-labelledby="m-g2""#), "{out}");
    }

    #[test]
    fn a_submenu_is_a_menu_inside_the_menu_opened_by_an_item() {
        let out = html(
            DropdownMenu::new("m", "A").submenu(
                MenuSub::new("Move to")
                    .item(MenuItem::link("Inbox", "/i"))
                    .submenu(MenuSub::new("Deeper").item(MenuItem::link("X", "/x"))),
            ),
        );
        assert!(
            out.contains(r#"aria-haspopup="menu" aria-expanded="false" data-sub-trigger"#),
            "{out}"
        );
        assert!(out.contains(r#"popovertarget="m-s1""#), "{out}");
        assert!(
            out.contains(r#"id="m-s1" popover="auto" role="menu""#),
            "{out}"
        );
        assert!(out.contains(r#"popovertarget="m-s2""#), "{out}");
        assert!(out.contains(r#"data-side="right""#), "{out}");
        // The submenu stands inside its parent, which keeps the parent open.
        let parent_end = out.rfind("</div>").unwrap();
        assert!(out.find(r#"id="m-s1""#).unwrap() < parent_end);
    }

    #[test]
    fn keep_open_is_set_where_it_was_asked() {
        let out = html(
            DropdownMenu::new("m", "A")
                .item(MenuItem::action("One").keep_open())
                .item(MenuItem::action("Two")),
        );
        assert_eq!(out.matches("data-keep-open").count(), 1, "{out}");
        let all = html(
            DropdownMenu::new("m", "A")
                .keep_open()
                .item(MenuItem::action("One")),
        );
        assert!(all.matches("data-keep-open").count() >= 2, "{all}");
    }

    #[test]
    fn side_align_and_the_icon_trigger() {
        let out = html(
            DropdownMenu::icon("m", icons::ELLIPSIS, "More")
                .side(Side::Top)
                .align(PopupAlign::End)
                .size(Size::Sm),
        );
        assert!(out.contains(r#"data-side="top" data-align="end""#), "{out}");
        assert!(out.contains(r#"aria-label="More""#), "{out}");
        assert!(out.contains("data-icon-only"), "{out}");
    }

    #[test]
    fn text_is_escaped_everywhere() {
        let out = html(
            DropdownMenu::new("m", "<i>A</i>")
                .item(MenuItem::action("<script>x</script>").shortcut("<k>"))
                .header("<b>me</b>")
                .group(MenuGroup::new("<u>g</u>"))
                .checkbox(MenuCheckbox::new("n", "<s>c</s>")),
        );
        for bad in ["<i>", "<script>", "<k>", "<b>", "<u>", "<s>"] {
            assert!(!out.contains(bad), "{bad}: {out}");
        }
    }

    #[test]
    fn an_unsafe_link_goes_nowhere() {
        let out =
            html(DropdownMenu::new("m", "A").item(MenuItem::link("x", "javascript:alert(1)")));
        assert!(!out.contains("javascript"), "{out}");
    }
}
