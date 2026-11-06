//! Themes, layouts and the theme builder: `/themes`, `/themes/layouts/{layout}`,
//! `/themes/builder` and `/themes/scopes`.
//!
//! These pages use a shell with the theme menu in its top bar and a "Look" group
//! in its sidebar; the reader's choice is kept in the toolkit's cookie. The
//! scopes page keeps a workspace's and a user's choice on the server, in memory,
//! and shows how the levels lay over each other.

use std::sync::{Arc, Mutex};

use http::{HeaderMap, Method, StatusCode};
use navree::http::extract::{Path, RawQuery, urlencoded};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::theme::{Appearance, BuilderDraft, Mode, Theme, ThemeBuilder, ThemeSetting};
use navree::ui::*;

use crate::agra_theme::agra_theme;

/// The themes a reader may choose here: every built-in and the app's own.
fn offered() -> Vec<Theme> {
    let mut themes = Theme::BUILT_IN.to_vec();
    themes.push(agra_theme());
    themes
}

/// The theme pages' shell: the app's, with the theme menu and the "Look" links.
fn look_shell() -> Shell {
    crate::shell().theme_menu().themes(offered()).group(
        NavGroup::new("Look")
            .item(NavItem::new("Themes", "/themes").icon(icons::PALETTE))
            .item(
                NavItem::new("Layouts", "/themes/layouts/inset")
                    .icon(icons::PANEL_LEFT)
                    .sub(NavItem::new("Sidebar", "/themes/layouts/sidebar"))
                    .sub(NavItem::new("Inset", "/themes/layouts/inset"))
                    .sub(NavItem::new("Floating", "/themes/layouts/floating"))
                    .sub(NavItem::new("Header", "/themes/layouts/header")),
            )
            .item(NavItem::new("Theme builder", "/themes/builder").icon(icons::SWATCH_BOOK))
            .item(NavItem::new("Scopes", "/themes/scopes").icon(icons::LAYERS)),
    )
}

/// `path` and its query, as the shell reads a request.
fn at(path: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_owned(),
    }
}

/// A few components in `theme`, in one mode: what a card of the overview shows.
fn sample(theme: &Theme, mode: Mode) -> Element {
    el("div")
        .attr("data-theme", theme.key().to_owned())
        .attr("data-mode", mode.id())
        .attr("data-proof-sample", "")
        .attr(
            "aria-label",
            format!("{} in {}", theme.name(), mode.label().to_lowercase()),
        )
        .attr("role", "group")
        .child(Card::new().body(Stack::gap(3).children((
            Row::gap(2).wrap().children((
                Button::primary("Save"),
                Button::new("Cancel"),
                Badge::new("Won").variant(BadgeVariant::Success),
                Badge::new("At risk").variant(BadgeVariant::Warning),
            )),
            Text::new("Body text on a card."),
            Text::muted("A hint, in the theme's quietest text."),
        ))))
}

fn overview(shell: &AppShell) -> AppPage {
    let mut page = shell.page("Themes");
    let current = page.current_appearance().clone();
    let cards: Vec<Element> = offered()
        .iter()
        .map(|theme| {
            el("section")
                .attr("aria-labelledby", format!("theme-{}", theme.id()))
                .child(
                    Stack::gap(3).children((
                        el("h2")
                            .attr("id", format!("theme-{}", theme.id()))
                            .attr("data-slot", "heading")
                            .attr("data-level", "3")
                            .child(theme.name().to_owned()),
                        Text::muted(theme.description().unwrap_or(
                            "A theme of the app's own, from the builder's Rust export.",
                        )),
                        Grid::cols(2)
                            .gap(3)
                            .children((sample(theme, Mode::Light), sample(theme, Mode::Dark))),
                    )),
                )
        })
        .collect();
    for theme in offered() {
        page = page.theme_sheet(&theme);
    }
    page.body(Stack::gap(6).children((
        Text::new(format!(
            "This page is in {} ({}). Pick another theme or mode from the button at the top right: it is kept in a cookie and the next page paints in it from its first byte. A link with ?theme= and ?mode= shows a page in another look for that page only.",
            current.theme().name(),
            current.mode().label().to_lowercase()
        )),
        Row::gap(2).wrap().children((
            Button::outline("Light, by the URL").href("/themes?mode=light"),
            Button::outline("Dark, by the URL").href("/themes?mode=dark"),
            Button::outline("Ocean, by the URL").href("/themes?theme=ocean"),
            Button::outline("The device's mode").href("/themes?mode=system"),
        )),
        Grid::cols(2).gap(6).children(cards),
    )))
}

