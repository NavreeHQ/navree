//! A whole page read into a [`Screen`], and the app's shell read into the
//! [`Shell`](navree_native::Shell) document.
//!
//! On the web the app's shell is drawn around every page. A native client
//! draws the shell itself, once, so a page's screen is what stands inside it:
//! the title, the way back from the breadcrumb, the actions beside the
//! heading, and the content. A toast left for the page comes with it as an op.

use std::borrow::Cow;

use navree_native::action::{Action, Pair};
use navree_native::node as n;
use navree_native::shell as s;
use navree_native::{Op, Screen};

use super::lower::{
    Cx, Lowered, Miss, N, Parts, attr, flat, has, id, label_of, one, slot, text_of, tone_of,
};
use super::slots;
use crate::node::{Element, Node};

/// A page as a native client gets it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// The page as a screen. Where a part could not be read stands a `web` node.
    pub screen: Screen,
    /// What comes with the screen: a toast left for it by the request before.
    pub ops: Vec<Op>,
    /// What could not be read. When this is empty the screen is whole.
    pub misses: Vec<Miss>,
    /// Decoration that was left out.
    pub dropped: Vec<String>,
}

impl Page {
    /// Whether every part of the page was read.
    pub fn is_native(&self) -> bool {
        self.misses.is_empty()
    }

    /// What could not be read, each thing once.
    pub fn why(&self) -> Vec<String> {
        let mut why: Vec<String> = Vec::new();
        for miss in &self.misses {
            if !why.contains(&miss.what) {
                why.push(miss.what.clone());
            }
        }
        why
    }
}

