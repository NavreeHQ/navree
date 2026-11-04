//! Line, area and bar charts, donut and pie, sparkline, funnel, and reports.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use navree::ui::charts::*;
use navree::ui::dates::{DatePreset, DateRules, RangeRules};
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("charts.rs");

pub fn all() -> Vec<Example> {
    vec![
        // Line chart
        single!("line-chart", "thumb", "A line chart", line_chart_thumb)
            .start()
            .width(420)
            .height(190),
        single!(
            "line-chart",
            "default",
            "Two series over seven days",
            line_chart_default
        )
        .start()
        .height(402),
        single!(
            "line-chart",
            "hours",
            "A time axis by the hour",
            line_chart_hours
        )
        .start()
        .height(372),
        single!("line-chart", "gaps", "Gaps and a target", line_chart_gaps)
            .start()
            .height(372),
        single!(
            "line-chart",
            "annotations",
            "A band, an event and a target",
            line_chart_annotations
        )
        .start()
        .height(350),
        single!(
            "line-chart",
            "toggles",
            "Series you can hide, and status colors",
            line_chart_toggles
        )
        .start()
        .height(410),
        single!(
            "line-chart",
            "many",
            "More than eight series",
            line_chart_many
        )
        .start()
        .height(410),
        single!(
            "line-chart",
            "states",
            "Loading, failed and empty",
            line_chart_states
        )
        .start()
        .width(820)
        .height(340),
        single!(
            "line-chart",
            "locales",
            "Numbers in German and Indian English",
            line_chart_locales
        )
        .start()
        .width(820)
        .height(351),
        single!("line-chart", "table", "The table, shown", line_chart_table)
            .start()
            .height(550),
        // Area chart
        single!("area-chart", "thumb", "An area chart", area_chart_thumb)
            .start()
            .width(420)
            .height(190),
        single!(
            "area-chart",
            "default",
            "A smooth area with an event",
            area_chart_default
        )
        .start()
        .height(372),
        single!(
            "area-chart",
            "stacked",
            "Parts of a total over time",
            area_chart_stacked
        )
        .start()
        .height(402),
        single!(
            "area-chart",
            "percent",
            "Stacked to 100%",
            area_chart_percent
        )
        .start()
        .height(402),
        // Bar chart
        single!("bar-chart", "thumb", "A bar chart", bar_chart_thumb)
            .start()
            .width(420)
            .height(190),
        single!(
            "bar-chart",
            "default",
            "One series against a goal",
            bar_chart_default
        )
        .start()
        .height(350),
        single!(
            "bar-chart",
            "grouped",
            "Grouped bars with switches",
            bar_chart_grouped
        )
        .start()
        .height(390),
        single!(
            "bar-chart",
            "stacked",
            "Stacked bars in status colors",
            bar_chart_stacked
        )
        .start()
        .height(380),
        single!("bar-chart", "percent", "Stacked to 100%", bar_chart_percent)
            .start()
            .height(380),
        single!(
            "bar-chart",
            "negative",
            "Values below zero",
            bar_chart_negative
        )
        .start()
        .height(350),
        single!(
            "bar-chart",
            "horizontal",
            "A ranked list",
            bar_chart_horizontal
        )
        .start()
        .height(330),
        single!(
            "bar-chart",
            "horizontal-stacked",
            "Horizontal and stacked",
            bar_chart_horizontal_stacked
        )
        .start()
        .height(255),
        single!("bar-chart", "many", "More bars than fit", bar_chart_many)
            .start()
            .width(820)
            .height(375),
        // Donut chart
        single!("donut-chart", "thumb", "A donut", donut_chart_thumb)
            .start()
            .width(420)
            .height(160),
        single!(
            "donut-chart",
            "default",
            "A donut with its total",
            donut_chart_default
        )
        .start()
        .height(310),
        single!("donut-chart", "pie", "A pie of shares", donut_chart_pie)
            .start()
            .height(310),
        single!(
            "donut-chart",
            "center",
            "Your own words in the middle",
            donut_chart_center
        )
        .start()
        .height(320),
        single!(
            "donut-chart",
            "states",
            "Loading, failed and empty",
            donut_chart_states
        )
        .start()
        .width(820)
        .height(275),
        // Sparkline
        single!("sparkline", "thumb", "A sparkline", sparkline_thumb)
            .start()
            .width(420)
            .height(200),
        single!(
            "sparkline",
            "default",
            "Sparklines beside figures",
            sparkline_default
        )
        .start()
        .width(820)
        .height(196),
        single!(
            "sparkline",
            "table",
            "A sparkline in each row",
            sparkline_table
        )
        .start()
        .height(210),
        single!(
            "sparkline",
            "states",
            "Loading, failed and empty",
            sparkline_states
        )
        .start()
        .width(820)
        .height(250),
        // Funnel
        single!("funnel", "thumb", "A funnel", funnel_thumb)
            .start()
            .width(520)
            .height(230),
        single!("funnel", "default", "Four stages", funnel_default)
            .start()
            .height(375),
        single!("funnel", "rates", "Rates in your own words", funnel_rates)
            .start()
            .height(450),
        single!(
            "funnel",
            "states",
            "Loading, failed and empty",
            funnel_states
        )
        .start()
        .width(820)
        .height(275),
        // Reports
        single!("reports", "thumb", "A report's controls", reports_thumb)
            .start()
            .height(100),
        single!(
            "reports",
            "default",
            "A report over a week",
            reports_default
        )
        .start()
        .height(746),
        single!(
            "reports",
            "refused",
            "A window the limits refuse",
            reports_refused
        )
        .start()
        .height(480),
        single!(
            "reports",
            "live",
            "Charts that refresh themselves",
            reports_live
        )
        .start()
        .height(421),
        gallery!(
            "reports",
            "grains",
            "The grains a window offers",
            reports_grains
        )
        .wide()
        .height(610),
    ]
}

