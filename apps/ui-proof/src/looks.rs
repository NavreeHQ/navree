//! The looks a component takes only when a page asks for one, in the states the browser suite
//! runs against.
//!
//! | Page | Shows |
//! |---|---|
//! | `/looks` | each look in a theme with a shape and colour roles (pill toggles, 16 px cards, a soft shadow, segment colours and a near-black inverse surface), beside the same parts on the inverse surface |
//! | `/looks/plain` | the same in the built-in theme, whose inverse surface is its other palette |
//!
//! `?mode=light` or `?mode=dark` picks the mode; `?part=team` makes the second segmented link
//! the current one.

use std::sync::Arc;

use http::{HeaderMap, Method};
use navree::http::extract::RawQuery;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::genui::ThreadList;
use navree::ui::theme::{
    Appearance, ControlSizes, Corner, Corners, Elevation, Inverse, Mode, Rgb, Roles, Shape, Sizes,
    Theme, ThemeId, Token,
};
use navree::ui::*;

/// A theme with what the looks read: pill toggles and buttons, 16 px cards, 12 px panels,
/// controls of 32, 40 and 48 px, a soft shadow, the segment's colours, white cards on a tinted
/// page and a near-black inverse surface with white text.
pub fn looks_theme() -> Theme {
    let rgb = Rgb::hex;
    let mut spec = *Theme::NEUTRAL.spec();
    spec.light.set(Token::Ink1, rgb(0xfafaf9));
    let shape = Shape::new()
        .corners(
            Corners::new()
                .button(Corner::Pill)
                .badge(Corner::Pill)
                .toggle(Corner::Pill)
                .card(Corner::Px(16))
                .panel(Corner::Px(12)),
        )
        .sizes(Sizes::new().controls(ControlSizes::new().sm(32).md(40).lg(48)))
        .elevation(Elevation::Soft);
    let roles = Roles::new()
        .card(rgb(0xffffff), rgb(0x121212))
        .segment_track(rgb(0xefefec), rgb(0x151515))
        .segment_on(rgb(0xffffff), rgb(0x1d1d1d))
        .inverse(
            Inverse::new()
                .surface(rgb(0x0a0a0a), rgb(0xffffff))
                .raised(rgb(0x161615), rgb(0xf2f2f0))
                .line(rgb(0x2a2a28), rgb(0xc4c4bf))
                .text(rgb(0xffffff), rgb(0x0a0a0a))
                .muted(rgb(0xb5b5b0), rgb(0x3d3d3a)),
        );
    Theme::custom(
        ThemeId::parse("proof-looks").expect("a theme id"),
        "Looks",
        spec,
    )
    .and_then(|theme| theme.with_shape(shape))
    .and_then(|theme| theme.with_roles(roles))
    .expect("the looks theme reads and its values are in range")
}

/// The query's `mode` and `part`, as the page's links keep them.
struct Asked {
    mode: Option<String>,
    part: String,
}

fn asked(query: Option<&str>) -> Asked {
    let mut out = Asked {
        mode: None,
        part: "mine".to_owned(),
    };
    for pair in query.unwrap_or_default().split('&') {
        match pair.split_once('=') {
            Some(("mode", value)) if matches!(value, "light" | "dark") => {
                out.mode = Some(value.to_owned());
            }
            Some(("part", value)) if matches!(value, "mine" | "team") => {
                out.part = value.to_owned();
            }
            _ => {}
        }
    }
    out
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(4).children(content)))
}

/// Mine and Team as segmented links to this page, the asked one current.
fn mode(path: &str, asked: &Asked, label: &'static str) -> ButtonGroup {
    let href = |part: &str| match &asked.mode {
        Some(mode) => format!("{path}?part={part}&mode={mode}"),
        None => format!("{path}?part={part}"),
    };
    ButtonGroup::new(label).segmented().block().children((
        Button::new("Mine")
            .href(href("mine"))
            .current(asked.part == "mine"),
        Button::new("Team")
            .href(href("team"))
            .current(asked.part == "team"),
    ))
}

