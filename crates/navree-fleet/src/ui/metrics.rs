//! The metrics page: throughput, runtime and wait charts for each queue, and
//! throughput and runtime charts for each job type, over the last hour, day
//! or week.

use std::time::Duration;

use chrono::NaiveDateTime;
use http::request::Parts;
use navree_http::AppError;
use navree_ui::charts::{Chart, NumberFormat, Series};
use navree_ui::{AppPage, Card, Empty, Grid, Node, Region, Stack, icons};
use serde::Deserialize;

use super::format::breakable;
use super::{Frame, parts};
use crate::dashboard::api_error;
use crate::{Fleet, MetricKind, MetricPoint, MetricSeries, MetricsPeriod};

/// How often the charts redraw: a snapshot is stored every
/// `fleet.snapshot_secs`, so there is little new between two looks.
const REFRESH: Duration = Duration::from_secs(30);

/// `?period=hour|day|week`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Params {
    period: Option<String>,
}

/// The period asked for; an hour for anything else.
fn period_of(text: Option<&str>) -> MetricsPeriod {
    match text {
        Some("day") => MetricsPeriod::Day,
        Some("week") => MetricsPeriod::Week,
        _ => MetricsPeriod::Hour,
    }
}

/// The page.
pub(crate) async fn page(
    fleet: &Fleet,
    query: &Params,
    parts: &mut Parts,
) -> Result<AppPage, AppError> {
    let frame = Frame::load(fleet, parts).await?;
    let period = period_of(query.period.as_deref());
    let series = fleet.metrics(period).await.map_err(api_error)?;
    Ok(draw(&frame, period, &series))
}

/// The page for `frame`.
fn draw(frame: &Frame, period: MetricsPeriod, series: &[MetricSeries]) -> AppPage {
    let base = format!("{}/metrics", frame.base);
    let periods = parts::choices(
        "Period",
        [
            ("Hour", "hour", MetricsPeriod::Hour),
            ("Day", "day", MetricsPeriod::Day),
            ("Week", "week", MetricsPeriod::Week),
        ]
        .into_iter()
        .map(|(label, name, this)| {
            (
                label.to_owned(),
                format!("{base}?period={name}"),
                this == period,
            )
        })
        .collect(),
    );
    let mut sections: Vec<Node> = vec![periods];
    if series.is_empty() {
        sections.push(
            Card::new()
                .body(
                    Empty::new("No snapshots yet")
                        .icon(icons::CHART_COLUMN)
                        .description(
                            "A running master stores one every fleet.snapshot_secs seconds.",
                        ),
                )
                .into(),
        );
    }
    let queues: Vec<&MetricSeries> = series
        .iter()
        .filter(|s| s.kind == MetricKind::Queue)
        .collect();
    let jobs: Vec<&MetricSeries> = series
        .iter()
        .filter(|s| s.kind == MetricKind::Job)
        .collect();
    if !queues.is_empty() {
        sections.push(parts::section("Queues"));
        for (at, queue) in queues.into_iter().enumerate() {
            sections.push(series_card("queue", at, queue, true).into());
        }
    }
    if !jobs.is_empty() {
        sections.push(parts::section("Job types"));
        for (at, job) in jobs.into_iter().enumerate() {
            sections.push(series_card("job", at, job, false).into());
        }
    }
    let region = Region::new("fleet-metrics")
        .src(frame.here.clone())
        .poll(REFRESH)
        .children(Stack::gap(4).children(sections));
    frame.page("Metrics").body(region)
}

/// The charts of one queue or job type.
fn series_card(kind: &str, at: usize, series: &MetricSeries, waits: bool) -> Card {
    let points = &series.points;
    let mut charts: Vec<Node> = vec![
        throughput(kind, at, points).into(),
        runtime(kind, at, points).into(),
    ];
    if waits {
        charts.push(wait(kind, at, points).into());
    }
    let columns = if waits { 3 } else { 2 };
    Card::titled(breakable(&series.name)).body(Grid::cols(columns).gap(4).children(charts))
}

/// The moments of `points`.
fn times(points: &[MetricPoint]) -> Vec<NaiveDateTime> {
    points.iter().map(|point| point.at.naive_utc()).collect()
}

/// Jobs finished per minute.
fn throughput(kind: &str, at: usize, points: &[MetricPoint]) -> Chart {
    Chart::area("Throughput")
        .id(format!("fleet-{kind}-{at}-throughput"))
        .times(times(points))
        .series(Series::new(
            "Jobs per minute",
            points.iter().map(|point| point.throughput_per_minute),
        ))
        .format(NumberFormat::number())
        .height(150)
}

/// The average runtime and the 95th percentile.
fn runtime(kind: &str, at: usize, points: &[MetricPoint]) -> Chart {
    Chart::line("Runtime")
        .id(format!("fleet-{kind}-{at}-runtime"))
        .times(times(points))
        .series(Series::new(
            "Average",
            points.iter().map(|point| point.runtime_avg_ms),
        ))
        .series(Series::new(
            "95th percentile",
            points.iter().map(|point| point.runtime_p95_ms),
        ))
        .format(NumberFormat::integer().suffix(" ms"))
        .height(150)
}

/// The longest wait of a queue's oldest job.
fn wait(kind: &str, at: usize, points: &[MetricPoint]) -> Chart {
    Chart::line("Wait")
        .id(format!("fleet-{kind}-{at}-wait"))
        .times(times(points))
        .series(Series::new(
            "Longest wait",
            points.iter().map(|point| point.wait_secs),
        ))
        .format(NumberFormat::number().suffix(" s"))
        .height(150)
}