fn layout_page(headers: &HeaderMap, layout: &str, query: Option<&str>) -> Response {
    let Some(layout) = ShellLayout::from_id(layout) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let collapse = urlencoded::parse(query.unwrap_or_default().as_bytes())
        .find(|(name, _)| name == "collapse")
        .map(|(_, value)| value.into_owned());
    let mut shell = look_shell().layout(layout);
    shell = match collapse.as_deref() {
        Some("offcanvas") => shell.collapse(Collapse::Offcanvas),
        Some("never") => shell.collapse(Collapse::Never),
        _ => shell,
    };
    let path = format!("/themes/layouts/{}", layout.id());
    let links: Vec<Node> = ShellLayout::ALL
        .iter()
        .map(|other| {
            Button::new(other.label())
                .variant(if *other == layout {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Outline
                })
                .href(format!("/themes/layouts/{}", other.id()))
                .into()
        })
        .collect();
    shell
        .for_request(at(&path, query), headers)
        .page(format!("{} layout", layout.label()))
        .crumb("Layouts", "/themes/layouts/sidebar")
        .actions(Button::primary("New deal").href("/deals/new"))
        .body(Stack::gap(6).children((
            Text::new("The same page in each layout. On a phone every layout puts the links in a sheet. Ctrl+B or the button folds the sidebar: to icons, off the canvas, or not at all."),
            Row::gap(2).wrap().children(links),
            Row::gap(2).wrap().children((
                Button::outline("Folds to icons").href(path.clone()),
                Button::outline("Folds away").href(format!("{path}?collapse=offcanvas")),
                Button::outline("Never folds").href(format!("{path}?collapse=never")),
            )),
            Grid::cols(3).gap(4).children((
                Stat::new("Open deals", "312").trend("+8%", true),
                Stat::new("Won this month", "₹84,20,000").trend("+12%", true),
                Stat::new("Lost", "₹9,80,000").trend("-3%", false),
            )),
            Card::titled("Recent deals").body(
                Table::new()
                    .caption("Recent deals")
                    .head((
                        TableHead::new("Deal"),
                        TableHead::new("Owner"),
                        TableHead::new("Stage"),
                        TableHead::new("Amount").numeric(),
                    ))
                    .rows(
                        [
                            ("Agra renewal", "Anu", "Won", "₹12,00,000"),
                            ("Gulati expansion", "Gauri", "Proposal", "₹48,50,000"),
                            ("Indrani pilot", "Laksh", "Lead", "₹3,20,000"),
                        ]
                        .into_iter()
                        .map(|(deal, owner, stage, amount)| {
                            TableRowEl::new((
                                TableCell::new(deal),
                                TableCell::new(owner),
                                TableCell::new(stage),
                                TableCell::new(amount).numeric(),
                            ))
                        })
                        .collect::<Vec<_>>(),
                    ),
            ),
        )))
        .into_response()
}

/// The themes saved from the builder, newest last: the app's own store.
static SAVED: Mutex<Vec<Theme>> = Mutex::new(Vec::new());

