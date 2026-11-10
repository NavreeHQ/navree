//! Menubar: a row of menu buttons, as in a desktop application. Each button opens a menu
//! with the entries of a dropdown menu. The `menubar` module gives the bar the keys of the
//! menu bar pattern (one tab stop, Left and Right between buttons, Down to open) and moves
//! from one open menu to the next; the `menus` module gives each menu the keys of a menu.
//! With no script every button still opens its menu, as a popover.

use super::dropdown::{Entry, menu_element, render_entries};
use super::{MenuCheckbox, MenuGroup, MenuItem, MenuRadios, MenuSub, PopupAlign};
use crate::html::Href;
use crate::node::{Node, Uses, el, into_node};

/// One menu of a [`Menubar`]: a button and the entries it opens.
#[derive(Clone, Debug)]
#[must_use]
pub struct MenubarMenu {
    label: Node,
    entries: Vec<Entry>,
    disabled: bool,
    keep_open: bool,
}

impl MenubarMenu {
    /// A menu opened by a button reading `label`.
    pub fn new(label: impl Into<Node>) -> Self {
        Self {
            label: label.into(),
            entries: Vec::new(),
            disabled: false,
            keep_open: false,
        }
    }

    /// Disables it: the button stays in the bar, the arrow keys still reach it, and it
    /// opens nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Leaves the menu open when any item is chosen.
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
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
}

#[derive(Clone, Debug)]
enum Part {
    Menu(MenubarMenu),
    Link { label: Node, href: Href },
}

/// A bar of menus.
///
/// ```
/// use navree_ui::{MenuItem, Menubar, MenubarMenu, render};
///
/// let bar = Menubar::new("app-bar", "Application")
///     .menu(MenubarMenu::new("File").item(MenuItem::link("New invoice", "/invoices/new")))
///     .link("Help", "/help");
/// let html = render(&bar.into());
/// assert!(html.contains(r#"role="menubar""#));
/// assert!(html.contains(r#"aria-haspopup="menu""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Menubar {
    id: String,
    label: String,
    parts: Vec<Part>,
}

impl Menubar {
    /// A menu bar with the id `id`, named `label` for assistive technology. The id also
    /// starts the ids of its menus.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            parts: Vec::new(),
        }
    }

    /// Adds a menu.
    pub fn menu(mut self, menu: MenubarMenu) -> Self {
        self.parts.push(Part::Menu(menu));
        self
    }

    /// Adds a button in the bar that has no menu: a link to a page.
    pub fn link(mut self, label: impl Into<Node>, href: impl Into<Href>) -> Self {
        self.parts.push(Part::Link {
            label: label.into(),
            href: href.into(),
        });
        self
    }

    fn into_node(self) -> Node {
        let mut items: Vec<Node> = Vec::with_capacity(self.parts.len());
        let mut menus = 0_usize;
        for part in &self.parts {
            let wrapper = el("div").slot("menubar-item").attr("role", "none");
            match part {
                Part::Link { label, href } => items.push(
                    wrapper
                        .child(
                            el("a")
                                .slot("menubar-trigger")
                                .attr("role", "menuitem")
                                .href(href.clone())
                                .child(label.clone()),
                        )
                        .into(),
                ),
                Part::Menu(menu) => {
                    menus += 1;
                    let popup_id = format!("{}-m{menus}", self.id);
                    let trigger_id = format!("{popup_id}-trigger");
                    let trigger = el("button")
                        .slot("menubar-trigger")
                        .attr("type", "button")
                        .attr("role", "menuitem")
                        .attr("id", trigger_id.clone());
                    if menu.disabled {
                        items.push(
                            wrapper
                                .child(
                                    trigger
                                        .attr("aria-disabled", "true")
                                        .flag("data-disabled", true)
                                        .child(menu.label.clone()),
                                )
                                .into(),
                        );
                        continue;
                    }
                    let (entries, posts) = render_entries(&popup_id, &menu.entries, menu.keep_open);
                    let trigger = trigger
                        .attr("popovertarget", popup_id.clone())
                        .attr("aria-haspopup", "menu")
                        .attr("aria-expanded", "false")
                        // The module keeps it true while the menu is open, through a morph.
                        .attr("data-ui-keep-attrs", "aria-expanded")
                        .child(menu.label.clone());
                    let mut popup = menu_element(
                        &popup_id,
                        &trigger_id,
                        None,
                        "bottom",
                        PopupAlign::Start,
                        false,
                    )
                    .flag("data-keep-open", menu.keep_open)
                    .child(entries);
                    if posts {
                        popup = popup.uses(Uses::CORE);
                    }
                    items.push(wrapper.child(Node::list((trigger, popup))).into());
                }
            }
        }
        el("div")
            .slot("menubar")
            .attr("role", "menubar")
            .attr("aria-label", self.label)
            .attr("id", self.id)
            .flag("data-ui-menubar", true)
            .uses(Uses::MENUBAR)
            .child(items)
            .into()
    }
}

into_node!(Menubar);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    #[test]
    fn the_bar_holds_menu_buttons_and_each_menu_follows_its_button() {
        let html = render(
            &Menubar::new("b", "App")
                .menu(MenubarMenu::new("File").item(MenuItem::link("New", "/n")))
                .into(),
        );
        assert!(
            html.contains(r#"role="menubar" aria-label="App""#),
            "{html}"
        );
        let button = html.find(r#"id="b-m1-trigger""#).expect("the button");
        let menu = html.find(r#"id="b-m1" popover="auto""#).expect("the menu");
        assert!(button < menu, "{html}");
    }

    #[test]
    fn the_bar_asks_for_its_module_and_the_menus_and_overlay() {
        let node: Node = Menubar::new("b", "App")
            .menu(MenubarMenu::new("File").item(MenuItem::link("New", "/n")))
            .into();
        let uses = crate::render::measure(&node).uses;
        for module in [Uses::MENUBAR, Uses::MENUS, Uses::OVERLAY] {
            assert!(uses.has(module));
        }
        assert!(!uses.has(Uses::CORE));
    }
}
