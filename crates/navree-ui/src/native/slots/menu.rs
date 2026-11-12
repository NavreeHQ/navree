//! `DropdownMenu`: the button that opens a menu and the menu it opens, read
//! as one menu.
//!
//! The page writes them side by side: the button names the popup it opens
//! (`popovertarget`), and the popup holds the items. A link item is a link, an
//! action is a button, a post item is a submit button that names the page's
//! one post form (`form="ui-post"`) and where to post (`formaction`): the form
//! holds only the session's token, which the page passes down. A box or a radio
//! button in a menu is a form control a menu of the system has no place for;
//! it is read as a structure no reader knows.

use navree_native::node as n;

use super::super::lower::{Cx, Fail, N, Parts, Read, attr, fail, flat, has, hooks, id, slot, text_of};
use crate::node::{Element, Node};

/// The hooks the popup of a menu carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-menu"];

/// A menu's button together with its popup, when `button` is the one and the
/// popup is among `rest`, the siblings that follow it. `None` for any other
/// element.
pub(crate) fn paired<'a>(
    cx: &mut Cx,
    button: &Element,
    rest: &[&'a Node],
) -> Option<(&'a Element, Read<N>)> {
    if slot(button).as_deref() != Some("button")
        || attr(button, "aria-haspopup").as_deref() != Some("menu")
    {
        return None;
    }
    let target = attr(button, "popovertarget")?;
    let popup = rest.iter().find_map(|node| match node {
        Node::Element(popup)
            if slot(popup).as_deref() == Some("dropdown-menu")
                && id(popup).as_deref() == Some(target.as_ref()) =>
        {
            Some(popup)
        }
        _ => None,
    })?;
    Some((popup, menu(cx, button, popup)))
}

/// A popup with no button of its own to open it.
pub(crate) fn alone(_: &mut Cx, _: &Element) -> Read {
    fail("a menu away from the button that opens it")
}

/// The menu `popup`, opened by `button`.
fn menu(cx: &mut Cx, button: &Element, popup: &Element) -> Read<N> {
    let opener = cx.button(button)?;
    let icon = match opener.icon_only {
        true => opener.icon,
        false => leading_icon(button)?,
    };
    opened(cx, opener.label, icon, popup)
}

/// The menu `popup`, under a button that says `label`.
fn opened(cx: &mut Cx, label: String, icon: Option<n::Icon>, popup: &Element) -> Read<N> {
    hooks(popup, HOOKS)?;
    Ok(N::Menu(n::Menu {
        id: id(popup),
        label,
        icon,
        items: items(cx, &flat(popup.child_nodes()))?,
    }))
}

/// The icon that stands before a button's words: the one after them is the
/// caret every system draws for a menu itself.
fn leading_icon(button: &Element) -> Read<Option<n::Icon>> {
    for child in flat(button.child_nodes()) {
        match child {
            Node::Element(part) if slot(part).as_deref() == Some("icon") => {
                return super::icon(part).map(Some);
            }
            Node::Text(text) if text.trim().is_empty() => {}
            _ => break,
        }
    }
    Ok(None)
}

/// What a menu holds, in order.
fn items(cx: &mut Cx, nodes: &[&Node]) -> Read<Vec<N>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(node) = nodes.get(at) {
        at += 1;
        let element = match node {
            Node::Text(text) if text.trim().is_empty() => continue,
            Node::Element(element) => element,
            _ => return fail("text among the items of a menu"),
        };
        match slot(element).as_deref() {
            Some("menu-separator") => out.push(N::Divider(n::Divider::default())),
            Some("menu-header") => out.extend(cx.of(element)),
            Some("menu-group") => out.push(group(cx, element)?),
            Some("menu-item") if has(element, "data-sub-trigger") => {
                // The submenu it opens stands right after it.
                let target = attr(element, "popovertarget");
                let popup = match nodes.get(at) {
                    Some(Node::Element(popup))
                        if slot(popup).as_deref() == Some("dropdown-menu")
                            && id(popup).as_deref() == target.as_deref() =>
                    {
                        popup
                    }
                    _ => return fail("a submenu away from the item that opens it"),
                };
                at += 1;
                hooks(element, &[])?;
                let (label, icon) = words(cx, element)?;
                out.push(opened(cx, label, icon, popup)?);
            }
            Some("menu-item") => out.push(item(cx, element)?),
            Some(other) => return fail(format!("an unexpected `{other}` in a menu")),
            None => return fail(format!("an unexpected <{}> in a menu", element.tag())),
        }
    }
    Ok(out)
}

