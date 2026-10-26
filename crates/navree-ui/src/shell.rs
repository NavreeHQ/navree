//! [`Shell`]: what the app shell (`ui/app.html`) renders around a page: the
//! sidebar (switcher, groups, items, user menu), the top bar (breadcrumb,
//! search) and the command palette.

use serde::Serialize;

use crate::assets;

/// The app shell of one page, passed to the templates as `shell`.
///
/// Build it per request with the app's navigation, then mark the current
/// page with [`Shell::current`]; the item whose link matches the path best
/// is shown as active (and its parent opened).
///
/// ```
/// use navree_ui::{Crumb, MenuItem, NavGroup, NavItem, Shell, UserMenu};
///
/// let shell = Shell::new("Helm")
///     .context("Admin panel")
///     .home("/helm")
///     .group(
///         NavGroup::new("Records")
///             .item(NavItem::new("Dashboard", "/helm").icon("layout-dashboard"))
///             .item(
///                 NavItem::new("Users", "/helm/users")
///                     .icon("users")
///                     .badge("1,204")
///                     .child("All users", "/helm/users")
///                     .child("Invitations", "/helm/users/invitations"),
///             ),
///     )
///     .user(UserMenu::new("Ada Lovelace", "ada@acme.test").item(MenuItem::link("Sign out", "/helm/logout").icon("log-out")))
///     .crumb(Crumb::link("Helm", "/helm"))
///     .crumb(Crumb::page("Invitations"))
///     .title("Invitations")
///     .current("/helm/users/invitations");
///
/// let json = serde_json::to_value(&shell).unwrap();
/// assert_eq!(json["title"], "Invitations · Helm");
/// let users = &json["groups"][0]["items"][1];
/// assert_eq!(users["open"], true);
/// assert_eq!(users["children"][1]["active"], true);
/// ```
#[derive(Clone, Debug)]
pub struct Shell {
    app: AppInfo,
    page_title: Option<String>,
    lang: String,
    assets: String,
    switcher: Vec<MenuItem>,
    groups: Vec<NavGroup>,
    user: Option<UserMenu>,
    breadcrumb: Vec<Crumb>,
    search: Option<String>,
    commands: Vec<CommandGroup>,
    toasts: Vec<Toast>,
    collapsed: bool,
    from_cookie: bool,
    current: Option<String>,
}

/// The app in the switcher: its name, a second line and its home link.
#[derive(Clone, Debug, Serialize)]
struct AppInfo {
    name: String,
    context: Option<String>,
    href: String,
}

