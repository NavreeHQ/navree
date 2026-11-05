//! Line, area and bar charts, donut and pie, sparkline, funnel, and reports.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Asia::Kolkata;
use navree::ui::charts::*;
use navree::ui::dates::{DateLocale, DatePreset, DateRules, RangeRules};
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
            "Numbers in Hindi and Indian English",
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
    Chart::line("Orders")
        .id("line-chart-thumb")
        .hide_title()
        .height(96)
        .table(TableMode::ScreenReader)
        .categories(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
        .series(Series::new("Orders", [412, 530, 498, 610, 584, 233, 198]))
}

fn line_chart_default() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::line("Orders and returns")
        .id("line-chart-default")
        .description("Per day, 28 October to 3 November, IST")
        .locale("en-IN")
        .days((0..7).map(|i| first + Duration::days(i)))
        .series(Series::new("Orders", [412, 530, 498, 610, 584, 233, 198]))
        .series(Series::new("Returns", [96, 141, 120, 168, 152, 51, 44]))
}

fn line_chart_hours() -> Chart {
    let midnight = NaiveDate::from_ymd_opt(2026, 11, 3)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .unwrap_or_default();
    Chart::line("Orders by hour")
        .id("line-chart-hours")
        .description("3 November, on the clock of Asia/Kolkata")
        .locale("en-IN")
        .times((0..24).map(|hour| midnight + Duration::hours(hour)))
        .series(Series::new(
            "Orders",
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
        .locale("en-IN")
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
    Chart::line("Orders per day")
        .id("line-chart-annotations")
        .locale("en-IN")
        .days((0..14).map(day))
        .series(Series::new(
            "Orders",
            [58, 61, 64, 55, 31, 28, 49, 66, 72, 85, 91, 77, 70, 63],
        ))
        .annotation(Annotation::band(day(4), day(6), "Weekend sale"))
        .annotation(Annotation::event(day(9), "Offer sent"))
        .annotation(Annotation::target(80, "Target"))
}

fn line_chart_toggles() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::line("UPI payments")
        .id("line-chart-toggles")
        .description("Click a name in the legend to hide its line")
        .locale("en-IN")
        .days((0..7).map(|i| first + Duration::days(i)))
        .toggles()
        .series(Series::new("Succeeded", [118, 131, 126, 140, 137, 122, 129]).tone(Tone::Success))
        .series(Series::new("Pending", [24, 31, 27, 35, 29, 38, 33]).tone(Tone::Warning))
        .series(Series::new("Failed", [6, 9, 4, 12, 8, 15, 10]).tone(Tone::Danger))
}

fn line_chart_many() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    let states = [
        "Punjab",
        "Haryana",
        "Delhi",
        "Uttar Pradesh",
        "Rajasthan",
        "Himachal Pradesh",
        "Maharashtra",
        "Gujarat",
        "Karnataka",
        "Tamil Nadu",
    ];
    Chart::line("Sign-ups by state")
        .id("line-chart-many")
        .description("Ten states: seven keep a color, the other three fold into Other")
        .locale("en-IN")
        .days((0..7).map(|i| first + Duration::days(i)))
        .toggles()
        .series_list(states.iter().enumerate().map(|(at, name)| {
            let at = at as i64;
            // The last three are small, so that the Other they fold into stays in scale.
            let base = if at < 7 {
                15 + at * 11
            } else {
                6 + (at - 7) * 3
            };
            Series::new(
                *name,
                (0..7).map(|d| base + (d * (at % 3 + 2) * 5 + at * 7) % 19),
            )
        }))
}

