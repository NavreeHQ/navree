//! The Requests page, one route's page and a stored trace.

use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_ui::charts::{Chart, Series};
use navree_ui::{
    Badge, BadgeVariant, CodeBlock, FactCard, FactGrid, Grid, Node, Row, TableRowEl, Text, Tone, el,
};
use uuid::Uuid;

use super::{
    cell, cell_num, code, facts, head, head_num, link, ms, percent, period_switch, table, titled,
    when_exact,
};
use crate::chart::{WaterfallRow, waterfall};
use crate::dashboard::{LighthouseDashboardController, Viewer, encode};
use crate::options::Period;
use crate::store::TimelinePoint;

/// Every route of the period, slowest first.
pub(crate) async fn routes(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    period: Period,
) -> Result<Response, AppError> {
    let routes = c.store()?.routes(period).await?;
    let mount = c.mount().to_owned();
    let rows: Vec<TableRowEl> = routes
        .iter()
        .map(|route| {
            let href = format!(
                "{mount}/requests/route?endpoint={}&period={}",
                encode(&route.endpoint),
                period.as_str()
            );
            let errors = percent(route.error_rate);
            TableRowEl::new(vec![
                cell(link(href, code(route.endpoint.clone()))),
                cell_num(route.count.to_string()),
                cell_num(format!("{:.1}", route.throughput_per_minute)).hide_on_mobile(),
                cell_num(if route.server_errors > 0 {
                    Badge::new(errors).variant(BadgeVariant::Danger).into()
                } else {
                    Node::from(errors)
                }),
                cell_num(ms(route.p50_ms)).hide_on_mobile(),
                cell_num(ms(route.p95_ms)),
                cell_num(ms(route.p99_ms)).hide_on_mobile(),
                cell_num(ms(route.max_ms)).hide_on_mobile(),
            ])
            .key(route.endpoint.clone())
        })
        .collect();
    let page = c
        .page(&viewer, "Requests")
        .await?
        .actions(period_switch(&format!("{mount}/requests?"), period))
        .body((
            Text::muted(format!(
                "Every route in the last {period}, slowest first. Percentiles come from every request."
            )),
            table(
                vec![
                    head("Route"),
                    head_num("Requests"),
                    head_num("Per minute").hide_on_mobile(),
                    head_num("Error rate"),
                    head_num("p50").hide_on_mobile(),
                    head_num("p95"),
                    head_num("p99").hide_on_mobile(),
                    head_num("Slowest").hide_on_mobile(),
                ],
                rows,
                "No requests in this period.",
            ),
        ));
    Ok(page.into_response())
}

/// Requests and errors over time.
fn timeline_chart(timeline: &[TimelinePoint]) -> Node {
    if timeline.is_empty() {
        return Text::muted("Nothing was recorded in this period.").into();
    }
    Chart::bar("Requests and errors")
        .hide_title()
        .times(timeline.iter().map(|point| point.at.naive_utc()))
        .series(Series::new(
            "Requests",
            timeline.iter().map(|point| point.count),
        ))
        .series(Series::new("Errors", timeline.iter().map(|point| point.errors)).tone(Tone::Danger))
        .height(150)
        .into()
}

/// One route: its figures, its timeline and its slowest stored traces.
pub(crate) async fn route(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    endpoint: &str,
    period: Period,
) -> Result<Response, AppError> {
    let detail = c.store()?.route(endpoint, period).await?;
    let mount = c.mount().to_owned();
    let stats = &detail.stats;
    let figures = FactGrid::new().cols(5).cards([
        FactCard::new("Requests", stats.count.to_string()),
        FactCard::new("Error rate", percent(stats.error_rate)).tone(if stats.server_errors > 0 {
            Tone::Danger
        } else {
            Tone::Neutral
        }),
        FactCard::new("p50", ms(stats.p50_ms)),
        FactCard::new("p95", ms(stats.p95_ms)),
        FactCard::new("p99", ms(stats.p99_ms)),
    ]);
    let rows: Vec<TableRowEl> = detail
        .slowest
        .iter()
        .map(|sample| {
            let mut name = vec![link(
                format!("{mount}/traces/{}", sample.id),
                when_exact(sample.at),
            )];
            if sample.slow {
                name.push(Node::from(" "));
                name.push(Badge::new("slow").variant(BadgeVariant::Warning).into());
            }
            TableRowEl::new(vec![
                cell(Node::list(name)),
                cell_num(ms(sample.duration_ms)),
                cell_num(sample.status.map(|s| s.to_string()).unwrap_or_default()),
                cell(code(sample.trace_id.clone().unwrap_or_default())).hide_on_mobile(),
            ])
            .key(sample.id.to_string())
        })
        .collect();
    let base = format!("{mount}/requests/route?endpoint={}&", encode(endpoint));
    let page = c
        .page(&viewer, endpoint.to_owned())
        .await?
        .crumb("Requests", format!("{mount}/requests"))
        .actions(period_switch(&base, period))
        .body((
            Text::muted(format!("The last {period}.")),
            figures,
            titled("Requests and errors", timeline_chart(&detail.timeline)),
            titled(
                "Slowest traces",
                table(
                    vec![
                        head("When"),
                        head_num("Duration"),
                        head_num("Status"),
                        head("Trace id").hide_on_mobile(),
                    ],
                    rows,
                    "No stored traces in this period (see lighthouse.sample.requests).",
                ),
            ),
        ));
    Ok(page.into_response())
}

