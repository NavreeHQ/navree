//! The Overview: the figures of a period with a line each, requests and
//! exceptions over time, the open issues that matter and a live feed of new
//! occurrences. The figures and the feed are a region that asks the server
//! again every ten seconds.

use std::time::Duration;

use chrono::TimeDelta;
use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_ui::charts::{Annotation, Chart, Series, Sparkline};
use navree_ui::{
    Bars, Card, Empty, Grid, Item, Node, Region, Stat, TableCell, TableRowEl, Text, Tone,
};

use super::{
    cell, cell_num, code, head, head_num, kind_badge, link, percent, period_switch, table, titled,
};
use crate::dashboard::{LighthouseDashboardController, Viewer, age_short, ago};
use crate::model::{IssueKind, Level};
use crate::options::Period;
use crate::store::{FeedItem, IssueFilter, IssueSummary, Overview};

/// How often the live region asks for the figures again.
const POLL: Duration = Duration::from_secs(10);

/// The page.
pub(crate) async fn page(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    period: Period,
) -> Result<Response, AppError> {
    let overview = overview_of(c, period).await?;
    let mount = c.mount().to_owned();
    let store = c.store()?;
    let current = store.current_release_label().await?;
    let release_line: Node = match current {
        Some((name, deployed)) => Text::muted(vec![
            Node::from("Release "),
            code(name),
            Node::from(format!(" · deployed {}", ago(deployed))),
        ])
        .into(),
        None => Text::muted(vec![
            link(format!("{mount}/releases"), "No release reported"),
            Node::from(" yet."),
        ])
        .into(),
    };
    let intro = Text::muted(format!(
        "The last {period}: {} requests, {} exceptions.",
        overview.requests, overview.exceptions
    ));
    let live = live_region(c, period, &overview).await?;
    let page = c
        .page(&viewer, "Overview")
        .await?
        .actions(period_switch(&format!("{mount}?"), period))
        .body((intro, release_line, requests_chart(&overview), live));
    Ok(page.into_response())
}

/// The region alone, for the poll.
pub(crate) async fn live(
    c: &LighthouseDashboardController,
    period: Period,
) -> Result<Region, AppError> {
    let overview = overview_of(c, period).await?;
    live_region(c, period, &overview).await
}

async fn overview_of(
    c: &LighthouseDashboardController,
    period: Period,
) -> Result<Overview, AppError> {
    let mut overview = c.store()?.overview(period).await?;
    overview.dropped = c.lighthouse.dropped();
    Ok(overview)
}

/// The figures, the open issues and the feed, refetched on a timer.
async fn live_region(
    c: &LighthouseDashboardController,
    period: Period,
    overview: &Overview,
) -> Result<Region, AppError> {
    let mount = c.mount().to_owned();
    let top = c
        .store()?
        .issues(&IssueFilter {
            status: Some("open".to_owned()),
            per_page: Some(5),
            ..IssueFilter::default()
        })
        .await?;
    Ok(Region::new("lighthouse-live")
        .src(format!(
            "{mount}/overview/live?period={}",
            period.as_str()
        ))
        .poll(POLL)
        .children((
            figures(overview, period),
            Grid::cols(2).gap(4).children((
                top_issues(&mount, &top.data),
                feed(&mount, &overview.feed),
            )),
        )))
}

/// One figure with its line, and a note under it.
fn tile(
    label: &'static str,
    value: String,
    note: String,
    line: Option<Sparkline>,
    bad: bool,
) -> Node {
    let mut stat = Stat::new(label, value);
    if let Some(line) = line {
        stat = stat.chart(line);
    }
    if bad {
        Card::new().body(stat.trend(note, false)).into()
    } else {
        Card::new().body((stat, Text::muted(note))).into()
    }
}

/// A sparkline of `values`, when there are enough to draw a line.
fn line<V: navree_ui::charts::IntoValue + Copy>(name: &str, values: &[V]) -> Option<Sparkline> {
    (values.len() >= 2).then(|| Sparkline::new(name.to_owned(), values.iter().copied()))
}

