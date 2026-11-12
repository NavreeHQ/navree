//! An app's own stylesheet and brand theme, the figure tile, the info tip, chart skeletons,
//! the breathing guide, the dock, the day pager and a view switcher.
//!
//! The people and places are invented: Punjabi and North Indian names, Indian cities, rupees in
//! lakh grouping and IST.

use navree::ui::charts::*;
use navree::ui::theme::{Appearance, Mode, Rgb, Theme, ThemeId};
use navree::ui::*;

use crate::catalog::{Example, gallery, page, single};

const SOURCE: &str = include_str!("app_blocks.rs");

pub fn all() -> Vec<Example> {
    vec![
        // App styles
        page!(
            "app-styles",
            "brand",
            "A brand theme and its own sheet",
            brand_page
        )
        .fill()
        .width(1100)
        .height(720),
        // Figure tiles
        single!("stat", "tile", "A figure tile", stat_tile)
            .start()
            .width(360)
            .height(210),
        single!("stat", "tile-ring", "A ring beside the figure", stat_ring)
            .start()
            .width(360)
            .height(150),
        single!("stat", "tiles", "A row of tiles", stat_tiles)
            .start()
            .width(1000)
            .height(420),
        // Info tip
        single!("info-tip", "thumb", "An info tip", info_tip_thumb)
            .show()
            .height(150),
        single!("info-tip", "default", "A note on a chart", info_tip_default)
            .show()
            .height(260),
        gallery!("info-tip", "marks", "Marks and sides", info_tip_marks).height(220),
        // Chart skeletons
        single!("chart-skeleton", "thumb", "A skeleton", skeleton_thumb)
            .start()
            .width(160)
            .height(140),
        gallery!("chart-skeleton", "shapes", "Every shape", skeleton_shapes)
            .wide()
            .height(420),
        single!(
            "chart-skeleton",
            "region",
            "In a region that loads late",
            skeleton_region
        )
        .start()
        .height(260),
        // Breathing guide
        single!(
            "breathing-guide",
            "thumb",
            "A breathing guide",
            breathing_thumb
        )
        .start()
        .width(300)
        .height(280),
        single!(
            "breathing-guide",
            "default",
            "The 4-7-8 guide",
            breathing_default
        )
        .start()
        .height(420),
        // Dock
        single!("dock", "thumb", "A dock", dock_thumb)
            .fill()
            .width(360)
            .height(120),
        single!("dock", "default", "The sections of an app", dock_default)
            .fill()
            .width(480)
            .height(200),
        // Day pager
        single!("day-pager", "thumb", "A day pager", day_pager_thumb)
            .start()
            .width(360)
            .height(80),
        single!(
            "day-pager",
            "default",
            "Scores and a calendar",
            day_pager_default
        )
        .start()
        .width(420)
        .height(300),
        // A view switcher
        single!("tabs", "view-switcher", "A view switcher", view_switcher)
            .start()
            .height(300),
    ]
}

// ---------------------------------------------------------------------------------------
// App styles
// ---------------------------------------------------------------------------------------

/// The sample app's stylesheet, compiled into the app: the toolkit serves it by its content hash.
static BRAND: AppSheet = AppSheet::new(
    "sutlej-fit",
    include_bytes!("../../data/brand.css"),
    8 * 1024,
);

/// The brand's teal as the accent in both palettes, with the text on it and the focus ring
/// derived. A theme whose text falls short of WCAG AA is refused, so this either passes or
/// the app doesn't start.
fn brand_theme() -> Theme {
    let spec = Theme::NAVREE
        .spec()
        .with_accent(Rgb::hex(0x2dd4bf), Rgb::hex(0x0f766e));
    Theme::custom(ThemeId::parse("sutlej-fit").unwrap(), "Sutlej Fit", spec).unwrap()
}

fn brand_page() -> AppPage {
    let shell = Shell::new("Sutlej Fit")
        .appearance(Appearance::new(brand_theme(), Mode::System))
        .stylesheet(&BRAND)
        .group(
            NavGroup::new("Today")
                .item(NavItem::new("Home", "/").icon(icons::HOUSE))
                .item(NavItem::new("Sleep", "/sleep").icon(icons::MOON))
                .item(NavItem::new("Heart", "/heart").icon(icons::HEART_PULSE))
                .item(NavItem::new("Fitness", "/fitness").icon(icons::ACTIVITY)),
        )
        .user(
            UserMenu::new("Gurpreet Kaur", "gurpreet@example.com").action("Sign out", "/sign-out"),
        );
    shell.page("Today", "/").body(Stack::gap(5).children((
        el("section").attr("class", "brand-hero").children((
            el("span")
                .attr("class", "brand-kicker")
                .child("Wednesday, 11 November"),
            el("h2").child("Good evening, Gurpreet. Your recovery is back on track."),
            el("p").child(
                "Seven hours twenty minutes of sleep and a resting heart rate two beats under your week.",
            ),
        )),
        el("div").attr("class", "brand-tiles").child(
            Grid::fit(10).gap(4).children((
                Stat::new("Steps", "9,412")
                    .icon(icons::FOOTPRINTS)
                    .tone(Tone::named("orange"))
                    .change(Trend::new(12.5))
                    .caption("vs last week"),
                Stat::new("Sleep", "7")
                    .icon(icons::MOON)
                    .tone(Tone::named("violet"))
                    .unit("h 20 min")
                    .change(Trend::new(6.1))
                    .caption("vs last week"),
                Stat::new("Resting heart rate", "58")
                    .icon(icons::HEART_PULSE)
                    .tone(Tone::Danger)
                    .unit("bpm")
                    .change(Trend::new(-3.3).up_is_bad())
                    .caption("vs last week"),
                Stat::new("Recovery", "82")
                    .icon(icons::ACTIVITY)
                    .tone(Tone::Success)
                    .unit("%")
                    .change(Trend::new(4.0))
                    .caption("vs yesterday"),
            )),
        ),
    )))
}

