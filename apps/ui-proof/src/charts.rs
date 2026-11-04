//! Charts in every state, and a report whose window and grain live in the URL.
//!
//! | Page | Shows |
//! |---|---|
//! | `/charts` | every chart type and state; `?case=<name>` draws one alone |
//! | `/charts/report?from=&to=&grain=` | a report: a range picker, a grain switch, charts and a table over one window |
//! | `/charts/big?n=` | one line of `n` points (1,000,000 by default), drawn from at most 640 |
//!
//! Today is 3 November 2026 here and the data is made by a fixed sequence, so a
//! screenshot or a test never depends on the clock.

use std::sync::{Arc, LazyLock};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use http::Method;
use navree::http::extract::Query;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::charts::*;
use navree::ui::dates::{DatePreset, DateRules, RangeRules};
use navree::ui::*;
use serde::Deserialize;

const NASTY: &str = "<script>window.__pwned=1</script><img src=x onerror=\"window.__pwned=1\"> \"quotes\" 'single' javascript:alert(1)";

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or(NaiveDate::MIN)
}

fn zone() -> Tz {
    "Asia/Kolkata".parse().unwrap_or(Tz::UTC)
}

/// A fixed sequence of numbers in `[0, 1)`.
struct Seq(u64);

impl Seq {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn days(n: i64) -> Vec<NaiveDate> {
    (0..n)
        .map(|i| today() - Duration::days(n - 1 - i))
        .collect()
}

fn hours(n: i64) -> Vec<NaiveDateTime> {
    let start = today().and_hms_opt(0, 0, 0).unwrap_or_default();
    (0..n).map(|i| start + Duration::hours(i)).collect()
}

fn wave(seed: u64, n: usize, base: f64, swing: f64) -> Vec<f64> {
    let mut seq = Seq(seed);
    (0..n)
        .map(|i| (base + swing * ((i as f64) / 3.0).sin() + swing * 0.6 * seq.next()).round())
        .collect()
}

#[derive(Default, Deserialize)]
pub struct Case {
    pub case: Option<String>,
}

/// A titled section the tests and screenshots find by name.
fn section(name: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("data-case", name)
        .child(Card::new().body(content))
}

/// Every case, by name.
fn cases() -> Vec<(&'static str, Node)> {
    let week = days(7);
    let month = days(30);
    vec![
        (
            "line",
            Chart::line("Opens and clicks by hour")
                .description("3 November, Asia/Kolkata time")
                .id("line")
                .times(hours(24))
                .series(Series::new("Opens", wave(1, 24, 120.0, 60.0)))
                .series(Series::new("Clicks", wave(2, 24, 30.0, 18.0)))
                .into(),
        ),
        (
            "line-many",
            Chart::line("Sign-ups by region")
                .id("line-many")
                .days(month.clone())
                .toggles()
                .series_list((0..10).map(|s| {
                    Series::new(
                        [
                            "North", "South", "East", "West", "Central", "Coast", "Hills",
                            "Islands", "Delta", "Plains",
                        ][s],
                        wave(10 + s as u64, 30, 40.0 + 9.0 * s as f64, 12.0),
                    )
                }))
                .into(),
        ),
        (
            "line-gaps",
            Chart::line("Response time")
                .id("line-gaps")
                .description(
                    "A gap is a day with nothing measured; one value between gaps is a dot",
                )
                .days(month.clone())
                .format(NumberFormat::number().suffix(" ms"))
                .y_zero(false)
                .series(Series::new(
                    "p95",
                    wave(3, 30, 420.0, 90.0)
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (!(8..12).contains(&i) && i != 14 && i != 16).then_some(v)),
                ))
                .annotation(Annotation::target(450, "Target 450 ms"))
                .into(),
        ),
        (
            "line-one",
            Chart::line("First day of sending")
                .id("line-one")
                .days(vec![today()])
                .series(Series::new("Sent", [1240]))
                .into(),
        ),
        (
            "area",
            Chart::area("Active users")
                .id("area")
                .days(month.clone())
                .smooth()
                .series(Series::new("Active", wave(4, 30, 3_400.0, 900.0)))
                .annotation(Annotation::event(
                    today() - Duration::days(9),
                    "Release 2.4",
                ))
                .into(),
        ),
        (
            "area-stacked",
            Chart::area("Traffic by source")
                .id("area-stacked")
                .days(month.clone())
                .stacked()
                .series(Series::new("Search", wave(5, 30, 900.0, 200.0)))
                .series(Series::new("Direct", wave(6, 30, 600.0, 150.0)))
                .series(Series::new("Referral", wave(7, 30, 300.0, 90.0)))
                .into(),
        ),
        (
            "bar",
            Chart::bar("Sent per day")
                .id("bar")
                .days(week.clone())
                .series(Series::new(
                    "Sent",
                    [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
                ))
                .annotation(Annotation::target(1_500, "Goal"))
                .into(),
        ),
        (
            "bar-grouped",
            Chart::bar("Sent and opened")
                .id("bar-grouped")
                .days(week.clone())
                .series(Series::new(
                    "Sent",
                    [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
                ))
                .series(Series::new(
                    "Opened",
                    [802, 951, 500, 1_200, 1_310, 222, 180],
                ))
                .series(Series::new("Clicked", [120, 180, 90, 260, 300, 40, 30]))
                .toggles()
                .into(),
        ),
        (
            "bar-stacked",
            Chart::bar("Outcomes by week")
                .id("bar-stacked")
                .categories(["Week 41", "Week 42", "Week 43", "Week 44", "Week 45"])
                .stacked()
                .series(Series::new("Reported", [42, 51, 63, 70, 81]).tone(Tone::Success))
                .series(Series::new("Ignored", [120, 110, 104, 98, 90]))
                .series(Series::new("Clicked", [30, 26, 21, 15, 12]).tone(Tone::Danger))
                .into(),
        ),
        (
            "bar-percent",
            Chart::bar("Share of outcomes")
                .id("bar-percent")
                .categories(["Sales", "Support", "Finance", "Engineering"])
                .stacked_percent()
                .series(Series::new("Reported", [42, 51, 63, 70]).tone(Tone::Success))
                .series(Series::new("Ignored", [120, 110, 104, 98]))
                .series(Series::new("Clicked", [30, 26, 21, 15]).tone(Tone::Danger))
                .into(),
        ),
        (
            "bar-negative",
            Chart::bar("Net change in seats")
                .id("bar-negative")
                .categories(["Jul", "Aug", "Sep", "Oct", "Nov"])
                .series(Series::new("Change", [14, -6, 22, -18, 9]))
                .into(),
        ),
        (
            "bar-horizontal",
            Chart::bar("Click rate by department")
                .id("bar-horizontal")
                .horizontal()
                .format(NumberFormat::percent())
                .categories([
                    "Customer success and account management",
                    "Engineering",
                    "Finance",
                    "Human resources",
                    "Legal",
                    "Sales",
                ])
                .series(Series::new(
                    "Click rate",
                    [0.142, 0.061, 0.093, 0.118, 0.044, 0.171],
                ))
                .into(),
        ),
        (
            "bar-horizontal-stacked",
            Chart::bar("Training by team")
                .id("bar-horizontal-stacked")
                .horizontal()
                .stacked()
                .categories(["Sales", "Support", "Finance"])
                .series(Series::new("Done", [42, 51, 63]).tone(Tone::Success))
                .series(Series::new("Started", [20, 11, 4]).tone(Tone::Warning))
                .series(Series::new("Not started", [8, 3, 1]))
                .into(),
        ),
        (
            "donut",
            Donut::new("Delivery")
                .id("donut")
                .part("Delivered", 9_412)
                .part_tone("Bounced", 188, Tone::Danger)
                .part("Deferred", 40)
                .part("Rejected", 12)
                .into(),
        ),
        (
            "pie",
            Donut::new("Devices")
                .id("pie")
                .pie()
                .parts([
                    ("Desktop", 52.0),
                    ("Phone", 38.0),
                    ("Tablet", 7.0),
                    ("Watch", 1.5),
                    ("TV", 0.8),
                    ("Car", 0.4),
                    ("Other devices", 0.3),
                ])
                .format(NumberFormat::percent_of_100())
                .into(),
        ),
        (
            "sparkline",
            Grid::cols(3)
                .gap(3)
                .children((
                    Stat::new("Opened", "4,120")
                        .trend("+12% on last week", true)
                        .chart(
                            Sparkline::new("Opens, last 24 hours", wave(8, 24, 50.0, 20.0))
                                .area()
                                .id("spark-1"),
                        ),
                    Stat::new("Bounced", "188")
                        .trend("+4 on last week", false)
                        .chart(
                            Sparkline::new("Bounces, last 24 hours", wave(9, 24, 8.0, 4.0))
                                .tone(Tone::Danger)
                                .id("spark-2"),
                        ),
                    Stat::new("Unsubscribed", "21").chart(
                        Sparkline::new("Unsubscribes, last 24 hours", wave(11, 24, 2.0, 1.5))
                            .id("spark-3"),
                    ),
                ))
                .into(),
        ),
        (
            "funnel",
            Funnel::new("Campaign funnel")
                .id("funnel")
                .description("Each step as a share of everyone it reached")
                .stage(Stage::new("Delivered", 9_412))
                .stage(Stage::new("Opened", 4_120))
                .stage(Stage::new("Clicked", 812))
                .stage(Stage::new("Converted", 96).tone(Tone::Success))
                .into(),
        ),
        (
            "annotations",
            Chart::line("Reports per day")
                .id("annotations")
                .days(month.clone())
                .series(Series::new("Reports", wave(12, 30, 60.0, 25.0)))
                .annotation(Annotation::band(
                    today() - Duration::days(20),
                    today() - Duration::days(14),
                    "Holiday",
                ))
                .annotation(Annotation::event(
                    today() - Duration::days(6),
                    "Reminder sent",
                ))
                .annotation(Annotation::target(80, "Target"))
                .into(),
        ),
        (
            "empty",
            Chart::bar("Bounces by domain")
                .id("empty")
                .categories(Vec::<String>::new())
                .series(Series::new("Bounces", Vec::<f64>::new()))
                .into(),
        ),
        (
            "loading",
            Chart::line("Opens by hour").id("loading").loading().into(),
        ),
        (
            "failed",
            Chart::line("Opens by hour")
                .id("failed")
                .failed(Some(
                    "The analytics service did not answer in time.".to_owned(),
                ))
                .retry("/charts?case=failed")
                .into(),
        ),
        (
            "hostile",
            Chart::bar(NASTY)
                .id("hostile")
                .description(NASTY)
                .categories([NASTY, "Plain"])
                .series(Series::new(NASTY, [3, 5]))
                .series(Series::new("Second", [f64::NAN, 2.0]))
                .annotation(Annotation::target(4, NASTY))
                .into(),
        ),
        (
            "extremes",
            Chart::line("Extreme values")
                .id("extremes")
                .numbers([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
                .series(Series::new(
                    "Huge",
                    [1e300, -1e300, f64::INFINITY, f64::NAN, 0.0, 1e-300],
                ))
                .series(Series::new("Tiny", [1e-9, 2e-9, 3e-9, 2e-9, 1e-9, 0.0]))
                .into(),
        ),
        (
            "rtl",
            Direction::rtl(
                Chart::bar("الرسائل المرسلة")
                    .id("rtl")
                    .rtl()
                    .locale("ar")
                    .days(week.clone())
                    .series(Series::new(
                        "أُرسلت",
                        [1_204, 1_480, 990, 1_610, 1_820, 410, 302],
                    ))
                    .series(Series::new("فُتحت", [802, 951, 500, 1_200, 1_310, 222, 180])),
            )
            .into(),
        ),
        (
            "locale",
            Grid::cols(2)
                .gap(3)
                .children((
                    Chart::bar("Umsatz pro Monat")
                        .id("locale-de")
                        .locale("de")
                        .categories(["Juli", "August", "September", "Oktober"])
                        .format(NumberFormat::currency("€ ").max_decimals(0))
                        .series(Series::new(
                            "Umsatz",
                            [1_250_000.0, 980_500.0, 1_402_300.0, 1_610_000.0],
                        )),
                    Chart::bar("Revenue by month")
                        .id("locale-in")
                        .locale("en-IN")
                        .categories(["Jul", "Aug", "Sep", "Oct"])
                        .format(NumberFormat::currency("₹").max_decimals(0))
                        .series(Series::new(
                            "Revenue",
                            [1_250_000.0, 980_500.0, 1_402_300.0, 1_610_000.0],
                        )),
                ))
                .into(),
        ),
        (
            "shown",
            Chart::bar("Seats by plan")
                .id("shown")
                .table(TableMode::Shown)
                .categories(["Free", "Team", "Business", "Enterprise"])
                .series(Series::new("Seats", [12_400, 3_120, 840, 96]))
                .into(),
        ),
    ]
}

fn gallery(shell: &AppShell, case: &Case) -> AppPage {
    let wanted = case.case.as_deref();
    let sections: Vec<Element> = cases()
        .into_iter()
        .filter(|(name, _)| wanted.is_none_or(|w| w == *name))
        .map(|(name, node)| section(name, node))
        .collect();
    shell.page("Charts").body(
        Stack::gap(4).children((
            Text::muted("Every chart type and state. Each chart's data is also its table: switch it on under the chart."),
            Grid::cols(2).gap(4).children(sections),
        )),
    )
}

/// The events behind the report: a year of opens, made by a fixed sequence.
static EVENTS: LazyLock<Vec<(DateTime<Utc>, &'static str)>> = LazyLock::new(|| {
    let mut seq = Seq(42);
    let start = Utc
        .with_ymd_and_hms(2025, 11, 1, 0, 0, 0)
        .single()
        .unwrap_or_default();
    let mut out = Vec::with_capacity(60_000);
    let mut at = start;
    let end = Utc
        .with_ymd_and_hms(2026, 11, 4, 0, 0, 0)
        .single()
        .unwrap_or_default();
    while at < end {
        // Busier in the working day, quieter at weekends.
        let hour = (at.timestamp() / 3600 % 24) as f64;
        let busy = 0.35 + 0.65 * ((hour - 4.0) / 24.0 * std::f64::consts::TAU).sin().max(0.0);
        let step = (900.0 / busy * (0.3 + seq.next())) as i64;
        at += Duration::seconds(step.max(30));
        let kind = match seq.next() {
            x if x < 0.62 => "opened",
            x if x < 0.9 => "clicked",
            _ => "reported",
        };
        out.push((at, kind));
    }
    out
});

fn limits() -> RangeRules {
    RangeRules::new(
        DateRules::new()
            .min(NaiveDate::from_ymd_opt(2025, 11, 1).unwrap_or(NaiveDate::MIN))
            .max(today()),
    )
    .max_days(400)
}

fn report_page(shell: &AppShell, query: &ReportQuery) -> AppPage {
    let tz = zone();
    let report = Report::from_query(query, &limits(), today(), DatePreset::Last30Days, &tz)
        .zone_name("Asia/Kolkata");
    let count =
        |kind: &str| report.count(EVENTS.iter().filter(|(_, k)| *k == kind).map(|(at, _)| *at));
    let opened = count("opened");
    let clicked = count("clicked");
    let reported = count("reported");
    let total: u64 = opened.iter().chain(&clicked).chain(&reported).sum();
    let form = el("form")
        .attr("method", "get")
        .attr("action", "/charts/report")
        .slot("report-filters")
        .children((
            report.picker().id("report-window").title("Window"),
            (report.grain() != report.auto_grain()).then(|| {
                el("input")
                    .attr("type", "hidden")
                    .attr("name", "grain")
                    .attr("value", report.grain().key())
            }),
            Button::outline("Apply").attr("type", "submit"),
        ));
    let refused = report.refused().map(|error| {
        Alert::new(AlertTone::Warning)
            .title("That window can't be shown")
            .body(format!(
                "{}. Showing the last 30 days.",
                error.message(&navree::ui::dates::DateLocale::english())
            ))
    });
    shell.page("Engagement report").body(
        Stack::gap(4).children((
            Row::gap(3)
                .wrap()
                .align(Align::Center)
                .children((form, report.grain_links("/charts/report", "Grain"))),
            refused,
            Grid::cols(3).gap(3).children((
                Stat::new("Events", total.to_string()),
                Stat::new("Opened", opened.iter().sum::<u64>().to_string()),
                Stat::new("Reported", reported.iter().sum::<u64>().to_string()),
            )),
            Card::new().body(
                report
                    .chart(Chart::area("Engagement over the window").id("report-area"))
                    .description(format!("Per {} in Asia/Kolkata time", report.grain().key()))
                    .series(report.series("Opened", &opened))
                    .series(report.series("Clicked", &clicked))
                    .toggles(),
            ),
            Card::new().body(
                report
                    .chart(Chart::bar("Reported per period").id("report-bars"))
                    .series(report.series("Reported", &reported).tone(Tone::Success))
                    .table(TableMode::Shown),
            ),
        )),
    )
}

#[derive(Default, Deserialize)]
pub struct Big {
    pub n: Option<usize>,
}

fn big(shell: &AppShell, query: &Big) -> AppPage {
    let n = query.n.unwrap_or(1_000_000).clamp(1, 5_000_000);
    let mut seq = Seq(7);
    let mut level = 0.0;
    let values: Vec<f64> = (0..n)
        .map(|_| {
            level += seq.next() - 0.5;
            level
        })
        .collect();
    let start = std::time::Instant::now();
    let chart = Chart::line(format!("{n} points"))
        .id("big")
        .numbers((0..n).map(|i| i as f64))
        .y_zero(false)
        .series(Series::new("Random walk", values));
    let node: Node = chart.into();
    let took = start.elapsed();
    shell.page("A large series").body(
        Stack::gap(3).children((
            Text::muted(format!("Built in {} ms.", took.as_millis()))
                .attr("data-built-ms", took.as_millis().to_string()),
            Card::new().body(node),
        )),
    )
}

pub struct ChartPages;

impl Injectable for ChartPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for ChartPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/charts",
                |shell: AppShell, Query(case): Query<Case>| async move { gallery(&shell, &case) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/charts/report",
                |shell: AppShell, Query(query): Query<ReportQuery>| async move {
                    report_page(&shell, &query)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/charts/big",
                |shell: AppShell, Query(query): Query<Big>| async move { big(&shell, &query) },
            )
            .public(),
        ]
    }
}