/// The seven figures.
fn figures(overview: &Overview, period: Period) -> Node {
    let series = &overview.series;
    let error_rate = overview.error_rate;
    Grid::cols(4)
        .gap(4)
        .children(vec![
            tile(
                "Requests per minute",
                format!("{:.1}", overview.requests_per_minute),
                format!("p50 {} ms", overview.p50_ms.round()),
                line("Requests per minute", &series.requests_per_minute),
                false,
            ),
            tile(
                "Error rate",
                percent(error_rate),
                "4xx and 5xx".to_owned(),
                line("Error rate", &series.error_rate),
                error_rate >= 0.05,
            ),
            tile(
                "p95 latency",
                format!("{} ms", overview.p95_ms.round()),
                "all routes".to_owned(),
                line("p95 latency", &series.p95_ms),
                false,
            ),
            tile(
                "Failed jobs",
                overview.failed_jobs.to_string(),
                format!("in {period}"),
                line("Failed jobs", &series.failed_jobs),
                overview.failed_jobs > 0,
            ),
            tile(
                "Open issues",
                overview.open_issues.to_string(),
                format!("{} new in {period}", overview.new_issues),
                None,
                false,
            ),
            tile(
                "Pool wait",
                format!("{} ms", overview.pool_wait_p95_ms.round()),
                format!(
                    "{} waited, {} timed out",
                    overview.pool_waits, overview.pool_timeouts
                ),
                None,
                overview.pool_timeouts > 0,
            ),
            tile(
                "Sampled out",
                overview.sampled_out.to_string(),
                "traces dropped".to_owned(),
                None,
                false,
            ),
        ])
        .into()
}

/// Requests and exceptions over the period, with a line for each deploy.
fn requests_chart(overview: &Overview) -> Node {
    if overview.timeline.is_empty() {
        return Empty::new("Nothing was recorded in this period.").into();
    }
    let mut chart = Chart::bar("Requests and exceptions")
        .hide_title()
        .times(overview.timeline.iter().map(|point| point.at.naive_utc()))
        .series(Series::new(
            "Requests",
            overview.timeline.iter().map(|point| point.count),
        ))
        .series(
            Series::new(
                "Exceptions",
                overview.timeline.iter().map(|point| point.errors),
            )
            .tone(Tone::Danger),
        )
        .height(180);
    // A deploy is a line only inside the chart.
    let first = overview.timeline[0].at;
    let length = TimeDelta::seconds(
        i64::from(overview.period.resolution()) * i64::try_from(overview.timeline.len()).unwrap_or(0),
    );
    for marker in overview
        .releases
        .iter()
        .filter(|marker| marker.at >= first && marker.at <= first + length)
    {
        chart = chart.annotation(Annotation::event(
            marker.at.naive_utc(),
            format!("Release {}", marker.name),
        ));
    }
    Card::titled("Requests and exceptions")
        .description("Times are UTC.")
        .body(chart)
        .into()
}

/// The open issues with the most to say, newest first.
fn top_issues(mount: &str, issues: &[IssueSummary]) -> Node {
    let rows: Vec<TableRowEl> = issues
        .iter()
        .map(|summary| {
            let issue = &summary.issue;
            let counts: Vec<u32> = summary
                .sparkline
                .iter()
                .map(|n| u32::try_from(*n).unwrap_or(0))
                .collect();
            TableRowEl::new(vec![
                cell(Node::list((
                    kind_badge(issue.kind, issue.level),
                    Node::from(" "),
                    link(format!("{mount}/issues/{}", issue.id), issue.title.clone()),
                    Text::muted(issue.culprit.clone()),
                ))),
                TableCell::new(Bars::new("Events per hour, last 24 hours", &counts))
                    .hide_on_mobile(),
                cell_num(issue.count.to_string()),
                cell_num(age_short(issue.last_seen)),
            ])
            .key(issue.id.to_string())
        })
        .collect();
    titled(
        "Open issues",
        table(
            vec![
                head("Issue"),
                head("24 hours").hide_on_mobile(),
                head_num("Events"),
                head_num("Last seen"),
            ],
            rows,
            "No open issues.",
        ),
    )
}

/// The newest occurrences, as they happen.
fn feed(mount: &str, feed: &[FeedItem]) -> Node {
    let items: Vec<Item> = feed
        .iter()
        .take(20)
        .map(|item| {
            let kind = IssueKind::parse(&item.kind);
            let level = Level::parse(&item.level).unwrap_or_default();
            let what = match kind {
                Some(kind) => kind_badge(kind, level).into(),
                None => Node::Empty,
            };
            Item::new(link(
                format!("{mount}/issues/{}", item.issue_id),
                item.title.clone(),
            ))
            .description(Node::list((what, Node::from(" "), item.culprit.clone())))
            .actions(Text::muted(age_short(item.at)).inline())
            .attr("data-key", item.occurrence_id.to_string())
        })
        .collect();
    let body: Node = if items.is_empty() {
        Empty::new("Nothing yet. New occurrences appear here as they happen.").into()
    } else {
        Node::list(items)
    };
    titled("New occurrences", body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_needs_two_values() {
        assert!(line("x", &[1.0_f64]).is_none());
        assert!(line("x", &[1.0_f64, 2.0]).is_some());
        assert!(line::<i64>("x", &[]).is_none());
    }

    #[test]
    fn a_failing_level_is_coloured_by_its_tone() {
        assert_eq!(super::super::level_tone(Level::Error), Tone::Danger);
    }
}