fn line_chart_states() -> Grid {
    let state =
        |caption: &'static str, chart: Chart| Stack::gap(2).children((Text::muted(caption), chart));
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Chart::line("Orders by hour")
                .id("line-chart-loading")
                .loading(),
        ),
        state(
            "Failed",
            Chart::line("Orders by hour")
                .id("line-chart-failed")
                .failed(Some("The analytics service did not answer.".to_owned()))
                .retry("/reports/orders"),
        ),
        state(
            "Empty",
            Chart::line("Orders by hour").id("line-chart-empty").empty(
                "No orders yet",
                Some("They appear here when a customer places one.".to_owned()),
            ),
        ),
    ))
}

fn line_chart_locales() -> Grid {
    let revenue = [12_50_000.0, 9_80_500.0, 14_02_300.0, 16_10_000.0];
    // The words a chart writes are fields: change the ones you need.
    let hindi = ChartLabels {
        show_table: "तालिका दिखाएँ".into(),
        chart: "{title}, चार्ट".into(),
        ..ChartLabels::english()
    };
    Grid::cols(2).gap(4).children((
        Chart::line("महीने का राजस्व")
            .id("line-chart-hi")
            .locale("hi")
            .labels(hindi)
            .categories(["जुलाई", "अगस्त", "सितंबर", "अक्टूबर"])
            .format(NumberFormat::currency("₹").max_decimals(0))
            .series(Series::new("राजस्व", revenue)),
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
    Chart::line("Orders by payment")
        .id("line-chart-table")
        .locale("en-IN")
        .days((0..5).map(|i| first + Duration::days(i)))
        .table(TableMode::Shown)
        .series(Series::new("Prepaid", [12, 18, 9, 22, 30]))
        .series(Series::new("Cash on delivery", [3, 4, 2, 6, 8]))
}

fn area_chart_thumb() -> Chart {
    Chart::area("Active shoppers")
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
    Chart::area("Active shoppers")
        .id("area-chart-default")
        .description("Per day, 21 October to 3 November")
        .locale("en-IN")
        .days((0..14).map(|i| first + Duration::days(i)))
        .smooth()
        .series(Series::new(
            "Active",
            [
                3_410, 3_620, 3_540, 3_980, 4_210, 3_300, 3_150, 3_700, 3_880, 4_050, 4_420, 4_610,
                3_820, 3_560,
            ],
        ))
        .annotation(Annotation::event(first + Duration::days(7), "App update"))
}

fn area_chart_stacked() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap_or_default();
    Chart::area("Traffic by source")
        .id("area-chart-stacked")
        .description("Visits per day; the top edge is the total")
        .locale("en-IN")
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
        .locale("en-IN")
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
    Chart::bar("Revenue")
        .id("bar-chart-thumb")
        .hide_title()
        .height(96)
        .table(TableMode::ScreenReader)
        .locale("en-IN")
        .format(NumberFormat::currency("₹").max_decimals(0))
        .categories(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
        .series(Series::new(
            "Revenue",
            [
                12_04_000, 14_80_000, 9_90_000, 16_10_000, 18_20_000, 4_10_000, 3_02_000,
            ],
        ))
}

fn bar_chart_default() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::bar("Revenue per day")
        .id("bar-chart-default")
        .locale("en-IN")
        .format(NumberFormat::currency("₹").max_decimals(0))
        .days((0..7).map(|i| first + Duration::days(i)))
        .series(Series::new(
            "Revenue",
            [
                12_04_000, 14_80_000, 9_90_000, 16_10_000, 18_20_000, 4_10_000, 3_02_000,
            ],
        ))
        .annotation(Annotation::target(15_00_000, "Goal"))
}

fn bar_chart_grouped() -> Chart {
    let first = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap_or_default();
    Chart::bar("Orders placed, paid and delivered")
        .id("bar-chart-grouped")
        .locale("en-IN")
        .days((0..7).map(|i| first + Duration::days(i)))
        .toggles()
        .series(Series::new(
            "Placed",
            [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
        ))
        .series(Series::new("Paid", [802, 951, 500, 1_200, 1_310, 222, 180]))
        .series(Series::new("Delivered", [120, 180, 90, 260, 300, 40, 30]))
}

fn bar_chart_stacked() -> Chart {
    Chart::bar("Orders by week")
        .id("bar-chart-stacked")
        .categories(["Week 41", "Week 42", "Week 43", "Week 44", "Week 45"])
        .stacked()
        .series(Series::new("Delivered", [42, 51, 63, 70, 81]).tone(Tone::Success))
        .series(Series::new("In transit", [120, 110, 104, 98, 90]))
        .series(Series::new("Returned", [30, 26, 21, 15, 12]).tone(Tone::Danger))
}

fn bar_chart_percent() -> Chart {
    Chart::bar("Share of orders by outcome")
        .id("bar-chart-percent")
        .categories(["Mumbai", "Delhi", "Ludhiana", "Bengaluru"])
        .stacked_percent()
        .series(Series::new("Delivered", [42, 51, 63, 70]).tone(Tone::Success))
        .series(Series::new("In transit", [120, 110, 104, 98]))
        .series(Series::new("Returned", [30, 26, 21, 15]).tone(Tone::Danger))
}

fn bar_chart_negative() -> Chart {
    Chart::bar("Net change in customers")
        .id("bar-chart-negative")
        .categories(["Jul", "Aug", "Sep", "Oct", "Nov"])
        .series(Series::new("Change", [14, -6, 22, -18, 9]))
}

fn bar_chart_horizontal() -> Chart {
    Chart::bar("Repeat purchase rate by city")
        .id("bar-chart-horizontal")
        .description("Ranked, highest first")
        .horizontal()
        .format(NumberFormat::percent())
        .categories([
            "Ludhiana",
            "Chandigarh, Mohali and Panchkula",
            "Amritsar",
            "Jaipur",
            "Lucknow",
            "Indore",
        ])
        .series(Series::new(
            "Repeat rate",
            [0.171, 0.142, 0.118, 0.093, 0.061, 0.044],
        ))
}

fn bar_chart_horizontal_stacked() -> Chart {
    Chart::bar("Deliveries by city")
        .id("bar-chart-horizontal-stacked")
        .horizontal()
        .stacked()
        .categories(["Mumbai", "Delhi", "Ludhiana"])
        .series(Series::new("Delivered", [42, 51, 63]).tone(Tone::Success))
        .series(Series::new("Out for delivery", [20, 11, 4]).tone(Tone::Warning))
        .series(Series::new("Not shipped", [8, 3, 1]))
}

fn bar_chart_many() -> Grid {
    let first = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap_or_default();
    Grid::cols(2).gap(4).children((
        // Days past `max_bars` are merged in runs, and added up.
        Chart::bar("Orders per day")
            .id("bar-chart-merged")
            .description("30 days in 10 bars")
            .locale("en-IN")
            .days((0..30).map(|i| first + Duration::days(i)))
            .max_bars(10)
            .series(Series::new("Orders", (0..30).map(|d| 900 + d * 137 % 700))),
        // Categories past it fold their tail into Other.
        Chart::bar("Orders by city")
            .id("bar-chart-folded")
            .description("Twelve cities, the last seven folded")
            .horizontal()
            .max_bars(6)
            .categories([
                "Mumbai",
                "Delhi",
                "Bengaluru",
                "Hyderabad",
                "Chennai",
                "Pune",
                "Kolkata",
                "Ahmedabad",
                "Jaipur",
                "Lucknow",
                "Ludhiana",
                "Amritsar",
            ])
            .series(Series::new(
                "Orders",
                [940, 610, 402, 311, 255, 190, 122, 95, 60, 44, 31, 17],
            )),
    ))
}

fn donut_chart_thumb() -> Donut {
    Donut::new("Payments")
        .id("donut-chart-thumb")
        .hide_title()
        .size(96)
        .table(TableMode::ScreenReader)
        .part("Succeeded", 9_412)
        .part_tone("Failed", 188, Tone::Danger)
}

fn donut_chart_default() -> Donut {
    Donut::new("Payments")
        .id("donut-chart-default")
        .description("Payments taken in October")
        .part("Succeeded", 9_412)
        .part_tone("Failed", 188, Tone::Danger)
        .part("Pending", 40)
        .part("Refunded", 12)
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
            Donut::new("Payments").id("donut-chart-loading").loading(),
        ),
        state(
            "Failed",
            Donut::new("Payments")
                .id("donut-chart-failed")
                .failed(None)
                .retry("/reports/payments"),
        ),
        state("Empty", Donut::new("Payments").id("donut-chart-empty")),
    ))
}

