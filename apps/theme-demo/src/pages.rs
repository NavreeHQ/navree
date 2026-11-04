//! The demo's pages: the theme builder, the themes and the layouts, under
//! [`BASE`](crate::BASE). Each is the page an app would serve, in a shell with
//! the theme menu in its top bar; nothing is kept on the server.

use std::sync::Arc;

use http::{HeaderMap, Method, StatusCode};
use navree::http::extract::{Path, RawQuery, urlencoded};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::theme::{BuilderDraft, ImportError, Mode, PairKind, Theme, ThemeBuilder};
use navree::ui::*;

use crate::BASE;

/// An address under the demo's mount: `/demo/ui` and `path`.
fn under(path: &str) -> String {
    format!("{BASE}{path}")
}

/// `path` and its query, as the shell reads a request.
fn at(path: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_owned(),
    }
}

/// The demo's shell: a CRM's settings, with the theme menu and the "Look" links.
pub fn shell() -> Shell {
    Shell::new("Sutlej CRM")
        .context("Theme demo")
        .home(under("/themes/builder"))
        .theme_menu()
        .group(
            NavGroup::new("Look")
                .item(
                    NavItem::new("Theme builder", under("/themes/builder"))
                        .icon(icons::SWATCH_BOOK),
                )
                .item(NavItem::new("Themes", under("/themes")).icon(icons::PALETTE))
                .item(
                    NavItem::new("Layouts", under("/themes/layouts/sidebar"))
                        .icon(icons::PANEL_LEFT)
                        .sub(NavItem::new("Sidebar", under("/themes/layouts/sidebar")))
                        .sub(NavItem::new("Inset", under("/themes/layouts/inset")))
                        .sub(NavItem::new("Floating", under("/themes/layouts/floating")))
                        .sub(NavItem::new("Header", under("/themes/layouts/header"))),
                ),
        )
}

/// A few components in `theme`, in one mode: what a card of the overview shows.
fn sample(theme: &Theme, mode: Mode) -> Element {
    el("div")
        .attr("data-theme", theme.key().to_owned())
        .attr("data-mode", mode.id())
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

fn overview(headers: &HeaderMap, query: Option<&str>) -> AppPage {
    let shell = shell().for_request(at(&under("/themes"), query), headers);
    let page = shell.page("Themes");
    let current = page.current_appearance().clone();
    let cards: Vec<Element> = Theme::BUILT_IN
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
                        Text::muted(theme.description().unwrap_or_default()),
                        Grid::cols(2)
                            .gap(3)
                            .children((sample(theme, Mode::Light), sample(theme, Mode::Dark))),
                    )),
                )
        })
        .collect();
    page.body(Stack::gap(6).children((
        Text::new(format!(
            "This page is in {} ({}). Pick another theme or mode from the button at the top right: it is kept in a cookie in your browser, and the next page paints in it from its first byte.",
            current.theme().name(),
            current.mode().label().to_lowercase()
        )),
        Grid::cols(2).gap(6).children(cards),
    )))
}

