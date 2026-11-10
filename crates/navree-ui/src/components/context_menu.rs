//! ContextMenu: a region that opens a menu where it is pressed. A right click, a long
//! press on a touch screen, and Shift+F10 or the menu key on the keyboard open the menu at
//! that place; the `menubar` module does that, and the `menus` module gives the menu the
//! keys of a menu. With no script the browser's own menu stays, so offer the same actions
//! somewhere else too.

use super::dropdown::{Entry, render_entries};
use super::{MenuCheckbox, MenuGroup, MenuItem, MenuRadios, MenuSub};
use crate::node::{IntoNodes, Node, Uses, el, into_node};

/// A region and the menu its right click opens.
///
/// The menu has the entries of a [`DropdownMenu`](crate::DropdownMenu): items that link,
/// post or run commands, checkboxes, radios, groups, submenus, shortcuts hints, and
/// destructive and disabled items. A page with no script keeps the browser's menu, so a
/// context menu never holds the only way to do something.
///
/// ```
/// use navree_ui::{ContextMenu, MenuItem, render};
///
/// let menu = ContextMenu::new("order-menu", "Order actions")
///     .content("Order 2041 for Gurpreet Singh, Ludhiana")
///     .item(MenuItem::link("Open order", "/orders/2041"));
/// let html = render(&menu.into());
/// assert!(html.contains(r#"data-ui-context="order-menu""#));
/// assert!(html.contains(r#"role="menu""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ContextMenu {
    id: String,
    label: String,
    content: Vec<Node>,
    entries: Vec<Entry>,
    focusable: bool,
    keep_open: bool,
}

impl ContextMenu {
    /// A context menu with the id `id`, named `label` for assistive technology.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            content: Vec::new(),
            entries: Vec::new(),
            focusable: false,
            keep_open: false,
        }
    }

    /// Adds what the right click opens the menu on: any content.
    pub fn content(mut self, content: impl IntoNodes) -> Self {
        content.push_into(&mut self.content);
        self
    }

    /// Lets the keyboard reach the region: it takes one tab stop, and Shift+F10 or the menu
    /// key opens the menu from it. Leave it off when the region holds links, buttons or
    /// fields of its own: the keys work from any of them.
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
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

    fn into_node(self) -> Node {
        let (items, posts) = render_entries(&self.id, &self.entries, self.keep_open);
        let mut region = el("div")
            .slot("context-menu")
            .attr("data-ui-context", self.id.clone())
            .uses(Uses::MENUBAR);
        if self.focusable {
            region = region
                .attr("tabindex", 0_i64)
                .attr("role", "group")
                .attr("aria-label", self.label.clone());
        }
        // A manual popover: the browser closes an automatic one on the mouse-up that follows
        // a right click on some systems. The module closes this one on Escape, on a press
        // outside, on Tab, on a scroll and when a choice is made.
        let mut menu = el("div")
            .slot("dropdown-menu")
            .attr("id", self.id.clone())
            .attr("popover", "manual")
            .attr("role", "menu")
            .attr("tabindex", -1_i64)
            .attr("aria-label", self.label)
            .flag("data-ui-menu", true)
            .flag("data-ui-context-menu", true)
            .flag("data-keep-open", self.keep_open)
            .uses(Uses::MENUS.with(Uses::MENUBAR))
            .child(items);
        if posts {
            menu = menu.uses(Uses::CORE);
        }
        Node::list((region.child(self.content), menu))
    }
}

into_node!(ContextMenu);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    #[test]
    fn the_region_names_its_menu_and_the_menu_is_a_manual_popover() {
        let html = render(
            &ContextMenu::new("m", "Actions")
                .content("Area")
                .item(MenuItem::link("Open", "/o"))
                .into(),
        );
        assert!(
            html.starts_with(r#"<div data-slot="context-menu" data-ui-context="m">Area</div>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"id="m" popover="manual" role="menu""#),
            "{html}"
        );
    }

    #[test]
    fn text_is_escaped() {
        let html = render(
            &ContextMenu::new("m", "<b>x</b>")
                .content("<i>y</i>")
                .item(MenuItem::action("<u>z</u>"))
                .into(),
        );
        for bad in ["<b>", "<i>", "<u>"] {
            assert!(!html.contains(bad), "{bad}: {html}");
        }
    }
}
