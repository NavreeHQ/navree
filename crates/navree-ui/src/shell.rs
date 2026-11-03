//! The application shell: a collapsible sidebar (the `sidebar-07` layout), a
//! top bar and the page. [`Shell`] is the app's one definition; [`AppShell`]
//! is it applied to a request; [`AppPage`] is what a handler returns.
//!
//! Provenance: the anatomy follows shadcn/ui's Sidebar (MIT). See THIRD_PARTY.md.

use std::borrow::Cow;
use std::sync::Arc;

use http::HeaderMap;
use http::header::{COOKIE, HeaderValue, InvalidHeaderValue};
use http::request::Parts;
use navree_http::extract::urlencoded;
use navree_http::extract::{FromRequestParts, Scoped};
use navree_http::response::{IntoResponse, Response};

use crate::components::{
    Avatar, Breadcrumb, Button, ButtonVariant, Separator, Size, Toast, Toaster, icon,
};
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, IntoNodes, Node, Uses, el};
use crate::op::ToastTone;
use crate::view::View;

/// The cookie holding a toast for the next page.
pub(crate) const FLASH_COOKIE: &str = "navree_ui_flash";

/// The cookie holding the session's CSRF token, when there is a session.
const CSRF_COOKIE: &str = "XSRF-TOKEN";

/// The cookie the runtime sets when the sidebar collapses or expands.
pub const SIDEBAR_COOKIE: &str = "navree_ui_sidebar";

/// The tone a flash holds before its text, success being the plain case. The text
/// is percent-encoded, so its first `:` is always this one.
const fn tone_prefix(tone: ToastTone) -> &'static str {
    match tone {
        ToastTone::Success => "",
        ToastTone::Info => "info:",
        ToastTone::Warning => "warning:",
        ToastTone::Danger => "danger:",
    }
}

