//! Heat maps of rows by columns, calendar and matrix heat maps, and the scatter plot.
//!
//! The data is made up and the people are made up: steps by hour, resting heart rates against
//! each person's usual, a week of workouts, a training calendar, a matrix of measures from a
//! wearable, and the gaps between heartbeats. The numbers that are rupees or counts are written
//! in lakh grouping by the charts themselves.

use navree::datetime::{Date, Weekday};
use navree::ui::charts::*;
use navree::ui::*;

use crate::catalog::{Example, single};

const SOURCE: &str = include_str!("grids.rs");

pub fn all() -> Vec<Example> {
    vec![
        // Heat map
        single!("heatmap", "thumb", "A heat map", heatmap_thumb)
            .start()
            .width(420)
            .height(200),
        single!(
            "heatmap",
            "default",
            "A week of steps by hour, with each day's total",
            heatmap_default
        )
        .start()
        .height(400),
        single!(
            "heatmap",
            "bands",
            "A diverging scale in five bands, with the values",
            heatmap_bands
        )
        .start()
        .height(300),
        single!(
            "heatmap",
            "categories",
            "A category in each cell",
            heatmap_categories
        )
        .start()
        .height(240),
        single!(
            "heatmap",
            "states",
            "Loading, failed and empty",
            heatmap_states
        )
        .start()
        .width(820)
        .height(300),
        // Calendar heat map
        single!(
            "calendar-heatmap",
            "thumb",
            "A calendar heat map",
            calendar_heatmap_thumb
        )
        .start()
        .width(420)
        .height(190),
        single!(
            "calendar-heatmap",
            "default",
            "181 days of training load",
            calendar_heatmap_default
        )
        .start()
        .height(330),
        single!(
            "calendar-heatmap",
            "steps",
            "Steps you choose, in a tone",
            calendar_heatmap_steps
        )
        .start()
        .height(330),
        single!(
            "calendar-heatmap",
            "short",
            "A few weeks, with days not recorded",
            calendar_heatmap_short
        )
        .start()
        .height(260),
        single!(
            "calendar-heatmap",
            "states",
            "Loading, failed and empty",
            calendar_heatmap_states
        )
        .start()
        .width(820)
        .height(300),
        // Matrix heat map
        single!(
            "matrix-heatmap",
            "thumb",
            "A matrix heat map",
            matrix_heatmap_thumb
        )
        .start()
        .width(420)
        .height(220),
        single!(
            "matrix-heatmap",
            "default",
            "What moves with what, below the diagonal",
            matrix_heatmap_default
        )
        .start()
        .height(420),
        single!(
            "matrix-heatmap",
            "full",
            "Every pair twice, with the values and tones",
            matrix_heatmap_full
        )
        .start()
        .height(480),
        single!(
            "matrix-heatmap",
            "states",
            "Loading, failed and empty",
            matrix_heatmap_states
        )
        .start()
        .width(820)
        .height(300),
        // Relation list
        single!(
            "relation-list",
            "thumb",
            "A ranked list of relations",
            relation_list_thumb
        )
        .start()
        .width(420)
        .height(240),
        single!(
            "relation-list",
            "default",
            "What moves together, strongest first",
            relation_list_default
        )
        .start()
        .height(470),
        single!(
            "relation-list",
            "significant",
            "Only the links found, beside the matrix",
            relation_list_significant
        )
        .start()
        .width(820)
        .height(470),
        // Scatter chart
        single!(
            "scatter-chart",
            "thumb",
            "A scatter plot",
            scatter_chart_thumb
        )
        .start()
        .width(420)
        .height(190),
        single!(
            "scatter-chart",
            "default",
            "Successive heartbeat intervals",
            scatter_chart_default
        )
        .start()
        .height(500),
        single!(
            "scatter-chart",
            "axes",
            "Two series with axes and an average",
            scatter_chart_axes
        )
        .start()
        .height(470),
        single!(
            "scatter-chart",
            "states",
            "Loading, failed and empty",
            scatter_chart_states
        )
        .start()
        .width(820)
        .height(320),
    ]
}

/// A small sequence in [0, 1) that is the same on every run, so the previews are.
fn spread(seed: u64) -> impl Iterator<Item = f64> {
    let mut state = seed;
    std::iter::repeat_with(move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / (1_u64 << 53) as f64
    })
}

/// Roughly normal, from the sum of twelve of the sequence.
fn bell(values: &mut impl Iterator<Item = f64>) -> f64 {
    values.by_ref().take(12).sum::<f64>() - 6.0
}

