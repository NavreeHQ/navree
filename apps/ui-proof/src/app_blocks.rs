//! The app blocks: the app's own stylesheet and typeface, the figure tile, the info tip, chart
//! skeletons, the breathing guide, the dock, the day pager and a view switcher.
//!
//! | Page | Shows |
//! |---|---|
//! | `/app-blocks` | every case; `?case=<name>` draws one alone |
//! | `/app-blocks/shell` | a shell of its own: the links in the top bar with the app's mark (an [`AppImage`]), a day pager in the middle of the bar, one theme (so the menu has modes only) and a title template |
//! | `/app-blocks/fragment/trends` | a region's answer that holds lazy tabs |
//! | `/app-blocks/fragment/trends/{range}` | a lazy tab's panel |
//!
//! The people and places are invented: Punjabi and North Indian names and Indian cities.

use std::sync::Arc;

use http::Method;
use navree::http::extract::Query;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::charts::*;
use navree::ui::theme::{Appearance, Mode, Theme};
use navree::ui::*;
use serde::Deserialize;

const NASTY: &str = "<script>window.__pwned=1</script><img src=x onerror=\"window.__pwned=1\"> \"quotes\" 'single' javascript:alert(1)";

/// The app's own stylesheet, compiled in and served by the toolkit at a content-hash address.
pub static BRAND: AppSheet = AppSheet::new(
    "proof-brand",
    include_bytes!("../assets/brand.css"),
    8 * 1024,
);

/// The app's own mark, compiled in and served by the toolkit at a content-hash address.
pub static MARK: AppImage =
    AppImage::new("proof-mark", include_bytes!("../assets/mark.svg"), 4 * 1024);

/// The app's own typeface: the toolkit's Geist file under another family name, so the page
/// shows a font that comes from the app's route and not from the toolkit's.
pub static FONT: AppFont = AppFont::new(
    "proof-sans",
    "Brand Sans",
    include_bytes!("../../../crates/navree-ui/assets/fonts/geist.woff2"),
    64 * 1024,
)
.weights(100, 900);

#[derive(Default, Deserialize)]
pub struct Case {
    pub case: Option<String>,
    pub tab: Option<String>,
}

fn section(name: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("data-case", name)
        .child(Card::new().body(content))
}

fn week(base: f64, swing: f64) -> Vec<f64> {
    (0..7)
        .map(|day| (base + swing * (f64::from(day) * 1.3).sin()).round())
        .collect()
}

fn tile_sleep() -> Stat {
    Stat::new("Sleep", "7")
        .icon(icons::MOON)
        .tone(Tone::named("violet"))
        .unit("h 20 min")
        .status(Badge::new("Good").tone(Tone::Success))
        .change(Trend::new(6.1))
        .caption("vs last week")
        .chart(
            Sparkline::new("Sleep, last 7 nights", week(7.0, 0.8))
                .area()
                .smooth()
                .height(36),
        )
}

fn tile_heart() -> Stat {
    Stat::new("Resting heart rate", "58")
        .icon(icons::HEART_PULSE)
        .tone(Tone::Danger)
        .unit("bpm")
        .status(Badge::new("Athletic").tone(Tone::Success).dot())
        .change(Trend::new(-3.3).up_is_bad())
        .caption("vs last week")
        .chart(
            Sparkline::new("Resting heart rate, last 7 days", week(60.0, 3.0))
                .smooth()
                .height(36),
        )
}

fn tile_recovery() -> Stat {
    Stat::new("Recovery", "82")
        .icon(icons::ACTIVITY)
        .tone(Tone::Success)
        .unit("%")
        .change(Trend::new(4.0))
        .caption("vs yesterday")
        .aside(Ring::new("Recovery", 82).size(64).glow())
}

fn tile_steps() -> Stat {
    Stat::new("Steps", "9,412")
        .icon(icons::FOOTPRINTS)
        .tone(Tone::named("orange"))
        .change(Trend::new(12.5))
        .caption("Goal 10,000")
        .aside(
            Ring::new("Steps", 9412)
                .max(10_000)
                .size(64)
                .tone(Tone::named("orange")),
        )
        .href("/app-blocks?case=tiles")
}

