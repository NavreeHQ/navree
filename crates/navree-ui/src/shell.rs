//! The application shell: a collapsible sidebar (the `sidebar-07` layout), a
//! top bar and the page. [`Shell`] is the app's one definition; [`AppShell`]
//! is it applied to a request; [`AppPage`] is what a handler returns.
//!
//! Provenance: the anatomy follows shadcn/ui's Sidebar (MIT). See THIRD_PARTY.md.

use std::borrow::Cow;
use std::sync::Arc;

use http::header::{COOKIE, HeaderValue, InvalidHeaderValue};
use http::request::Parts;
use navree_http::extract::{FromRequestParts, Scoped};
use navree_http::extract::urlencoded;
use navree_http::response::{IntoResponse, Response};

use crate::components::{Alert, AlertTone, Avatar, Breadcrumb, Button, ButtonVariant, Separator, Size, icon};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, IntoNodes, Node, Uses, el};
use crate::view::View;

/// The cookie holding a toast for the next page.
pub(crate) const FLASH_COOKIE: &str = "navree_ui_flash";

/// The cookie the runtime sets when the sidebar collapses or expands.
pub const SIDEBAR_COOKIE: &str = "navree_ui_sidebar";

/// A cookie that shows `text` on the next page, when JavaScript is off.
pub(crate) fn flash_cookie(text: &str) -> Result<HeaderValue, InvalidHeaderValue> {
    let mut value = String::with_capacity(text.len() + 64);
    value.push_str(FLASH_COOKIE);
    value.push('=');
    for byte in text.bytes().take(512) {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => value.push(char::from(byte)),
            _ => {
                value.push('%');
                value.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
                value.push(char::from_digit(u32::from(byte & 15), 16).unwrap_or('0'));
            }
        }
    }
    value.push_str("; Path=/; Max-Age=30; SameSite=Lax; HttpOnly");
    HeaderValue::from_str(&value)
}

/// One link in the sidebar.
#[derive(Clone, Debug)]
pub struct NavItem {
    label: Cow<'static, str>,
    href: Href,
    glyph: Option<Icon>,
    count: Option<Cow<'static, str>>,
    sub: Vec<NavItem>,
}

impl NavItem {
    /// A link. Every top-level item should have an [`icon`](Self::icon): the
    /// collapsed sidebar shows only that.
    pub fn new(label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        Self { label: label.into(), href: href.into(), glyph: None, count: None, sub: Vec::new() }
    }

    /// Sets the icon.
    #[must_use]
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Adds a count at the end.
    #[must_use]
    pub fn count(mut self, count: impl Into<Cow<'static, str>>) -> Self {
        self.count = Some(count.into());
        self
    }

    /// Adds an item below this one.
    #[must_use]
    pub fn sub(mut self, item: NavItem) -> Self {
        self.sub.push(item);
        self
    }

    /// How well this item's link matches `path`: longer is better.
    fn score(&self, path: &str, home: &str) -> usize {
        let href = self.href.as_str();
        let href = href.split(['?', '#']).next().unwrap_or(href);
        if href == path {
            return href.len() + 1;
        }
        if href != home && path.strip_prefix(href).is_some_and(|rest| rest.starts_with('/')) {
            return href.len();
        }
        0
    }

    fn best(&self, path: &str, home: &str) -> usize {
        self.sub.iter().map(|item| item.best(path, home)).fold(self.score(path, home), usize::max)
    }

    fn to_node(&self, path: &str, home: &str, best: usize, nested: bool) -> Element {
        let active = best > 0 && self.score(path, home) == best;
        let link = el("a")
            .slot(if nested { "sidebar-sub-item" } else { "sidebar-item" })
            .href(self.href.clone())
            .attr("title", self.label.clone())
            .attr_opt("aria-current", active.then_some("page"))
            .children((
                self.glyph.map(icon),
                el("span").slot("sidebar-label").child(self.label.clone()),
                self.count.clone().map(|count| el("span").slot("sidebar-count").child(count)),
            ));
        let sub = (!self.sub.is_empty()).then(|| {
            el("ul").slot("sidebar-sub").child(
                self.sub.iter().map(|item| el("li").child(item.to_node(path, home, best, true))).collect::<Vec<_>>(),
            )
        });
        el("li").children((link, sub))
    }
}

/// A labelled group of links in the sidebar.
#[derive(Clone, Debug)]
pub struct NavGroup {
    label: Option<Cow<'static, str>>,
    items: Vec<NavItem>,
}