/// A day's training load for `days` days ending 12 November 2026: heavier at the weekend,
/// a rest day in the week, a build-up over the months, and a few days the watch was off.
fn training(days: i64, seed: u64) -> Vec<(Date, Option<f64>)> {
    let last = Date::new(2026, 11, 12).unwrap_or_default();
    let mut noise = spread(seed);
    (0..days)
        .map(|back| {
            let date = last.add_days(-(days - 1 - back)).unwrap_or(last);
            let usual = [62.0, 74.0, 0.0, 88.0, 50.0, 128.0, 96.0]
                [usize::from(date.weekday().days_from_monday())];
            let (shape, off) = (noise.next().unwrap_or(0.5), noise.next().unwrap_or(0.5));
            let build = 0.7 + 0.5 * (back as f64 / days as f64);
            let load = (usual * build * (0.55 + 0.9 * shape)).round();
            (date, (off > 0.06).then_some(load))
        })
        .collect()
}

/// A day's steps for `days` days ending 12 November 2026.
fn walking(days: i64, seed: u64) -> Vec<(Date, Option<f64>)> {
    let last = Date::new(2026, 11, 12).unwrap_or_default();
    let mut noise = spread(seed);
    (0..days)
        .map(|back| {
            let date = last.add_days(-(days - 1 - back)).unwrap_or(last);
            let steps = 3_500.0 + 11_500.0 * noise.next().unwrap_or(0.5).powf(1.4);
            let missed = noise.next().unwrap_or(0.5) < 0.1;
            (date, (!missed).then_some((steps / 50.0).round() * 50.0))
        })
        .collect()
}

/// The hours of a day, from midnight: `12 am`, `1 am` ... `11 pm`.
fn hours() -> Vec<String> {
    (0..24)
        .map(|hour| match hour {
            0 => "12 am".to_owned(),
            1..=11 => format!("{hour} am"),
            12 => "12 pm".to_owned(),
            _ => format!("{} pm", hour - 12),
        })
        .collect()
}

/// A week of steps by hour, Monday first: quiet at night, a walk to work on weekdays, a long
/// walk on Sunday morning, and an hour now and then the watch was off.
fn steps_by_hour(seed: u64) -> Vec<(&'static str, Vec<Option<f64>>)> {
    let mut noise = spread(seed);
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .into_iter()
        .enumerate()
        .map(|(day, name)| {
            let weekend = day >= 5;
            let values = (0..24)
                .map(|hour| {
                    let usual = match hour {
                        0..=5 => 0.0,
                        8 | 18 if !weekend => 1_900.0,
                        9..=11 if day == 6 => 2_600.0,
                        7..=21 => 650.0,
                        _ => 120.0,
                    };
                    let (wobble, off) = (noise.next().unwrap_or(0.5), noise.next().unwrap_or(0.5));
                    let steps = (usual * (0.5 + wobble) / 10.0).round() * 10.0;
                    (off > 0.03).then_some(steps)
                })
                .collect();
            (name, values)
        })
        .collect()
}

fn heatmap_thumb() -> Heatmap {
    steps_by_hour(4).into_iter().fold(
        Heatmap::new("Steps by hour")
            .id("heatmap-thumb")
            .hide_title()
            .table(TableMode::ScreenReader)
            .columns(hours())
            .ink(Ink::slot(3)),
        |chart, (day, values)| chart.row(day, values),
    )
}

fn heatmap_default() -> Heatmap {
    steps_by_hour(9).into_iter().fold(
        Heatmap::new("Steps by hour")
            .id("heatmap-default")
            .description("Last week, Harleen Kaur")
            .locale("en-IN")
            .row_name("Day")
            .columns(hours())
            .range(0, 2_500)
            .value_name("Steps")
            .ink(Ink::slot(3))
            .row_summary(RowSummary::Total),
        |chart, (day, values)| chart.row(day, values),
    )
}

fn heatmap_bands() -> Heatmap {
    Heatmap::new("Resting heart rate against usual")
        .id("heatmap-bands")
        .description("Each week's average, as a share of the person's own usual")
        .row_name("Person")
        .columns(["Week 1", "Week 2", "Week 3", "Week 4", "Week 5", "Week 6"])
        .row("Harleen Kaur", [0.97, 1.01, 1.04, 0.99, 0.95, 0.94])
        .row(
            "Gurpreet Singh",
            [
                Some(1.02),
                Some(1.09),
                None,
                Some(1.13),
                Some(1.06),
                Some(1.01),
            ],
        )
        .row("Simran Kaur", [0.99, 0.98, 1.00, 1.03, 1.08, 1.12])
        .row("Arjun Mehta", [1.05, 1.00, 0.96, 0.93, 0.91, 0.95])
        .colors(
            ColorScale::new(Ink::slot(1))
                .label("Well under")
                .from(0.94, Ink::slot(1).soft())
                .label("Under")
                .from(0.98, Ink::muted().soft())
                .label("As usual")
                .from(1.02, Ink::slot(2).soft())
                .label("Over")
                .from(1.08, Ink::slot(2))
                .label("Well over"),
        )
        .format(NumberFormat::percent().max_decimals(0))
        .value_name("Of usual")
        .show_values()
}

