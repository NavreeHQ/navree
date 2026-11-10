//! The panel's shell: its name, the menu the registry gives the user (with the
//! counts beside it fetched once the page is up), the user's menu, the workspace
//! form and the command menu, applied to the request a page answers.

use std::collections::HashMap;

use http::HeaderMap;
use navree_access::READ;
use navree_auth::AuthService;
use navree_ui::form::csrf_field;
use navree_ui::icons::{self, Icon};
use navree_ui::theme::{Appearance, Mode, Theme};
use navree_ui::{
    AppPage, Button, Command, CommandGroup, CommandItem, Field, Input, NavGroup, NavItem, Node,
    Shell, Size, Stack, UserMenu, el,
};

use crate::core::Core;
use crate::cx::Cx;
use crate::icons::icon;
use crate::panel::Panel;
use crate::session::PanelSession;

/// The id of the command menu.
const COMMAND: &str = "helm-go";

/// The id of the `n`th group of the menu, when its counts come after the page.
fn group_id(n: usize) -> String {
    format!("helm-menu-{n}")
}

/// One link of the menu.
struct Entry {
    label: String,
    url: String,
    icon: Icon,
    /// The resource whose count goes beside it, when it has one.
    counted: Option<String>,
}

/// The menu's links under one heading.
struct MenuGroup {
    label: String,
    entries: Vec<Entry>,
}

/// The menu the user may see: each resource they may read and each page that lets
/// them in, by group, then Settings.
async fn menu(cx: &Cx) -> Vec<MenuGroup> {
    let core = &cx.core;
    let base = core.options.base();
    let mut groups: Vec<MenuGroup> = Vec::new();
    let mut push = |group: &str, entry: Entry| match groups.iter_mut().find(|g| g.label == group) {
        Some(found) => found.entries.push(entry),
        None => groups.push(MenuGroup {
            label: group.to_owned(),
            entries: vec![entry],
        }),
    };
    for resource in core.registry.resources() {
        let info = resource.info();
        if !cx.can(READ, info.subject, None) {
            continue;
        }
        push(
            &info.group,
            Entry {
                label: info.label.to_owned(),
                url: format!("{base}/resources/{}", info.slug),
                icon: icon(info.icon),
                counted: info.sidebar_count.then(|| info.slug.clone()),
            },
        );
    }
    for entry in core.registry.pages() {
        let page = &entry.page;
        if page.can.as_ref().is_some_and(|(a, s)| !cx.can(a, s, None)) {
            continue;
        }
        push(
            page.group.as_deref().unwrap_or("Pages"),
            Entry {
                label: page.title.clone(),
                url: format!("{base}/pages/{}", page.slug),
                icon: icon(&page.icon),
                counted: None,
            },
        );
    }
    #[cfg(feature = "settings")]
    if super::settings::visible(cx).await {
        push(
            "System",
            Entry {
                label: "Settings".to_owned(),
                url: format!("{base}/settings"),
                icon: icons::SLIDERS_HORIZONTAL,
                counted: None,
            },
        );
    }
    groups
}

/// The signed-in user's name and email.
async fn user_of(cx: &Cx) -> (String, String) {
    if let Some(auth) = cx.app.try_get::<AuthService>().await
        && let Ok(Some(user)) = auth.find_user(cx.user_id).await
    {
        return (user.name, user.email);
    }
    ("Signed in".to_owned(), String::new())
}

/// The form that picks the workspace the panel works in, for a browser signed in on
/// the session, when a resource the user reads lives in workspaces.
fn workspace_form(cx: &Cx, csrf: Option<&str>) -> Option<Node> {
    let core = &cx.core;
    let token = csrf?;
    let works_in_workspaces = core.registry.resources().iter().any(|resource| {
        let info = resource.info();
        info.workspaces.is_some() && cx.can(READ, info.subject, None)
    });
    if !works_in_workspaces {
        return None;
    }
    let mut input = Input::text("workspace")
        .id("helm-workspace")
        .placeholder("Workspace id")
        .autocomplete("off")
        .size(Size::Sm)
        .attr("spellcheck", "false");
    if let Some(current) = cx.uow.workspace() {
        input = input.value(current.to_string());
    }
    Some(
        el("form")
            .attr("method", "post")
            .attr("action", format!("{}/workspace", core.options.base()))
            .flag("data-ui-form", true)
            .child(
                Stack::gap(2).children((
                    csrf_field(token.to_owned()),
                    Field::new("helm-workspace", "Workspace").control(input),
                    Button::outline("Work in this workspace")
                        .size(Size::Sm)
                        .block()
                        .submit(),
                )),
            )
            .into(),
    )
}

