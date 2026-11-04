//! The application shell: a sidebar (docked, inset or floating, collapsing to
//! icons by default) or links in the top bar, a top bar and the page. [`Shell`]
//! is the app's one definition; [`AppShell`] is it applied to a request;
//! [`AppPage`] is what a handler returns.
//!
//! Provenance: the anatomy follows shadcn/ui's Sidebar (MIT). See THIRD_PARTY.md.

use std::borrow::Cow;
use std::sync::Arc;

use http::header::{COOKIE, HeaderValue, InvalidHeaderValue};
use http::request::Parts;
use http::{HeaderMap, StatusCode};
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
use crate::theme::{Appearance, Density, Theme, ThemeMenu, ThemeSetting};
use crate::view::View;

/// The cookie holding a toast for the next page.
pub(crate) const FLASH_COOKIE: &str = "navree_ui_flash";

/// The cookie holding the session's CSRF token, when there is a session.
const CSRF_COOKIE: &str = "XSRF-TOKEN";

/// The cookie the runtime sets when the sidebar collapses or expands.
pub const SIDEBAR_COOKIE: &str = "navree_ui_sidebar";

/// Where an app's navigation stands. On phones every layout puts it in a sheet
/// that slides over the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShellLayout {
    /// A sidebar docked to the edge of the window: the default.
    #[default]
    Sidebar,
    /// The sidebar on the frame's colour, and the page in a card beside it.
    Inset,
    /// The sidebar as a card floating over the frame.
    Floating,
    /// The links in the top bar, and no sidebar on wide screens.
    Header,
}

impl ShellLayout {
    /// Every layout.
    pub const ALL: [Self; 4] = [Self::Sidebar, Self::Inset, Self::Floating, Self::Header];

    /// The `data-layout` value, and the id a form posts.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Inset => "inset",
            Self::Floating => "floating",
            Self::Header => "header",
        }
    }

    /// The name shown.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sidebar => "Sidebar",
            Self::Inset => "Inset",
            Self::Floating => "Floating",
            Self::Header => "Header",
        }
    }

    /// The layout with the id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.id() == id)
    }
}

/// What the sidebar does when its button or `Ctrl/Cmd+B` folds it, on wide
/// screens. On phones it is always a sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Collapse {
    /// Folds to a rail of icons: the default.
    #[default]
    Icons,
    /// Slides away entirely.
    Offcanvas,
    /// Never folds: no button on wide screens.
    Never,
}