fn line_chart_thumb() -> Chart {
    Chart::line("Opens")
        .id("line-chart-thumb")
        .hide_title()
        .height(96)
        .table(TableMode::ScreenReader)
        .categories(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
        .series(Series::new("Opens", [412, 530, 498, 610, 584, 233, 198]))
}

fn line_chart_default() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::line("Opens and clicks")
        .id("line-chart-default")
        .description("Per day, 28 October to 3 November")
        .days((0..7).map(|i| first + Duration::days(i)))
        .series(Series::new("Opens", [412, 530, 498, 610, 584, 233, 198]))
        .series(Series::new("Clicks", [96, 141, 120, 168, 152, 51, 44]))
}

fn line_chart_hours() -> Chart {
    let midnight = NaiveDate::from_ymd_opt(2026, 11, 3)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .unwrap_or_default();
    Chart::line("Opens by hour")
        .id("line-chart-hours")
        .description("3 November, on the clock of Asia/Kolkata")
        .times((0..24).map(|hour| midnight + Duration::hours(hour)))
        .series(Series::new(
            "Opens",
            [
                21, 14, 9, 7, 6, 11, 28, 64, 118, 152, 171, 164, 140, 133, 148, 160, 142, 118, 96,
                81, 66, 52, 38, 27,
            ],
        ))
}

fn line_chart_gaps() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap_or_default();
    Chart::line("Response time")
        .id("line-chart-gaps")
        .description("A day with no reading is a gap, not a zero")
        .days((0..14).map(|i| first + Duration::days(i)))
        .format(NumberFormat::number().suffix(" ms"))
        .y_zero(false)
        .series(Series::new(
            "p95",
            [
                Some(412.0),
                Some(430.0),
                None,
                None,
                Some(455.0),
                Some(470.0),
                Some(f64::NAN),
                Some(402.0),
                Some(389.0),
                None,
                Some(396.0),
                Some(401.0),
                Some(f64::INFINITY),
                Some(407.0),
            ],
        ))
        .annotation(Annotation::target(450, "Target 450 ms"))
}

fn line_chart_annotations() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap_or_default();
    let day = |offset| first + Duration::days(offset);
    Chart::line("Reports per day")
        .id("line-chart-annotations")
        .days((0..14).map(day))
        .series(Series::new(
            "Reports",
            [58, 61, 64, 55, 31, 28, 49, 66, 72, 85, 91, 77, 70, 63],
        ))
        .annotation(Annotation::band(day(4), day(6), "Holiday"))
        .annotation(Annotation::event(day(9), "Reminder sent"))
        .annotation(Annotation::target(80, "Target"))
}