/// The first element under `nodes`, at any depth, whose slot is `name`.
pub(crate) fn find<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Element> {
    for node in nodes {
        match node {
            Node::Element(element) => {
                if slot(element).as_deref() == Some(name) {
                    return Some(element);
                }
                if let Some(found) = find(element.child_nodes(), name) {
                    return Some(found);
                }
            }
            Node::Fragment(nodes) => {
                if let Some(found) = find(nodes, name) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// Reads a page titled `title` whose content is `body`.
pub(crate) fn read(title: &str, body: &Node, keep_markup: bool) -> Page {
    let mut cx = Cx::new(keep_markup);
    let mut screen = Screen::new(title, N::Stack(n::Stack::default()));
    let mut ops = Vec::new();
    let mut content: Vec<N> = Vec::new();
    for node in flat(std::slice::from_ref(body)) {
        match node {
            Node::Element(element) if slot(element).as_deref() == Some("sidebar-wrapper") => {
                in_shell(&mut cx, element, &mut screen, &mut content);
            }
            Node::Element(element) if slot(element).as_deref() == Some("toaster") => {
                toasts(element, &mut ops);
            }
            other => content.extend(cx.flat_sequence(&[other])),
        }
    }
    // A page that is one part with an id is that part: an op that changes the
    // whole page names it.
    screen.id = match content.as_slice() {
        [only] => only.id().map(str::to_owned),
        _ => None,
    };
    screen.body = one(content);
    let Lowered {
        misses, dropped, ..
    } = cx.finish(Vec::new());
    Page {
        screen,
        ops,
        misses,
        dropped,
    }
}

/// A page drawn in the app's shell: what stands in `<main>` is the screen, and
/// the shell around it says where the screen is in the app.
fn in_shell(cx: &mut Cx, wrapper: &Element, screen: &mut Screen, content: &mut Vec<N>) {
    let children = wrapper.child_nodes();
    // The destination this page belongs to, as the sidebar or the top bar marks it.
    screen.nav = current(children);
    let crumbs = find(children, "breadcrumb").and_then(|crumbs| slots::trail(crumbs).ok());
    if let Some(trail) = crumbs {
        screen.back = trail.steps.last().cloned();
        if trail.steps.len() > 1 {
            screen.trail = trail.steps;
        }
    }
    let Some(main) = find(children, "page") else {
        let web = cx.miss(
            "slot:sidebar-wrapper".to_owned(),
            "a shell with no page in it".to_owned(),
            wrapper,
        );
        content.push(web);
        return;
    };
    let mut parts = Parts::of(main);
    // With the navigation in a top bar, the trail stands over the heading.
    let _crumbs = parts.slot("breadcrumb");
    if let Some(header) = parts.slot("page-header") {
        let mut header = Parts::of(header);
        if let Some(title) = header.slot("page-title") {
            screen.title = text_of(title);
        }
        if let Some(actions) = header.slot("page-actions") {
            screen.actions = cx.of(actions);
        }
        let rest = cx.flat_sequence(header.left());
        content.extend(rest);
    }
    content.extend(cx.flat_sequence(parts.left()));
}

/// The address of the shell's link that is marked as the current page.
fn current(nodes: &[Node]) -> Option<String> {
    for node in nodes {
        match node {
            Node::Element(element) => {
                if element.tag() == "a"
                    && matches!(
                        slot(element).as_deref(),
                        Some("sidebar-item" | "sidebar-sub-item" | "topnav-item" | "menu-link")
                    )
                    && attr(element, "aria-current").as_deref() == Some("page")
                {
                    return attr(element, "href").map(Cow::into_owned);
                }
                // The page's own content marks nothing of the shell.
                if slot(element).as_deref() == Some("page") {
                    continue;
                }
                if let Some(found) = current(element.child_nodes()) {
                    return Some(found);
                }
            }
            Node::Fragment(nodes) => {
                if let Some(found) = current(nodes) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// The toasts a page was drawn with, as ops.
fn toasts(toaster: &Element, ops: &mut Vec<Op>) {
    for child in flat(toaster.child_nodes()) {
        let Node::Element(toast) = child else {
            continue;
        };
        if slot(toast).as_deref() != Some("toast") {
            continue;
        }
        let title = find(toast.child_nodes(), "toast-title").map(text_of);
        let text = find(toast.child_nodes(), "toast-text")
            .map(text_of)
            .unwrap_or_default();
        ops.push(Op::Toast(navree_native::op::Toast {
            tone: tone_of(toast).unwrap_or_default(),
            text,
            title,
            id: attr(toast, "data-toast-id").map(Cow::into_owned),
            action: None,
        }));
    }
}

/// Reads the shell document from a page drawn in the app's shell: its name,
/// its destinations and the person signed in. `None` when `body` is not such
/// a page.
pub(crate) fn shell(build: &str, body: &Node) -> Option<navree_native::Shell> {
    let top = std::slice::from_ref(body);
    let wrapper = find(top, "sidebar-wrapper")?;
    let children = wrapper.child_nodes();
    let brand = find(children, "sidebar-brand");
    let name = find(children, "sidebar-brand-name")
        .map(text_of)
        .or_else(|| brand.and_then(|brand| attr(brand, "title").map(Cow::into_owned)))
        .unwrap_or_default();
    let mut shell = navree_native::Shell::new(build, name);
    shell.context = find(children, "sidebar-brand-context").map(text_of);
    if let Some(home) = brand.and_then(|brand| attr(brand, "href")) {
        shell.home = home.into_owned();
    }
    let sidebar = find(children, "sidebar-content")?;
    for group in flat(sidebar.child_nodes()) {
        let Node::Element(group) = group else {
            continue;
        };
        if slot(group).as_deref() != Some("sidebar-group") {
            continue;
        }
        let mut parts = Parts::of(group);
        let label = parts.slot("sidebar-group-label").map(text_of);
        let items = parts
            .slot("sidebar-menu")
            .map(nav_items)
            .unwrap_or_default();
        shell.nav.groups.push(s::NavGroup { label, items });
    }
    if let Some(user) = find(children, "sidebar-user") {
        let mut menu = Vec::new();
        if let Some(popover) = children_after(children, user) {
            user_menu(popover, &mut menu);
        }
        shell.user = Some(s::User {
            name: find(user.child_nodes(), "sidebar-user-name")
                .map(text_of)
                .unwrap_or_default(),
            email: find(user.child_nodes(), "sidebar-user-email").map(text_of),
            menu,
        });
    }
    Some(shell)
}

/// The links of one list of the sidebar, with the links under each.
fn nav_items(list: &Element) -> Vec<s::NavItem> {
    let mut items = Vec::new();
    for entry in flat(list.child_nodes()) {
        if let Node::Element(entry) = entry {
            nav_entry(entry, &mut items);
        }
    }
    items
}

/// One entry of the sidebar: its link, and the list under it. An entry that
/// wraps another is read through.
fn nav_entry(entry: &Element, items: &mut Vec<s::NavItem>) {
    let mut item = None;
    let mut under = Vec::new();
    for part in flat(entry.child_nodes()) {
        let Node::Element(part) = part else { continue };
        match slot(part).as_deref() {
            Some("sidebar-item" | "sidebar-sub-item") => {
                let mut link = Parts::of(part);
                let icon = link.slot("icon").and_then(|icon| slots::icon(icon).ok());
                let label = link
                    .slot("sidebar-label")
                    .map(text_of)
                    .unwrap_or_else(|| label_of(part));
                item = Some(s::NavItem {
                    label,
                    icon,
                    href: attr(part, "href").map(Cow::into_owned).unwrap_or_default(),
                    badge: link.slot("sidebar-count").map(text_of),
                    items: Vec::new(),
                });
            }
            Some("sidebar-sub") => under = nav_items(part),
            None if part.tag() == "li" => nav_entry(part, items),
            _ => {}
        }
    }
    if let Some(mut item) = item {
        item.items = under;
        items.push(item);
    }
}

/// The popover that follows the user's button: the element with the id the
/// button's `popovertarget` names.
fn children_after<'a>(nodes: &'a [Node], button: &Element) -> Option<&'a Element> {
    let target = attr(button, "popovertarget")?;
    fn by_id<'a>(nodes: &'a [Node], wanted: &str) -> Option<&'a Element> {
        for node in nodes {
            match node {
                Node::Element(element) => {
                    if id(element).as_deref() == Some(wanted) {
                        return Some(element);
                    }
                    if let Some(found) = by_id(element.child_nodes(), wanted) {
                        return Some(found);
                    }
                }
                Node::Fragment(nodes) => {
                    if let Some(found) = by_id(nodes, wanted) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    by_id(nodes, &target)
}

/// The user's menu: its links, and its forms of one button as buttons that post.
fn user_menu(popover: &Element, menu: &mut Vec<N>) {
    for entry in flat(popover.child_nodes()) {
        let Node::Element(entry) = entry else {
            continue;
        };
        match entry.tag() {
            "a" => {
                if let Some(href) = attr(entry, "href") {
                    menu.push(N::Link(n::Link {
                        id: None,
                        label: label_of(entry),
                        action: Action::navigate(href.into_owned()),
                        children: Vec::new(),
                    }));
                }
            }
            "form" => {
                let mut fields = Vec::new();
                let mut label = String::new();
                for part in flat(entry.child_nodes()) {
                    let Node::Element(part) = part else { continue };
                    if part.tag() == "input" {
                        fields.push(Pair {
                            name: attr(part, "name").map(Cow::into_owned).unwrap_or_default(),
                            value: attr(part, "value").map(Cow::into_owned).unwrap_or_default(),
                        });
                    } else if part.tag() == "button" {
                        label = label_of(part);
                    }
                }
                menu.push(N::Button(n::Button {
                    label,
                    action: Some(Action::Request {
                        method: if attr(entry, "method").as_deref() == Some("post") {
                            n::Method::Post
                        } else {
                            n::Method::Get
                        },
                        href: attr(entry, "action")
                            .map(Cow::into_owned)
                            .unwrap_or_default(),
                        fields,
                    }),
                    ..n::Button::default()
                }));
            }
            _ if has(entry, "data-slot") => user_menu(entry, menu),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_native::Back;

    use super::*;
    use crate::{Button, NavGroup, NavItem, Shell, Text, ToastTone, UserMenu, View, icons};

    fn app() -> Shell {
        Shell::new("Charkha Supply Co.")
            .context("Store operations")
            .group(
                NavGroup::new("Store")
                    .item(NavItem::new("Dashboard", "/").icon(icons::LAYOUT_DASHBOARD))
                    .item(
                        NavItem::new("Orders", "/orders")
                            .icon(icons::SHOPPING_BAG)
                            .count("12")
                            .sub(NavItem::new("Returns", "/orders/returns")),
                    ),
            )
            .group(NavGroup::plain().item(NavItem::new("Inventory", "/inventory")))
            .user(
                UserMenu::new("Ravneet Kaur", "ravneet.kaur@example.com")
                    .link("Profile", "/profile")
                    .action("Sign out", "/sign-out"),
            )
    }

    #[test]
    fn a_page_in_the_shell_is_the_screen_inside_it() {
        let view = app()
            .page("Order CS-20481", "/orders/1")
            .crumb("Dashboard", "/")
            .crumb("Orders", "/orders")
            .actions(Button::primary("Refund").href("/orders/1/refund"))
            .body(crate::el("div").id("order-page").child(Text::new("Paid")))
            .to_view();
        let page = read("ignored", view.body_node(), false);
        assert!(page.is_native(), "{:?}", page.misses);
        assert_eq!(page.screen.title, "Order CS-20481");
        assert_eq!(page.screen.nav.as_deref(), Some("/orders"));
        assert_eq!(
            page.screen.back,
            Some(Back {
                title: "Orders".into(),
                href: "/orders".into()
            })
        );
        assert_eq!(page.screen.trail.len(), 2);
        assert_eq!(page.screen.trail[0].title, "Dashboard");
        let [N::Button(refund)] = page.screen.actions.as_slice() else {
            panic!("not one action: {:?}", page.screen.actions);
        };
        assert_eq!(refund.role, n::Role::Primary);
        assert_eq!(page.screen.id.as_deref(), Some("order-page"));
        assert_eq!(
            page.screen.body,
            N::Text(n::Text {
                id: Some("order-page".into()),
                value: "Paid".into(),
                ..n::Text::default()
            })
        );
        assert!(page.ops.is_empty());
    }

    #[test]
    fn a_toast_left_for_the_page_comes_with_it_as_an_op() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            crate::shell::flash_cookie_toned(ToastTone::Success, "Restocked")
                .unwrap()
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        let view = app()
            .for_request("/inventory", &headers)
            .page("Inventory")
            .body(Text::new("x"))
            .to_view();
        let page = read("Inventory", view.body_node(), false);
        assert_eq!(
            page.ops,
            [Op::Toast(navree_native::op::Toast {
                tone: n::Tone::Success,
                text: "Restocked".into(),
                ..navree_native::op::Toast::default()
            })]
        );
    }

    #[test]
    fn a_page_with_no_shell_is_its_title_and_its_content() {
        let view = View::new("Figures").body((Text::new("a"), Text::new("b")));
        let page = read("Figures", view.body_node(), false);
        assert_eq!(page.screen.title, "Figures");
        assert_eq!(page.screen.back, None);
        assert_eq!(page.screen.nav, None);
        assert_eq!(
            page.screen.body,
            N::Stack(n::Stack {
                children: vec![N::text("a"), N::text("b")],
                ..n::Stack::default()
            })
        );
    }

    #[test]
    fn what_a_page_holds_that_can_not_be_read_is_named() {
        let view = app()
            .page("Board", "/board")
            .body(crate::el("canvas"))
            .to_view();
        let page = read("Board", view.body_node(), false);
        assert_eq!(page.why(), ["tag:canvas"]);
    }

    #[test]
    fn the_shell_document_is_read_from_the_shell_a_page_is_drawn_in() {
        let view = app()
            .for_request("/", &http::HeaderMap::new())
            .csrf("tok")
            .page("x")
            .to_view();
        let shell = super::shell("b1", view.body_node()).unwrap();
        assert_eq!(shell.name, "Charkha Supply Co.");
        assert_eq!(shell.context.as_deref(), Some("Store operations"));
        assert_eq!(shell.home, "/");
        assert_eq!(shell.build, "b1");
        let [store, plain] = shell.nav.groups.as_slice() else {
            panic!("not two groups");
        };
        assert_eq!(store.label.as_deref(), Some("Store"));
        assert_eq!(plain.label, None);
        let [dashboard, orders] = store.items.as_slice() else {
            panic!("not two items");
        };
        assert_eq!(dashboard.label, "Dashboard");
        assert_eq!(dashboard.href, "/");
        assert_eq!(
            dashboard.icon.as_ref().unwrap().name.as_deref(),
            Some("layout-dashboard")
        );
        assert_eq!(orders.badge.as_deref(), Some("12"));
        assert_eq!(orders.items.len(), 1);
        assert_eq!(orders.items[0].href, "/orders/returns");
        let user = shell.user.unwrap();
        assert_eq!(user.name, "Ravneet Kaur");
        assert_eq!(user.email.as_deref(), Some("ravneet.kaur@example.com"));
        assert_eq!(
            user.menu,
            [
                N::Link(n::Link {
                    id: None,
                    label: "Profile".into(),
                    action: Action::navigate("/profile"),
                    children: Vec::new(),
                }),
                N::Button(n::Button {
                    label: "Sign out".into(),
                    action: Some(Action::Request {
                        method: n::Method::Post,
                        href: "/sign-out".into(),
                        fields: vec![Pair {
                            name: "_csrf".into(),
                            value: "tok".into()
                        }],
                    }),
                    ..n::Button::default()
                }),
            ]
        );
        assert!(super::shell("b1", View::new("x").body(Text::new("plain")).body_node()).is_none());
    }
}
