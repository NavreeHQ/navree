//! The app's shell for a native client: a page in the shell as a note its
//! [`View`] keeps, and the shell as the document a native client draws its
//! own chrome from.
//!
//! A native client draws the chrome itself, so a page built while one asks
//! never builds the sidebar, the top bar or the heading: the view keeps what
//! the page is made of, and the screen is read from that. Should the view be
//! written as HTML after all (a page kept and given to a browser later), the
//! chrome is built then, and the bytes are the ones a browser always gets.

use std::sync::OnceLock;

use http::HeaderValue;
use navree_native::action::{Action, Pair, Present};
use navree_native::node as n;
use navree_native::shell as s;

use super::{AppPage, AppShell, FLASH_COOKIE, MenuEntry, NavItem, Shell, UserMenu};
use crate::form::CSRF_FIELD;
use crate::node::Node;
use crate::op::ToastTone;
use crate::theme::{ThemeMenu, Token};
use crate::view::View;

/// What a page in the shell is made of, kept by the [`View`] it became while
/// a native client asked: its title, its steps, its actions and its body, and
/// the shell around it.
#[derive(Clone, Debug)]
pub(crate) struct PageNote {
    page: AppPage,
    /// The body a browser gets, built the first time something writes the
    /// view as HTML.
    chrome: OnceLock<Node>,
}

impl PageNote {
    /// The page's title.
    pub(crate) fn title(&self) -> &str {
        &self.page.title
    }

    /// The steps before the page, the app's first page first: each one's name
    /// and address.
    pub(crate) fn steps(&self) -> impl Iterator<Item = (&str, &str)> {
        self.page
            .crumbs
            .iter()
            .map(|(label, href)| (label.as_ref(), href.as_str()))
    }

    /// What stands beside the page's heading.
    pub(crate) fn actions(&self) -> &[Node] {
        &self.page.actions
    }

    /// The page's content.
    pub(crate) fn body(&self) -> &[Node] {
        &self.page.body
    }

    /// The address of the shell's destination the page belongs to: the link
    /// the sidebar marks as current.
    pub(crate) fn nav(&self) -> Option<String> {
        let shell = &self.page.shell;
        let path = self.page.path.as_str();
        let home = shell.home.as_str();
        let best = shell.best(path);
        fn current(items: &[NavItem], path: &str, home: &str, best: usize) -> Option<String> {
            items.iter().find_map(|item| {
                let active = item
                    .current
                    .unwrap_or(best > 0 && item.score(path, home) == best);
                if active {
                    Some(item.href.as_str().to_owned())
                } else {
                    current(&item.sub, path, home, best)
                }
            })
        }
        shell
            .groups
            .iter()
            .find_map(|group| current(&group.items, path, home, best))
    }

    /// A toast left for the page by the request before.
    pub(crate) fn flash(&self) -> Option<(ToastTone, &str)> {
        self.page
            .flash
            .as_ref()
            .map(|(tone, text)| (*tone, text.as_str()))
    }

    /// The body a browser gets: the shell with the page in it.
    pub(crate) fn chrome(&self) -> &Node {
        self.chrome
            .get_or_init(|| self.page.clone().in_shell().take_body())
    }

    /// Whether the chrome was built.
    #[cfg(test)]
    pub(crate) fn chrome_built(&self) -> bool {
        self.chrome.get().is_some()
    }

    /// [`chrome`](Self::chrome), owned.
    pub(crate) fn into_chrome(self) -> Node {
        match self.chrome.into_inner() {
            Some(body) => body,
            None => self.page.in_shell().take_body(),
        }
    }
}

/// The view of `page` for a native client: everything a browser's view has
/// but the chrome, with the page kept beside it.
pub(super) fn noted(page: AppPage) -> View {
    let mut view = View::new(page.title.clone())
        .appearance(page.appearance.clone())
        .status(page.status);
    if let Some(density) = page.density {
        view = view.density(density);
    }
    // The theme menu's sheets, as the chrome links them, so a view written as
    // HTML later has them in the same order.
    let mut sheets = page.sheets.clone();
    if page.shell.theme_menu {
        let mut menu = ThemeMenu::new(&page.appearance, &page.shell.themes);
        if page.theme_locked {
            menu = menu.without_themes();
        }
        sheets.extend(menu.sheets());
    }
    for theme in &sheets {
        view = view.theme_sheet(theme);
    }
    if let Some(token) = &page.csrf {
        view = view.csrf(token.clone());
    }
    if page.flash.is_some()
        && let Ok(clear) = HeaderValue::from_str(&format!(
            "{FLASH_COOKIE}=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly"
        ))
    {
        view = view.cookie(clear);
    }
    view.with_note(PageNote {
        page,
        chrome: OnceLock::new(),
    })
}