/// The panel's shell for `cx`: the menu (with `counts` beside it when they are
/// known), the user's menu, the workspace form and the command menu.
async fn shell(cx: &Cx, csrf: Option<&str>, counts: Option<&HashMap<String, String>>) -> Shell {
    let core = &cx.core;
    let base = core.options.base().to_owned();
    let theme = Theme::builtin(&core.options.theme).unwrap_or(Theme::NAVREE);
    let mut shell = Shell::new(core.options.title.clone())
        .context("Admin panel")
        .home(base.clone())
        .appearance(Appearance::new(theme, Mode::Dark));
    if let Some(logo) = &core.options.logo {
        shell = shell.logo(logo.clone());
    }
    shell = shell.group(
        NavGroup::plain()
            .item(NavItem::new("Dashboard", base.clone()).icon(icons::LAYOUT_DASHBOARD)),
    );
    let mut palette = Command::new(COMMAND)
        .title("Go to")
        .placeholder("Go to a resource or page…")
        .group(
            CommandGroup::new(core.options.title.clone())
                .item(CommandItem::link("Dashboard", base.clone()).icon(icons::LAYOUT_DASHBOARD)),
        );
    for (n, group) in menu(cx).await.into_iter().enumerate() {
        let mut nav = NavGroup::new(group.label.clone());
        let mut commands = CommandGroup::new(group.label.clone());
        let mut counted = false;
        for entry in group.entries {
            commands = commands
                .item(CommandItem::link(entry.label.clone(), entry.url.clone()).icon(entry.icon));
            let mut item = NavItem::new(entry.label, entry.url).icon(entry.icon);
            if let Some(slug) = &entry.counted {
                counted = true;
                if let Some(text) = counts.and_then(|counts| counts.get(slug)) {
                    item = item.count(text.clone());
                }
            }
            nav = nav.item(item);
        }
        if counted {
            nav = nav.lazy(group_id(n), format!("{base}/counts"));
        }
        shell = shell.group(nav);
        palette = palette.group(commands);
    }
    let (name, email) = user_of(cx).await;
    let mut user = UserMenu::new(name, email);
    if csrf.is_some() {
        user = user.action("Sign out", navree_auth::SIGN_OUT_PATH);
    }
    shell = shell.user(user).search(Node::list((
        Command::trigger(COMMAND, "Go to…").size(Size::Sm),
        palette,
    )));
    if let Some(form) = workspace_form(cx, csrf) {
        shell = shell.header_slot(form);
    }
    shell
}

/// What a page shows around its content.
pub(crate) struct Frame<'a> {
    pub(crate) cx: &'a Cx,
    session: &'a PanelSession,
    headers: &'a HeaderMap,
    /// The page's own path and query: the menu marks its link, and the theme comes
    /// back to it.
    path: String,
}

impl<'a> Frame<'a> {
    /// The frame of `panel`'s page at `path`.
    pub(crate) fn new(panel: &'a Panel, headers: &'a HeaderMap, path: impl Into<String>) -> Self {
        Frame {
            cx: &panel.cx,
            session: &panel.session,
            headers,
            path: path.into(),
        }
    }

    pub(crate) fn core(&self) -> &Core {
        &self.cx.core
    }

    pub(crate) fn base(&self) -> &str {
        self.cx.core.options.base()
    }

    /// The session's CSRF token, for a browser signed in on the session.
    pub(crate) fn csrf(&self) -> Option<&str> {
        self.session.csrf.as_deref()
    }

    /// A page titled `title` in the panel's shell.
    pub(crate) async fn page(&self, title: impl Into<String>) -> AppPage {
        let shell = shell(self.cx, self.csrf(), None).await;
        let mut app = shell.for_request(self.path.clone(), self.headers);
        if let Some(token) = self.csrf() {
            app = app.csrf(token.to_owned());
        }
        let title: String = title.into();
        app.page(title)
    }
}

/// The menu's group `region` with the counts beside it, for the page at `at`: what
/// the group fetches once the page is up. `None` when there is no such group.
pub(crate) async fn menu_fragment(
    cx: &Cx,
    csrf: Option<&str>,
    region: &str,
    at: &str,
    counts: &HashMap<String, String>,
) -> Option<Node> {
    let shell = shell(cx, csrf, Some(counts)).await;
    shell.live_group(region, at)
}

/// The slugs of the counted resources of the menu's group `region`, as the menu
/// draws them for `cx`.
pub(crate) async fn counted_in(cx: &Cx, region: &str) -> Vec<String> {
    menu(cx)
        .await
        .into_iter()
        .enumerate()
        .find(|(n, _)| group_id(*n) == region)
        .map(|(_, group)| {
            group
                .entries
                .into_iter()
                .filter_map(|entry| entry.counted)
                .collect()
        })
        .unwrap_or_default()
}