fn heatmap_categories() -> Heatmap {
    Heatmap::new("Workouts by day")
        .id("heatmap-categories")
        .description("Last week")
        .row_name("Person")
        .columns(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
        .category("Run", Ink::slot(1))
        .category("Strength", Ink::slot(2))
        .category("Yoga", Ink::slot(7))
        .category("Rest", Ink::muted())
        .category_row(
            "Harleen Kaur",
            ["Run", "Strength", "Rest", "Run", "Yoga", "", "Run"],
        )
        .category_row(
            "Gurpreet Singh",
            [
                "Strength", "Rest", "Strength", "Rest", "Strength", "Run", "Rest",
            ],
        )
        .category_row(
            "Simran Kaur",
            ["Yoga", "Yoga", "Run", "Yoga", "Rest", "Yoga", "Run"],
        )
}

fn heatmap_states() -> Grid {
    let state = |caption: &'static str, chart: Heatmap| {
        Stack::gap(2).children((Text::muted(caption), chart))
    };
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Heatmap::new("Steps by hour")
                .id("heatmap-loading")
                .loading(),
        ),
        state(
            "Failed",
            Heatmap::new("Steps by hour")
                .id("heatmap-failed")
                .failed(None)
                .retry("/activity"),
        ),
        state(
            "Empty",
            Heatmap::new("Steps by hour").id("heatmap-empty").empty(
                "No steps yet",
                Some("Hours appear here once your watch syncs.".to_owned()),
            ),
        ),
    ))
}

fn calendar_heatmap_thumb() -> CalendarHeatmap {
    CalendarHeatmap::new("Training consistency")
        .id("calendar-heatmap-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .days(training(91, 7))
        .ink(Ink::slot(5))
}

fn calendar_heatmap_default() -> CalendarHeatmap {
    CalendarHeatmap::new("Training consistency")
        .id("calendar-heatmap-default")
        .description("Last 6 months of activity, Harleen Kaur")
        .locale("en-IN")
        .days(training(181, 11))
        .ink(Ink::slot(5))
        .value_name("Training load")
        .unit("AU")
}

fn calendar_heatmap_steps() -> CalendarHeatmap {
    CalendarHeatmap::new("Daily steps")
        .id("calendar-heatmap-steps")
        .description("Four steps at 4,000, 8,000 and 12,000, Gurpreet Singh")
        .locale("en-IN")
        .days(walking(150, 3))
        .ink(Ink::tone(Tone::Success))
        .steps(HeatSteps::at(4_000, 8_000, 12_000))
        .unit("steps")
        .week_start(Weekday::Sunday)
}

fn calendar_heatmap_short() -> CalendarHeatmap {
    let first = Date::new(2026, 10, 5).unwrap_or_default();
    let last = Date::new(2026, 11, 12).unwrap_or_default();
    CalendarHeatmap::new("Meditation")
        .id("calendar-heatmap-short")
        .description("Minutes a day since 5 October, Simran Kaur")
        .locale("en-IN")
        .days((0..24).map(|d| {
            let minutes = [10, 0, 15, 20, 5, 25, 0, 30][(d * 3 % 8) as usize];
            (
                first.add_days(d + 6).unwrap_or(first),
                Some(f64::from(minutes)),
            )
        }))
        .range(first, last)
        .steps(HeatSteps::Quartiles)
        .unit("min")
        .no_weekdays()
}

fn calendar_heatmap_states() -> Grid {
    let state = |caption: &'static str, chart: CalendarHeatmap| {
        Stack::gap(2).children((Text::muted(caption), chart))
    };
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            CalendarHeatmap::new("Training consistency")
                .id("calendar-heatmap-loading")
                .loading(),
        ),
        state(
            "Failed",
            CalendarHeatmap::new("Training consistency")
                .id("calendar-heatmap-failed")
                .failed(None)
                .retry("/fitness"),
        ),
        state(
            "Empty",
            CalendarHeatmap::new("Training consistency")
                .id("calendar-heatmap-empty")
                .empty(
                    "No activity yet",
                    Some("Days appear here once your watch syncs.".to_owned()),
                ),
        ),
    ))
}