// ---------------------------------------------------------------------------------------
// Figure tiles
// ---------------------------------------------------------------------------------------

fn stat_tile() -> Stat {
    Stat::new("Resting heart rate", "58")
        .icon(icons::HEART_PULSE)
        .tone(Tone::Danger)
        .unit("bpm")
        .status(Badge::new("Athletic").tone(Tone::Success).dot())
        .change(Trend::new(-3.3).up_is_bad())
        .caption("vs last week")
        .chart(
            Sparkline::new(
                "Resting heart rate, last 7 days",
                [61, 60, 62, 59, 58, 59, 58],
            )
            .smooth()
            .height(36),
        )
}

fn stat_ring() -> Stat {
    Stat::new("Steps", "9,412")
        .icon(icons::FOOTPRINTS)
        .tone(Tone::named("orange"))
        .change(Trend::new(12.5))
        .caption("Goal 10,000")
        .aside(
            Ring::new("Steps", 9_412)
                .max(10_000)
                .tone(Tone::named("orange"))
                .size(64),
        )
}

fn stat_tiles() -> Grid {
    Grid::fit(14).gap(4).children((
        Stat::new("Sleep", "7")
            .icon(icons::MOON)
            .tone(Tone::named("violet"))
            .unit("h 20 min")
            .status(Badge::new("Good").tone(Tone::Success))
            .change(Trend::new(6.1))
            .caption("vs last week")
            .chart(
                Sparkline::new("Sleep, last 7 nights", [6.6, 7.1, 7.4, 6.9, 7.8, 7.2, 7.3])
                    .area()
                    .smooth()
                    .height(36),
            ),
        Stat::new("Recovery", "82")
            .icon(icons::ACTIVITY)
            .tone(Tone::Success)
            .unit("%")
            .change(Trend::new(4.0))
            .caption("vs yesterday")
            .aside(Ring::new("Recovery", 82).size(64).glow()),
        Stat::new("Steps", "9,412")
            .icon(icons::FOOTPRINTS)
            .tone(Tone::named("orange"))
            .change(Trend::new(12.5))
            .caption("Goal 10,000")
            .aside(
                Ring::new("Steps", 9_412)
                    .max(10_000)
                    .tone(Tone::named("orange"))
                    .size(64),
            ),
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
    ))
}

// ---------------------------------------------------------------------------------------
// Info tip
// ---------------------------------------------------------------------------------------

fn info_tip_thumb() -> InfoTip {
    InfoTip::new("thumb-tip", "Brain age")
        .body("A wellness estimate from your sleep and heart rate variability.")
}

fn info_tip_default() -> Row {
    Row::gap(2).children((
        Heading::h3("Autonomic balance"),
        InfoTip::new("autonomic-tip", "Autonomic balance")
            .title("How to read this chart")
            .align(PopupAlign::Start)
            .body((
                Text::new(
                    "It compares your resting and your active nervous system through the night.",
                ),
                Text::muted("A wellness estimate, not a diagnosis."),
            )),
    ))
}

fn info_tip_marks() -> Vec<(&'static str, Row)> {
    vec![
        (
            "A question mark",
            Row::gap(2).children((
                Text::muted("HRV status"),
                InfoTip::new("mark-question", "HRV status")
                    .body("Your heart rate variability against your own baseline."),
            )),
        ),
        (
            "An i, opening to the right",
            Row::gap(2).children((
                Text::muted("Training status"),
                InfoTip::new("mark-info", "Training status")
                    .mark(InfoMark::Info)
                    .side(Side::Right)
                    .body("Productive, maintaining or detraining, from your last four weeks."),
            )),
        ),
    ]
}

// ---------------------------------------------------------------------------------------
// Chart skeletons
// ---------------------------------------------------------------------------------------

fn skeleton_thumb() -> ChartSkeleton {
    ChartSkeleton::ring(96).label("Loading recovery")
}