fn cases(tab: Option<&str>) -> Vec<(&'static str, Node)> {
    vec![
        (
            "tiles",
            Grid::fit(14)
                .gap(4)
                .children((
                    tile_sleep(),
                    tile_heart(),
                    tile_recovery(),
                    tile_steps(),
                    Stat::new("Distance", "6.8").icon(icons::MAP_PIN).unit("km"),
                    Stat::new("Body temperature", "36.6")
                        .icon(icons::THERMOMETER)
                        .tone(Tone::Warning)
                        .unit("°C")
                        .status(Badge::new("Normal").tone(Tone::Success))
                        .change(Trend::new(0.0)),
                    Stat::new("Blood oxygen", "–")
                        .icon(icons::DROPLETS)
                        .tone(Tone::Info)
                        .caption("No reading yet today"),
                    Stat::new(NASTY, NASTY)
                        .icon(icons::HEART)
                        .unit(NASTY)
                        .caption(NASTY)
                        .status(Badge::new(NASTY)),
                ))
                .into(),
        ),
        (
            "plain-tiles",
            Grid::fit(12)
                .gap(4)
                .children((
                    Stat::new("Open deals", "312").trend("+12 this week", true),
                    Stat::new("Pipeline value", "₹42,00,000").trend("+3.1%", true),
                    Stat::new("Won this month", "₹6,40,000").change(Trend::new(-2.4)),
                ))
                .into(),
        ),
        (
            "info-tip",
            Stack::gap(4)
                .children((
                    Row::gap(2).children((
                        Heading::h2("Autonomic balance"),
                        InfoTip::new("tip-autonomic", "Autonomic balance")
                            .title("How to read this chart")
                            .body((
                                Text::new("The balance compares your resting and your active nervous system through the night."),
                                el("ul").children((
                                    el("li").child("Sympathetic: the stress response."),
                                    el("li").child("Parasympathetic: rest and digest."),
                                )),
                            )),
                    )),
                    Row::gap(2).children((
                        Text::muted("Brain age"),
                        InfoTip::new("tip-brain", "Brain age")
                            .mark(InfoMark::Info)
                            .side(Side::Right)
                            .body("A wellness estimate from your sleep and heart rate variability. It is not a diagnosis."),
                    )),
                    Row::gap(2).children((
                        Text::muted(NASTY),
                        InfoTip::new("tip-hostile", NASTY)
                            .title(NASTY)
                            .align(PopupAlign::End)
                            .body(NASTY),
                    )),
                ))
                .into(),
        ),
        (
            "skeletons",
            Grid::fit(14)
                .gap(6)
                .children((
                    ChartSkeleton::ring(120).label("Loading recovery"),
                    ChartSkeleton::gauge(120).label("Loading stress"),
                    ChartSkeleton::line(140).label("Loading heart rate"),
                    ChartSkeleton::bars(12, 140).label("Loading stress by hour"),
                    ChartSkeleton::heat_map(26).label("Loading activity"),
                    ChartSkeleton::matrix(6).label("Loading correlations"),
                    ChartSkeleton::meter().label("Loading sleep stages"),
                ))
                .into(),
        ),
        (
            "breathing",
            Grid::fit(20)
                .gap(6)
                .children((
                    BreathingGuide::new("guide", "4-7-8 breathing").size(220),
                    BreathingGuide::new("guide-small", "Resonance breathing")
                        .size(120)
                        .tone(Tone::named("cyan")),
                ))
                .into(),
        ),
        (
            "dock",
            Stack::gap(3)
                .children((
                    Text::muted("The dock floats at the bottom of this page."),
                    Dock::new("Sections")
                        .item(
                            DockItem::new("Sleep", "/app-blocks?case=dock")
                                .icon(icons::MOON)
                                .tone(Tone::named("violet")),
                        )
                        .item(
                            DockItem::new("Heart", "/app-blocks?case=dock")
                                .icon(icons::HEART_PULSE)
                                .tone(Tone::Danger),
                        )
                        .item(
                            DockItem::new("Home", "/app-blocks?case=dock")
                                .icon(icons::HOUSE)
                                .prominent()
                                .current(),
                        )
                        .item(
                            DockItem::new("Mind", "/app-blocks?case=dock")
                                .icon(icons::BRAIN)
                                .tone(Tone::named("cyan")),
                        )
                        .item(
                            DockItem::new("Fitness", "/app-blocks?case=dock")
                                .icon(icons::ACTIVITY)
                                .tone(Tone::Success),
                        ),
                ))
                .into(),
        ),
        (
            "day-pager",
            Stack::gap(4)
                .children((
                    DayPager::new("day-today", "Today")
                        .previous("/app-blocks?case=day-pager&date=2026-11-02")
                        .scores(scores()),
                    DayPager::new("day-first", "Wed, 8 Apr")
                        .next("/app-blocks?case=day-pager&date=2026-04-09")
                        .scores(scores()),
                    DayPager::new("day-picker", "Tue, 3 Nov")
                        .previous("/app-blocks?case=day-pager&date=2026-11-02")
                        .next("/app-blocks?case=day-pager&date=2026-11-04")
                        .scores(scores())
                        .picker((
                            Text::muted("Days with data"),
                            Text::new("8 April to 3 November"),
                        )),
                    DayPager::new("day-plain", "Thu, 5 Nov"),
                ))
                .into(),
        ),
        (
            "view-switcher",
            Tabs::new("view-switcher", "Day view")
                .variant(TabsVariant::Segmented)
                .keep("case", "view-switcher")
                .tab(Tab::new("radial", "Radial").icon(icons::CHART_PIE).panel(
                    Rings::new("Today's scores")
                        .ring(RingValue::new("Movement", 35))
                        .ring(RingValue::new("Recovery", 72))
                        .ring(RingValue::new("Sleep", 78))
                        .size(160)
                        .center_values()
                        .legend(),
                ))
                .tab(Tab::new("story", "Story").icon(icons::LAYOUT_LIST).panel(
                    MiniColumns::new("Workouts this week")
                        .id("view-columns")
                        .height(120)
                        .column_name("Day")
                        .part("Strength", Ink::slot(2))
                        .part("Running", Ink::slot(1))
                        .column(MiniColumn::new("Mon", [45, 0]))
                        .column(MiniColumn::new("Wed", [50, 20]))
                        .column(MiniColumn::new("Sat", [30, 60])),
                ))
                .active_from(tab)
                .into(),
        ),
        (
            "copy-field",
            Stack::gap(4)
                .children((
                    CopyField::new("invite-link", "Invite link", "https://kesar.example/join/8k2m-q4rt")
                        .help("Anyone with the link can ask to join your family plan."),
                    CopyField::new("copy-hostile", NASTY, NASTY).labels("Copy it", "Done"),
                ))
                .into(),
        ),
        (
            "ratio-grid",
            Stack::gap(4)
                .children((
                    Grid::ratio([7, 5]).gap(4).children((
                        Card::new().title("Trends").body(Text::new("Seven parts of twelve: the wide column.")),
                        Card::new().title("Notes").body(Text::new("Five parts of twelve.")),
                    )),
                    Grid::ratio([1, 2, 1]).gap(4).children((
                        Card::new().body(Text::new("One")),
                        Card::new().body(Text::new("Two parts")),
                        Card::new().body(Text::new("One")),
                    )),
                ))
                .into(),
        ),
        (
            "dock-phones",
            Stack::gap(3)
                .children((
                    Text::muted("This dock shows on phones only: from 768 px the top bar has the sections."),
                    Dock::new("Sections on a phone")
                        .below(Screen::Md)
                        .item(DockItem::new("Sleep", "/app-blocks?case=dock-phones").icon(icons::MOON))
                        .item(
                            DockItem::new("Home", "/app-blocks?case=dock-phones")
                                .icon(icons::HOUSE)
                                .prominent()
                                .current(),
                        )
                        .item(DockItem::new("Mind", "/app-blocks?case=dock-phones").icon(icons::BRAIN)),
                ))
                .into(),
        ),
        (
            "app-image",
            Row::gap(4)
                .children((
                    el("img")
                        .attr("src", MARK.url())
                        .attr("alt", "The Kesar mark")
                        .attr("width", 48_i64)
                        .attr("height", 48_i64),
                    Picture::new(&MARK, "The Kesar mark, larger", 96, 96),
                ))
                .into(),
        ),
        (
            "nested-lazy",
            Region::new("nested-region")
                .src("/app-blocks/fragment/trends")
                .lazy()
                .into(),
        ),
        (
            "brand",
            el("div")
                .attr("class", "proof-brand")
                .children((
                    el("h2").child("Good evening, Gurpreet"),
                    el("p").child("Seven hours twenty minutes of sleep, and a resting heart rate two beats under your week."),
                ))
                .into(),
        ),
    ]
}