/// The track of one span: a lane with the span's share of the whole drawn
/// on it. Attributes only, so the page's policy needs no style.
fn track(row: &WaterfallRow) -> Node {
    el("svg")
        .attr("viewBox", "0 0 1000 10")
        .attr("preserveAspectRatio", "none")
        .attr("width", "100%")
        .attr("height", 10_i64)
        .attr("aria-hidden", "true")
        .child((
            el("rect")
                .attr("x", 0_i64)
                .attr("y", 0_i64)
                .attr("width", 1000_i64)
                .attr("height", 10_i64)
                .attr("fill", "currentColor")
                .attr("opacity", "0.12"),
            el("rect")
                .attr("x", format!("{:.2}", row.x))
                .attr("y", 0_i64)
                .attr("width", format!("{:.2}", row.w))
                .attr("height", 10_i64)
                .attr("fill", "currentColor"),
        ))
        .into()
}

/// A stored trace with its spans.
pub(crate) async fn trace(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    id: Uuid,
) -> Result<Response, AppError> {
    let sample = c.store()?.sample(id).await?;
    let mount = c.mount().to_owned();
    let summary = &sample.summary;
    let rows = waterfall(&sample.spans, summary.duration_ms, 1000.0);
    let trace_link = c.trace_link(summary.trace_id.as_deref());

    let mut badges: Vec<Node> = vec![Badge::new(summary.kind.clone()).into()];
    badges.push(
        Badge::new(summary.outcome.replace('_', " "))
            .variant(if summary.outcome == "ok" {
                BadgeVariant::Success
            } else {
                BadgeVariant::Danger
            })
            .into(),
    );
    if summary.slow {
        badges.push(Badge::new("slow").variant(BadgeVariant::Warning).into());
    }

    let mut meta = vec![when_exact(summary.at)];
    if let Some(status) = summary.status {
        meta.push(status.to_string());
    }
    if let Some(request_id) = &summary.request_id {
        meta.push(format!("request {request_id}"));
    }
    if let Some(job_id) = &sample.job_id {
        meta.push(format!("job {job_id}"));
    }
    let trace_id: Option<Node> = summary.trace_id.as_ref().map(|trace_id| match &trace_link {
        Some(href) => el("a")
            .href(href.clone())
            .attr("rel", "noopener noreferrer")
            .child(code(trace_id.clone()))
            .into(),
        None => code(trace_id.clone()),
    });

    let span_rows: Vec<TableRowEl> = rows
        .iter()
        .enumerate()
        .map(|(at, row)| {
            let mut label = vec![code(row.label.clone())];
            if row.count > 1 {
                label.push(Node::from(" "));
                label.push(Badge::new(format!("× {}", row.count)).into());
            }
            if row.failed {
                label.push(Node::from(" "));
                label.push(Badge::new("failed").variant(BadgeVariant::Danger).into());
            }
            TableRowEl::new(vec![
                cell_num(ms(row.start_ms)),
                cell(row.kind.clone()),
                cell(Node::list(label)),
                cell_num(ms(row.duration_ms)),
                cell(track(row)).hide_on_mobile(),
            ])
            .key(at.to_string())
        })
        .collect();

    let error: Option<Node> = sample
        .error
        .as_ref()
        .map(|error| titled("Error", CodeBlock::new("", error.clone())));
    let labels = facts(vec![
        ("User", code(sample.user_id.clone().unwrap_or_default())),
        (
            "Workspace",
            code(sample.workspace_id.clone().unwrap_or_default()),
        ),
        (
            "Release",
            Node::from(sample.release.clone().unwrap_or_default()),
        ),
        (
            "Environment",
            Node::from(sample.environment.clone().unwrap_or_default()),
        ),
        (
            "Server",
            Node::from(sample.server.clone().unwrap_or_default()),
        ),
        (
            "Process",
            Node::from(sample.process.clone().unwrap_or_default()),
        ),
    ]);
    let title = format!("{} · {}", summary.name, ms(summary.duration_ms));
    let page = c
        .page(&viewer, title)
        .await?
        .crumb("Requests", format!("{mount}/requests"))
        .body((
            Row::gap(2).wrap().children(badges),
            Text::muted(meta.join(" · ")),
            trace_id,
            error,
            titled(
                "Timeline",
                table(
                    vec![
                        head_num("Start"),
                        head("Kind"),
                        head("Span"),
                        head_num("Duration"),
                        head("Timeline").hide_on_mobile(),
                    ],
                    span_rows,
                    "No queries, calls or cache operations were recorded.",
                ),
            ),
            Grid::cols(2).gap(4).children((
                titled(
                    "Context",
                    CodeBlock::new(
                        "json",
                        serde_json::to_string_pretty(&sample.context).unwrap_or_default(),
                    ),
                ),
                titled("Labels", labels),
            )),
        ));
    Ok(page.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_is_drawn_with_attributes_only() {
        let row = WaterfallRow {
            kind: "db".to_owned(),
            label: "SELECT 1".to_owned(),
            count: 1,
            start_ms: 10.0,
            duration_ms: 5.0,
            failed: false,
            x: 100.0,
            w: 50.0,
        };
        let html = navree_ui::render(&track(&row));
        assert!(html.contains(r#"x="100.00""#), "{html}");
        assert!(html.contains(r#"width="50.00""#), "{html}");
        assert!(!html.contains("style="), "{html}");
    }
}
