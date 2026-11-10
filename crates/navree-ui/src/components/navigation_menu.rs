//! NavigationMenu: the links of a site, some of them opening panels of more links. It is
//! the disclosure navigation pattern, not a menu: a button shows and hides a panel, and
//! the panel holds ordinary links that Tab walks through. Each panel is a popover, so it
//! opens with no script; the `menubar` module adds opening on hover, closing when focus
//! leaves, and the arrow keys between the top-level items.

use super::{PopupAlign, icon};
use crate::html::Href;
use crate::icons;
use crate::node::{Node, Uses, el, into_node};

/// A link in a panel of a [`NavigationMenu`]: its name, an optional line about where it
/// goes, and whether it is the page the reader is on.
#[derive(Clone, Debug)]
#[must_use]
pub struct NavMenuLink {
    label: Node,
    href: Href,
    description: Option<Node>,
    current: bool,
}

impl NavMenuLink {
    /// A link reading `label` to `href`.
    pub fn new(label: impl Into<Node>, href: impl Into<Href>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            description: None,
            current: false,
        }
    }

    /// Adds a short line under the name.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Marks it as the page the reader is on.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

#[derive(Clone, Debug)]
enum Kind {
    Link {
        href: Href,
        current: bool,
    },
    Panel {
        entries: Vec<NavMenuLink>,
        featured: Option<Node>,
    },
}

/// One top-level item of a [`NavigationMenu`]: a link, or a button that opens a panel of
/// links.
#[derive(Clone, Debug)]
#[must_use]
pub struct NavMenuItem {
    label: Node,
    kind: Kind,
}

impl NavMenuItem {
    /// A link reading `label` to `href`.
    pub fn link(label: impl Into<Node>, href: impl Into<Href>) -> Self {
        Self {
            label: label.into(),
            kind: Kind::Link {
                href: href.into(),
                current: false,
            },
        }
    }

    /// A button reading `label` that opens a panel. Add its links with
    /// [`entry`](Self::entry).
    pub fn panel(label: impl Into<Node>) -> Self {
        Self {
            label: label.into(),
            kind: Kind::Panel {
                entries: Vec::new(),
                featured: None,
            },
        }
    }

    /// Marks a link as the page the reader is on. A panel is marked by its own current
    /// link, so this changes nothing on one.
    pub fn current(mut self, current: bool) -> Self {
        if let Kind::Link { current: on, .. } = &mut self.kind {
            *on = current;
        }
        self
    }

    /// Adds a link to a panel. A plain link takes none.
    pub fn entry(mut self, link: NavMenuLink) -> Self {
        if let Kind::Panel { entries, .. } = &mut self.kind {
            entries.push(link);
        }
        self
    }

    /// Adds a block of any content before the links of a panel: a promotion or a summary.
    pub fn featured(mut self, content: impl Into<Node>) -> Self {
        if let Kind::Panel { featured, .. } = &mut self.kind {
            *featured = Some(content.into());
        }
        self
    }
}

/// The links of a site, with panels of more links.
///
/// ```
/// use navree_ui::{NavMenuItem, NavMenuLink, NavigationMenu, render};
///
/// let menu = NavigationMenu::new("site-nav", "Main")
///     .item(
///         NavMenuItem::panel("Products")
///             .entry(NavMenuLink::new("Invoices", "/invoices").description("GST invoices"))
///             .entry(NavMenuLink::new("Payments", "/payments")),
///     )
///     .item(NavMenuItem::link("Pricing", "/pricing"));
/// let html = render(&menu.into());
/// assert!(html.starts_with("<nav"));
/// assert!(html.contains(r#"popovertarget="site-nav-p1""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct NavigationMenu {
    id: String,
    label: String,
    items: Vec<NavMenuItem>,
}