fn scores() -> Rings {
    Rings::new("Scores for the day")
        .ring(RingValue::new("Movement", 35))
        .ring(RingValue::new("Recovery", 72))
        .ring(RingValue::new("Sleep", 78))
        .size(28)
        .inline()
}

/// A shell of its own, with its links in the top bar, the app's mark, one theme and a title
/// template.
fn own_shell() -> Shell {
    Shell::new("Kesar")
        .logo(&MARK)
        .layout(ShellLayout::Header)
        .themes([Theme::OCEAN])
        .appearance(Appearance::new(Theme::OCEAN, Mode::Dark))
        .theme_menu()
        .title_template("{} · Kesar")
        .group(
            NavGroup::plain()
                .item(NavItem::new("Today", "/app-blocks/shell").icon(icons::HOUSE))
                .item(NavItem::new("Sleep", "/app-blocks?case=dock").icon(icons::MOON))
                .item(NavItem::new("Heart", "/app-blocks?case=tiles").icon(icons::HEART_PULSE)),
        )
}

fn shell_page(headers: &http::HeaderMap, query: Option<&str>) -> AppPage {
    let path = match query {
        Some(query) => format!("/app-blocks/shell?{query}"),
        None => "/app-blocks/shell".to_owned(),
    };
    own_shell()
        .for_request(path, headers)
        .page("Good evening, Simran")
        .document_title("Today")
        .topbar(
            DayPager::new("shell-day", "Today")
                .previous("/app-blocks/shell?date=2026-11-11")
                .scores(scores()),
        )
        .body((
            Text::muted("The day pager stands in the middle of the bar; the mark stays on phones."),
            Grid::ratio([7, 5]).gap(4).children((
                Card::new()
                    .title("Sleep")
                    .body(Text::new("7 h 20 min, 88 points.")),
                Card::new()
                    .title("Heart")
                    .body(Text::new("Resting 58 bpm.")),
            )),
            Dock::new("Sections")
                .below(Screen::Md)
                .item(
                    DockItem::new("Today", "/app-blocks/shell")
                        .icon(icons::HOUSE)
                        .current(),
                )
                .item(DockItem::new("Sleep", "/app-blocks?case=dock").icon(icons::MOON)),
        ))
}