fn saved() -> Vec<Theme> {
    SAVED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn builder(
    headers: &HeaderMap,
    query: Option<&str>,
    status: StatusCode,
    import: Option<(String, Vec<String>)>,
) -> Response {
    let draft = BuilderDraft::from_query(query.unwrap_or_default());
    let shell = look_shell().for_request(at("/themes/builder", query), headers);
    let mut builder = ThemeBuilder::new(&draft, "/themes/builder")
        .save("/themes/builder/save")
        .import("/themes/builder/import");
    if let Some((text, errors)) = import {
        builder = builder.import_failed(text, errors);
    }
    let list = saved();
    let mut page = builder.page(shell.page("Theme builder").crumb("Themes", "/themes"));
    if !list.is_empty() {
        page = page.actions(Text::muted(format!(
            "Saved here: {}",
            list.iter().map(Theme::name).collect::<Vec<_>>().join(", ")
        )));
    }
    let mut response = page.into_response();
    *response.status_mut() = status;
    response
}

fn field(body: &str, name: &str) -> String {
    urlencoded::parse(body.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn save(headers: &HeaderMap, body: &str) -> Response {
    let query = field(body, "draft");
    let draft = BuilderDraft::from_query(&query);
    match draft.to_theme() {
        Ok(theme) => {
            let mut store = SAVED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            store.retain(|kept| kept.id() != theme.id());
            store.push(theme.clone());
            if store.len() > 20 {
                store.remove(0);
            }
            drop(store);
            Reply::redirect(format!("/themes/builder?{query}"))
                .toast(
                    ToastTone::Success,
                    format!("Saved \u{201c}{}\u{201d}.", theme.name()),
                )
                .into_response()
        }
        // The server is the authority: a draft the page showed as savable is checked again.
        Err(_) => builder(
            headers,
            Some(&query),
            StatusCode::UNPROCESSABLE_ENTITY,
            None,
        ),
    }
}

fn import(headers: &HeaderMap, body: &str) -> Response {
    let text = field(body, "json");
    match Theme::from_json(&text) {
        Ok(theme) => Reply::redirect(format!(
            "/themes/builder?{}",
            BuilderDraft::from_theme(&theme).to_query()
        ))
        .toast(
            ToastTone::Success,
            format!("Imported \u{201c}{}\u{201d}.", theme.name()),
        )
        .into_response(),
        Err(error) => {
            let errors = match error {
                navree::ui::theme::ImportError::Fields(list) => {
                    let more = list.len().saturating_sub(8);
                    let mut lines: Vec<String> = list
                        .into_iter()
                        .take(8)
                        .map(|(at, what)| format!("{at}: {what}"))
                        .collect();
                    if more > 0 {
                        lines.push(format!("And {more} more."));
                    }
                    lines
                }
                navree::ui::theme::ImportError::Contrast(list) => vec![format!(
                    "{} text pairs fall short of WCAG AA, so it can't be used as it is.",
                    list.iter()
                        .filter(|s| s.pair.kind == navree::ui::theme::PairKind::Text)
                        .count()
                )],
            };
            builder(
                headers,
                None,
                StatusCode::UNPROCESSABLE_ENTITY,
                Some((text, errors)),
            )
        }
    }
}

/// The workspace's and the user's choices, kept on the server: one demo of each.
static WORKSPACE: Mutex<Option<(String, bool)>> = Mutex::new(None);
static USER: Mutex<Option<String>> = Mutex::new(None);

fn setting(value: &str) -> ThemeSetting {
    ThemeSetting::parse(value).unwrap_or_default()
}

fn theme_choices(name: &'static str, legend: &'static str, chosen: &str) -> RadioGroup {
    let mut group = RadioGroup::new(name, legend).option("", "Not set");
    for theme in offered() {
        for mode in Mode::ALL {
            let value = Appearance::new(theme.clone(), mode).to_value();
            group = group.option(
                value,
                format!("{}, {}", theme.name(), mode.label().to_lowercase()),
            );
        }
    }
    group.selected(chosen.to_owned())
}

fn scopes(headers: &HeaderMap, query: Option<&str>) -> AppPage {
    let workspace = WORKSPACE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let user = USER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let mut shell = look_shell().for_request(at("/themes/scopes", query), headers);
    if let Some((value, locked)) = &workspace {
        let level = setting(value);
        shell = shell.workspace(if *locked { level.locked() } else { level });
    }
    if let Some(value) = &user {
        shell = shell.user(setting(value));
    }
    let shown = shell.appearance();
    let rows = [
        ("App default", "Navree, dark".to_owned()),
        (
            "Workspace",
            workspace
                .as_ref()
                .map_or("Not set".to_owned(), |(value, locked)| {
                    format!("{value}{}", if *locked { " (locked)" } else { "" })
                }),
        ),
        (
            "User, on the server",
            user.clone()
                .unwrap_or_else(|| "Not set: the cookie counts".to_owned()),
        ),
        ("This page", shown.to_value()),
    ];
    let table = Table::new()
        .caption("What each level says")
        .head((TableHead::new("Level"), TableHead::new("Says")))
        .rows(
            rows.into_iter()
                .map(|(level, says)| TableRowEl::new((TableCell::new(level), TableCell::new(says))))
                .collect::<Vec<_>>(),
        );
    let workspace_value = workspace
        .as_ref()
        .map(|(value, _)| value.clone())
        .unwrap_or_default();
    let locked = workspace.as_ref().is_some_and(|(_, locked)| *locked);
    shell
        .page("Scopes")
        .crumb("Themes", "/themes")
        .body(Stack::gap(6).children((
            Text::new("How the levels lay over each other: the app's default, then the workspace's, then the user's (kept on the server here, else the cookie), then ?theme= and ?mode= on the URL. A locked workspace keeps its theme; the user still picks the mode."),
            Card::titled("What each level says").body(table),
            Grid::cols(2).gap(6).children((
                Card::titled("Workspace").body(
                    el("form").attr("method", "post").attr("action", "/themes/scopes/workspace").child(Stack::gap(4).children((
                        theme_choices("value", "The workspace's theme", &workspace_value),
                        Checkbox::new("locked").checked(locked).label("Lock it: users keep this theme"),
                        Button::primary("Save the workspace's").submit(),
                    ))),
                ),
                Card::titled("User").body(
                    el("form").attr("method", "post").attr("action", "/themes/scopes/user").child(Stack::gap(4).children((
                        theme_choices("value", "The user's theme", user.as_deref().unwrap_or_default()),
                        Button::primary("Save the user's").submit(),
                    ))),
                ),
            )),
        )))
}

fn store_scope(body: &str, workspace: bool) -> Response {
    let value = field(body, "value");
    let value = (!value.is_empty() && ThemeSetting::parse(&value).is_some()).then_some(value);
    if workspace {
        let locked = !field(body, "locked").is_empty();
        *WORKSPACE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            value.map(|value| (value, locked));
    } else {
        *USER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
    }
    Reply::redirect("/themes/scopes")
        .toast(ToastTone::Success, "Saved.")
        .into_response()
}

pub struct ThemePages;

impl Injectable for ThemePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for ThemePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/themes", |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                overview(&look_shell().for_request(at("/themes", query.as_deref()), &headers))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/themes/layouts/{layout}",
                |headers: HeaderMap, Path(layout): Path<String>, RawQuery(query): RawQuery| async move {
                    layout_page(&headers, &layout, query.as_deref())
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/themes/builder", |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                builder(&headers, query.as_deref(), StatusCode::OK, None)
            })
            .public(),
            RouteDef::on(Method::POST, "!/themes/builder/save", |headers: HeaderMap, body: String| async move {
                save(&headers, &body)
            })
            .public(),
            RouteDef::on(Method::POST, "!/themes/builder/import", |headers: HeaderMap, body: String| async move {
                import(&headers, &body)
            })
            .public(),
            RouteDef::on(Method::GET, "!/themes/scopes", |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                scopes(&headers, query.as_deref())
            })
            .public(),
            RouteDef::on(Method::POST, "!/themes/scopes/workspace", |body: String| async move {
                store_scope(&body, true)
            })
            .public(),
            RouteDef::on(Method::POST, "!/themes/scopes/user", |body: String| async move {
                store_scope(&body, false)
            })
            .public(),
            // For the browser suite: back to nothing set.
            RouteDef::on(Method::POST, "!/themes/scopes/reset", || async {
                *WORKSPACE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                *USER.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                SAVED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
                StatusCode::NO_CONTENT
            })
            .public(),
        ]
    }
}