impl Shell {
    /// The shell of app `name` (shown in the switcher and the document
    /// title), with the kit's assets at the default mount
    /// ([`assets::DEFAULT_MOUNT`]) and search on.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            app: AppInfo {
                name: name.into(),
                context: None,
                href: "/".to_owned(),
            },
            page_title: None,
            lang: "en".to_owned(),
            assets: assets::url(assets::DEFAULT_MOUNT),
            switcher: Vec::new(),
            groups: Vec::new(),
            user: None,
            breadcrumb: Vec::new(),
            search: Some("Search…".to_owned()),
            commands: Vec::new(),
            toasts: Vec::new(),
            collapsed: false,
            from_cookie: false,
            current: None,
        }
    }

    /// The second line of the switcher: the plan, the environment or what
    /// the app is (`"Admin panel"`, `"Production"`).
    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.app.context = Some(context.into());
        self
    }

    /// The app's home link (the switcher links there when it has no menu).
    pub fn home(mut self, href: impl Into<String>) -> Self {
        self.app.href = href.into();
        self
    }

    /// The page title: the document title becomes `"{title} · {app}"`.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.page_title = Some(title.into());
        self
    }

    /// The document language (`lang` attribute), `"en"` by default.
    pub fn lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = lang.into();
        self
    }

    /// Where the kit's assets are served, with the version: what
    /// [`Ui::assets_url`](crate::Ui::assets_url) returns, or a relative
    /// directory for static HTML files (the gallery uses `"assets"`).
    pub fn assets(mut self, url: impl Into<String>) -> Self {
        self.assets = url.into().trim_end_matches('/').to_owned();
        self
    }

    /// An entry of the switcher's menu (other apps, workspaces, environments).
    /// With entries, the switcher opens a menu; without, it links home.
    pub fn switch(mut self, item: MenuItem) -> Self {
        self.switcher.push(item);
        self
    }

    /// A group of sidebar items.
    pub fn group(mut self, group: NavGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// The signed-in user, shown in the sidebar footer with their menu.
    pub fn user(mut self, user: UserMenu) -> Self {
        self.user = Some(user);
        self
    }

    /// A breadcrumb entry; the last one is the current page.
    pub fn crumb(mut self, crumb: Crumb) -> Self {
        self.breadcrumb.push(crumb);
        self
    }

    /// The search box's placeholder (it opens the command palette); `None`
    /// hides the search box (the palette still opens with Ctrl/Cmd+K).
    pub fn search(mut self, placeholder: Option<&str>) -> Self {
        self.search = placeholder.map(str::to_owned);
        self
    }

    /// A group of command palette entries (the sidebar's items are always
    /// in the palette, under "Go to").
    pub fn commands(mut self, group: CommandGroup) -> Self {
        self.commands.push(group);
        self
    }

    /// A toast shown when the page loads (a flash message after a redirect).
    pub fn toast(mut self, toast: Toast) -> Self {
        self.toasts.push(toast);
        self
    }

    /// Renders the sidebar collapsed to icons (desktop). Pass
    /// [`sidebar_collapsed`](crate::sidebar_collapsed) of the request's
    /// headers so the page keeps the user's choice without a flash.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self.from_cookie = true;
        self
    }

    /// Marks the current page: the item (or sub-item) whose link equals
    /// `path`, or else the longest link `path` starts with (at a `/`
    /// boundary), is active, and the item holding an active sub-item opens.
    /// The app's [`home`](Shell::home) link only matches itself.
    pub fn current(mut self, path: impl Into<String>) -> Self {
        self.current = Some(path.into());
        self
    }

    /// The groups with the active item marked.
    fn marked_groups(&self) -> Vec<NavGroup> {
        let mut groups = self.groups.clone();
        let Some(path) = self.current.as_deref() else {
            return groups;
        };
        let path = path.split(['?', '#']).next().unwrap_or(path);
        let home = self.app.href.trim_end_matches('/');
        // (group, item, child) of the best match and its score.
        let mut best: Option<((usize, usize, Option<usize>), usize)> = None;
        let mut consider = |at: (usize, usize, Option<usize>), href: &str| {
            // The app's home is a prefix of every page: it matches only itself.
            if href.trim_end_matches('/') == home && path.trim_end_matches('/') != home {
                return;
            }
            if let Some(score) = match_score(path, href)
                && best.is_none_or(|(_, top)| score > top)
            {
                best = Some((at, score));
            }
        };
        for (g, group) in groups.iter().enumerate() {
            for (i, item) in group.items.iter().enumerate() {
                consider((g, i, None), &item.href);
                for (c, child) in item.children.iter().enumerate() {
                    consider((g, i, Some(c)), &child.href);
                }
            }
        }
        if let Some(((g, i, child), _)) = best {
            let item = &mut groups[g].items[i];
            match child {
                Some(c) => {
                    item.children[c].active = true;
                    item.open = true;
                    item.has_active_child = true;
                }
                None if !item.children.is_empty() => {
                    item.open = true;
                    item.has_active_child = true;
                }
                None => item.active = true,
            }
        }
        groups
    }
}

/// How well `href` matches `path`: exact matches beat prefixes, longer
/// prefixes beat shorter ones; `None` when it does not match.
fn match_score(path: &str, href: &str) -> Option<usize> {
    let href = href.split(['?', '#']).next().unwrap_or(href);
    if href.is_empty() {
        return None;
    }
    let trimmed_path = path.trim_end_matches('/');
    let trimmed_href = href.trim_end_matches('/');
    if trimmed_path == trimmed_href {
        return Some(usize::MAX / 2 + href.len());
    }
    if trimmed_href.is_empty() {
        return None;
    }
    let rest = trimmed_path.strip_prefix(trimmed_href)?;
    rest.starts_with('/').then_some(trimmed_href.len())
}

