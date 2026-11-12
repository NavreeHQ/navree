//! `Tabs`: the views a reader switches between.
//!
//! Every tab is a link to the same page with its key in the query, and every
//! panel is in the page: the open one shown, the others hidden. A panel that
//! loads the first time its tab is shown is a region with an address. A tab
//! holds its panel, so a client switches without asking the server; a lazy
//! one is fetched when it is first in view.

use super::super::lower::{Cx, N, Parts, Read, attr, elements, fail, flat, has, hooks, id, slot, text_of};
use crate::node::{Element, Node};
use navree_native::node as n;

/// The hooks the root of tabs carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-tabs"];

/// The hooks a lazy panel carries: it is a region.
const PANEL: &[&str] = &["data-ui-region", "data-ui-src", "data-ui-lazy", "data-ui-loaded"];

/// `Tabs`.
pub(crate) fn tabs(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let list = parts.slot("tabs-list");
    let panels = parts.slots("tabs-panel");
    parts.done()?;
    let Some(list) = list else {
        return fail("tabs with no list of tabs");
    };
    hooks(list, &[])?;
    let mut items = Vec::new();
    let mut used = Vec::new();
    for tab in elements(list)? {
        if slot(tab).as_deref() != Some("tab") {
            return fail("something that is not a tab in a list of tabs");
        }
        hooks(tab, &["data-ui-noboost"])?;
        if attr(tab, "aria-disabled").as_deref() == Some("true") {
            return fail("a tab that is switched off");
        }
        let mut item = words(tab)?;
        item.selected = attr(tab, "aria-selected").as_deref() == Some("true");
        let panel = attr(tab, "aria-controls").and_then(|wanted| {
            panels
                .iter()
                .position(|panel| id(panel).as_deref() == Some(wanted.as_ref()))
        });
        let Some(at) = panel else {
            return fail("a tab with no panel");
        };
        used.push(at);
        item.children = panel_of(cx, panels[at])?;
        items.push(item);
    }
    if used.len() != panels.len() {
        return fail("a panel that no tab opens");
    }
    if items.iter().filter(|item| item.selected).count() != 1 {
        return fail("tabs that do not have exactly one open");
    }
    Ok(vec![N::Tabs(n::Tabs {
        id: id(element),
        items,
    })])
}

/// A tab's name, with the count beside it, and its icon.
fn words(tab: &Element) -> Read<n::Tab> {
    let mut label = String::new();
    let mut count = None;
    let mut icon = None;
    for child in flat(tab.child_nodes()) {
        match child {
            Node::Text(text) => {
                label.push_str(text);
                label.push(' ');
            }
            Node::Element(part) => match slot(part).as_deref() {
                Some("icon") => {
                    if icon.is_none() {
                        icon = Some(super::icon(part)?);
                    }
                }
                Some("tab-count") => count = Some(text_of(part)),
                _ => label.push_str(&format!("{} ", text_of(part))),
            },
            _ => {}
        }
    }
    let mut label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.is_empty() {
        label = attr(tab, "aria-label")
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_default();
    }
    if label.is_empty() {
        return fail("a tab with no name");
    }
    if let Some(count) = count {
        label = format!("{label} ({count})");
    }
    Ok(n::Tab {
        label,
        icon,
        ..n::Tab::default()
    })
}