fn line_chart_toggles() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::line("Health checks")
        .id("line-chart-toggles")
        .description("Click a name in the legend to hide its line")
        .days((0..7).map(|i| first + Duration::days(i)))
        .toggles()
        .series(Series::new("Passed", [118, 131, 126, 140, 137, 122, 129]).tone(Tone::Success))
        .series(Series::new("Slow", [24, 31, 27, 35, 29, 38, 33]).tone(Tone::Warning))
        .series(Series::new("Failed", [6, 9, 4, 12, 8, 15, 10]).tone(Tone::Danger))
}

fn line_chart_many() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    let regions = [
        "North", "South", "East", "West", "Central", "Coast", "Hills", "Islands", "Delta", "Plains",
    ];
    Chart::line("Sign-ups by region")
        .id("line-chart-many")
        .description("Ten regions: seven keep a color, the other three fold into Other")
        .days((0..7).map(|i| first + Duration::days(i)))
        .toggles()
        .series_list(regions.iter().enumerate().map(|(at, name)| {
            let at = at as i64;
            Series::new(*name, (0..7).map(|d| 70 - at * 5 + d * (at % 4 + 1) % 9))
        }))
}

fn line_chart_states() -> Grid {
    let state =
        |caption: &'static str, chart: Chart| Stack::gap(2).children((Text::muted(caption), chart));
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Chart::line("Opens by hour")
                .id("line-chart-loading")
                .loading(),
        ),
        state(
            "Failed",
            Chart::line("Opens by hour")
                .id("line-chart-failed")
                .failed(Some("The analytics service did not answer.".to_owned()))
                .retry("/reports/opens"),
        ),
        state(
            "Empty",
            Chart::line("Opens by hour").id("line-chart-empty").empty(
                "No opens yet",
                Some("They appear here when a message is read.".to_owned()),
            ),
        ),
    ))
}

fn line_chart_locales() -> Grid {
    let revenue = [1_250_000.0, 980_500.0, 1_402_300.0, 1_610_000.0];
    // The words a chart writes are fields: change the ones you need.
    let german = ChartLabels {
        show_table: "Tabelle anzeigen".into(),
        chart: "{title}, Diagramm".into(),
        ..ChartLabels::english()
    };
    Grid::cols(2).gap(4).children((
        Chart::line("Umsatz pro Monat")
            .id("line-chart-de")
            .locale("de")
            .labels(german)
            .categories(["Juli", "August", "September", "Oktober"])
            .format(NumberFormat::decimals(0).suffix(" €"))
            .series(Series::new("Umsatz", revenue)),
        Chart::line("Revenue by month")
            .id("line-chart-en-in")
            .locale("en-IN")
            .categories(["Jul", "Aug", "Sep", "Oct"])
            .format(NumberFormat::currency("₹").max_decimals(0))
            .series(Series::new("Revenue", revenue)),
    ))
}

fn line_chart_table() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 30).unwrap_or_default();
    Chart::line("Sign-ups")
        .id("line-chart-table")
        .days((0..5).map(|i| first + Duration::days(i)))
        .table(TableMode::Shown)
        .series(Series::new("Trial", [12, 18, 9, 22, 30]))
        .series(Series::new("Paid", [3, 4, 2, 6, 8]))
}

fn area_chart_thumb() -> Chart {
    Chart::area("Active users")
        .id("area-chart-thumb")
        .hide_title()
        .height(96)
        .smooth()
        .table(TableMode::ScreenReader)
        .categories(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
        .series(Series::new(
            "Active",
            [3_410, 3_620, 3_540, 3_980, 4_210, 3_300, 3_150],
        ))
}

fn area_chart_default() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap_or_default();
    Chart::area("Active users")
        .id("area-chart-default")
        .description("Per day, 21 October to 3 November")
        .days((0..14).map(|i| first + Duration::days(i)))
        .smooth()
        .series(Series::new(
            "Active",
            [
                3_410, 3_620, 3_540, 3_980, 4_210, 3_300, 3_150, 3_700, 3_880, 4_050, 4_420, 4_610,
                3_820, 3_560,
            ],
        ))
        .annotation(Annotation::event(first + Duration::days(7), "Release 2.4"))
}