/// A region's answer: a card with tabs that load their panels when shown, drawn as a whole
/// view, as an app answers a fragment.
fn trends_fragment() -> View {
    View::new("Trends").body(
        Tabs::new("trend-range", "Nights to show")
            .variant(TabsVariant::Segmented)
            .tab(
                Tab::new("7", "7 nights")
                    .panel(Text::new("Seven nights: 7 h 12 min on average."))
                    .lazy("/app-blocks/fragment/trends/7"),
            )
            .tab(Tab::new("30", "30 nights").lazy("/app-blocks/fragment/trends/30")),
    )
}

fn gallery(shell: &AppShell, case: &Case) -> AppPage {
    let wanted = case.case.as_deref();
    let sections: Vec<Element> = cases(case.tab.as_deref())
        .into_iter()
        .filter(|(name, _)| wanted.is_none_or(|w| w == *name))
        .map(|(name, node)| section(name, node))
        .collect();
    shell
        .page("App blocks")
        .stylesheet(&BRAND)
        .font(&FONT)
        .body(Stack::gap(4).children((
            Text::muted("The blocks of an app's dashboard: tiles, a tip, skeletons, a breathing guide, a dock and a day pager."),
            Grid::cols(if wanted.is_some() { 1 } else { 2 })
                .gap(4)
                .children(sections),
        )))
}

pub struct AppBlockPages;

impl Injectable for AppBlockPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for AppBlockPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/app-blocks",
                |shell: AppShell, Query(case): Query<Case>| async move { gallery(&shell, &case) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/app-blocks/shell",
                |parts: http::request::Parts| async move {
                    shell_page(&parts.headers, parts.uri.query())
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/app-blocks/fragment/trends",
                || async { trends_fragment() },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/app-blocks/fragment/trends/{range}",
                |navree::http::extract::Path(range): navree::http::extract::Path<String>| async move {
                    let nights = if range == "30" { "Thirty nights: 7 h 4 min on average." } else { "Seven nights: 7 h 12 min on average." };
                    View::new("Trends").body(Text::new(nights))
                },
            )
            .public(),
        ]
    }
}