/// A tab's panel: what it holds, or the region that fetches what it holds.
fn panel_of(cx: &mut Cx, panel: &Element) -> Read<Vec<N>> {
    hooks(panel, PANEL)?;
    if !has(panel, "data-ui-region") {
        return Ok(cx.of(panel));
    }
    let (Some(panel_id), Some(src)) = (id(panel), attr(panel, "data-ui-src")) else {
        return fail("a panel that fetches itself with no id or no address");
    };
    // What stands in it while it loads is the browser's.
    let mut parts = Parts::of(panel);
    let _waiting = parts.slots("skeleton");
    let children = cx.flat_sequence(parts.left());
    Ok(vec![N::Region(n::Region {
        id: panel_id,
        src: src.into_owned(),
        // One that came empty is fetched when its tab is first in view.
        lazy: has(panel, "data-ui-lazy") || has(panel, "aria-busy"),
        children,
        ..n::Region::default()
    })])
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::{Tab, Tabs, Text, icons};

    fn tabs_of(node: N) -> n::Tabs {
        let N::Tabs(tabs) = node else {
            panic!("not tabs: {node:?}");
        };
        tabs
    }

    #[test]
    fn tabs_hold_their_panels_and_say_which_is_open() {
        let tabs = tabs_of(read(
            Tabs::new("deal", "Deal sections")
                .tab(Tab::new("overview", "Overview").panel(Text::new("The overview")))
                .tab(
                    Tab::new("notes", "Notes")
                        .icon(icons::MESSAGE_SQUARE)
                        .count("3")
                        .panel(Text::new("Three notes")),
                )
                .active("notes"),
        ));
        assert_eq!(tabs.id.as_deref(), Some("deal"));
        let [overview, notes] = tabs.items.as_slice() else {
            panic!("not two tabs");
        };
        assert_eq!(overview.label, "Overview");
        assert!(!overview.selected);
        assert_eq!(overview.children, [N::text("The overview")]);
        assert_eq!(notes.label, "Notes (3)");
        assert!(notes.selected);
        assert_eq!(
            notes.icon.as_ref().and_then(|icon| icon.name.as_deref()),
            Some("message-square")
        );
        assert_eq!(notes.children, [N::text("Three notes")]);
    }

    #[test]
    fn an_icon_alone_is_named_by_what_the_page_says_it_is() {
        let tabs = tabs_of(read(
            Tabs::new("t", "Views")
                .tab(Tab::icon_only("grid", icons::LAYOUT_GRID, "Grid").panel(Text::new("g"))),
        ));
        assert_eq!(tabs.items[0].label, "Grid");
        assert!(tabs.items[0].icon.is_some());
    }

    #[test]
    fn a_panel_that_loads_late_is_a_region_with_its_address() {
        let tabs = tabs_of(read(
            Tabs::new("deal", "Deal sections")
                .tab(Tab::new("overview", "Overview").panel(Text::new("The overview")))
                .tab(Tab::new("history", "History").lazy("/deals/7/history"))
                .active("overview"),
        ));
        let history = &tabs.items[1];
        let [N::Region(region)] = history.children.as_slice() else {
            panic!("not a region: {:?}", history.children);
        };
        assert_eq!(region.src, "/deals/7/history");
        assert_eq!(region.id, "deal-panel-history");
        assert!(region.lazy);
        assert!(region.children.is_empty());
        // The one open when the page is drawn is fetched with it.
        let tabs = tabs_of(read(
            Tabs::new("deal", "Deal sections")
                .tab(Tab::new("history", "History").lazy("/deals/7/history"))
                .active("history"),
        ));
        let [N::Region(region)] = tabs.items[0].children.as_slice() else {
            panic!("not a region");
        };
        assert!(region.lazy);
    }

    #[test]
    fn a_tab_that_is_switched_off_or_fetched_every_time_it_shows_is_a_miss() {
        let off: crate::node::Node = Tabs::new("t", "Views")
            .tab(Tab::new("a", "A").panel(Text::new("a")))
            .tab(Tab::new("b", "B").disabled(true).panel(Text::new("b")))
            .into();
        assert_eq!(misses(off), ["slot:tabs"]);
        let fresh: crate::node::Node = Tabs::new("t", "Views")
            .tab(Tab::new("a", "A").lazy("/a").fresh())
            .into();
        assert_eq!(misses(fresh), ["hook:data-ui-fresh"]);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_tabs_is_still_a_miss() {
        for name in ["tabs", "tab", "tabs-panel"] {
            let node: crate::node::Node = Tabs::new("t", "Views")
                .tab(Tab::new("a", "A").panel(Text::new("a")))
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