/// A group of items under a heading.
fn group(cx: &mut Cx, element: &Element) -> Read<N> {
    hooks(element, &[])?;
    let mut parts = Parts::of(element);
    let title = parts.slot("menu-label").map(text_of);
    let children = items(cx, parts.left())?;
    Ok(N::Section(n::Section {
        id: id(element),
        title,
        children,
        ..n::Section::default()
    }))
}

/// What an item shows: its words, and the icon that stands before them. The
/// key it shows beside them is a hint for a keyboard, and the mark after them
/// (the arrow of a submenu) is every system's own.
fn words(cx: &mut Cx, element: &Element) -> Read<(String, Option<n::Icon>)> {
    let mut label = None;
    let mut icon = None;
    for child in flat(element.child_nodes()) {
        let Node::Element(part) = child else {
            if matches!(child, Node::Text(text) if text.trim().is_empty()) {
                continue;
            }
            return fail("words in a menu item outside its text");
        };
        match slot(part).as_deref() {
            Some("menu-text") => label = Some(text_of(part)),
            Some("menu-shortcut") => cx.dropped.push("menu-shortcut".to_owned()),
            Some("icon") => {
                if label.is_none() && icon.is_none() {
                    icon = Some(super::icon(part)?);
                }
            }
            Some(other) => return fail(format!("a menu item that holds a `{other}`")),
            None => return fail(format!("a menu item that holds a <{}>", part.tag())),
        }
    }
    match label {
        Some(label) => Ok((label, icon)),
        None => fail("a menu item with no words"),
    }
}

/// One item: a button that does what the link, the post or the command says.
fn item(cx: &mut Cx, element: &Element) -> Read<N> {
    hooks(element, &["data-ui-click"])?;
    if element.tag() == "label" {
        return Err(Fail {
            what: None,
            why: "a box or a radio button in a menu".to_owned(),
        });
    }
    let (label, icon) = words(cx, element)?;
    let disabled = attr(element, "aria-disabled").as_deref() == Some("true");
    // A link that is switched off has no address; it does nothing.
    let action = if disabled && element.tag() == "a" {
        None
    } else {
        cx.action_of(element)?
    };
    if action.is_none() && !disabled {
        return fail("a menu item that does nothing");
    }
    Ok(N::Button(n::Button {
        id: id(element),
        label,
        icon,
        role: if has(element, "data-destructive") {
            n::Role::Destructive
        } else {
            n::Role::None
        },
        disabled,
        action,
        ..n::Button::default()
    }))
}

#[cfg(test)]
mod tests {
    use navree_native::action::{Action, Command, Pair};

    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::native::TestNative;
    use crate::{
        DropdownMenu, MenuGroup, MenuItem, MenuRadios, MenuSub, MenuCheckbox, View, icons,
    };

    fn menu_of(node: N) -> n::Menu {
        let N::Menu(menu) = node else {
            panic!("not a menu: {node:?}");
        };
        menu
    }

    fn button_of(node: &N) -> &n::Button {
        let N::Button(button) = node else {
            panic!("not a button: {node:?}");
        };
        button
    }

    #[test]
    fn a_button_and_the_menu_it_opens_are_one_menu_of_buttons() {
        let menu = menu_of(read(
            DropdownMenu::new("acts", "Actions")
                .item(MenuItem::link("Open", "/orders/7").icon(icons::EXTERNAL_LINK))
                .item(MenuItem::action("Refund").opens_dialog("refund"))
                .separator()
                .item(MenuItem::link("Export", "/orders/7/export").shortcut("Ctrl E"))
                .item(MenuItem::link("Pending", "/x").disabled(true)),
        ));
        assert_eq!(menu.id.as_deref(), Some("acts"));
        assert_eq!(menu.label, "Actions");
        assert_eq!(menu.icon, None);
        let [open, refund, divider, export, pending] = menu.items.as_slice() else {
            panic!("not five items: {:?}", menu.items);
        };
        assert_eq!(button_of(open).label, "Open");
        assert_eq!(
            button_of(open)
                .icon
                .as_ref()
                .and_then(|icon| icon.name.as_deref()),
            Some("external-link")
        );
        assert_eq!(button_of(open).action, Some(Action::navigate("/orders/7")));
        assert_eq!(
            button_of(refund).action,
            Some(Action::command(Command::Open {
                id: "refund".into()
            }))
        );
        assert!(matches!(divider, N::Divider(_)));
        assert_eq!(button_of(export).label, "Export");
        assert!(button_of(pending).disabled);
        assert_eq!(button_of(pending).action, None);
    }

    #[test]
    fn a_destructive_item_has_the_destructive_role() {
        let menu = menu_of(read(
            DropdownMenu::new("m", "More")
                .item(MenuItem::post("Delete", "/orders/7/delete").destructive())
                .item(MenuItem::post("Archive", "/orders/7/archive")),
        ));
        assert_eq!(button_of(&menu.items[0]).role, n::Role::Destructive);
        assert_eq!(button_of(&menu.items[1]).role, n::Role::None);
    }

