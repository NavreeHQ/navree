//! The grid charts in every state: calendar and matrix heat maps and the scatter plot.
//!
//! | Page | Shows |
//! |---|---|
//! | `/charts/grids` | every grid chart and state; `?case=<name>` draws one alone |
//!
//! Today is 12 November 2026 here and the data is made by a fixed sequence, so a screenshot
//! or a test never depends on the clock.

use std::sync::Arc;

use chrono::{Duration, NaiveDate, Weekday};
use http::Method;
use navree::http::extract::Query;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::charts::*;
use navree::ui::*;
use serde::Deserialize;

const NASTY: &str = "<script>window.__pwned=1</script><img src=x onerror=\"window.__pwned=1\"> \"quotes\" 'single' javascript:alert(1)";

fn last_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or(NaiveDate::MIN)
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

    /// Roughly normal, from twelve of the sequence.
    fn bell(&mut self) -> f64 {
        (0..12).map(|_| self.next()).sum::<f64>() - 6.0
    }
}

/// A day of training for `n` days ending today: rest on Wednesdays, some days with no data.
fn load(n: i64, seed: u64) -> Vec<(NaiveDate, Option<f64>)> {
    let mut seq = Seq(seed);
    (0..n)
        .map(|i| {
            let date = last_day() - Duration::days(n - 1 - i);
            let usual = [60.0, 75.0, 0.0, 90.0, 50.0, 130.0, 95.0]
                [chrono::Datelike::weekday(&date).num_days_from_monday() as usize];
            let (a, b) = (seq.next(), seq.next());
            (date, (b > 0.07).then(|| (usual * (0.55 + 0.9 * a)).round()))
        })
        .collect()
}

/// Gaps between heartbeats, each pulled toward 850 ms.
fn beats(n: usize, seed: u64) -> Vec<(f64, f64)> {
    let mut seq = Seq(seed);
    let mut now = 850.0_f64;
    let mut gaps = vec![now];
    for _ in 0..n {
        now = (850.0 + 0.62 * (now - 850.0) + 26.0 * seq.bell()).clamp(520.0, 1_250.0);
        gaps.push(now.round());
    }
    gaps.windows(2).map(|p| (p[0], p[1])).collect()
}

const MEASURES: [&str; 6] = ["Sleep", "Stress", "Steps", "HRV", "Resting HR", "Mood"];