impl Serialize for Shell {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            title: String,
            lang: &'a str,
            assets: &'a str,
            app: &'a AppInfo,
            switcher: &'a [MenuItem],
            groups: Vec<NavGroup>,
            user: Option<&'a UserMenu>,
            breadcrumb: &'a [Crumb],
            search: Option<&'a str>,
            commands: &'a [CommandGroup],
            toasts: &'a [Toast],
            collapsed: bool,
            from_cookie: bool,
        }

        let title = match &self.page_title {
            Some(page) if page != &self.app.name => format!("{page} · {}", self.app.name),
            _ => self.app.name.clone(),
        };
        View {
            title,
            lang: &self.lang,
            assets: &self.assets,
            app: &self.app,
            switcher: &self.switcher,
            groups: self.marked_groups(),
            user: self.user.as_ref(),
            breadcrumb: &self.breadcrumb,
            search: self.search.as_deref(),
            commands: &self.commands,
            toasts: &self.toasts,
            collapsed: self.collapsed,
            from_cookie: self.from_cookie,
        }
        .serialize(serializer)
    }
}

/// A labelled group of sidebar items (`"Platform"`, `"Operate"`). The label
/// is optional.
#[derive(Clone, Debug, Default, Serialize)]
pub struct NavGroup {
    label: Option<String>,
    items: Vec<NavItem>,
}

impl NavGroup {
    /// A group with a label (sentence case: `"Records"`, not `"RECORDS"`).
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            items: Vec::new(),
        }
    }

    /// A group without a label.
    pub fn unlabeled() -> Self {
        Self::default()
    }

    /// Adds an item.
    pub fn item(mut self, item: NavItem) -> Self {
        self.items.push(item);
        self
    }
}

/// A sidebar item: a 16px icon, a label, an optional count, and optional
/// sub-items (then the item opens and closes them; collapsed to icons, it
/// follows its own link).
#[derive(Clone, Debug, Serialize)]
pub struct NavItem {
    label: String,
    href: String,
    icon: Option<String>,
    badge: Option<String>,
    children: Vec<NavLink>,
    active: bool,
    open: bool,
    has_active_child: bool,
}

impl NavItem {
    /// An item linking to `href`.
    pub fn new(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            icon: None,
            badge: None,
            children: Vec::new(),
            active: false,
            open: false,
            has_active_child: false,
        }
    }

    /// The lucide icon (one of [`ICONS`](crate::ICONS), e.g. `"users"`).
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        debug_assert!(crate::is_icon(&name), "unknown navree-ui icon `{name}`");
        self.icon = Some(name);
        self
    }

    /// A count or short badge, right-aligned in mono (`"12"`, `"1.2k"`).
    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Adds a sub-item.
    pub fn child(mut self, label: impl Into<String>, href: impl Into<String>) -> Self {
        self.children.push(NavLink {
            label: label.into(),
            href: href.into(),
            badge: None,
            active: false,
        });
        self
    }

    /// Adds a sub-item with a count.
    pub fn child_with_badge(
        mut self,
        label: impl Into<String>,
        href: impl Into<String>,
        badge: impl Into<String>,
    ) -> Self {
        self.children.push(NavLink {
            label: label.into(),
            href: href.into(),
            badge: Some(badge.into()),
            active: false,
        });
        self
    }

    /// Shows the item as the current page (or, with sub-items, open);
    /// [`Shell::current`] does this from the path.
    pub fn active(mut self) -> Self {
        if self.children.is_empty() {
            self.active = true;
        } else {
            self.open = true;
        }
        self
    }

    /// Shows the sub-items without an active one.
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }
}