/// A cookie that shows `text` in `tone` on the next page, when JavaScript is off.
pub(crate) fn flash_cookie_toned(
    tone: ToastTone,
    text: &str,
) -> Result<HeaderValue, InvalidHeaderValue> {
    let mut value = String::with_capacity(text.len() + 64);
    value.push_str(FLASH_COOKIE);
    value.push('=');
    value.push_str(tone_prefix(tone));
    for byte in text.bytes().take(512) {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                value.push(char::from(byte))
            }
            _ => {
                value.push('%');
                value.push(
                    char::from_digit(u32::from(byte >> 4), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
                value.push(
                    char::from_digit(u32::from(byte & 15), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    value.push_str("; Path=/; Max-Age=30; SameSite=Lax; HttpOnly");
    HeaderValue::from_str(&value)
}

/// A flash cookie's tone and text.
fn read_flash(value: &str) -> Option<(ToastTone, String)> {
    let (tone, text) = match value.split_once(':') {
        Some(("info", text)) => (ToastTone::Info, text),
        Some(("warning", text)) => (ToastTone::Warning, text),
        Some(("danger", text)) => (ToastTone::Danger, text),
        _ => (ToastTone::Success, value),
    };
    let (_, text) = urlencoded::parse(format!("x={text}").as_bytes())
        .next()
        .map(|(name, text)| (name.into_owned(), text.into_owned()))?;
    Some((tone, text))
}

/// One link in the sidebar.
#[derive(Clone, Debug)]
pub struct NavItem {
    label: Cow<'static, str>,
    href: Href,
    glyph: Option<Icon>,
    count: Option<Cow<'static, str>>,
    sub: Vec<NavItem>,
    prefetch: Prefetch,
    current: Option<bool>,
}

/// When a sidebar link is fetched ahead of a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefetch {
    /// Never.
    No,
    /// When the pointer rests on it or it takes focus.
    Intent,
    /// As soon as the page is quiet.
    Eager,
}

impl NavItem {
    /// A link. Every top-level item should have an [`icon`](Self::icon): the
    /// collapsed sidebar shows only that.
    pub fn new(label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            glyph: None,
            count: None,
            sub: Vec::new(),
            prefetch: Prefetch::No,
            current: None,
        }
    }

    /// Says whether this is the current page, in place of what the path says. For
    /// links that differ only in their query (a list under several filters), which
    /// the path can't tell apart.
    #[must_use]
    pub fn current(mut self, current: bool) -> Self {
        self.current = Some(current);
        self
    }

    /// Fetches this link when the pointer rests on it or it takes focus, so the page
    /// is often there before the click. Nothing is fetched ahead unless asked, link by
    /// link: only ask for a page whose `GET` changes nothing.
    #[must_use]
    pub fn prefetch(mut self) -> Self {
        self.prefetch = Prefetch::Intent;
        self
    }

    /// Fetches this link only when it is clicked, which is the default: undoes
    /// [`prefetch`](Self::prefetch) and [`prefetch_eager`](Self::prefetch_eager).
    #[must_use]
    pub fn no_prefetch(mut self) -> Self {
        self.prefetch = Prefetch::No;
        self
    }

    /// Fetches this link as soon as the page is quiet, with no hover: for the page
    /// users almost always open next. See
    /// [`Element::prefetch_eager`](crate::Element::prefetch_eager) for what it costs.
    #[must_use]
    pub fn prefetch_eager(mut self) -> Self {
        self.prefetch = Prefetch::Eager;
        self
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
    #[allow(clippy::should_implement_trait, reason = "a sub-item, not subtraction")]
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
        if href != home
            && path
                .strip_prefix(href)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            return href.len();
        }
        0
    }

    fn best(&self, path: &str, home: &str) -> usize {
        self.sub
            .iter()
            .map(|item| item.best(path, home))
            .fold(self.score(path, home), usize::max)
    }

    fn to_node(&self, path: &str, home: &str, best: usize, nested: bool) -> Element {
        let active = self
            .current
            .unwrap_or(best > 0 && self.score(path, home) == best);
        let link = el("a")
            .slot(if nested {
                "sidebar-sub-item"
            } else {
                "sidebar-item"
            })
            .href(self.href.clone())
            .attr("title", self.label.clone())
            .attr_opt("aria-current", active.then_some("page"))
            .flag("data-ui-prefetch", self.prefetch == Prefetch::Intent)
            .attr_opt(
                "data-ui-prefetch",
                (self.prefetch == Prefetch::Eager).then_some("eager"),
            )
            .children((
                self.glyph.map(icon),
                el("span").slot("sidebar-label").child(self.label.clone()),
                self.count
                    .clone()
                    .map(|count| el("span").slot("sidebar-count").child(count)),
            ));
        let sub = (!self.sub.is_empty()).then(|| {
            el("ul").slot("sidebar-sub").child(
                self.sub
                    .iter()
                    .map(|item| el("li").child(item.to_node(path, home, best, true)))
                    .collect::<Vec<_>>(),
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
        Self {
            label: Some(label.into()),
            items: Vec::new(),
        }
    }

    /// A group with no label.
    pub fn plain() -> Self {
        Self {
            label: None,
            items: Vec::new(),
        }
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
    links: Vec<MenuEntry>,
}

/// One line of a [`UserMenu`].
#[derive(Clone, Debug)]
enum MenuEntry {
    Link(Cow<'static, str>, Href),
    Action(Cow<'static, str>, Href),
}

impl UserMenu {
    /// The user `name`.
    pub fn new(name: impl Into<Cow<'static, str>>, email: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
            links: Vec::new(),
        }
    }

    /// Adds a link to the menu, for a page to read. It isn't fetched ahead of a click.
    /// Something that changes state, such as signing out, is an [`action`](Self::action).
    #[must_use]
    pub fn link(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.links.push(MenuEntry::Link(label.into(), href.into()));
        self
    }

    /// Adds a button that posts to `href`: signing out, switching workspace. It works
    /// with JavaScript off, carrying the session's token as a field when there is one.
    #[must_use]
    pub fn action(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.links
            .push(MenuEntry::Action(label.into(), href.into()));
        self
    }

    fn to_node(&self, csrf: Option<&str>) -> Node {
        let trigger = el("button")
            .slot("sidebar-user")
            .attr("type", "button")
            .attr("popovertarget", "ui-user-menu")
            .attr("aria-haspopup", "dialog")
            .attr("title", self.name.clone())
            .children((
                Avatar::placeholder(self.name.clone().into_owned()),
                el("span").slot("sidebar-user-text").children((
                    el("span")
                        .slot("sidebar-user-name")
                        .child(self.name.clone()),
                    el("span")
                        .slot("sidebar-user-email")
                        .child(self.email.clone()),
                )),
                icon(icons::CHEVRONS_UP_DOWN),
            ));
        let links: Vec<Element> = self
            .links
            .iter()
            .map(|entry| match entry {
                MenuEntry::Link(label, href) => el("a")
                    .slot("menu-link")
                    .href(href.clone())
                    .child(label.clone()),
                MenuEntry::Action(label, href) => el("form")
                    .attr("method", "post")
                    .attr("action", href.clone())
                    .flag("data-ui-form", true)
                    .uses(Uses::CORE)
                    .children((
                        csrf.map(|token| csrf_field(token.to_owned())),
                        el("button")
                            .slot("menu-link")
                            .attr("type", "submit")
                            .child(label.clone()),
                    )),
            })
            .collect();
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
    prefetch_home: bool,
}

impl Shell {
    /// A shell for the app `name`.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            context: None,
            home: Href::from("/"),
            groups: Vec::new(),
            user: None,
            search: None,
            prefetch_home: false,
        }
    }

    /// Fetches the page the app's name links to when the pointer rests on it or it
    /// takes focus. Off unless asked, as for every link.
    #[must_use]
    pub fn prefetch_home(mut self) -> Self {
        self.prefetch_home = true;
        self
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
        AppPage::new(
            Arc::new(self.clone()),
            title.into(),
            path.into(),
            false,
            None,
            None,
        )
    }

    /// This shell applied to a request: which link is active, whether the sidebar is
    /// collapsed, a toast left for this page. It is what the [`AppShell`] extractor
    /// does for the app's own `Shell`, for a module that serves pages of its own
    /// and has no business registering one for the whole app (a debugging tool).
    pub fn for_request(&self, path: impl Into<String>, headers: &HeaderMap) -> AppShell {
        AppShell::read(Arc::new(self.clone()), path.into(), headers)
    }

    fn sidebar(&self, path: &str, csrf: Option<&str>) -> Element {
        let home = self.home.as_str();
        let header = el("a")
            .slot("sidebar-brand")
            .href(self.home.clone())
            .attr("title", self.name.clone())
            .flag("data-ui-prefetch", self.prefetch_home)
            .children((
                el("span").slot("sidebar-logo").child(Node::Static(LOGO)),
                el("span").slot("sidebar-brand-text").children((
                    el("span")
                        .slot("sidebar-brand-name")
                        .child(self.name.clone()),
                    self.context
                        .clone()
                        .map(|context| el("span").slot("sidebar-brand-context").child(context)),
                )),
            ));
        let best = self
            .groups
            .iter()
            .flat_map(|group| &group.items)
            .map(|item| item.best(path, home))
            .max()
            .unwrap_or(0);
        let groups: Vec<Element> = self
            .groups
            .iter()
            .map(|group| {
                el("div").slot("sidebar-group").children((
                    group
                        .label
                        .clone()
                        .map(|label| el("p").slot("sidebar-group-label").child(label)),
                    el("ul").slot("sidebar-menu").child(
                        group
                            .items
                            .iter()
                            .map(|item| item.to_node(path, home, best, false))
                            .collect::<Vec<_>>(),
                    ),
                ))
            })
            .collect();
        el("aside")
            .slot("sidebar")
            .attr("aria-label", "Sidebar")
            .children((
                el("div").slot("sidebar-header").child(header),
                el("nav")
                    .slot("sidebar-content")
                    .attr("aria-label", "Main")
                    .child(groups),
                self.user
                    .as_ref()
                    .map(|user| el("div").slot("sidebar-footer").child(user.to_node(csrf))),
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
    flash: Option<(ToastTone, String)>,
    csrf: Option<String>,
    crumbs: Vec<(Cow<'static, str>, Href)>,
    prefetch_crumbs: bool,
    actions: Vec<Node>,
    body: Vec<Node>,
}

impl AppPage {
    fn new(
        shell: Arc<Shell>,
        title: Cow<'static, str>,
        path: String,
        collapsed: bool,
        flash: Option<(ToastTone, String)>,
        csrf: Option<String>,
    ) -> Self {
        Self {
            shell,
            title,
            path,
            collapsed,
            flash,
            csrf,
            crumbs: Vec::new(),
            prefetch_crumbs: false,
            actions: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Adds a step to the breadcrumb, before this page.
    pub fn crumb(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.crumbs.push((label.into(), href.into()));
        self
    }

    /// Fetches the breadcrumb's links when the pointer rests on one or it takes focus.
    /// Off unless asked, as for every link.
    pub fn prefetch_crumbs(mut self) -> Self {
        self.prefetch_crumbs = true;
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
        if self.prefetch_crumbs {
            crumbs = crumbs.prefetch();
        }
        let trigger = Button::icon_only(icons::PANEL_LEFT, "Toggle sidebar")
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .attr("data-ui-sidebar-trigger", "")
            .attr(
                "aria-expanded",
                if self.collapsed { "false" } else { "true" },
            );
        let topbar = el("header").slot("topbar").children((
            trigger,
            Separator::vertical(),
            crumbs,
            el("div").slot("topbar-spacer"),
            shell.search.clone(),
        ));
        let had_flash = self.flash.is_some();
        let mut toaster = Toaster::new();
        if let Some((tone, text)) = self.flash {
            toaster = toaster.toast(Toast::new(tone, text));
        }
        if let Some(token) = &self.csrf {
            toaster = toaster.csrf(token.clone());
        }
        let heading = el("div").slot("page-header").children((
            el("h1").slot("page-title").child(self.title.clone()),
            (!self.actions.is_empty()).then(|| el("div").slot("page-actions").child(self.actions)),
        ));
        let main = el("main")
            .slot("page")
            .attr("id", "main")
            .attr("tabindex", -1_i64)
            .children((heading, self.body));
        let wrapper = el("div")
            .slot("sidebar-wrapper")
            .attr("data-ui-keep-attrs", "data-collapsed data-mobile-open")
            .flag("data-collapsed", self.collapsed)
            .uses(Uses::SIDEBAR.with(Uses::CORE))
            .children((
                shell.sidebar(&self.path, self.csrf.as_deref()),
                el("div").slot("sidebar-overlay"),
                el("div").slot("sidebar-inset").children((topbar, main)),
            ));
        let mut view = View::new(self.title).body((wrapper, toaster));
        if had_flash
            && let Ok(clear) = HeaderValue::from_str(&format!(
                "{FLASH_COOKIE}=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly"
            ))
        {
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
    flash: Option<(ToastTone, String)>,
    csrf: Option<String>,
}

impl AppShell {
    /// A page titled `title`.
    pub fn page(&self, title: impl Into<Cow<'static, str>>) -> AppPage {
        AppPage::new(
            self.shell.clone(),
            title.into(),
            self.path.clone(),
            self.collapsed,
            self.flash.clone(),
            self.csrf.clone(),
        )
    }
}

impl AppShell {
    /// `shell` applied to a request with these headers, at `path`.
    fn read(shell: Arc<Shell>, path: String, headers: &HeaderMap) -> Self {
        let mut collapsed = false;
        let mut flash = None;
        let mut csrf = None;
        // A page fetched ahead of a click may never be seen: its toast stays for the next.
        let prefetch = crate::op::is_prefetch(headers);
        for value in headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
        {
            for pair in value.split(';') {
                match pair.trim().split_once('=') {
                    Some((SIDEBAR_COOKIE, "collapsed")) => collapsed = true,
                    Some((FLASH_COOKIE, text)) if !prefetch => flash = read_flash(text),
                    Some((CSRF_COOKIE, token)) => {
                        csrf = urlencoded::parse(format!("x={token}").as_bytes())
                            .next()
                            .map(|(_, token)| token.into_owned())
                    }
                    _ => {}
                }
            }
        }
        Self {
            shell,
            path,
            collapsed,
            flash,
            csrf,
        }
    }
}

impl FromRequestParts for AppShell {
    type Rejection = <Scoped<Shell> as FromRequestParts>::Rejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let Scoped(shell) = Scoped::<Shell>::from_request_parts(parts).await?;
        Ok(Self::read(
            shell,
            parts.uri.path().to_owned(),
            &parts.headers,
        ))
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
                    .item(
                        NavItem::new("Deals", "/deals")
                            .icon(icons::HANDSHAKE)
                            .count("12")
                            .sub(NavItem::new("Won", "/deals/won")),
                    )
                    .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
            )
    }

    #[test]
    fn the_longest_match_is_the_active_link() {
        let html = shell().page("x", "/deals/won").to_view().to_html();
        assert_eq!(
            html.matches("aria-current=\"page\"").count(),
            2,
            "the sub-item and the breadcrumb: {html}"
        );
        assert!(html.contains(r#"<a data-slot="sidebar-sub-item" href="/deals/won" title="Won" aria-current="page">"#), "{html}");
        let html = shell().page("x", "/deals/12").to_view().to_html();
        assert!(
            html.contains(r#"href="/deals" title="Deals" aria-current="page""#),
            "{html}"
        );
        let html = shell().page("x", "/").to_view().to_html();
        assert!(
            html.contains(r#"href="/" title="Home" aria-current="page""#),
            "{html}"
        );
    }

    #[test]
    fn the_home_link_only_matches_itself() {
        let html = shell().page("x", "/contacts").to_view().to_html();
        assert!(
            !html.contains(r#"href="/" title="Home" aria-current"#),
            "{html}"
        );
    }

    #[test]
    fn a_shell_page_loads_what_it_uses() {
        let view = shell().page("x", "/").to_view();
        assert!(view.uses().has(Uses::CORE));
        assert!(view.uses().has(Uses::SIDEBAR));
        assert!(
            !view.uses().has(Uses::OVERLAY),
            "no menu, no overlay module"
        );
    }

    #[test]
    fn flash_cookies_round_trip() {
        let cookie = flash_cookie_toned(ToastTone::Success, "Deal \"Acme\" created; ok").unwrap();
        let text = cookie.to_str().unwrap();
        assert!(
            !text.split(';').next().unwrap()[FLASH_COOKIE.len() + 1..].contains([' ', '"']),
            "{text}"
        );
        let value = text.split(';').next().unwrap().split_once('=').unwrap().1;
        let input = format!("x={value}");
        let (_, decoded) = urlencoded::parse(input.as_bytes()).next().unwrap();
        assert_eq!(decoded, "Deal \"Acme\" created; ok");
    }

    #[test]
    fn a_flash_reads_back_with_its_tone() {
        assert_eq!(
            read_flash("Done"),
            Some((ToastTone::Success, "Done".to_owned()))
        );
        assert_eq!(
            read_flash("danger:Payment%20failed"),
            Some((ToastTone::Danger, "Payment failed".to_owned()))
        );
        assert_eq!(
            read_flash("warning:a%3Ab"),
            Some((ToastTone::Warning, "a:b".to_owned()))
        );
        assert_eq!(
            read_flash("info:x"),
            Some((ToastTone::Info, "x".to_owned()))
        );
        assert_eq!(
            read_flash("forged:x"),
            Some((ToastTone::Success, "forged:x".to_owned())),
            "any other value is just text"
        );
    }

    #[test]
    fn a_page_shows_a_danger_flash_as_danger_and_clears_it() {
        let shell = Arc::new(shell());
        let page = AppPage::new(
            shell,
            "x".into(),
            "/".to_owned(),
            false,
            read_flash("danger:Payment%20failed"),
            None,
        );
        let view = page.to_view();
        let html = view.to_html();
        assert!(html.contains(r#"data-tone="danger""#), "{html}");
        assert!(html.contains("Payment failed"), "{html}");
        let response = view.into_response();
        assert!(
            response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }

    #[test]
    fn every_page_has_the_toaster_and_a_flash_is_a_toast_in_it() {
        let html = shell().page("x", "/").to_view().to_html();
        assert!(
            html.contains(r#"<section data-slot="toaster" aria-label="Notifications""#),
            "{html}"
        );
        assert!(!html.contains(r#"data-slot="toast""#), "{html}");
        let flash = AppPage::new(
            Arc::new(shell()),
            "x".into(),
            "/".to_owned(),
            false,
            read_flash("warning:Almost%20full"),
            None,
        )
        .to_view()
        .to_html();
        let toaster = flash.split(r#"data-slot="toaster""#).nth(1).unwrap();
        assert!(toaster.contains(r#"data-tone="warning""#), "{flash}");
        assert!(toaster.contains(r#"role="status""#), "{flash}");
        assert!(toaster.contains("Almost full"), "{flash}");
        // It stands after the page, so it is the last thing a keyboard reaches.
        assert!(
            flash.find("sidebar-wrapper").unwrap() < flash.find(r#"data-slot="toaster""#).unwrap()
        );
        assert!(
            flash.trim_end().ends_with("</section></body></html>"),
            "{flash}"
        );
    }

    #[test]
    fn an_item_can_say_whether_it_is_the_current_page() {
        // Three links to one path that differ only in their query: the path can't tell them apart.
        let shell = Shell::new("Acme").group(
            NavGroup::plain()
                .item(NavItem::new("All", "/log").icon(icons::LIST).current(false))
                .item(
                    NavItem::new("Queries", "/log?type=query")
                        .icon(icons::DATABASE)
                        .current(true),
                )
                .item(
                    NavItem::new("Mail", "/log?type=mail")
                        .icon(icons::MAIL)
                        .current(false),
                ),
        );
        let html = shell.page("x", "/log").to_view().to_html();
        assert!(
            html.contains(r#"href="/log?type=query" title="Queries" aria-current="page""#),
            "{html}"
        );
        assert_eq!(
            html.matches("data-slot=\"sidebar-item\"").count(),
            3,
            "{html}"
        );
        assert_eq!(
            html.matches("aria-current=\"page\"").count(),
            2,
            "the item and the breadcrumb: {html}"
        );
    }

    #[test]
    fn a_shell_applies_to_a_request_it_did_not_come_from() {
        use http::header::{COOKIE, HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("navree_ui_sidebar=collapsed; navree_ui_flash=Saved"),
        );
        let html = shell()
            .for_request("/deals", &headers)
            .page("Deals")
            .to_view()
            .to_html();
        assert!(
            html.contains(r#"data-mobile-open" data-collapsed>"#),
            "{html}"
        );
        assert!(
            html.contains("Saved"),
            "the toast the last page left: {html}"
        );
        assert!(
            html.contains(r#"href="/deals" title="Deals" aria-current="page""#),
            "{html}"
        );
        // A page fetched ahead of a click leaves the toast for the one that is seen.
        headers.insert("x-navree-prefetch", HeaderValue::from_static("1"));
        let html = shell()
            .for_request("/deals", &headers)
            .page("Deals")
            .to_view()
            .to_html();
        assert!(!html.contains("Saved"), "{html}");
        // No cookies, an expanded sidebar.
        let html = shell()
            .for_request("/deals", &HeaderMap::new())
            .page("Deals")
            .to_view()
            .to_html();
        assert!(
            !html.contains(r#"data-mobile-open" data-collapsed>"#),
            "{html}"
        );
    }

    #[test]
    fn there_is_no_shape_for_initials_in_an_avatar() {
        let html = render(&Avatar::placeholder("Ada").into());
        assert!(!html.contains(">A<"));
        assert!(html.contains("<svg"));
    }
}