impl NavigationMenu {
    /// A navigation menu with the id `id`, named `label` for assistive technology. The id
    /// also starts the ids of its panels.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items: Vec::new(),
        }
    }

    /// Adds an item.
    pub fn item(mut self, item: NavMenuItem) -> Self {
        self.items.push(item);
        self
    }

    fn into_node(self) -> Node {
        let mut panels = 0_usize;
        let items: Vec<Node> = self
            .items
            .iter()
            .map(|item| -> Node {
                let slot = el("li").slot("navigation-menu-item");
                match &item.kind {
                    Kind::Link { href, current } => slot
                        .child(
                            el("a")
                                .slot("navigation-menu-trigger")
                                .href(href.clone())
                                .attr_opt("aria-current", current.then_some("page"))
                                .child(item.label.clone()),
                        )
                        .into(),
                    Kind::Panel { entries, featured } => {
                        panels += 1;
                        let panel_id = format!("{}-p{panels}", self.id);
                        let active = entries.iter().any(|entry| entry.current);
                        let links: Vec<Node> = entries.iter().map(link_node).collect();
                        let trigger = el("button")
                            .slot("navigation-menu-trigger")
                            .attr("type", "button")
                            .attr("id", format!("{panel_id}-trigger"))
                            .attr("popovertarget", panel_id.clone())
                            .attr("aria-controls", panel_id.clone())
                            // The module writes aria-expanded when it runs and keeps it right
                            // through a morph; with no script the browser's own state for a
                            // button that opens a popover is the true one.
                            .attr("data-ui-keep-attrs", "aria-expanded")
                            .flag("data-active", active)
                            .children((item.label.clone(), icon(icons::CHEVRON_DOWN)));
                        let panel = el("div")
                            .slot("navigation-menu-content")
                            .attr("id", panel_id)
                            .attr("popover", "auto")
                            .flag("data-ui-place", true)
                            .attr("data-side", "bottom")
                            .attr_opt("data-align", PopupAlign::Start.as_str())
                            .uses(Uses::OVERLAY)
                            .children((
                                featured.clone().map(|content| {
                                    el("div").slot("navigation-menu-featured").child(content)
                                }),
                                el("ul").slot("navigation-menu-links").child(links),
                            ));
                        slot.child(Node::list((trigger, panel))).into()
                    }
                }
            })
            .collect();
        el("nav")
            .slot("navigation-menu")
            .attr("aria-label", self.label)
            .attr("id", self.id)
            .flag("data-ui-navmenu", true)
            .uses(Uses::MENUBAR)
            .child(el("ul").slot("navigation-menu-list").child(items))
            .into()
    }
}

/// One link of a panel, in its list item.
fn link_node(link: &NavMenuLink) -> Node {
    el("li")
        .child(
            el("a")
                .slot("navigation-menu-link")
                .href(link.href.clone())
                .attr_opt("aria-current", link.current.then_some("page"))
                .children((
                    el("span")
                        .slot("navigation-menu-link-title")
                        .child(link.label.clone()),
                    link.description.clone().map(|text| {
                        el("span")
                            .slot("navigation-menu-link-description")
                            .child(text)
                    }),
                )),
        )
        .into()
}

into_node!(NavigationMenu);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn menu() -> NavigationMenu {
        NavigationMenu::new("n", "Main")
            .item(
                NavMenuItem::panel("Products")
                    .entry(NavMenuLink::new("Invoices", "/invoices").current(true))
                    .entry(NavMenuLink::new("Payments", "/payments")),
            )
            .item(NavMenuItem::link("Pricing", "/pricing"))
    }

    #[test]
    fn a_panel_button_names_its_panel_and_the_panel_is_a_popover() {
        let html = render(&menu().into());
        assert!(html.contains(r#"popovertarget="n-p1""#), "{html}");
        assert!(html.contains(r#"aria-controls="n-p1""#), "{html}");
        assert!(html.contains(r#"id="n-p1" popover="auto""#), "{html}");
        assert!(!html.contains("role=\"menu\""), "{html}");
    }

    #[test]
    fn a_link_is_current_only_when_it_says_so() {
        let html = render(&menu().into());
        assert_eq!(html.matches(r#"aria-current="page""#).count(), 1, "{html}");
        let again = render(
            &menu()
                .item(NavMenuItem::link("Docs", "/docs").current(true))
                .into(),
        );
        assert_eq!(
            again.matches(r#"aria-current="page""#).count(),
            2,
            "{again}"
        );
    }

    #[test]
    fn a_link_item_takes_no_entries_and_a_panel_ignores_current() {
        let html = render(
            &NavigationMenu::new("n", "Main")
                .item(NavMenuItem::link("Home", "/").entry(NavMenuLink::new("X", "/x")))
                .item(NavMenuItem::panel("P").current(true).featured("F"))
                .into(),
        );
        assert!(!html.contains(r#"href="/x""#), "{html}");
        assert!(!html.contains("aria-current"), "{html}");
        assert!(html.contains(">F<"), "{html}");
    }
}