/// The six measures a wearable pairs up, and how each pair came out over 30 days.
const MEASURES: [&str; 6] = ["Sleep", "Stress", "Steps", "HRV", "Resting HR", "Mood"];

/// A pair, then its value, whether it is significant, and the note.
const LINKS: [(&str, &str, f64, bool); 14] = [
    ("Sleep", "Stress", -0.58, true),
    ("Sleep", "Steps", 0.21, false),
    ("Sleep", "HRV", 0.71, true),
    ("Sleep", "Resting HR", -0.64, true),
    ("Sleep", "Mood", 0.46, true),
    ("Stress", "Steps", -0.27, false),
    ("Stress", "HRV", -0.52, true),
    ("Stress", "Resting HR", 0.44, true),
    ("Stress", "Mood", -0.67, true),
    ("Steps", "HRV", 0.38, true),
    ("Steps", "Resting HR", -0.31, false),
    ("Steps", "Mood", 0.29, false),
    ("HRV", "Resting HR", -0.77, true),
    ("HRV", "Mood", 0.41, true),
];

/// The matrix of those measures: the last pair, resting heart rate and mood, was never tested.
fn correlations(title: &str) -> MatrixHeatmap {
    LINKS.iter().fold(
        MatrixHeatmap::new(title)
            .metrics(MEASURES)
            .value_name("Correlation"),
        |matrix, (a, b, r, significant)| {
            let note = if *significant {
                "30 days, p < 0.05"
            } else {
                "30 days, could be chance"
            };
            matrix.relation(a, b, Relation::new(*r).significant(*significant).note(note))
        },
    )
}

fn matrix_heatmap_thumb() -> MatrixHeatmap {
    correlations("Metric correlations")
        .id("matrix-heatmap-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .lower_triangle()
}

fn matrix_heatmap_default() -> MatrixHeatmap {
    correlations("Metric correlations")
        .id("matrix-heatmap-default")
        .description("Last 30 days, Gurpreet Singh")
        .lower_triangle()
}

fn matrix_heatmap_full() -> MatrixHeatmap {
    correlations("Metric correlations")
        .id("matrix-heatmap-full")
        .description("Positive in green, negative in rose")
        .colors(Ink::tone(Tone::Success), Ink::slot(5))
        .show_values()
}

fn matrix_heatmap_states() -> Grid {
    let state = |caption: &'static str, chart: MatrixHeatmap| {
        Stack::gap(2).children((Text::muted(caption), chart))
    };
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            MatrixHeatmap::new("Metric correlations")
                .id("matrix-heatmap-loading")
                .loading(),
        ),
        state(
            "Failed",
            MatrixHeatmap::new("Metric correlations")
                .id("matrix-heatmap-failed")
                .failed(None)
                .retry("/mind"),
        ),
        state(
            "Empty",
            MatrixHeatmap::new("Metric correlations")
                .id("matrix-heatmap-empty")
                .empty(
                    "Not enough days yet",
                    Some("Correlations appear after 14 days of data.".to_owned()),
                ),
        ),
    ))
}

