//! The Queries, Jobs and Outgoing pages: a table each, for a period.

use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_ui::{Badge, BadgeVariant, Node, TableRowEl, Text};

use super::{cell, cell_num, code, head, head_num, link, ms, percent, period_switch, table};
use crate::dashboard::{LighthouseDashboardController, Viewer, encode};
use crate::options::Period;
use crate::store::Origin;

/// `GET /api/deals (212), POST /api/deals (3)`: where something came from.
fn origins(origins: &[Origin]) -> String {
    origins
        .iter()
        .take(3)
        .map(|origin| format!("{} ({})", origin.name, origin.count))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Normalized statements by total time.
pub(crate) async fn queries(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    period: Period,
) -> Result<Response, AppError> {
    let queries = c.store()?.queries(period).await?;
    let mount = c.mount().to_owned();
    let rows: Vec<TableRowEl> = queries
        .iter()
        .map(|query| {
            let mut statement = vec![code(query.statement.clone())];
            if query.errors > 0 {
                statement.push(Node::from(" "));
                statement.push(
                    Badge::new(format!("{} failed", query.errors))
                        .variant(BadgeVariant::Danger)
                        .into(),
                );
            }
            TableRowEl::new(vec![
                cell(Node::list(statement)),
                cell_num(query.count.to_string()),
                cell_num(ms(query.total_ms)),
                cell_num(ms(query.avg_ms)).hide_on_mobile(),
                cell_num(ms(query.p95_ms)),
                cell_num(ms(query.max_ms)).hide_on_mobile(),
                cell(origins(&query.origins)).hide_on_mobile(),
            ])
            .key(query.statement.clone())
        })
        .collect();
    let page = c
        .page(&viewer, "Queries")
        .await?
        .actions(period_switch(&format!("{mount}/queries?"), period))
        .body((
            Text::muted(format!(
                "Normalized statements in the last {period}, by total time."
            )),
            table(
                vec![
                    head("Statement"),
                    head_num("Runs"),
                    head_num("Total"),
                    head_num("Average").hide_on_mobile(),
                    head_num("p95"),
                    head_num("Slowest").hide_on_mobile(),
                    head("Run by").hide_on_mobile(),
                ],
                rows,
                "No queries in this period.",
            ),
        ));
    Ok(page.into_response())
}

/// Every job type of the period.
pub(crate) async fn jobs(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    period: Period,
) -> Result<Response, AppError> {
    let jobs = c.store()?.jobs(period).await?;
    let mount = c.mount().to_owned();
    let rows: Vec<TableRowEl> = jobs
        .iter()
        .map(|job| {
            let href = format!(
                "{mount}/issues?status=&kind=failed_job&job={}",
                encode(&job.job_type)
            );
            let failed: Node = if job.failed_jobs > 0 {
                Badge::new(job.failed_jobs.to_string())
                    .variant(BadgeVariant::Danger)
                    .into()
            } else {
                Node::from(job.failed_jobs.to_string())
            };
            TableRowEl::new(vec![
                cell(link(href, code(job.job_type.clone()))),
                cell_num(job.attempts.to_string()),
                cell_num(format!("{:.1}", job.throughput_per_minute)).hide_on_mobile(),
                cell_num(job.failed_attempts.to_string()).hide_on_mobile(),
                cell_num(failed),
                cell_num(ms(job.avg_ms)).hide_on_mobile(),
                cell_num(ms(job.p95_ms)),
                cell_num(ms(job.max_ms)).hide_on_mobile(),
            ])
            .key(job.job_type.clone())
        })
        .collect();
    let page = c
        .page(&viewer, "Jobs")
        .await?
        .actions(period_switch(&format!("{mount}/jobs?"), period))
        .body((
            Text::muted(format!("Every job type in the last {period}.")),
            table(
                vec![
                    head("Job"),
                    head_num("Attempts"),
                    head_num("Per minute").hide_on_mobile(),
                    head_num("Failed attempts").hide_on_mobile(),
                    head_num("Failed jobs"),
                    head_num("Average").hide_on_mobile(),
                    head_num("p95"),
                    head_num("Longest").hide_on_mobile(),
                ],
                rows,
                "No jobs ran in this period.",
            ),
        ));
    Ok(page.into_response())
}

/// Every host called through the HTTP client.
pub(crate) async fn outgoing(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    period: Period,
) -> Result<Response, AppError> {
    let hosts = c.store()?.outgoing(period).await?;
    let mount = c.mount().to_owned();
    let rows: Vec<TableRowEl> = hosts
        .iter()
        .map(|host| {
            let errors = percent(host.error_rate);
            let rate: Node = if host.errors > 0 {
                Badge::new(errors).variant(BadgeVariant::Danger).into()
            } else {
                Node::from(errors)
            };
            let slowest: Vec<Node> = host
                .slowest
                .iter()
                .take(3)
                .flat_map(|call| {
                    let status = call.status.map(|s| format!(", {s}")).unwrap_or_default();
                    [
                        code(format!(
                            "{} {} ({}{status})",
                            call.method,
                            call.url,
                            ms(call.duration_ms)
                        )),
                        Node::from(" "),
                    ]
                })
                .collect();
            TableRowEl::new(vec![
                cell(code(host.host.clone())),
                cell_num(host.count.to_string()),
                cell_num(rate),
                cell_num(ms(host.avg_ms)).hide_on_mobile(),
                cell_num(ms(host.p95_ms)),
                cell_num(ms(host.max_ms)).hide_on_mobile(),
                cell(Node::list(slowest)).hide_on_mobile(),
            ])
            .key(host.host.clone())
        })
        .collect();
    let page = c
        .page(&viewer, "Outgoing")
        .await?
        .actions(period_switch(&format!("{mount}/outgoing?"), period))
        .body((
            Text::muted(format!(
                "Every host called through HttpClient in the last {period}, slowest first."
            )),
            table(
                vec![
                    head("Host"),
                    head_num("Calls"),
                    head_num("Error rate"),
                    head_num("Average").hide_on_mobile(),
                    head_num("p95"),
                    head_num("Slowest").hide_on_mobile(),
                    head("Slowest calls").hide_on_mobile(),
                ],
                rows,
                "No outgoing calls in this period.",
            ),
        ));
    Ok(page.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_name_the_top_three() {
        let make = |name: &str, count: u64| Origin {
            name: name.to_owned(),
            count,
        };
        let shown = origins(&[
            make("GET /deals", 40),
            make("POST /deals", 3),
            make("job SendInvoice", 2),
            make("GET /contacts", 1),
        ]);
        assert_eq!(
            shown,
            "GET /deals (40), POST /deals (3), job SendInvoice (2)"
        );
        assert_eq!(origins(&[]), "");
    }
}