impl NavGroup {
    /// A group labelled `label`.
    pub fn new(label: impl Into<Cow<'static, str>>) -> Self {
        Self { label: Some(label.into()), items: Vec::new() }
    }

    /// A group with no label.
    pub fn plain() -> Self {
        Self { label: None, items: Vec::new() }
    }

    /// Adds a link.
    #[must_use]
    pub fn item(mut self, item: NavItem) -> Self {
        self.items.push(item);
        self
    }
}

/// The user at the foot of the sidebar: an avatar, name and email, and a menu.
#[derive(Clone, Debug)]
pub struct UserMenu {
    name: Cow<'static, str>,
    email: Cow<'static, str>,
    links: Vec<(Cow<'static, str>, Href)>,
}

impl UserMenu {
    /// The user `name`.
    pub fn new(name: impl Into<Cow<'static, str>>, email: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), email: email.into(), links: Vec::new() }
    }

    /// Adds a link to the menu.
    #[must_use]
    pub fn link(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.links.push((label.into(), href.into()));
        self
    }

    fn to_node(&self) -> Node {
        let trigger = el("button")
            .slot("sidebar-user")
            .attr("type", "button")
            .attr("popovertarget", "ui-user-menu")
            .attr("aria-haspopup", "dialog")
            .attr("title", self.name.clone())
            .children((
                Avatar::placeholder(self.name.clone().into_owned()),
                el("span").slot("sidebar-user-text").children((
                    el("span").slot("sidebar-user-name").child(self.name.clone()),
                    el("span").slot("sidebar-user-email").child(self.email.clone()),
                )),
                icon(icons::CHEVRONS_UP_DOWN),
            ));
        let links: Vec<Element> =
            self.links.iter().map(|(label, href)| el("a").slot("menu-link").href(href.clone()).child(label.clone())).collect();
        let menu = el("div")
            .slot("popover-content")
            .attr("id", "ui-user-menu")
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .flag("autofocus", true)
            .attr("aria-label", "Account")
            .flag("data-ui-place", true)
            .attr("data-side", "right")
            .uses(Uses::OVERLAY)
            .child(links);
        Node::list((trigger, menu))
    }
}

/// The app's one shell definition. Register it as a provider so [`AppShell`]
/// can find it.
///
/// ```
/// use navree_ui::{NavGroup, NavItem, Shell, icons};
///
/// let shell = Shell::new("Acme").context("Production").group(
///     NavGroup::new("Sales").item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)),
/// );
/// let html = shell.page("Deals", "/deals").body("Hello").to_view().to_html();
/// assert!(html.contains(r#"aria-current="page""#));
/// ```
#[derive(Clone, Debug)]
pub struct Shell {
    name: Cow<'static, str>,
    context: Option<Cow<'static, str>>,
    home: Href,
    groups: Vec<NavGroup>,
    user: Option<UserMenu>,
    search: Option<Node>,
}

impl Shell {
    /// A shell for the app `name`.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), context: None, home: Href::from("/"), groups: Vec::new(), user: None, search: None }
    }