fn area_chart_stacked() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap_or_default();
    Chart::area("Traffic by source")
        .id("area-chart-stacked")
        .description("Visits per day; the top edge is the total")
        .days((0..10).map(|i| first + Duration::days(i)))
        .stacked()
        .series(Series::new(
            "Search",
            [
                920, 1_010, 1_180, 1_240, 1_130, 1_090, 960, 1_320, 1_410, 1_280,
            ],
        ))
        .series(Series::new(
            "Direct",
            [610, 640, 590, 700, 660, 540, 520, 690, 720, 680],
        ))
        .series(Series::new(
            "Referral",
            [310, 280, 340, 420, 380, 260, 240, 390, 450, 410],
        ))
}

fn area_chart_percent() -> Chart {
    Chart::area("Visits by device")
        .id("area-chart-percent")
        .description("Each month's share of its total")
        .categories(["Jun", "Jul", "Aug", "Sep", "Oct", "Nov"])
        .stacked_percent()
        .series(Series::new(
            "Desktop",
            [5_400, 5_150, 4_900, 4_700, 4_450, 4_300],
        ))
        .series(Series::new(
            "Phone",
            [3_900, 4_300, 4_700, 5_000, 5_400, 5_650],
        ))
        .series(Series::new("Tablet", [700, 650, 600, 640, 610, 590]))
}

fn bar_chart_thumb() -> Chart {
    Chart::bar("Sent")
        .id("bar-chart-thumb")
        .hide_title()
        .height(96)
        .table(TableMode::ScreenReader)
        .categories(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
        .series(Series::new(
            "Sent",
            [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
        ))
}

fn bar_chart_default() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::bar("Sent per day")
        .id("bar-chart-default")
        .days((0..7).map(|i| first + Duration::days(i)))
        .series(Series::new(
            "Sent",
            [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
        ))
        .annotation(Annotation::target(1_500, "Goal"))
}

fn bar_chart_grouped() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::bar("Sent, opened and clicked")
        .id("bar-chart-grouped")
        .days((0..7).map(|i| first + Duration::days(i)))
        .toggles()
        .series(Series::new(
            "Sent",
            [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
        ))
        .series(Series::new(
            "Opened",
            [802, 951, 500, 1_200, 1_310, 222, 180],
        ))
        .series(Series::new("Clicked", [120, 180, 90, 260, 300, 40, 30]))
}

fn bar_chart_stacked() -> Chart {
    Chart::bar("Outcomes by week")
        .id("bar-chart-stacked")
        .categories(["Week 41", "Week 42", "Week 43", "Week 44", "Week 45"])
        .stacked()
        .series(Series::new("Reported", [42, 51, 63, 70, 81]).tone(Tone::Success))
        .series(Series::new("Ignored", [120, 110, 104, 98, 90]))
        .series(Series::new("Clicked", [30, 26, 21, 15, 12]).tone(Tone::Danger))
}

fn bar_chart_percent() -> Chart {
    Chart::bar("Share of outcomes")
        .id("bar-chart-percent")
        .categories(["Sales", "Support", "Finance", "Engineering"])
        .stacked_percent()
        .series(Series::new("Reported", [42, 51, 63, 70]).tone(Tone::Success))
        .series(Series::new("Ignored", [120, 110, 104, 98]))
        .series(Series::new("Clicked", [30, 26, 21, 15]).tone(Tone::Danger))
}

fn bar_chart_negative() -> Chart {
    Chart::bar("Net change in seats")
        .id("bar-chart-negative")
        .categories(["Jul", "Aug", "Sep", "Oct", "Nov"])
        .series(Series::new("Change", [14, -6, 22, -18, 9]))
}

fn bar_chart_horizontal() -> Chart {
    Chart::bar("Click rate by department")
        .id("bar-chart-horizontal")
        .description("Ranked, highest first")
        .horizontal()
        .format(NumberFormat::percent())
        .categories([
            "Sales",
            "Customer success and account management",
            "Human resources",
            "Finance",
            "Engineering",
            "Legal",
        ])
        .series(Series::new(
            "Click rate",
            [0.171, 0.142, 0.118, 0.093, 0.061, 0.044],
        ))
}

fn bar_chart_horizontal_stacked() -> Chart {
    Chart::bar("Training by team")
        .id("bar-chart-horizontal-stacked")
        .horizontal()
        .stacked()
        .categories(["Sales", "Support", "Finance"])
        .series(Series::new("Done", [42, 51, 63]).tone(Tone::Success))
        .series(Series::new("Started", [20, 11, 4]).tone(Tone::Warning))
        .series(Series::new("Not started", [8, 3, 1]))
}

fn bar_chart_many() -> Grid {
    let first = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap_or_default();
    Grid::cols(2).gap(4).children((
        // Days past `max_bars` are merged in runs, and added up.
        Chart::bar("Sent per day")
            .id("bar-chart-merged")
            .description("30 days in 10 bars")
            .days((0..30).map(|i| first + Duration::days(i)))
            .max_bars(10)
            .series(Series::new("Sent", (0..30).map(|d| 900 + d * 137 % 700))),
        // Categories past it fold their tail into Other.
        Chart::bar("Bounces by domain")
            .id("bar-chart-folded")
            .description("Twelve domains, the last seven folded")
            .horizontal()
            .max_bars(6)
            .categories([
                "gmail.com",
                "outlook.com",
                "yahoo.com",
                "icloud.com",
                "proton.me",
                "aol.com",
                "gmx.net",
                "zoho.com",
                "fastmail.com",
                "hey.com",
                "mail.ru",
                "qq.com",
            ])
            .series(Series::new(
                "Bounces",
                [940, 610, 402, 311, 255, 190, 122, 95, 60, 44, 31, 17],
            )),
    ))
}

fn donut_chart_thumb() -> Donut {
    Donut::new("Delivery")
        .id("donut-chart-thumb")
        .hide_title()
        .size(96)
        .table(TableMode::ScreenReader)
        .part("Delivered", 9_412)
        .part_tone("Bounced", 188, Tone::Danger)
}

fn donut_chart_default() -> Donut {
    Donut::new("Delivery")
        .id("donut-chart-default")
        .description("Messages sent in October")
        .part("Delivered", 9_412)
        .part_tone("Bounced", 188, Tone::Danger)
        .part("Deferred", 40)
        .part("Rejected", 12)
}

fn donut_chart_pie() -> Donut {
    Donut::new("Visits by device")
        .id("donut-chart-pie")
        .description("Seven kinds of device: the two smallest fold into Other")
        .pie()
        .parts([
            ("Desktop", 5_200),
            ("Phone", 3_800),
            ("Tablet", 700),
            ("Watch", 150),
            ("TV", 80),
            ("Car", 40),
            ("Other devices", 30),
        ])
}

fn donut_chart_center() -> Donut {
    Donut::new("Plans")
        .id("donut-chart-center")
        .size(200)
        .center("94%", "on a paid plan")
        .parts([("Team", 5_120), ("Business", 1_980), ("Enterprise", 640)])
        .part("Free", 520)
}

fn donut_chart_states() -> Grid {
    let state =
        |caption: &'static str, chart: Donut| Stack::gap(2).children((Text::muted(caption), chart));
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Donut::new("Delivery").id("donut-chart-loading").loading(),
        ),
        state(
            "Failed",
            Donut::new("Delivery")
                .id("donut-chart-failed")
                .failed(None)
                .retry("/reports/delivery"),
        ),
        state("Empty", Donut::new("Delivery").id("donut-chart-empty")),
    ))
}