impl Collapse {
    /// The `data-collapse` value.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Icons => "icons",
            Self::Offcanvas => "offcanvas",
            Self::Never => "never",
        }
    }
}

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

    fn to_node(&self, csrf: Option<&str>, side: &'static str) -> Node {
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
            .attr("data-side", side)
            .attr_opt("data-align", (side == "bottom").then_some("end"))
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
/// let shell = Shell::new("Cloudworks CRM").context("Production").group(
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
    appearance: Appearance,
    themes: Vec<Theme>,
    theme_menu: bool,
    layout: ShellLayout,
    collapse: Collapse,
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
            appearance: Appearance::default(),
            themes: Theme::BUILT_IN.to_vec(),
            theme_menu: false,
            layout: ShellLayout::Sidebar,
            collapse: Collapse::Icons,
        }
    }

    /// Sets how every page looks unless a workspace, the reader or the URL says
    /// otherwise: the app's default theme and mode. Dark Navree with nothing set.
    #[must_use]
    pub fn appearance(mut self, appearance: Appearance) -> Self {
        self.appearance = appearance;
        self
    }

    /// Sets the themes a reader may choose: the [`ThemeMenu`]'s cards, and the
    /// only themes a cookie or the URL can pick. Every built-in by default.
    #[must_use]
    pub fn themes(mut self, themes: impl IntoIterator<Item = Theme>) -> Self {
        self.themes = themes.into_iter().collect();
        self
    }

    /// Puts a [`ThemeMenu`] in the top bar, posting to the toolkit's route,
    /// which keeps the reader's choice in a cookie.
    #[must_use]
    pub fn theme_menu(mut self) -> Self {
        self.theme_menu = true;
        self
    }

    /// Sets where the navigation stands: [`ShellLayout::Sidebar`] by default.
    #[must_use]
    pub fn layout(mut self, layout: ShellLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Sets what folding the sidebar does: [`Collapse::Icons`] by default.
    #[must_use]
    pub fn collapse(mut self, collapse: Collapse) -> Self {
        self.collapse = collapse;
        self
    }

    /// The app's default appearance.
    pub fn default_appearance(&self) -> &Appearance {
        &self.appearance
    }

    /// The themes a reader may choose.
    pub fn offered_themes(&self) -> &[Theme] {
        &self.themes
    }

    /// `setting` with its theme kept only when the app offers it.
    fn offered(&self, mut setting: ThemeSetting) -> ThemeSetting {
        if setting.theme.as_ref().is_some_and(|theme| {
            !self
                .themes
                .iter()
                .any(|offered| offered.short_form() == theme.short_form())
        }) {
            setting.theme = None;
        }
        setting
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
        let appearance = self.appearance.clone();
        AppPage::new(
            Arc::new(self.clone()),
            title.into(),
            path.into(),
            false,
            None,
            None,
        )
        .appearance(appearance)
    }

    /// This shell applied to a request: which link is active, whether the sidebar is
    /// collapsed, a toast left for this page, the reader's theme. It is what the
    /// [`AppShell`] extractor does for the app's own `Shell`, for a module that
    /// serves pages of its own and has no business registering one for the whole
    /// app (a debugging tool). A `?` in `path` starts its query.
    pub fn for_request(&self, path: impl Into<String>, headers: &HeaderMap) -> AppShell {
        let path = path.into();
        let (path, query) = match path.split_once('?') {
            Some((path, query)) => (path.to_owned(), Some(query.to_owned())),
            None => (path, None),
        };
        AppShell::read(Arc::new(self.clone()), path, query, headers)
    }

    /// The app's mark and name, linking home.
    fn brand(&self) -> Element {
        el("a")
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
            ))
    }

    /// How well the best link matches `path`.
    fn best(&self, path: &str) -> usize {
        let home = self.home.as_str();
        self.groups
            .iter()
            .flat_map(|group| &group.items)
            .map(|item| item.best(path, home))
            .max()
            .unwrap_or(0)
    }

    fn sidebar(&self, path: &str, csrf: Option<&str>, footer: bool) -> Element {
        let home = self.home.as_str();
        let header = self.brand();
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
                self.user.as_ref().filter(|_| footer).map(|user| {
                    el("div")
                        .slot("sidebar-footer")
                        .child(user.to_node(csrf, "right"))
                }),
            ))
    }

    /// The navigation in a row, for the top bar of [`ShellLayout::Header`]: the
    /// links of a group with no label, and a menu for each labelled group, so the
    /// bar stays short. An item with items below it opens them in a menu too.
    fn topnav(&self, path: &str) -> Element {
        let home = self.home.as_str();
        let best = self.best(path);
        let here = |item: &NavItem| {
            item.current
                .unwrap_or(best > 0 && item.score(path, home) == best)
        };
        let within = |item: &NavItem| here(item) || item.sub.iter().any(&here);
        let link = |item: &NavItem, slot: &'static str| {
            el("a")
                .slot(slot)
                .href(item.href.clone())
                .attr_opt("aria-current", here(item).then_some("page"))
                .flag("data-ui-prefetch", item.prefetch == Prefetch::Intent)
                .children((item.glyph.map(icon), el("span").child(item.label.clone())))
        };
        let menu = |id: String,
                    label: Cow<'static, str>,
                    glyph: Option<Icon>,
                    active: bool,
                    links: Vec<Element>| {
            el("li").children((
                el("button")
                    .slot("topnav-item")
                    .attr("type", "button")
                    .attr("popovertarget", id.clone())
                    .flag("data-active", active)
                    .children((
                        glyph.map(icon),
                        el("span").child(label.clone()),
                        icon(icons::CHEVRON_DOWN),
                    )),
                el("div")
                    .slot("popover-content")
                    .attr("id", id)
                    .attr("popover", "auto")
                    .attr("aria-label", label)
                    .attr("tabindex", -1_i64)
                    .flag("data-ui-place", true)
                    .attr("data-side", "bottom")
                    .attr("data-align", "start")
                    .uses(Uses::OVERLAY)
                    .child(el("ul").slot("topnav-sub").child(links)),
            ))
        };
        // An item and the items below it, as rows of a menu.
        let rows = |item: &NavItem| -> Vec<Element> {
            std::iter::once(el("li").child(link(item, "menu-link")))
                .chain(item.sub.iter().map(|sub| {
                    el("li")
                        .attr("data-nested", "")
                        .child(link(sub, "menu-link"))
                }))
                .collect()
        };
        let mut items: Vec<Element> = Vec::new();
        for (at, group) in self.groups.iter().enumerate() {
            match &group.label {
                Some(label) => {
                    let active = group.items.iter().any(within);
                    let links = group.items.iter().flat_map(rows).collect();
                    items.push(menu(
                        format!("ui-topnav-{at}"),
                        label.clone(),
                        None,
                        active,
                        links,
                    ));
                }
                None => {
                    for (n, item) in group.items.iter().enumerate() {
                        if item.sub.is_empty() {
                            items.push(el("li").child(link(item, "topnav-item")));
                        } else {
                            items.push(menu(
                                format!("ui-topnav-{at}-{n}"),
                                item.label.clone(),
                                item.glyph,
                                within(item),
                                rows(item),
                            ));
                        }
                    }
                }
            }
        }
        el("nav")
            .slot("topnav")
            .attr("aria-label", "Main")
            .child(el("ul").slot("topnav-menu").child(items))
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
    appearance: Appearance,
    layout: ShellLayout,
    sheets: Vec<Theme>,
    density: Option<Density>,
    back: Option<String>,
    theme_locked: bool,
    status: StatusCode,
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
            title,
            path,
            collapsed,
            flash,
            csrf,
            crumbs: Vec::new(),
            prefetch_crumbs: false,
            actions: Vec::new(),
            body: Vec::new(),
            appearance: shell.appearance.clone(),
            layout: shell.layout,
            sheets: Vec::new(),
            density: None,
            back: None,
            theme_locked: false,
            status: StatusCode::OK,
            shell,
        }
    }

    /// Sets how this page looks, in place of what the app, the workspace and the
    /// reader chose: a preview, say.
    pub fn appearance(mut self, appearance: Appearance) -> Self {
        self.appearance = appearance;
        self
    }

    /// Sets where this page's navigation stands, in place of the app's layout.
    pub fn layout(mut self, layout: ShellLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Sets this page's density, in place of its theme's.
    pub fn density(mut self, density: Density) -> Self {
        self.density = Some(density);
        self
    }

    /// Links the stylesheet of a theme of your own that a part of the page is
    /// drawn in. See [`View::theme_sheet`].
    pub fn theme_sheet(mut self, theme: &Theme) -> Self {
        self.sheets.push(theme.clone());
        self
    }

    /// The appearance the page is drawn in.
    pub fn current_appearance(&self) -> &Appearance {
        &self.appearance
    }

    /// Sets the status the page answers with (`200` by default): the status of an
    /// error page drawn in the shell.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
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
    pub fn to_view(mut self) -> View {
        let shell = self.shell.clone();
        let crumbs_empty = self.crumbs.is_empty();
        let mut crumbs = Breadcrumb::new();
        for (label, href) in std::mem::take(&mut self.crumbs) {
            crumbs = crumbs.link(label, href);
        }
        crumbs = crumbs.current(self.title.clone());
        if self.prefetch_crumbs {
            crumbs = crumbs.prefetch();
        }
        let header = self.layout == ShellLayout::Header;
        let collapsed = self.collapsed && shell.collapse != Collapse::Never && !header;
        let trigger = Button::icon_only(icons::PANEL_LEFT, "Toggle sidebar")
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .attr("data-ui-sidebar-trigger", "")
            .attr("aria-expanded", if collapsed { "false" } else { "true" });
        let menu = shell.theme_menu.then(|| {
            let mut menu = ThemeMenu::new(&self.appearance, &shell.themes)
                .back(self.back.clone().unwrap_or_else(|| self.path.clone()));
            // A locked workspace keeps its theme: the reader picks only the mode.
            if self.theme_locked {
                menu = menu.without_themes();
            }
            if let Some(token) = &self.csrf {
                menu = menu.csrf(token.clone());
            }
            menu
        });
        if let Some(menu) = &menu {
            self.sheets.extend(menu.sheets());
        }
        let topbar = el("header").slot("topbar").children((
            trigger,
            (!header).then(Separator::vertical),
            header.then(|| el("div").slot("topbar-brand").child(shell.brand())),
            header.then(|| shell.topnav(&self.path)),
            (!header).then(|| crumbs.clone()),
            el("div").slot("topbar-spacer"),
            shell.search.clone(),
            menu,
            shell.user.as_ref().filter(|_| header).map(|user| {
                el("div")
                    .slot("topbar-user")
                    .child(user.to_node(self.csrf.as_deref(), "bottom"))
            }),
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
            (!self.actions.is_empty()).then(|| {
                el("div")
                    .slot("page-actions")
                    .child(std::mem::take(&mut self.actions))
            }),
        ));
        let main = el("main")
            .slot("page")
            .attr("id", "main")
            .attr("tabindex", -1_i64)
            .children((
                // With the links in the top bar, the trail stands over the heading.
                (header && !crumbs_empty).then_some(crumbs),
                heading,
                std::mem::take(&mut self.body),
            ));
        let wrapper = el("div")
            .slot("sidebar-wrapper")
            .attr("data-ui-keep-attrs", "data-collapsed data-mobile-open")
            .attr_opt(
                "data-layout",
                (self.layout != ShellLayout::Sidebar).then_some(self.layout.id()),
            )
            .attr_opt(
                "data-collapse",
                (shell.collapse != Collapse::Icons).then_some(shell.collapse.id()),
            )
            .flag("data-collapsed", collapsed)
            .uses(Uses::SIDEBAR.with(Uses::CORE))
            .children((
                shell.sidebar(&self.path, self.csrf.as_deref(), !header),
                el("div").slot("sidebar-overlay"),
                el("div").slot("sidebar-inset").children((topbar, main)),
            ));
        let mut view = View::new(self.title)
            .appearance(self.appearance)
            .status(self.status)
            .body((wrapper, toaster));
        if let Some(density) = self.density {
            view = view.density(density);
        }
        for theme in &self.sheets {
            view = view.theme_sheet(theme);
        }
        if let Some(token) = &self.csrf {
            view = view.csrf(token.clone());
        }
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
    query: Option<String>,
    collapsed: bool,
    flash: Option<(ToastTone, String)>,
    csrf: Option<String>,
    cookie: ThemeSetting,
    url: ThemeSetting,
    workspace: ThemeSetting,
    user: Option<ThemeSetting>,
}

impl AppShell {
    /// A page titled `title`.
    pub fn page(&self, title: impl Into<Cow<'static, str>>) -> AppPage {
        let mut page = AppPage::new(
            self.shell.clone(),
            title.into(),
            self.path.clone(),
            self.collapsed,
            self.flash.clone(),
            self.csrf.clone(),
        )
        .appearance(self.appearance());
        page.back = Some(match self.query.as_deref().map(without_appearance) {
            Some(query) if !query.is_empty() => format!("{}?{query}", self.path),
            _ => self.path.clone(),
        });
        page.theme_locked = self.workspace.locked;
        page
    }

    /// Sets what the workspace chose, from the app's store: it lies over the
    /// app's default, and when locked the reader can't change its theme.
    #[must_use]
    pub fn workspace(mut self, setting: ThemeSetting) -> Self {
        self.workspace = setting;
        self
    }

    /// Sets what the reader chose, from the app's store, in place of the cookie.
    /// A theme set here is the app's word: it needn't be one the shell offers.
    #[must_use]
    pub fn user(mut self, setting: ThemeSetting) -> Self {
        self.user = Some(setting);
        self
    }

    /// How this request's pages look: the app's default, then the workspace's,
    /// then the reader's (the app's store, else the cookie), then `?theme=` and
    /// `?mode=`. A locked workspace keeps its theme; the mode is still the reader's.
    pub fn appearance(&self) -> Appearance {
        let mut appearance = self.shell.appearance.clone();
        self.workspace.over(&mut appearance, false);
        let locked = self.workspace.locked;
        self.user
            .as_ref()
            .unwrap_or(&self.cookie)
            .over(&mut appearance, locked);
        self.url.over(&mut appearance, locked);
        appearance
    }
}

/// `query` without `theme` and `mode`, however spelt: the theme menu comes back
/// to this address, which would otherwise undo the choice just made.
fn without_appearance(query: &str) -> String {
    query
        .split('&')
        .filter(|pair| {
            let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
            let key = urlencoded::parse(format!("{key}=").as_bytes())
                .next()
                .map(|(key, _)| key.into_owned())
                .unwrap_or_default();
            !pair.is_empty() && key != "theme" && key != "mode"
        })
        .collect::<Vec<_>>()
        .join("&")
}

impl AppShell {
    /// `shell` applied to a request with these headers, at `path`.
    fn read(shell: Arc<Shell>, path: String, query: Option<String>, headers: &HeaderMap) -> Self {
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
        let (cookie, url) = crate::theme::read_setting(headers, query.as_deref());
        let (cookie, url) = (shell.offered(cookie), shell.offered(url));
        Self {
            shell,
            path,
            query,
            collapsed,
            flash,
            csrf,
            cookie,
            url,
            workspace: ThemeSetting::none(),
            user: None,
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
            parts.uri.query().map(str::to_owned),
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
            html.contains(
                r#"<section data-slot="toaster" id="ui-toaster" aria-label="Notifications""#
            ),
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
    fn a_menus_post_items_post_through_the_one_post_form_with_the_token() {
        use crate::components::{DropdownMenu, MenuItem};
        let menu = || {
            DropdownMenu::new("m", "Actions")
                .item(MenuItem::post("Archive", "/a"))
                .item(MenuItem::post("Delete", "/d"))
        };
        let page = |csrf: Option<&str>, body: crate::node::Node| {
            AppPage::new(
                Arc::new(shell()),
                "x".into(),
                "/".to_owned(),
                false,
                None,
                csrf.map(str::to_owned),
            )
            .body(body)
            .to_view()
            .to_html()
        };
        // Inside a form of the page, as the docs invite: the items still belong to the post form.
        let form = el("form").attr("action", "/save").child(menu());
        let html = page(Some("tok"), form.into());
        assert_eq!(html.matches(r#"<form id="ui-post""#).count(), 1, "{html}");
        assert!(
            html.contains(r#"<form id="ui-post" method="post" novalidate hidden data-ui-form><input type="hidden" name="_csrf" value="tok"></form>"#),
            "{html}"
        );
        // After everything else, outside every form of the page.
        let post_form = html.find(r#"<form id="ui-post""#).unwrap();
        assert!(html.find("</form>").unwrap() < post_form, "{html}");
        // No post item, no post form.
        let plain = page(Some("tok"), el("p").child("hi").into());
        assert!(!plain.contains("ui-post"), "{plain}");
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
