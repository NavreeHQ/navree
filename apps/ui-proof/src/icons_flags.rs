//! Icons and flags, every one of them, drawn the way a page would: what the
//! browser suites check for axe, content security policy reports and rendering.

use std::sync::Arc;

use http::Method;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::flags::{self, CodeKind};
use navree::ui::*;

/// The app's own icons: every `.svg` file of `icons/`, checked while this builds.
mod own {
    navree::ui::icon_set!("icons");
}

fn section(title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("data-gallery", title)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

fn icon_grid() -> Element {
    el("div").attr("data-icons", "all").children(
        icons::ALL
            .iter()
            .map(|(name, shipped)| {
                el("span")
                    .attr("data-icon-name", *name)
                    .child(components::icon(*shipped))
            })
            .collect::<Vec<_>>(),
    )
}

fn icons_section() -> Element {
    section(
        "Icons",
        (
            Row::gap(3).wrap().children((
                Button::primary("New deal").icon(icons::PLUS),
                Button::outline("Open").icon(lucide!("mail-open")),
                Button::icon_only(icons::TRASH, "Delete"),
                Badge::new("Delivered").icon(icons::CIRCLE_CHECK),
                components::icon_labelled(icons::CIRCLE_CHECK, "Delivered"),
            )),
            icon_grid(),
        ),
    )
}

fn own_section() -> Element {
    section(
        "Own icons",
        Row::gap(3).wrap().children((
            Button::primary("Launch").icon(own::PENNANT),
            Button::outline("Orbit").icon(own::ORBIT),
            Badge::new("Verified").icon(own::SEAL),
            el("span")
                .attr("data-own", "seal")
                .child(components::icon(own::SEAL)),
        )),
    )
}

/// Every flag in a row of cells, in the shape given.
fn flag_grid(id: &'static str, shape: FlagShape, scope: Option<&'static str>) -> Element {
    el("div").attr("data-flags", id).children(
        flags::ALL
            .iter()
            .map(|code| {
                el("span").attr("data-flag-code", code.as_str()).child({
                    let flag = Flag::new(*code).shape(shape).size(FlagSize::Md);
                    match scope {
                        Some(scope) => flag.scope(scope),
                        None => flag,
                    }
                })
            })
            .collect::<Vec<_>>(),
    )
}

fn flags_section(hidden_first: bool) -> Element {
    // Copies of every flag that are not drawn: a closed `details`, a `hidden` box and a
    // closed `dialog`. They come before the visible flags, so a flag whose art refers to
    // an id would find the hidden copy's first, if ids could clash. The three flags that
    // clip are scoped, as an app's hidden menu should scope them. The `details` is on
    // the page either way, so the visible flags sit in the same place and can be compared
    // pixel for pixel.
    let hidden: Vec<Element> = vec![
        el("details").attr("data-hidden", "details").children((
            el("summary").child("More flags"),
            hidden_first.then(|| flag_grid("details", FlagShape::Rect, Some("in-details"))),
        )),
        el("div")
            .attr("data-hidden", "box")
            .flag("hidden", true)
            .child(hidden_first.then(|| flag_grid("box", FlagShape::Rect, Some("in-box")))),
        el("dialog")
            .attr("data-hidden", "dialog")
            .child(hidden_first.then(|| flag_grid("dialog", FlagShape::Square, Some("in-dialog")))),
    ];
    let sizes: Vec<Element> = [
        FlagSize::Xs,
        FlagSize::Sm,
        FlagSize::Md,
        FlagSize::Lg,
        FlagSize::Xl,
    ]
    .into_iter()
    .map(|size| {
        el("span")
            .attr("data-flag-size", "x")
            .child(flag!("BR").size(size))
    })
    .collect();
    let opens = [
        (flags::FR, "France", "12,480"),
        (flags::BR, "Brazil", "9,215"),
        (flags::AE, "United Arab Emirates", "4,310"),
    ];
    let rows: Vec<TableRowEl> = opens
        .into_iter()
        .map(|(code, name, count)| {
            TableRowEl::new((
                TableCell::new(Row::gap(2).children((Flag::new(code).decorative(), name))),
                TableCell::new(count).numeric(),
            ))
        })
        .collect();
    let countries = flags::ALL
        .iter()
        .filter(|c| c.kind() == CodeKind::Country)
        .count();
    section(
        "Flags",
        (
            hidden,
            flag_grid("rect", FlagShape::Rect, None),
            flag_grid("square", FlagShape::Square, None),
            flag_grid("round", FlagShape::Round, None),
            Row::gap(3).wrap().children((
                sizes,
                Flag::country("not-a-code"),
                Flag::country("not-a-code").round(),
            )),
            Row::gap(2).wrap().children((
                Badge::new(Row::gap(1).children((flag!("DE").decorative(), "Germany"))),
                Badge::new(Row::gap(1).children((flag!("JP").decorative(), "Japan")))
                    .variant(BadgeVariant::Outline),
                flag!("IN"),
            )),
            Table::new()
                .head((TableHead::new("Country"), TableHead::new("Opens").numeric()))
                .rows(rows),
            el("p")
                .attr("data-flag-count", "x")
                .child(format!("{} flags, {countries} countries", flags::ALL.len())),
        ),
    )
}

fn page(shell: &AppShell, hidden_first: bool) -> AppPage {
    shell.page("Icons and flags").body(Stack::gap(8).children((
        icons_section(),
        own_section(),
        flags_section(hidden_first),
    )))
}

pub struct IconPages;

impl Injectable for IconPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for IconPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/icons-flags", |shell: AppShell| async move {
                page(&shell, false)
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/icons-flags/hidden-first",
                |shell: AppShell| async move { page(&shell, true) },
            )
            .public(),
        ]
    }
}