fn sparkline_thumb() -> Stat {
    Stat::new("Opened", "4,120")
        .trend("+12% on last week", true)
        .chart(
            Sparkline::new(
                "Opens, last 12 hours",
                [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24],
            )
            .area()
            .id("sparkline-thumb"),
        )
}

fn sparkline_default() -> Grid {
    Grid::cols(3).gap(4).children((
        Stat::new("Opened", "4,120")
            .trend("+12% on last week", true)
            .chart(
                Sparkline::new(
                    "Opens, last 12 hours",
                    [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24],
                )
                .area()
                .id("sparkline-opens"),
            ),
        Stat::new("Bounced", "188")
            .trend("+4 on last week", false)
            .chart(
                Sparkline::new(
                    "Bounces, last 12 hours",
                    [2, 1, 3, 2, 6, 4, 3, 7, 5, 9, 6, 8],
                )
                .tone(Tone::Danger)
                .id("sparkline-bounces"),
            ),
        Stat::new("Unsubscribed", "21").chart(
            Sparkline::new(
                "Unsubscribes, last 12 hours",
                [
                    Some(1.0),
                    Some(0.0),
                    None,
                    Some(2.0),
                    Some(1.0),
                    Some(3.0),
                    Some(2.0),
                    None,
                    Some(1.0),
                    Some(0.0),
                    Some(2.0),
                    Some(1.0),
                ],
            )
            .id("sparkline-unsubscribes"),
        ),
    ))
}