    /// Sets the line below the app's name: a workspace, a plan.
    #[must_use]
    pub fn context(mut self, context: impl Into<Cow<'static, str>>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Sets where the app's name links to, and which link only matches itself.
    #[must_use]
    pub fn home(mut self, home: impl Into<Href>) -> Self {
        self.home = home.into();
        self
    }

    /// Adds a group of links.
    #[must_use]
    pub fn group(mut self, group: NavGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Sets the user at the foot.
    #[must_use]
    pub fn user(mut self, user: UserMenu) -> Self {
        self.user = Some(user);
        self
    }

    /// Adds search to the top bar.
    #[must_use]
    pub fn search(mut self, search: impl Into<Node>) -> Self {
        self.search = Some(search.into());
        self
    }

    /// A page of this shell at `path`, for use without the [`AppShell`] extractor.
    pub fn page(&self, title: impl Into<Cow<'static, str>>, path: impl Into<String>) -> AppPage {
        AppPage::new(Arc::new(self.clone()), title.into(), path.into(), false, None)
    }

    fn sidebar(&self, path: &str) -> Element {
        let home = self.home.as_str();
        let header = el("a").slot("sidebar-brand").href(self.home.clone()).attr("title", self.name.clone()).children((
            el("span").slot("sidebar-logo").child(Node::Static(LOGO)),
            el("span").slot("sidebar-brand-text").children((
                el("span").slot("sidebar-brand-name").child(self.name.clone()),
                self.context.clone().map(|context| el("span").slot("sidebar-brand-context").child(context)),
            )),
        ));
        let best = self.groups.iter().flat_map(|group| &group.items).map(|item| item.best(path, home)).max().unwrap_or(0);
        let groups: Vec<Element> = self
            .groups
            .iter()
            .map(|group| {
                el("div").slot("sidebar-group").children((
                    group.label.clone().map(|label| el("p").slot("sidebar-group-label").child(label)),
                    el("ul").slot("sidebar-menu").child(group.items.iter().map(|item| item.to_node(path, home, best, false)).collect::<Vec<_>>()),
                ))
            })
            .collect();
        el("aside").slot("sidebar").attr("aria-label", "Sidebar").children((
            el("div").slot("sidebar-header").child(header),
            el("nav").slot("sidebar-content").attr("aria-label", "Main").child(groups),
            self.user.as_ref().map(|user| el("div").slot("sidebar-footer").child(user.to_node())),
        ))
    }
}

/// The Navree mark, as compile-time markup.
const LOGO: &str = r#"<svg viewBox="0 0 32 32" aria-hidden="true" fill="currentColor"><path d="M1.5 9.5 30.5 7c-.9 2.4-1.9 4.3-3.1 5.7H3.4c-.8-.9-1.4-1.9-1.9-3.2Z"/><path d="M4.4 14.5H26c-1.6 2-3.6 3.3-6.5 3.8H10c-2.4-.5-4.2-1.8-5.6-3.8Z"/><path d="M13 18.3h6.4L16.4 26h-2Z"/></svg>"#;

/// A page in the shell: what a handler returns.
#[derive(Clone, Debug)]
#[must_use]
pub struct AppPage {
    shell: Arc<Shell>,
    title: Cow<'static, str>,
    path: String,
    collapsed: bool,
    flash: Option<String>,
    crumbs: Vec<(Cow<'static, str>, Href)>,
    actions: Vec<Node>,
    body: Vec<Node>,
}

impl AppPage {
    fn new(shell: Arc<Shell>, title: Cow<'static, str>, path: String, collapsed: bool, flash: Option<String>) -> Self {
        Self { shell, title, path, collapsed, flash, crumbs: Vec::new(), actions: Vec::new(), body: Vec::new() }
    }

    /// Adds a step to the breadcrumb, before this page.
    pub fn crumb(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.crumbs.push((label.into(), href.into()));
        self
    }

    /// Adds actions beside the page's heading.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        actions.push_into(&mut self.actions);
        self
    }

    /// Adds to the page.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    /// The whole document.
    pub fn to_view(self) -> View {
        let shell = self.shell;
        let mut crumbs = Breadcrumb::new();
        for (label, href) in self.crumbs {
            crumbs = crumbs.link(label, href);
        }
        crumbs = crumbs.current(self.title.clone());
        let trigger = Button::icon_only(icons::PANEL_LEFT, "Toggle sidebar")
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .attr("data-ui-sidebar-trigger", "")
            .attr("aria-expanded", if self.collapsed { "false" } else { "true" });
        let topbar = el("header").slot("topbar").children((
            trigger,
            Separator::vertical(),
            crumbs,
            el("div").slot("topbar-spacer"),
            shell.search.clone(),
        ));
        let flash = self.flash.map(|text| Alert::new(AlertTone::Success).title(text));
        let heading = el("div").slot("page-header").children((
            el("h1").slot("page-title").child(self.title.clone()),
            (!self.actions.is_empty()).then(|| el("div").slot("page-actions").child(self.actions)),
        ));
        let main = el("main").slot("page").attr("id", "main").attr("tabindex", -1_i64).children((flash, heading, self.body));
        let had_flash = main.child_nodes().first().is_some_and(|node| !matches!(node, Node::Empty));
        let wrapper = el("div")
            .slot("sidebar-wrapper")
            .attr("data-ui-keep-attrs", "data-collapsed data-mobile-open")
            .flag("data-collapsed", self.collapsed)
            .uses(Uses::SIDEBAR.with(Uses::CORE))
            .children((
                shell.sidebar(&self.path),
                el("div").slot("sidebar-overlay"),
                el("div").slot("sidebar-inset").children((topbar, main)),
            ));
        let mut view = View::new(self.title).body(wrapper);
        if had_flash && let Ok(clear) = HeaderValue::from_str(&format!("{FLASH_COOKIE}=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly")) {
            view = view.cookie(clear);
        }
        view
    }
}

impl IntoResponse for AppPage {
    fn into_response(self) -> Response {
        self.to_view().into_response()
    }
}

/// The app's [`Shell`], applied to this request: which link is active, whether
/// the sidebar is collapsed, a toast left for this page.
///
/// ```ignore
/// async fn show(shell: AppShell) -> AppPage {
///     shell.page("Deals").body(Button::primary("New deal").href("/deals/new"))
/// }
/// ```
#[derive(Clone, Debug)]
pub struct AppShell {
    shell: Arc<Shell>,
    path: String,
    collapsed: bool,
    flash: Option<String>,
}

impl AppShell {
    /// A page titled `title`.
    pub fn page(&self, title: impl Into<Cow<'static, str>>) -> AppPage {
        AppPage::new(self.shell.clone(), title.into(), self.path.clone(), self.collapsed, self.flash.clone())
    }
}

impl FromRequestParts for AppShell {
    type Rejection = <Scoped<Shell> as FromRequestParts>::Rejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let Scoped(shell) = Scoped::<Shell>::from_request_parts(parts).await?;
        let mut collapsed = false;
        let mut flash = None;
        for value in parts.headers.get_all(COOKIE).iter().filter_map(|value| value.to_str().ok()) {
            for pair in value.split(';') {
                match pair.trim().split_once('=') {
                    Some((SIDEBAR_COOKIE, "collapsed")) => collapsed = true,
                    Some((FLASH_COOKIE, text)) => flash = urlencoded::parse(format!("x={text}").as_bytes()).next().map(|(_, text)| text.into_owned()),
                    _ => {}
                }
            }
        }
        Ok(Self { shell, path: parts.uri.path().to_owned(), collapsed, flash })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn shell() -> Shell {
        Shell::new("Acme")
            .group(NavGroup::plain().item(NavItem::new("Home", "/").icon(icons::HOUSE)))
            .group(
                NavGroup::new("Sales")
                    .item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE).count("12").sub(NavItem::new("Won", "/deals/won")))
                    .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
            )
    }