fn layout_page(headers: &HeaderMap, layout: &str, query: Option<&str>) -> Response {
    let Some(layout) = ShellLayout::from_id(layout) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = under(&format!("/themes/layouts/{}", layout.id()));
    let links: Vec<Node> = ShellLayout::ALL
        .iter()
        .map(|other| {
            Button::new(other.label())
                .variant(if *other == layout {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Outline
                })
                .href(under(&format!("/themes/layouts/{}", other.id())))
                .into()
        })
        .collect();
    shell()
        .layout(layout)
        .for_request(at(&path, query), headers)
        .page(format!("{} layout", layout.label()))
        .crumb("Layouts", under("/themes/layouts/sidebar"))
        .body(Stack::gap(6).children((
            Text::new("The same page in each layout. On a phone every layout puts the links in a sheet. Ctrl+B or the button folds the sidebar."),
            Row::gap(2).wrap().children(links),
            Grid::cols(3).gap(4).children((
                Stat::new("Open deals", "312").trend("+8%", true),
                Stat::new("Won this month", "₹8,42,000").trend("+12%", true),
                Stat::new("Lost", "₹98,000").trend("-3%", false),
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
                            ("Sutlej Supply renewal", "Harleen", "Won", "₹1,20,000"),
                            ("Doaba Traders expansion", "Gurpreet", "Proposal", "₹4,85,000"),
                            ("Malwa Foods pilot", "Manpreet", "Lead", "₹32,000"),
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

fn builder(
    headers: &HeaderMap,
    query: Option<&str>,
    status: StatusCode,
    import: Option<(String, Vec<String>)>,
) -> Response {
    let draft = BuilderDraft::from_query(query.unwrap_or_default());
    let path = under("/themes/builder");
    let shell = shell().for_request(at(&path, query), headers);
    let mut builder = ThemeBuilder::new(&draft, &path)
        .save(under("/themes/builder/save"))
        .import(under("/themes/builder/import"));
    if let Some((text, errors)) = import {
        builder = builder.import_failed(text, errors);
    }
    let mut response = builder
        .page(
            shell
                .page("Theme builder")
                .crumb("Themes", under("/themes"))
                .actions(Text::muted(
                    "A demo: Save checks a theme and keeps nothing.",
                )),
        )
        .into_response();
    *response.status_mut() = status;
    response
}

fn field(body: &str, name: &str) -> String {
    urlencoded::parse(body.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

/// Checks a draft as an app's save does, and sends the reader back to it. Nothing
/// is stored: the toast says what an app would do.
fn save(headers: &HeaderMap, body: &str) -> Response {
    let draft = BuilderDraft::from_query(&field(body, "draft"));
    match draft.to_theme() {
        Ok(theme) => Reply::redirect(format!(
            "{}?{}",
            under("/themes/builder"),
            draft.to_query()
        ))
        .toast(
            ToastTone::Success,
            format!(
                "\u{201c}{}\u{201d} passes every check. An app would store it now; this demo keeps nothing.",
                theme.name()
            ),
        )
        .into_response(),
        // The server is the authority: a draft the page showed as savable is checked again.
        Err(_) => builder(
            headers,
            Some(&draft.to_query()),
            StatusCode::UNPROCESSABLE_ENTITY,
            None,
        ),
    }
}

/// Reads a stored theme and opens it in the builder, or shows why it can't be read.
fn import(headers: &HeaderMap, body: &str) -> Response {
    let text = field(body, "json");
    match Theme::from_json(&text) {
        Ok(theme) => Reply::redirect(format!(
            "{}?{}",
            under("/themes/builder"),
            BuilderDraft::from_theme(&theme).to_query()
        ))
        .toast(
            ToastTone::Success,
            format!("Opened \u{201c}{}\u{201d} in the builder.", theme.name()),
        )
        .into_response(),
        Err(error) => {
            let errors = match error {
                ImportError::Fields(list) => {
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
                ImportError::Contrast(list) => vec![format!(
                    "{} text pairs fall short of WCAG AA, so it can't be used as it is.",
                    list.iter()
                        .filter(|shortfall| shortfall.pair.kind == PairKind::Text)
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

pub struct DemoPages;

impl Injectable for DemoPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for DemoPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/demo/ui/health", || async {
                StatusCode::NO_CONTENT
            })
            .public(),
            RouteDef::on(Method::GET, "!/demo/ui", || async {
                Reply::redirect(under("/themes/builder"))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/demo/ui/themes",
                |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                    overview(&headers, query.as_deref())
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/demo/ui/themes/layouts/{layout}",
                |headers: HeaderMap, Path(layout): Path<String>, RawQuery(query): RawQuery| async move {
                    layout_page(&headers, &layout, query.as_deref())
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/demo/ui/themes/builder",
                |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                    builder(&headers, query.as_deref(), StatusCode::OK, None)
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/demo/ui/themes/builder/save",
                |headers: HeaderMap, body: String| async move { save(&headers, &body) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/demo/ui/themes/builder/import",
                |headers: HeaderMap, body: String| async move { import(&headers, &body) },
            )
            .public(),
        ]
    }
}