    #[test]
    fn a_post_item_is_a_request_that_carries_the_pages_token() {
        let node =
            || DropdownMenu::new("m", "More").item(MenuItem::post("Archive", "/orders/7/archive"));
        let page = TestNative::of(&View::new("Order").csrf("tok-1").body(node()));
        assert!(page.is_native(), "{:?}", page.why());
        assert_eq!(
            page.tap("Archive"),
            Some(Action::Request {
                method: n::Method::Post,
                href: "/orders/7/archive".into(),
                fields: vec![Pair {
                    name: "_csrf".into(),
                    value: "tok-1".into(),
                }],
            })
        );
        // A page with no token in its post form posts none.
        let page = TestNative::of(&View::new("Order").body(node()));
        assert_eq!(
            page.tap("Archive"),
            Some(Action::Request {
                method: n::Method::Post,
                href: "/orders/7/archive".into(),
                fields: Vec::new(),
            })
        );
    }

    #[test]
    fn a_menu_opened_by_an_icon_has_the_icon_and_a_caret_is_not_one() {
        let menu = menu_of(read(
            DropdownMenu::icon("m", icons::ELLIPSIS, "More actions")
                .item(MenuItem::link("Open", "/x")),
        ));
        assert_eq!(menu.label, "More actions");
        assert_eq!(
            menu.icon.as_ref().and_then(|icon| icon.name.as_deref()),
            Some("ellipsis")
        );
        let menu = menu_of(read(
            DropdownMenu::new("m", "Sort")
                .caret()
                .item(MenuItem::link("Open", "/x")),
        ));
        assert_eq!(menu.icon, None);
        let menu = menu_of(read(
            DropdownMenu::button("m", crate::Button::outline("Filter").icon(icons::FUNNEL))
                .item(MenuItem::link("Open", "/x")),
        ));
        assert_eq!(
            menu.icon.as_ref().and_then(|icon| icon.name.as_deref()),
            Some("funnel")
        );
    }

    #[test]
    fn a_group_is_a_section_and_a_submenu_is_a_menu_in_the_menu() {
        let menu = menu_of(read(
            DropdownMenu::new("m", "Account")
                .header("Gurpreet Sandhu")
                .group(MenuGroup::new("Team").item(MenuItem::link("Members", "/team")))
                .submenu(MenuSub::new("Switch store").item(MenuItem::link("Ludhiana", "/s/1"))),
        ));
        let [header, group, sub] = menu.items.as_slice() else {
            panic!("not three: {:?}", menu.items);
        };
        assert_eq!(*header, N::text("Gurpreet Sandhu"));
        let N::Section(section) = group else {
            panic!("not a section");
        };
        assert_eq!(section.title.as_deref(), Some("Team"));
        assert_eq!(button_of(&section.children[0]).label, "Members");
        let sub = menu_of(sub.clone());
        assert_eq!(sub.label, "Switch store");
        assert_eq!(button_of(&sub.items[0]).label, "Ludhiana");
    }

    #[test]
    fn a_box_or_a_radio_button_in_a_menu_is_a_miss() {
        let boxes: Node = DropdownMenu::new("m", "View")
            .checkbox(MenuCheckbox::new("done", "Show done"))
            .into();
        assert_eq!(misses(boxes), ["slot:dropdown-menu"]);
        let radios: Node = DropdownMenu::new("m", "Sort")
            .radios(MenuRadios::new("sort", "Sort by").option("a", "Date"))
            .into();
        assert_eq!(misses(radios), ["slot:dropdown-menu"]);
    }

    #[test]
    fn a_menu_away_from_its_button_is_a_miss_and_a_button_for_no_menu_is_a_button() {
        let both =
            Node::from(DropdownMenu::new("m", "Actions").item(MenuItem::link("Open", "/x")));
        let Node::Fragment(parts) = &both else {
            panic!("not two nodes");
        };
        // The popup alone, with its button elsewhere.
        assert_eq!(misses(parts[1].clone()), ["slot:dropdown-menu"]);
        // The button alone opens a popover that is not there: a button with a command.
        let N::Button(button) = read(parts[0].clone()) else {
            panic!("not a button");
        };
        assert_eq!(
            button.action,
            Some(Action::command(Command::Open { id: "m".into() }))
        );
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_menu_is_still_a_miss() {
        for name in ["dropdown-menu", "menu-item"] {
            let node: Node = DropdownMenu::new("m", "Actions")
                .item(MenuItem::link("Open", "/x"))
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