/// A sub-item of a [`NavItem`].
#[derive(Clone, Debug, Serialize)]
pub struct NavLink {
    label: String,
    href: String,
    badge: Option<String>,
    active: bool,
}

/// A breadcrumb entry.
#[derive(Clone, Debug, Serialize)]
pub struct Crumb {
    label: String,
    href: Option<String>,
}

impl Crumb {
    /// An entry linking to `href`.
    pub fn link(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
        }
    }

    /// The current page (no link).
    pub fn page(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: None,
        }
    }
}

/// The signed-in user in the sidebar footer: a 32px square avatar (the
/// image, or the initials), the name and email, and a menu.
#[derive(Clone, Debug, Serialize)]
pub struct UserMenu {
    name: String,
    email: String,
    avatar: Option<String>,
    initials: String,
    menu: Vec<MenuItem>,
}

impl UserMenu {
    /// The user `name` with `email`.
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            initials: initials(&name),
            name,
            email: email.into(),
            avatar: None,
            menu: Vec::new(),
        }
    }

    /// The avatar image URL (same origin: the pages' policy allows images
    /// from `'self'` and `data:` only).
    pub fn avatar(mut self, url: impl Into<String>) -> Self {
        self.avatar = Some(url.into());
        self
    }

    /// Adds a menu entry.
    pub fn item(mut self, item: MenuItem) -> Self {
        self.menu.push(item);
        self
    }
}

/// Up to two initials: `"Ada Lovelace"` is `"AL"`, `"ada"` is `"A"`.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let pick = match words.as_slice() {
        [] => Vec::new(),
        [one] => vec![*one],
        [first, .., last] => vec![*first, *last],
    };
    pick.iter()
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// An entry of a menu (the switcher, the user menu, row actions, dropdowns):
/// a link, a button that opens a dialog, a separator or a label.
#[derive(Clone, Debug, Serialize)]
pub struct MenuItem {
    kind: &'static str,
    label: String,
    href: Option<String>,
    dialog: Option<String>,
    icon: Option<String>,
    shortcut: Option<String>,
    description: Option<String>,
    danger: bool,
    checked: Option<bool>,
}

impl MenuItem {
    fn base(kind: &'static str, label: String) -> Self {
        Self {
            kind,
            label,
            href: None,
            dialog: None,
            icon: None,
            shortcut: None,
            description: None,
            danger: false,
            checked: None,
        }
    }

    /// A link.
    pub fn link(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            href: Some(href.into()),
            ..Self::base("item", label.into())
        }
    }

    /// A button opening the dialog with id `dialog` (see the `dialog` and
    /// `confirm_dialog` macros).
    pub fn dialog(label: impl Into<String>, dialog: impl Into<String>) -> Self {
        Self {
            dialog: Some(dialog.into()),
            ..Self::base("item", label.into())
        }
    }

    /// A separator line.
    pub fn separator() -> Self {
        Self::base("separator", String::new())
    }

    /// A small label above the entries that follow.
    pub fn label(label: impl Into<String>) -> Self {
        Self::base("label", label.into())
    }

    /// The lucide icon.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        debug_assert!(crate::is_icon(&name), "unknown navree-ui icon `{name}`");
        self.icon = Some(name);
        self
    }

    /// A keyboard shortcut shown on the right (`"⇧⌘Q"`).
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// A second line of text (the switcher shows it, e.g. a plan).
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Destructive: drawn in the danger color.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// A checkable entry (`menuitemcheckbox`), checked or not; the switcher
    /// marks the current app or workspace this way.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
}

/// A group of command palette entries.
#[derive(Clone, Debug, Serialize)]
pub struct CommandGroup {
    label: String,
    items: Vec<Command>,
}

impl CommandGroup {
    /// A group titled `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            items: Vec::new(),
        }
    }

    /// Adds an entry.
    pub fn item(mut self, command: Command) -> Self {
        self.items.push(command);
        self
    }
}

/// A command palette entry: a link or a dialog to open.
#[derive(Clone, Debug, Serialize)]
pub struct Command {
    label: String,
    href: Option<String>,
    dialog: Option<String>,
    icon: Option<String>,
    hint: Option<String>,
    keywords: Option<String>,
}