fn sparkline_thumb() -> Stat {
    Stat::new("Orders", "4,120")
        .trend("+12% on last week", true)
        .chart(
            Sparkline::new(
                "Orders, last 12 hours",
                [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24],
            )
            .area()
            .id("sparkline-thumb"),
        )
}

fn sparkline_default() -> Grid {
    Grid::cols(3).gap(4).children((
        Stat::new("Orders", "4,120")
            .trend("+12% on last week", true)
            .chart(
                Sparkline::new(
                    "Orders, last 12 hours",
                    [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24],
                )
                .area()
                .id("sparkline-orders"),
            ),
        Stat::new("Returns", "188")
            .trend("+4 on last week", false)
            .chart(
                Sparkline::new(
                    "Returns, last 12 hours",
                    [2, 1, 3, 2, 6, 4, 3, 7, 5, 9, 6, 8],
                )
                .tone(Tone::Danger)
                .id("sparkline-returns"),
            ),
        Stat::new("Cancelled", "21").chart(
            Sparkline::new(
                "Cancellations, last 12 hours",
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
            .id("sparkline-cancellations"),
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
            TableHead::new("City"),
            TableHead::new("Orders").numeric(),
            TableHead::new("Last 12 hours"),
        ))
        .rows((
            row(
                "Ludhiana",
                "4,120",
                [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24],
            ),
            row("Amritsar", "1,806", [9, 8, 9, 7, 8, 6, 7, 5, 6, 4, 5, 3]),
            row("Jaipur", "932", [1, 1, 2, 2, 3, 2, 3, 4, 4, 5, 4, 6]),
        ))
}

fn sparkline_states() -> Grid {
    let state =
        |caption: &'static str, stat: Stat| Stack::gap(2).children((Text::muted(caption), stat));
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Stat::new("Orders", "4,120").chart(
                Sparkline::new("Orders", [0])
                    .id("sparkline-loading")
                    .loading(),
            ),
        ),
        state(
            "Failed",
            Stat::new("Orders", "4,120").chart(
                Sparkline::new("Orders", [0])
                    .id("sparkline-failed")
                    .failed(None)
                    .retry("/reports/orders"),
            ),
        ),
        state(
            "Empty",
            Stat::new("Orders", "0")
                .chart(Sparkline::new("Orders", Vec::<u32>::new()).id("sparkline-empty")),
        ),
    ))
}