/// Gaps between successive heartbeats in milliseconds: each pulls back toward 850 and wanders.
fn beats(count: usize, seed: u64) -> Vec<(f64, f64)> {
    let mut noise = spread(seed);
    let mut now = 850.0_f64;
    let mut gaps = Vec::with_capacity(count + 1);
    gaps.push(now);
    for _ in 0..count {
        now = (850.0 + 0.62 * (now - 850.0) + 26.0 * bell(&mut noise)).clamp(520.0, 1_250.0);
        gaps.push(now.round());
    }
    gaps.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

fn scatter_chart_thumb() -> Scatter {
    Scatter::new("Successive intervals")
        .id("scatter-chart-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .series(PointSeries::new("Resting", beats(60, 5)).ink(Ink::slot(8)))
        .x_axis(Axis::hidden())
        .y_axis(Axis::hidden())
        .grid(Gridlines::none())
        .height(140)
}

fn scatter_chart_default() -> Scatter {
    let cloud = beats(250, 9);
    let names: Vec<String> = (1..=cloud.len()).map(|n| format!("Beat {n}")).collect();
    Scatter::new("Successive heartbeat intervals")
        .id("scatter-chart-default")
        .description("Each point is a beat against the next, Simran Kaur, resting")
        .locale("en-IN")
        .series(
            PointSeries::new("Resting", cloud)
                .ink(Ink::slot(8))
                .labels(names),
        )
        .x_name("This beat")
        .y_name("Next beat")
        .unit("ms")
        .equal_domains()
        .x_axis(Axis::hidden())
        .y_axis(Axis::hidden())
        .grid(Gridlines::none())
        .reference(Reference::identity().label("Equal"))
        .height(320)
        .motion(Motion::new())
}

fn scatter_chart_axes() -> Scatter {
    let mut noise = spread(21);
    let mut day = |weekend: bool| {
        let steps = if weekend { 9_000.0 } else { 5_000.0 } + 9_000.0 * noise.next().unwrap_or(0.5);
        let resting = 67.0 - steps / 2_500.0
            + 3.5 * bell(&mut noise) / 2.0
            + if weekend { -1.0 } else { 1.0 };
        (steps.round(), resting.round())
    };
    let weekdays: Vec<(f64, f64)> = (0..60).map(|_| day(false)).collect();
    let weekends: Vec<(f64, f64)> = (0..24).map(|_| day(true)).collect();
    Scatter::new("Steps and resting heart rate")
        .id("scatter-chart-axes")
        .description("A point for each day, Arjun Sharma")
        .locale("en-IN")
        .series(PointSeries::new("Weekdays", weekdays).ink(Ink::slot(1)))
        .series(PointSeries::new("Weekends", weekends).ink(Ink::slot(2)))
        .x_name("Steps")
        .y_name("Resting heart rate")
        .y_unit("BPM")
        .x_axis(Axis::new().line())
        .y_axis(Axis::new().line())
        .grid(Gridlines::horizontal().dashed())
        .y_domain(Domain::padded(2))
        .reference(Reference::average().label("Average {value}"))
        .height(280)
}

fn scatter_chart_states() -> Grid {
    let state = |caption: &'static str, chart: Scatter| {
        Stack::gap(2).children((Text::muted(caption), chart))
    };
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Scatter::new("Successive intervals")
                .id("scatter-chart-loading")
                .loading(),
        ),
        state(
            "Failed",
            Scatter::new("Successive intervals")
                .id("scatter-chart-failed")
                .failed(None)
                .retry("/heart"),
        ),
        state(
            "Empty",
            Scatter::new("Successive intervals")
                .id("scatter-chart-empty")
                .empty(
                    "No beats yet",
                    Some("They appear here after a resting reading.".to_owned()),
                ),
        ),
    ))
}

/// Pairs tested over 30 days: the coefficient, whether it is significant and its note.
const FOUND: [(&str, &str, f64, bool, &str); 8] = [
    ("Sleep", "Next-day HRV", 0.62, true, "30 days, p < 0.001"),
    ("Steps", "Stress", -0.48, true, "28 days, p = 0.004"),
    (
        "Workout load",
        "Resting HR",
        0.41,
        true,
        "30 days, next day, p = 0.02",
    ),
    ("Screen time", "Sleep", -0.37, true, "26 days, p = 0.04"),
    (
        "Caffeine",
        "Deep sleep",
        -0.22,
        false,
        "21 days, could be chance",
    ),
    ("Mood", "Steps", 0.18, false, "30 days, could be chance"),
    ("Water", "HRV", 0.05, false, "17 days, could be chance"),
    (
        "Alcohol",
        "Recovery",
        f64::NAN,
        false,
        "too few days with both",
    ),
];

fn found(title: &str) -> RelationList {
    FOUND
        .iter()
        .fold(
            RelationList::new(title),
            |list, (a, b, r, significant, note)| {
                let relation = if r.is_nan() {
                    Relation::untested().note(*note)
                } else {
                    Relation::new(*r).significant(*significant).note(*note)
                };
                list.relation(*a, *b, relation)
            },
        )
        .value_name("Correlation (r)")
}

fn relation_list_thumb() -> RelationList {
    found("What moves together")
        .id("relation-list-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .only_significant()
}

fn relation_list_default() -> RelationList {
    found("What moves together")
        .id("relation-list-default")
        .description("Last 30 days, Gurpreet Singh")
        .motion(Motion::new())
}

fn relation_list_significant() -> Grid {
    Grid::cols(2).gap(6).children((
        found("Links found")
            .id("relation-list-found")
            .colors(Ink::tone(Tone::Success), Ink::slot(5))
            .only_significant(),
        correlations("Every pair tested")
            .id("relation-list-matrix")
            .colors(Ink::tone(Tone::Success), Ink::slot(5))
            .lower_triangle(),
    ))
}