fn correlations(id: &'static str) -> MatrixHeatmap {
    MatrixHeatmap::new("Metric correlations")
        .id(id)
        .metrics(MEASURES)
        .value_name("Correlation")
        .relation("Sleep", "Stress", Relation::new(-0.58).note("30 days"))
        .relation("Sleep", "Steps", Relation::new(0.21).significant(false))
        .relation("Sleep", "HRV", Relation::new(0.71))
        .relation("Sleep", "Resting HR", Relation::new(-0.64))
        .relation("Sleep", "Mood", Relation::new(0.46))
        .relation("Stress", "HRV", Relation::new(-0.52))
        .relation("Stress", "Mood", Relation::new(-0.67))
        .relation("Steps", "HRV", Relation::new(0.38))
        .relation("HRV", "Resting HR", Relation::new(-0.77))
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
    let first = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap_or_default();
    vec![
        (
            "calendar",
            CalendarHeatmap::new("Training consistency")
                .id("calendar")
                .description("Last 6 months, Harleen Kaur")
                .locale("en-IN")
                .days(load(181, 11))
                .ink(Ink::slot(5))
                .unit("AU")
                .into(),
        ),
        (
            "calendar-short",
            CalendarHeatmap::new("Meditation")
                .id("calendar-short")
                .days([
                    (first + Duration::days(8), 0.0),
                    (first + Duration::days(9), 10.0),
                    (first + Duration::days(10), 20.0),
                    (first + Duration::days(15), 30.0),
                ])
                .range(first, last_day())
                .week_start(Weekday::Mon)
                .steps(HeatSteps::at(10, 20, 30))
                .unit("min")
                .no_weekdays()
                .into(),
        ),
        (
            "calendar-year",
            CalendarHeatmap::new("A year of mornings")
                .id("calendar-year")
                .days(load(365, 5))
                .steps(HeatSteps::Quartiles)
                .into(),
        ),
        ("matrix", correlations("matrix").lower_triangle().into()),
        (
            "relations",
            RelationList::new("What moves together")
                .id("relations")
                .relation(
                    "Sleep",
                    "Next-day HRV",
                    Relation::new(0.62).note("30 days, p < 0.001"),
                )
                .relation("Steps", "Stress", Relation::new(-0.48).note("28 days"))
                .relation(
                    "Caffeine",
                    "Deep sleep",
                    Relation::new(-0.22).significant(false),
                )
                .relation(NASTY, "Recovery", Relation::untested().note(NASTY))
                .value_name("Correlation (r)")
                .into(),
        ),
        (
            "matrix-full",
            correlations("matrix-full")
                .colors(Ink::tone(Tone::Success), Ink::tone(Tone::Danger))
                .show_values()
                .into(),
        ),
        (
            "scatter",
            Scatter::new("Successive heartbeat intervals")
                .id("scatter")
                .locale("en-IN")
                .series(
                    PointSeries::new("Resting", beats(250, 9))
                        .ink(Ink::slot(8))
                        .labels((1..=250).map(|n| format!("Beat {n}"))),
                )
                .x_name("This beat")
                .y_name("Next beat")
                .unit("ms")
                .equal_domains()
                .x_axis(Axis::hidden())
                .y_axis(Axis::hidden())
                .grid(Gridlines::none())
                .reference(Reference::identity().label("Equal"))
                .into(),
        ),
        (
            "scatter-axes",
            Scatter::new("Steps and resting heart rate")
                .id("scatter-axes")
                .series(PointSeries::new("Weekdays", {
                    let mut seq = Seq(3);
                    (0..60)
                        .map(|_| {
                            let steps = 5_000.0 + 9_000.0 * seq.next();
                            (steps.round(), (67.0 - steps / 2_500.0 + seq.bell()).round())
                        })
                        .collect::<Vec<_>>()
                }))
                .series(PointSeries::new("Weekends", {
                    let mut seq = Seq(4);
                    (0..24)
                        .map(|_| {
                            let steps = 9_000.0 + 9_000.0 * seq.next();
                            (steps.round(), (65.0 - steps / 2_500.0 + seq.bell()).round())
                        })
                        .collect::<Vec<_>>()
                }))
                .x_name("Steps")
                .y_name("Resting heart rate")
                .y_unit("BPM")
                .x_axis(Axis::new().line())
                .y_axis(Axis::new().line())
                .y_domain(Domain::padded(2))
                .reference(Reference::average().label("Average {value}"))
                .into(),
        ),
        (
            "scatter-big",
            Scatter::new("Five thousand readings")
                .id("scatter-big")
                .series(PointSeries::new("All", beats(5_000, 21)))
                .x_name("This")
                .y_name("Next")
                .into(),
        ),
        (
            "locale",
            Grid::cols(1)
                .gap(3)
                .children((
                    CalendarHeatmap::new("Rupees spent")
                        .id("locale-calendar")
                        .locale("en-IN")
                        .days((0..40).map(|d| {
                            (
                                last_day() - Duration::days(39 - d),
                                120_000.0 * (d % 7) as f64 + 250.0,
                            )
                        }))
                        .format(NumberFormat::currency("₹").max_decimals(0)),
                    Scatter::new("Orders and revenue")
                        .id("locale-scatter")
                        .locale("en-IN")
                        .series(PointSeries::new(
                            "Days",
                            [(12_000, 1_250_000), (18_500, 2_100_000), (9_000, 725_000)],
                        ))
                        .x_name("Orders")
                        .y_name("Revenue")
                        .y_unit("₹"),
                ))
                .into(),
        ),
        (
            "motion",
            Grid::cols(1)
                .gap(3)
                .children((
                    CalendarHeatmap::new("Moving calendar")
                        .id("motion-calendar")
                        .days(load(60, 2))
                        .motion(Motion::new()),
                    correlations("motion-matrix").motion(Motion::new()),
                    Scatter::new("Moving points")
                        .id("motion-scatter")
                        .series(PointSeries::new("a", beats(40, 1)))
                        .motion(Motion::new()),
                ))
                .into(),
        ),
        (
            "hostile",
            Grid::cols(1)
                .gap(3)
                .children((
                    CalendarHeatmap::new(NASTY)
                        .id("hostile-calendar")
                        .days(load(20, 1))
                        .value_name(NASTY)
                        .unit(NASTY),
                    MatrixHeatmap::new(NASTY)
                        .id("hostile-matrix")
                        .metrics([
                            NASTY,
                            "A measure with a very long name that no header could hold",
                            "Short",
                        ])
                        .relation(NASTY, "Short", Relation::new(0.5).note(NASTY))
                        .relation(
                            "Short",
                            "A measure with a very long name that no header could hold",
                            Relation::new(-0.9),
                        ),
                    Scatter::new(NASTY)
                        .id("hostile-scatter")
                        .series(
                            PointSeries::new(NASTY, [(1, 2), (2, 3), (3, 5)])
                                .labels([NASTY, "b", "c"]),
                        )
                        .x_name(NASTY)
                        .y_name(NASTY)
                        .unit(NASTY),
                ))
                .into(),
        ),
        (
            "states",
            Grid::cols(3)
                .gap(3)
                .children((
                    CalendarHeatmap::new("Loading")
                        .id("state-calendar-loading")
                        .loading(),
                    MatrixHeatmap::new("Failed")
                        .id("state-matrix-failed")
                        .failed(None),
                    Scatter::new("Empty").id("state-scatter-empty"),
                ))
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
    shell.page("Grid charts").body(Stack::gap(4).children((
        Text::muted("Calendar and matrix heat maps and the scatter plot, in every state."),
        Grid::cols(1).gap(4).children(sections),
    )))
}

pub struct GridPages;

impl Injectable for GridPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for GridPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/charts/grids",
                |shell: AppShell, Query(case): Query<Case>| async move { gallery(&shell, &case) },
            )
            .public(),
        ]
    }
}