fn sparkline_table() -> Table {
    let row = |name: &'static str, total: &'static str, trend: [u32; 12]| {
        TableRowEl::new((
            TableCell::new(name),
            TableCell::new(total).numeric(),
            TableCell::new(
                Sparkline::new(format!("{name}, last 12 hours"), trend)
                    .id(format!("sparkline-row-{}", name.to_lowercase())),
            ),
        ))
    };
    Table::new()
        .head((
            TableHead::new("Campaign"),
            TableHead::new("Opens").numeric(),
            TableHead::new("Last 12 hours"),
        ))
        .rows((
            row(
                "Autumn launch",
                "4,120",
                [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24],
            ),
            row(
                "Renewal notice",
                "1,806",
                [9, 8, 9, 7, 8, 6, 7, 5, 6, 4, 5, 3],
            ),
            row("Product tips", "932", [1, 1, 2, 2, 3, 2, 3, 4, 4, 5, 4, 6]),
        ))
}

fn sparkline_states() -> Grid {
    let state =
        |caption: &'static str, stat: Stat| Stack::gap(2).children((Text::muted(caption), stat));
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Stat::new("Opened", "4,120").chart(
                Sparkline::new("Opens", [0])
                    .id("sparkline-loading")
                    .loading(),
            ),
        ),
        state(
            "Failed",
            Stat::new("Opened", "4,120").chart(
                Sparkline::new("Opens", [0])
                    .id("sparkline-failed")
                    .failed(None)
                    .retry("/reports/opens"),
            ),
        ),
        state(
            "Empty",
            Stat::new("Opened", "0")
                .chart(Sparkline::new("Opens", Vec::<u32>::new()).id("sparkline-empty")),
        ),
    ))
}

fn funnel_thumb() -> Funnel {
    Funnel::new("Campaign")
        .id("funnel-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .stage(Stage::new("Delivered", 9_412))
        .stage(Stage::new("Opened", 4_120))
        .stage(Stage::new("Clicked", 812))
}

fn funnel_default() -> Funnel {
    Funnel::new("Campaign funnel")
        .id("funnel-default")
        .description("Each step as a share of everyone it reached")
        .stage(Stage::new("Delivered", 9_412))
        .stage(Stage::new("Opened", 4_120))
        .stage(Stage::new("Clicked", 812))
        .stage(Stage::new("Converted", 96).tone(Tone::Success))
}

fn funnel_rates() -> Funnel {
    Funnel::new("Onboarding")
        .id("funnel-rates")
        .description("Rates as the server worded them")
        .stage(Stage::new("Signed up", 1_280))
        .stage(Stage::new("Verified email", 1_104).rate("86% of signed up"))
        .stage(Stage::new("Created a project", 702).rate("64% of verified"))
        .stage(Stage::new("Invited a teammate", 311).rate("44% of project owners"))
        .stage(
            Stage::new("Paid", 96)
                .rate("31% of those invited")
                .tone(Tone::Success),
        )
}

fn funnel_states() -> Grid {
    let state = |caption: &'static str, chart: Funnel| {
        Stack::gap(2).children((Text::muted(caption), chart))
    };
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Funnel::new("Campaign funnel")
                .id("funnel-loading")
                .loading(),
        ),
        state(
            "Failed",
            Funnel::new("Campaign funnel")
                .id("funnel-failed")
                .failed(None)
                .retry("/reports/funnel"),
        ),
        state(
            "Empty",
            Funnel::new("Campaign funnel").id("funnel-empty").empty(
                "No messages sent",
                Some("Send a campaign to see how far it gets.".to_owned()),
            ),
        ),
    ))
}

fn reports_thumb() -> Row {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let limits = RangeRules::new(DateRules::new().max(today)).max_days(400);
    let report = Report::from_query(
        &ReportQuery::default(),
        &limits,
        today,
        DatePreset::Last30Days,
        &Utc,
    );
    Row::gap(3).wrap().children((
        report.picker().id("reports-thumb-window"),
        report.grain_links("/reports", "Grain"),
    ))
}