impl AppShell {
    /// The shell as the document a native client draws its chrome from: the
    /// app's name, its destinations, the person signed in with their menu,
    /// the theme's accent for a light and a dark ground, and how screens are
    /// presented. `build` is the app's stamp; `sign_in` is where a guest goes,
    /// which a client presents over everything, with no chrome.
    pub(crate) fn native_document(
        &self,
        build: &str,
        sign_in: Option<&str>,
    ) -> navree_native::Shell {
        let mut document = document(
            &self.shell,
            self.user_menu.as_ref(),
            self.csrf.as_deref(),
            &self.appearance(),
            build,
        );
        document.routes = routes(sign_in);
        document
    }
}

impl PageNote {
    /// The stamp of the shell the page is drawn in, as the shell's own
    /// document says it, so a client that sees another fetches the shell again.
    pub(crate) fn shell_stamp(&self) -> String {
        let page = &self.page;
        shell_stamp(&document(
            &page.shell,
            page.user.as_ref(),
            page.csrf.as_deref(),
            &page.appearance,
            "",
        ))
    }
}

/// The stamp of a shell document: of what the person signed in sees in it,
/// which is the same whether the document or a page of it is asked for.
pub(crate) fn shell_stamp(document: &navree_native::Shell) -> String {
    let seen = (
        &document.name,
        &document.context,
        &document.home,
        &document.nav,
        &document.user,
        &document.theme,
    );
    crate::native::routes::stamp_of(&serde_json::to_string(&seen).unwrap_or_default())
}

/// The document of `shell`, for the person `user` with the session's token
/// `csrf`, in `appearance`.
fn document(
    shell: &Shell,
    user: Option<&UserMenu>,
    csrf: Option<&str>,
    appearance: &crate::theme::Appearance,
    build: &str,
) -> navree_native::Shell {
    let mut document = navree_native::Shell::new(build, shell.name.clone().into_owned());
    document.context = shell.context.clone().map(std::borrow::Cow::into_owned);
    document.home = shell.home.as_str().to_owned();
    document.nav.groups = shell
        .groups
        .iter()
        .map(|group| s::NavGroup {
            label: group.label.clone().map(std::borrow::Cow::into_owned),
            items: group.items.iter().map(nav_item).collect(),
        })
        .collect();
    document.user = user.or(shell.user.as_ref()).map(|user| user_of(user, csrf));
    let spec = appearance.theme().spec();
    document.theme = Some(s::Theme {
        name: Some(appearance.theme().key().to_owned()),
        accent: Some(spec.light.get(Token::Accent).to_hex()),
        accent_dark: Some(spec.dark.get(Token::Accent).to_hex()),
    });
    document
}

/// The person signed in, with their menu: links, and buttons that post with
/// the session's token.
fn user_of(user: &UserMenu, csrf: Option<&str>) -> s::User {
    let menu = user
        .links
        .iter()
        .map(|entry| match entry {
            MenuEntry::Link(label, href) => N::Link(n::Link {
                id: None,
                label: label.clone().into_owned(),
                action: Action::navigate(href.as_str()),
                children: Vec::new(),
            }),
            MenuEntry::Action(label, href) => N::Button(n::Button {
                label: label.clone().into_owned(),
                action: Some(Action::Request {
                    method: n::Method::Post,
                    href: href.as_str().to_owned(),
                    fields: csrf
                        .iter()
                        .map(|token| Pair {
                            name: CSRF_FIELD.to_owned(),
                            value: (*token).to_owned(),
                        })
                        .collect(),
                }),
                ..n::Button::default()
            }),
        })
        .collect();
    s::User {
        name: user.name.clone().into_owned(),
        email: (!user.email.is_empty()).then(|| user.email.clone().into_owned()),
        menu,
    }
}

type N = n::Node;

/// One destination, with the ones under it.
fn nav_item(item: &NavItem) -> s::NavItem {
    s::NavItem {
        label: item.label.clone().into_owned(),
        icon: item.glyph.and_then(crate::native::page::icon_of),
        href: item.href.as_str().to_owned(),
        badge: item.count.clone().map(std::borrow::Cow::into_owned),
        items: item.sub.iter().map(nav_item).collect(),
    }
}

/// How screens are presented, the first match first: the sign-in page over
/// everything with no chrome, and a new or an edited record in a sheet.
pub(crate) fn routes(sign_in: Option<&str>) -> Vec<s::Route> {
    let mut routes = Vec::with_capacity(4);
    if let Some(sign_in) = sign_in {
        routes.push(s::Route {
            matches: sign_in
                .split(['?', '#'])
                .next()
                .unwrap_or(sign_in)
                .to_owned(),
            present: Some(Present::Full),
            chrome: false,
        });
    }
    for matches in ["/*/new", "/*/*/edit"] {
        routes.push(s::Route {
            matches: matches.to_owned(),
            present: Some(Present::Sheet),
            chrome: true,
        });
    }
    routes
}