fn segmented(path: &str, asked: &Asked) -> Grid {
    Grid::cols(3).gap(4).children((
        el("div")
            .attr("data-proof", "segmented")
            .child(mode(path, asked, "Mode")),
        el("div").attr("data-proof", "segmented-small").child(
            ButtonGroup::new("Range").segmented().children((
                Button::new("7 days")
                    .size(Size::Sm)
                    .href(format!("{path}#segmented"))
                    .current(true),
                Button::new("30 days")
                    .size(Size::Sm)
                    .href(format!("{path}#segmented")),
                Button::new("90 days")
                    .size(Size::Sm)
                    .href(format!("{path}#segmented")),
            )),
        ),
        el("div").attr("data-proof", "plain-group").child(
            ButtonGroup::new("View").children((Button::outline("List"), Button::outline("Board"))),
        ),
    ))
}

/// A card, a stat and a code block on the inverse surface, with the parts the other looks
/// draw inside the card, beside a card drawn as before.
fn inverse(path: &str, asked: &Asked) -> Grid {
    Grid::cols(2).gap(4).children((
        el("div").attr("data-proof", "inverse-card").child(
            Card::titled("Security, in every mode")
                .variant(CardVariant::Inverse)
                .description("Each team in its own pipeline.")
                .body(
                    Stack::gap(3).children((
                        Text::new("Text on the inverse surface."),
                        Text::muted("A quieter line."),
                        Badge::new("Isolated").variant(BadgeVariant::Success),
                        el("div")
                            .attr("data-proof", "inverse-status-dot")
                            .child(StatusDot::new("Every invoice paid").tone(Tone::Success)),
                        el("div")
                            .attr("data-proof", "inverse-track")
                            .child(deal_track()),
                        el("div").attr("data-proof", "inverse-destructive").child(
                            Button::new("Revoke access").variant(ButtonVariant::DestructiveOutline),
                        ),
                        el("div")
                            .attr("data-proof", "inverse-segmented")
                            .child(mode(path, asked, "Mode on the inverse surface")),
                    )),
                )
                .footer((
                    Button::primary("Request a quote"),
                    Button::outline("Read the details"),
                )),
        ),
        el("div").attr("data-proof", "plain-card").child(
            Card::titled("This quarter")
                .description("Drawn as before.")
                .body(Text::new("Body text on the page's own card.")),
        ),
        el("div").attr("data-proof", "inverse-stat").child(
            Stat::new("Invoices due, Mon 16 Nov", "₹9,12,400")
                .caption("from 14 customers")
                .variant(StatVariant::Inverse),
        ),
        el("div").attr("data-proof", "inverse-code").child(
            CodeBlock::new(
                "rust",
                "// Import the contacts.\nlet file = \"contacts.csv\";\nlet tries: u8 = 3;\nfn import(file: &str) -> Result<(), Error> {\n    crm::import!(file)\n}",
            )
            .copy_button()
            .inverse(),
        ),
    ))
}

/// A status dot, a plain timeline, narrow fact tiles, a raised card, popovers opened by an
/// icon and by a count, and a thread list in groups, each beside its look as before.
fn small() -> Stack {
    let events = || {
        [
            TimelineEvent::new("Deal 4 signed")
                .time_text("09:41")
                .detail("Eight seats added"),
            TimelineEvent::new("Rep assigned")
                .time_text("09:12")
                .detail("By Harleen Kaur"),
        ]
    };
    Stack::gap(6).children((
        el("div")
            .attr("data-proof", "status-dot")
            .child(Row::gap(4).wrap().children((
                StatusDot::new("Awaiting signature").tone(Tone::Success),
                StatusDot::new("No deals yet"),
            ))),
        Grid::cols(2).gap(4).children((
            el("div").attr("data-proof", "plain-timeline").child(
                Timeline::new("deal-events", "Deal events")
                    .variant(TimelineVariant::Plain)
                    .day("Today", events()),
            ),
            el("div").attr("data-proof", "rail-timeline").child(
                Timeline::new("rail-events", "Deal events on a rail").day("Today", events()),
            ),
        )),
        el("div").attr("data-proof", "reps").child(
            FactGrid::new()
                .min_tile(128)
                .cards((1..=8).map(|n| FactCard::new(format!("Rep {n}"), "Away").detail("8 won"))),
        ),
        el("div").attr("data-proof", "raised-card").child(
            Card::titled("Plan")
                .raised()
                .body(Text::new("A card on the theme's soft shadow.")),
        ),
        Row::gap(2).children((
            Popover::icon_only("looks-alerts", icons::BELL, "Notifications")
                .label("Notifications")
                .body("Nothing new."),
            Popover::new("looks-inbox", "3")
                .trigger_label("3 new messages")
                .label("Messages")
                .body("Three messages."),
        )),
        ThreadList::new("Your conversations")
            .group(
                "Today",
                [
                    ("Refunds this week", "/looks#threads", true),
                    ("Orders from Pune", "/looks#threads", false),
                ],
            )
            .group(
                "Earlier",
                [("GST summary for October", "/looks#threads", false)],
            ),
    ))
}