fn reports_default() -> Stack {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let limits = RangeRules::new(DateRules::new().max(today)).max_days(400);
    // What the request asked for: `/reports?from=2026-10-28&to=2026-11-03`.
    let query = ReportQuery {
        from: Some("2026-10-28".into()),
        to: Some("2026-11-03".into()),
        grain: None,
    };
    let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Utc);
    // Stand-ins for a query's rows: the instant of each event.
    let start = report.bounds().0;
    let opens: Vec<DateTime<Utc>> = (0..7)
        .flat_map(|day| {
            (0..40 + day * 7 % 23 * 3)
                .map(move |k| start + Duration::days(day) + Duration::minutes(k * 12))
        })
        .collect();
    let opened = report.count(opens);
    let total: u64 = opened.iter().sum();
    Stack::gap(4).children((
        Row::gap(3).wrap().children((
            el("form")
                .attr("method", "get")
                .attr("action", "/reports")
                .attr("autocomplete", "off")
                .children((
                    report.picker().id("reports-default-window").title("Window"),
                    // A grain chosen by hand rides along as a hidden field.
                    (report.grain() != report.auto_grain()).then(|| {
                        el("input")
                            .attr("type", "hidden")
                            .attr("name", "grain")
                            .attr("value", report.grain().key())
                    }),
                    Button::outline("Apply").submit(),
                )),
            report.grain_links("/reports", "Grain"),
        )),
        Stat::new("Opens", total.to_string()),
        report
            .chart(Chart::area("Opens over the window").id("reports-default-chart"))
            .description(format!("Per {} in UTC", report.grain().key()))
            .series(report.series("Opens", &opened))
            .table(TableMode::Shown),
    ))
}

fn reports_refused() -> Stack {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    // The limits allow at most 90 days.
    let limits = RangeRules::new(DateRules::new().max(today)).max_days(90);
    let query = ReportQuery {
        from: Some("2026-01-01".into()),
        to: Some("2026-11-03".into()),
        grain: None,
    };
    let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Utc);
    // The report shows its default window, and says why it is not the one asked for.
    let refused = report.refused().map(|error| {
        Alert::new(AlertTone::Warning)
            .title("That window can't be shown")
            .body(format!(
                "{}. Showing the last 30 days.",
                error
                    .message(&navree::ui::dates::DateLocale::english())
                    .trim_end_matches('.')
            ))
    });
    let start = report.bounds().0;
    let opens: Vec<DateTime<Utc>> = (0..30)
        .flat_map(|day| {
            (0..30 + day * 7 % 23 * 2)
                .map(move |k| start + Duration::days(day) + Duration::minutes(k * 18))
        })
        .collect();
    let opened = report.count(opens);
    Stack::gap(4).children((
        report.picker().id("reports-refused-window").title("Window"),
        refused,
        report
            .chart(Chart::bar("Opens").id("reports-refused-chart"))
            .series(report.series("Opens", &opened)),
    ))
}

fn reports_live() -> Card {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let limits = RangeRules::new(DateRules::new().max(today)).max_days(400);
    let query = ReportQuery {
        from: Some("2026-10-28".into()),
        to: Some("2026-11-03".into()),
        grain: None,
    };
    let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Utc);
    let start = report.bounds().0;
    let opens: Vec<DateTime<Utc>> = (0..7)
        .flat_map(|day| {
            (0..40 + day * 7 % 23 * 3)
                .map(move |k| start + Duration::days(day) + Duration::minutes(k * 12))
        })
        .collect();
    let opened = report.count(opens);
    // The region fetches the report's own address again when the stream says something
    // changed. The address keeps the window, so the refresh keeps it too.
    Card::titled("Opens").body(
        Region::new("report-opens")
            .src(report.href("/reports", None))
            .live_from("/events/opens")
            .children(
                report
                    .chart(Chart::area("Opens over the window").id("reports-live-chart"))
                    .series(report.series("Opens", &opened)),
            ),
    )
}

fn reports_grains() -> impl IntoIterator<Item = (&'static str, Node)> {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let limits = RangeRules::new(DateRules::new().max(today)).max_days(4000);
    let window = |from: &str| {
        let query = ReportQuery {
            from: Some(from.into()),
            to: Some("2026-11-03".into()),
            grain: None,
        };
        Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Utc)
    };
    [
        ("1 day: hours", window("2026-11-03")),
        ("7 days: days", window("2026-10-28")),
        ("30 days: days", window("2026-10-05")),
        ("200 days: weeks", window("2026-04-17")),
        ("2 years: months", window("2024-11-04")),
    ]
    .map(|(caption, report)| (caption, report.grain_links("/reports", "Grain")))
}