fn funnel_thumb() -> Funnel {
    Funnel::new("Checkout")
        .id("funnel-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .stage(Stage::new("Viewed", 9_412))
        .stage(Stage::new("In cart", 4_120))
        .stage(Stage::new("Paid", 1_460))
}

fn funnel_default() -> Funnel {
    Funnel::new("Checkout funnel")
        .id("funnel-default")
        .description("Each step as a share of everyone it reached")
        .stage(Stage::new("Viewed a product", 9_412))
        .stage(Stage::new("Added to cart", 4_120))
        .stage(Stage::new("Started checkout", 2_204))
        .stage(Stage::new("Paid", 1_460).tone(Tone::Success))
}

fn funnel_rates() -> Funnel {
    Funnel::new("Seller onboarding")
        .id("funnel-rates")
        .description("Rates as the server worded them")
        .stage(Stage::new("Signed up", 1_280))
        .stage(Stage::new("Verified a phone number", 1_104).rate("86% of signed up"))
        .stage(Stage::new("Listed a product", 702).rate("64% of verified"))
        .stage(Stage::new("Received an order", 311).rate("44% of sellers with a product"))
        .stage(
            Stage::new("Received a payout", 96)
                .rate("31% of sellers with an order")
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
            Funnel::new("Checkout funnel")
                .id("funnel-loading")
                .loading(),
        ),
        state(
            "Failed",
            Funnel::new("Checkout funnel")
                .id("funnel-failed")
                .failed(None)
                .retry("/reports/funnel"),
        ),
        state(
            "Empty",
            Funnel::new("Checkout funnel").id("funnel-empty").empty(
                "No visits yet",
                Some("They appear here once customers open your store.".to_owned()),
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
        &Kolkata,
    );
    Row::gap(3).wrap().children((
        report
            .picker()
            .locale(DateLocale::of("en-IN"))
            .id("reports-thumb-window"),
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
    let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Kolkata);
    // Stand-ins for a query's rows: the instant of each event.
    let start = report.bounds().0;
    let orders: Vec<DateTime<Utc>> = (0..7)
        .flat_map(|day| {
            (0..40 + day * 7 % 23 * 3)
                .map(move |k| start + Duration::days(day) + Duration::minutes(k * 12))
        })
        .collect();
    let placed = report.count(orders);
    let total: u64 = placed.iter().sum();
    Stack::gap(4).children((
        Row::gap(3).wrap().children((
            el("form")
                .attr("method", "get")
                .attr("action", "/reports")
                .attr("autocomplete", "off")
                .children((
                    report
                        .picker()
                        .locale(DateLocale::of("en-IN"))
                        .id("reports-default-window")
                        .title("Window"),
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
        Stat::new("Orders", total.to_string()),
        report
            .chart(Chart::area("Orders over the window").id("reports-default-chart"))
            .locale("en-IN")
            .description(format!("Per {} in IST", report.grain().key()))
            .series(report.series("Orders", &placed))
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
    let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Kolkata);
    // The report shows its default window, and says why it is not the one asked for.
    let refused = report.refused().map(|error| {
        Alert::new(AlertTone::Warning)
            .title("That window can't be shown")
            .body(format!(
                "{}. Showing the last 30 days.",
                error
                    .message(&DateLocale::of("en-IN"))
                    .trim_end_matches('.')
            ))
    });
    let start = report.bounds().0;
    let orders: Vec<DateTime<Utc>> = (0..30)
        .flat_map(|day| {
            (0..30 + day * 7 % 23 * 2)
                .map(move |k| start + Duration::days(day) + Duration::minutes(k * 18))
        })
        .collect();
    let placed = report.count(orders);
    Stack::gap(4).children((
        report
            .picker()
            .locale(DateLocale::of("en-IN"))
            .id("reports-refused-window")
            .title("Window"),
        refused,
        report
            .chart(Chart::bar("Orders").id("reports-refused-chart"))
            .locale("en-IN")
            .series(report.series("Orders", &placed)),
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
    let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Kolkata);
    let start = report.bounds().0;
    let orders: Vec<DateTime<Utc>> = (0..7)
        .flat_map(|day| {
            (0..40 + day * 7 % 23 * 3)
                .map(move |k| start + Duration::days(day) + Duration::minutes(k * 12))
        })
        .collect();
    let placed = report.count(orders);
    // The region fetches the report's own address again when the stream says something
    // changed. The address keeps the window, so the refresh keeps it too.
    Card::titled("Orders").body(
        Region::new("report-orders")
            .src(report.href("/reports", None))
            .live_from("/events/orders")
            .children(
                report
                    .chart(Chart::area("Orders over the window").id("reports-live-chart"))
                    .locale("en-IN")
                    .series(report.series("Orders", &placed)),
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
        Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Kolkata)
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