impl Command {
    /// An entry going to `href`.
    pub fn link(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
            dialog: None,
            icon: None,
            hint: None,
            keywords: None,
        }
    }

    /// An entry opening the dialog with id `dialog`.
    pub fn dialog(label: impl Into<String>, dialog: impl Into<String>) -> Self {
        Self {
            href: None,
            dialog: Some(dialog.into()),
            ..Self::link(label, "")
        }
    }

    /// The lucide icon.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// Text on the right (a shortcut or where the entry leads).
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Extra words the filter matches (not shown).
    pub fn keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }
}

/// A toast: a short message in the corner that dismisses itself.
#[derive(Clone, Debug, Serialize)]
pub struct Toast {
    variant: &'static str,
    title: String,
    description: Option<String>,
}

impl Toast {
    fn new(variant: &'static str, title: impl Into<String>) -> Self {
        Self {
            variant,
            title: title.into(),
            description: None,
        }
    }

    /// A success (a check in the accent color).
    pub fn success(title: impl Into<String>) -> Self {
        Self::new("success", title)
    }

    /// An error.
    pub fn error(title: impl Into<String>) -> Self {
        Self::new("error", title)
    }

    /// A warning.
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new("warning", title)
    }

    /// Neutral information.
    pub fn info(title: impl Into<String>) -> Self {
        Self::new("info", title)
    }

    /// A second line.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_and_last_words() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("Grace Brewster Murray Hopper"), "GH");
        assert_eq!(initials("ada"), "A");
        assert_eq!(initials("  "), "");
        assert_eq!(initials("élodie durand"), "ÉD");
    }

    #[test]
    fn match_scores_prefer_exact_then_longer_prefixes() {
        assert!(match_score("/helm/users", "/helm/users").is_some());
        assert_eq!(match_score("/helm/users", "/helm/us"), None);
        assert_eq!(match_score("/helm/users/42", "/helm/users"), Some(11));
        assert_eq!(match_score("/helm/users/42", "/helm"), Some(5));
        assert!(match_score("/helm", "/helm/") > match_score("/helm/users", "/helm"));
        assert_eq!(match_score("/helm", "/"), None);
        assert!(match_score("/", "/").is_some());
        assert!(match_score("/helm/users", "/helm/users?sort=name").is_some());
    }

    #[test]
    fn current_marks_the_best_match_and_opens_its_parent() {
        let shell = Shell::new("Helm")
            .home("/helm")
            .group(
                NavGroup::new("Records")
                    .item(NavItem::new("Dashboard", "/helm"))
                    .item(
                        NavItem::new("Users", "/helm/users")
                            .child("All users", "/helm/users")
                            .child("Roles", "/helm/users/roles"),
                    ),
            )
            .current("/helm/users/roles/7");
        let json = serde_json::to_value(&shell).unwrap();
        let items = &json["groups"][0]["items"];
        assert_eq!(items[0]["active"], false, "the dashboard is a shorter prefix");
        assert_eq!(items[1]["open"], true);
        assert_eq!(items[1]["has_active_child"], true);
        assert_eq!(items[1]["children"][0]["active"], false);
        assert_eq!(items[1]["children"][1]["active"], true);

        // A page outside the navigation leaves the home item alone.
        let json = serde_json::to_value(
            Shell::new("Helm")
                .home("/helm")
                .group(NavGroup::unlabeled().item(NavItem::new("Dashboard", "/helm")))
                .current("/helm/settings"),
        )
        .unwrap();
        assert_eq!(json["groups"][0]["items"][0]["active"], false);
    }

    #[test]
    fn title_joins_page_and_app() {
        let json = serde_json::to_value(Shell::new("Fleet")).unwrap();
        assert_eq!(json["title"], "Fleet");
        let json = serde_json::to_value(Shell::new("Fleet").title("Queues")).unwrap();
        assert_eq!(json["title"], "Queues · Fleet");
    }
}