fn skeleton_shapes() -> Vec<(&'static str, ChartSkeleton)> {
    vec![
        ("Ring", ChartSkeleton::ring(120).label("Loading recovery")),
        ("Gauge", ChartSkeleton::gauge(120).label("Loading stress")),
        ("Line", ChartSkeleton::line(120).label("Loading heart rate")),
        (
            "Bars",
            ChartSkeleton::bars(12, 120).label("Loading stress by hour"),
        ),
        (
            "Heat map",
            ChartSkeleton::heat_map(26).label("Loading activity"),
        ),
        (
            "Matrix",
            ChartSkeleton::matrix(6).label("Loading correlations"),
        ),
        (
            "Meter",
            ChartSkeleton::meter().label("Loading sleep stages"),
        ),
    ]
}

fn skeleton_region() -> Card {
    Card::titled("24-hour heart rate").body(
        Region::new("heart-rate")
            .src("/fragments/heart-rate")
            .lazy()
            .children(ChartSkeleton::line(160).label("Loading heart rate")),
    )
}

// ---------------------------------------------------------------------------------------
// Breathing guide
// ---------------------------------------------------------------------------------------

fn breathing_thumb() -> BreathingGuide {
    BreathingGuide::new("thumb-guide", "4-7-8 breathing").size(160)
}

fn breathing_default() -> Card {
    Card::titled("Relax").description("4-7-8 breathing").body(
        BreathingGuide::new("relax-guide", "4-7-8 breathing")
            .size(240)
            .tone(Tone::named("cyan")),
    )
}

// ---------------------------------------------------------------------------------------
// Dock
// ---------------------------------------------------------------------------------------

fn dock_thumb() -> Dock {
    Dock::new("Sections")
        .item(DockItem::new("Sleep", "/sleep").icon(icons::MOON))
        .item(
            DockItem::new("Home", "/")
                .icon(icons::HOUSE)
                .prominent()
                .current(),
        )
        .item(DockItem::new("Fitness", "/fitness").icon(icons::ACTIVITY))
}

fn dock_default() -> Dock {
    Dock::new("Sections")
        .item(
            DockItem::new("Sleep", "/sleep")
                .icon(icons::MOON)
                .tone(Tone::named("violet")),
        )
        .item(
            DockItem::new("Heart", "/heart")
                .icon(icons::HEART_PULSE)
                .tone(Tone::Danger),
        )
        .item(
            DockItem::new("Home", "/")
                .icon(icons::HOUSE)
                .prominent()
                .current(),
        )
        .item(
            DockItem::new("Mind", "/mind")
                .icon(icons::BRAIN)
                .tone(Tone::named("cyan")),
        )
        .item(
            DockItem::new("Fitness", "/fitness")
                .icon(icons::ACTIVITY)
                .tone(Tone::Success),
        )
}

// ---------------------------------------------------------------------------------------
// Day pager
// ---------------------------------------------------------------------------------------

fn day_pager_thumb() -> DayPager {
    DayPager::new("thumb-day", "Today").previous("/?date=2026-11-11")
}

fn day_pager_default() -> Stack {
    let scores = || {
        Rings::new("Scores for the day")
            .ring(RingValue::new("Movement", 35))
            .ring(RingValue::new("Recovery", 72))
            .ring(RingValue::new("Sleep", 78))
            .size(28)
            .inline()
    };
    Stack::gap(4).children((
        DayPager::new("day-recent", "Wed, 11 Nov")
            .previous("/?date=2026-11-10")
            .next("/?date=2026-11-12")
            .scores(scores()),
        DayPager::new("day-picker", "Tue, 10 Nov")
            .previous("/?date=2026-11-09")
            .next("/?date=2026-11-11")
            .scores(scores())
            .picker((
                Text::muted("Days with data"),
                Text::new("8 April to 11 November"),
            )),
    ))
}

// ---------------------------------------------------------------------------------------
// A view switcher
// ---------------------------------------------------------------------------------------

fn view_switcher() -> Tabs {
    Tabs::new("day-view", "Day view")
        .variant(TabsVariant::Segmented)
        .tab(
            Tab::new("radial", "Radial").icon(icons::CHART_PIE).panel(
                Rings::new("Today's scores")
                    .ring(RingValue::new("Movement", 35))
                    .ring(RingValue::new("Recovery", 72))
                    .ring(RingValue::new("Sleep", 78))
                    .size(160)
                    .center_values()
                    .legend(),
            ),
        )
        .tab(
            Tab::new("story", "Story").icon(icons::LAYOUT_LIST).panel(
                MiniColumns::new("Workouts this week")
                    .id("week-columns")
                    .height(120)
                    .column_name("Day")
                    .part("Strength", Ink::slot(2))
                    .part("Running", Ink::slot(1))
                    .column(MiniColumn::new("Mon", [45, 0]))
                    .column(MiniColumn::new("Wed", [50, 20]))
                    .column(MiniColumn::new("Sat", [30, 60])),
            ),
        )
        .active("radial")
}