    #[test]
    fn the_longest_match_is_the_active_link() {
        let html = shell().page("x", "/deals/won").to_view().to_html();
        assert_eq!(html.matches("aria-current=\"page\"").count(), 2, "the sub-item and the breadcrumb: {html}");
        assert!(html.contains(r#"<a data-slot="sidebar-sub-item" href="/deals/won" title="Won" aria-current="page">"#), "{html}");
        let html = shell().page("x", "/deals/12").to_view().to_html();
        assert!(html.contains(r#"href="/deals" title="Deals" aria-current="page""#), "{html}");
        let html = shell().page("x", "/").to_view().to_html();
        assert!(html.contains(r#"href="/" title="Home" aria-current="page""#), "{html}");
    }

    #[test]
    fn the_home_link_only_matches_itself() {
        let html = shell().page("x", "/contacts").to_view().to_html();
        assert!(!html.contains(r#"href="/" title="Home" aria-current"#), "{html}");
    }

    #[test]
    fn a_shell_page_loads_what_it_uses() {
        let view = shell().page("x", "/").to_view();
        assert!(view.uses().has(Uses::CORE));
        assert!(view.uses().has(Uses::SIDEBAR));
        assert!(!view.uses().has(Uses::OVERLAY), "no menu, no overlay module");
    }

    #[test]
    fn flash_cookies_round_trip() {
        let cookie = flash_cookie("Deal \"Acme\" created; ok").unwrap();
        let text = cookie.to_str().unwrap();
        assert!(!text.split(';').next().unwrap()[FLASH_COOKIE.len() + 1..].contains([' ', '"']), "{text}");
        let value = text.split(';').next().unwrap().split_once('=').unwrap().1;
        let input = format!("x={value}");
        let (_, decoded) = urlencoded::parse(input.as_bytes()).next().unwrap();
        assert_eq!(decoded, "Deal \"Acme\" created; ok");
    }

    #[test]
    fn there_is_no_shape_for_initials_in_an_avatar() {
        let html = render(&Avatar::placeholder("Ada").into());
        assert!(!html.contains(">A<"));
        assert!(html.contains("<svg"));
    }
}