/// An import's steps as tiles, and a deal's place in its pipeline as a track.
fn steps() -> Stack {
    Stack::gap(4).children((
        el("div").attr("data-proof", "tiles").child(import_setup()),
        el("div").attr("data-proof", "track").child(deal_track()),
    ))
}

fn import_setup() -> Stepper {
    Stepper::new("Import contacts")
        .variant(StepperVariant::Tiles)
        .step(Step::new("File").link("/looks#file"))
        .step(Step::new("Columns").link("/looks#columns"))
        .step(Step::new("Matches").description("Waiting for the first row"))
        .step(Step::new("Owners"))
        .step(Step::new("Import"))
        .current(2)
}

fn deal_track() -> Stepper {
    Stepper::new("Deal")
        .variant(StepperVariant::Track)
        .steps(["Lead", "Qualified", "Proposal", "Closing", "Won"].map(Step::new))
        .current(2)
}

/// The destructive outline button beside the variants drawn as before.
fn buttons() -> Row {
    Row::gap(2).wrap().children((
        el("span")
            .attr("data-proof", "destructive-outline")
            .child(Button::new("Delete deal").variant(ButtonVariant::DestructiveOutline)),
        el("span")
            .attr("data-proof", "destructive-outline-link")
            .child(
                Button::new("Refund")
                    .variant(ButtonVariant::DestructiveOutline)
                    .size(Size::Sm)
                    .icon(icons::ROTATE_CCW)
                    .href("/looks#buttons"),
            ),
        el("span")
            .attr("data-proof", "destructive")
            .child(Button::destructive("Delete")),
        el("span")
            .attr("data-proof", "outline")
            .child(Button::outline("Cancel")),
    ))
}

fn page(shell: AppShell, path: &str, asked: &Asked) -> AppPage {
    shell.page("Looks").body(Stack::gap(10).children((
        section("segmented", "Segmented links", segmented(path, asked)),
        section("buttons", "Buttons", buttons()),
        section("steps", "Steps", steps()),
        section("small", "Small options", small()),
        section("inverse", "The inverse surface", inverse(path, asked)),
    )))
}

fn at(path: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_owned(),
    }
}

fn shell() -> Shell {
    crate::shell().group(
        NavGroup::new("Looks")
            .item(NavItem::new("Looks", "/looks").icon(icons::PALETTE))
            .item(NavItem::new("Looks, built-in theme", "/looks/plain").icon(icons::PALETTE)),
    )
}

pub struct LookPages;

impl Injectable for LookPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for LookPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/looks",
                |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                    let asked = asked(query.as_deref());
                    let shell = shell()
                        .appearance(Appearance::new(looks_theme(), Mode::Light))
                        .for_request(at("/looks", query.as_deref()), &headers);
                    page(shell, "/looks", &asked)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/looks/plain",
                |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                    let asked = asked(query.as_deref());
                    let shell = shell().for_request(at("/looks/plain", query.as_deref()), &headers);
                    page(shell, "/looks/plain", &asked)
                },
            )
            .public(),
        ]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_looks_theme_passes_the_contrast_check_in_both_modes() {
        let theme = super::looks_theme();
        assert!(theme.css().is_some_and(|css| css.contains("--segment-on:")));
    }
}
