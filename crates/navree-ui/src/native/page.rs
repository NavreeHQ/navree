//! A whole page read into a [`Screen`], and the app's shell read into the
//! [`Shell`](navree_native::Shell) document.
//!
//! On the web the app's shell is drawn around every page. A native client
//! draws the shell itself, once, so a page's screen is what stands inside it:
//! the title, the way back from the breadcrumb, the actions beside the
//! heading, and the content. A toast left for the page comes with it as an op.

use std::borrow::Cow;

#[cfg(test)]
use navree_native::action::{Action, Pair};
use navree_native::node as n;
use navree_native::{Op, Screen};

use super::lower::{Cx, Lowered, Miss, N, Parts, attr, flat, id, one, slot, text_of, tone_of};
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

/// Reads a page: from the note of the page in the app's shell it was made
/// from, when a native client asked while it was built, or else from its tree.
pub(crate) fn of_view(view: &mut crate::view::View, keep_markup: bool) -> Page {
    let csrf = view.csrf_token().map(str::to_owned);
    if let Some(note) = view.note() {
        return read_note(note, keep_markup, csrf.as_deref());
    }
    let parts = view.native_parts();
    read(parts.title, parts.body, keep_markup, csrf.as_deref())
}

/// Reads a page in the app's shell from its note: the title, the way back
/// from its steps, the destination it belongs to, the actions beside its
/// heading and its content. The shell itself is never built.
pub(crate) fn read_note(
    note: &crate::shell::PageNote,
    keep_markup: bool,
    csrf: Option<&str>,
) -> Page {
    let mut cx = Cx::new(keep_markup).with_csrf(csrf);
    let mut screen = Screen::new(note.title(), N::Stack(n::Stack::default()));
    screen.nav = note.nav();
    let steps: Vec<navree_native::Back> = note
        .steps()
        .map(|(title, href)| navree_native::Back {
            title: title.to_owned(),
            href: href.to_owned(),
        })
        .collect();
    screen.back = steps.last().cloned();
    if steps.len() > 1 {
        screen.trail = steps;
    }
    screen.actions = cx.sequence(note.actions());
    let content = cx.sequence(note.body());
    screen.id = match content.as_slice() {
        [only] => only.id().map(str::to_owned),
        _ => None,
    };
    screen.body = one(content);
    let mut ops = Vec::new();
    if let Some((tone, text)) = note.flash() {
        // Read as the toaster a browser gets would be.
        let toaster: Node = crate::components::Toaster::new()
            .toast(crate::components::Toast::new(tone, text.to_owned()))
            .into();
        if let Node::Element(toaster) = &toaster {
            toasts(toaster, &mut ops);
        }
    }
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

/// An icon of the toolkit as a native client draws it.
pub(crate) fn icon_of(glyph: crate::icons::Icon) -> Option<n::Icon> {
    match &crate::components::icon(glyph) {
        Node::Element(element) => slots::icon(element).ok(),
        _ => None,
    }
}

/// Reads a page titled `title` whose content is `body`.
pub(crate) fn read(title: &str, body: &Node, keep_markup: bool, csrf: Option<&str>) -> Page {
    let mut cx = Cx::new(keep_markup).with_csrf(csrf);
    let mut screen = Screen::new(title, N::Stack(n::Stack::default()));
    let mut ops = Vec::new();
    let mut content: Vec<N> = Vec::new();
    // The parts read together, since a menu's button and its popup are two
    // siblings that are read as one.
    let mut plain: Vec<&Node> = Vec::new();
    for node in flat(std::slice::from_ref(body)) {
        match node {
            Node::Element(element) if slot(element).as_deref() == Some("sidebar-wrapper") => {
                content.extend(cx.flat_sequence(&std::mem::take(&mut plain)));
                in_shell(&mut cx, element, &mut screen, &mut content);
            }
            Node::Element(element) if slot(element).as_deref() == Some("toaster") => {
                toasts(element, &mut ops);
            }
            // An error page on its own: the state it says is the screen.
            Node::Element(element) if slot(element).as_deref() == Some("state-page") => {
                content.extend(cx.flat_sequence(&std::mem::take(&mut plain)));
                for part in flat(element.child_nodes()) {
                    match part {
                        Node::Element(found) if slot(found).as_deref() == Some("state") => {
                            match state(&mut cx, found) {
                                Ok(read) => content.push(read),
                                Err(fail) => {
                                    let web = cx.miss("slot:state".to_owned(), fail.why, found);
                                    content.push(web);
                                }
                            }
                        }
                        other => content.extend(cx.flat_sequence(&[other])),
                    }
                }
            }
            other => plain.push(other),
        }
    }
    content.extend(cx.flat_sequence(&plain));
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

/// A state the toolkit draws for an error or an empty place: its headline,
/// what it says, and the ways on from it, as an empty state. Its reference
/// for support stays with its words; what a development build adds is left
/// out, since a client has no place for it.
pub(crate) fn state(cx: &mut Cx, element: &Element) -> super::lower::Read<N> {
    let mut parts = Parts::of(element);
    let icon = match parts.slot("state-media") {
        Some(media) => find(media.child_nodes(), "icon").and_then(|icon| slots::icon(icon).ok()),
        None => None,
    };
    let Some(body) = parts.slot("state-body") else {
        return super::lower::fail("a state with nothing to say");
    };
    parts.done()?;
    let mut body = Parts::of(body);
    let _code = body.slot("state-code");
    let Some(title) = body.slot("state-title") else {
        return super::lower::fail("a state with no headline");
    };
    let mut words: Vec<String> = Vec::new();
    for name in [
        "state-description",
        "state-detail",
        "state-wait",
        "state-reference",
    ] {
        if let Some(part) = body.slot(name) {
            words.push(text_of(part));
        }
    }
    let actions = match body.slot("state-actions") {
        Some(actions) => cx.of(actions),
        None => Vec::new(),
    };
    if let Some(debug) = body.slot("state-debug") {
        cx.dropped.push(format!(
            "slot:{}",
            slot(debug).unwrap_or(Cow::Borrowed("state-debug"))
        ));
    }
    body.done()?;
    Ok(N::Empty(n::Empty {
        id: id(element),
        icon,
        title: text_of(title),
        text: (!words.is_empty()).then(|| words.join(" ")),
        actions,
    }))
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
        let page = read("ignored", view.body_node(), false, None);
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
        let page = read("Inventory", view.body_node(), false, None);
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
        let page = read("Figures", view.body_node(), false, None);
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
        let page = read("Board", view.body_node(), false, None);
        assert_eq!(page.why(), ["tag:canvas"]);
    }

    #[test]
    fn the_shell_document_comes_from_the_shell_itself() {
        let shell = app()
            .for_request("/", &http::HeaderMap::new())
            .csrf("tok")
            .native_document(Some("/sign-in?from=app"));
        assert_eq!(shell.name, "Charkha Supply Co.");
        assert_eq!(shell.context.as_deref(), Some("Store operations"));
        assert_eq!(shell.home, "/");
        assert_eq!(shell.build, crate::assets::BUILD);
        assert_eq!(shell.stamp, shell.clone().stamped().stamp);
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
        // The theme's accent, as the token table has it for each ground.
        let theme = shell.theme.unwrap();
        let appearance = crate::theme::Appearance::default();
        let spec = appearance.theme().spec();
        assert_eq!(
            theme.accent,
            Some(spec.light.get(crate::theme::Token::Accent).to_hex())
        );
        assert_eq!(
            theme.accent_dark,
            Some(spec.dark.get(crate::theme::Token::Accent).to_hex())
        );
        assert!(theme.accent.unwrap().starts_with('#'));
        // Sign-in over everything with no chrome; a new or edited record in a sheet.
        let routes: Vec<(&str, Option<navree_native::action::Present>, bool)> = shell
            .routes
            .iter()
            .map(|route| (route.matches.as_str(), route.present, route.chrome))
            .collect();
        assert_eq!(
            routes,
            [
                (
                    "/sign-in",
                    Some(navree_native::action::Present::Full),
                    false
                ),
                ("/*/new", Some(navree_native::action::Present::Sheet), true),
                (
                    "/*/*/edit",
                    Some(navree_native::action::Present::Sheet),
                    true
                ),
            ]
        );
    }

    #[test]
    fn a_page_in_the_shell_is_read_from_its_note() {
        let build = || {
            app()
                .page("Order CS-20481", "/orders/1")
                .crumb("Dashboard", "/")
                .crumb("Orders", "/orders")
                .actions(Button::primary("Refund").href("/orders/1/refund"))
                .body(crate::el("div").id("order-page").child(Text::new("Paid")))
                .to_view()
        };
        // Read from the chrome a browser gets, and from the note a native client's view keeps.
        let from_chrome = read("ignored", build().body_node(), false, None);
        let page = crate::native::TestNative::build(
            navree_native::Client::new(
                navree_native::Platform::Ios,
                navree_native::FormFactor::Phone,
            ),
            build,
        );
        assert!(page.is_native(), "{:?}", page.why());
        assert_eq!(page.page(), &from_chrome);
        // The view a native client got never built the sidebar.
        let view = crate::native::scope::sync(
            crate::native::scope::Asked::Native(std::sync::Arc::new(
                crate::native::scope::Request::new(
                    navree_native::Client::new(
                        navree_native::Platform::Android,
                        navree_native::FormFactor::Phone,
                    ),
                    "/orders/1",
                ),
            )),
            build,
        );
        let note = view.note().expect("a note");
        assert_eq!(note.title(), "Order CS-20481");
        assert_eq!(note.nav().as_deref(), Some("/orders"));
        assert!(!note.chrome_built());
        // Written as HTML after all, it is the page a browser always gets.
        assert_eq!(view.to_html(), build().to_html());
    }
}
