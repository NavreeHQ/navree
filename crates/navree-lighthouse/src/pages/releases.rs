//! The Releases page: the releases seen, and for one of them the issues it
//! introduced and the ones that came back in it.

use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_ui::{Badge, BadgeVariant, CodeBlock, Grid, Node, TableRowEl, Text};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    cell, cell_num, code, head, head_num, kind_badge, link, status_badge, table, titled,
};
use crate::dashboard::{LighthouseDashboardController, Viewer, ago, encode};
use crate::model::{IssueKind, IssueStatus, Level};
use crate::release::ReleaseSummary;

/// `?release=` on the Releases page: the release to look into.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ReleaseQuery {
    release: Option<String>,
}

/// An issue of a release: what it is, its title and how often it happened.
type ReleaseIssue = (Uuid, String, IssueKind, Level, IssueStatus, i64);

/// The issues of one release, as a table.
fn issues_table(mount: &str, issues: &[ReleaseIssue], empty: &'static str) -> Node {
    let rows: Vec<TableRowEl> = issues
        .iter()
        .map(|(id, title, kind, level, status, count)| {
            TableRowEl::new(vec![
                cell(Node::list((
                    kind_badge(*kind, *level),
                    Node::from(" "),
                    link(format!("{mount}/issues/{id}"), title.clone()),
                ))),
                cell_num(count.to_string()),
                cell(status_badge(*status)),
            ])
            .key(id.to_string())
        })
        .collect();
    table(
        vec![head("Issue"), head_num("Events"), head("Status")],
        rows,
        empty,
    )
}

/// One release of the list.
fn release_row(mount: &str, release: &ReleaseSummary, selected: bool) -> TableRowEl {
    let mut name = vec![link(
        format!("{mount}/releases?release={}", encode(&release.name)),
        code(release.name.clone()),
    )];
    if release.current {
        name.push(Node::from(" "));
        name.push(Badge::new("Current").variant(BadgeVariant::Success).into());
    }
    let mut detail = vec![Node::from(release.environment.clone())];
    match (&release.commit, &release.url) {
        (Some(commit), Some(url)) => {
            detail.push(Node::from(" · "));
            detail.push(
                navree_ui::el("a")
                    .href(url.clone())
                    .attr("rel", "noopener noreferrer")
                    .child(code(commit.clone()))
                    .into(),
            );
        }
        (Some(commit), None) => {
            detail.push(Node::from(" · "));
            detail.push(code(commit.clone()));
        }
        (None, Some(url)) => {
            detail.push(Node::from(" · "));
            detail.push(
                navree_ui::el("a")
                    .href(url.clone())
                    .attr("rel", "noopener noreferrer")
                    .child("Changes")
                    .into(),
            );
        }
        (None, None) => {}
    }
    let mut first: Vec<Node> = vec![Node::list(name), Text::muted(Node::list(detail)).into()];
    if let Some(notes) = &release.notes {
        first.push(Text::muted(notes.clone()).into());
    }
    let source = if release.source == "api" {
        "a deploy or the app's code"
    } else {
        "a process at boot"
    };
    TableRowEl::new(vec![
        cell(Node::list(first)),
        cell(ago(release.deployed_at)).hide_on_mobile(),
        cell(source).hide_on_mobile(),
        cell_num(release.introduced.to_string()),
        cell_num(release.regressed.to_string()),
    ])
    .key(release.name.clone())
    .selected(selected)
}

/// The page.
pub(crate) async fn page(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    query: ReleaseQuery,
) -> Result<Response, AppError> {
    let store = c.store()?;
    let mount = c.mount().to_owned();
    let releases = store.releases(50).await?;
    let selected = query
        .release
        .as_deref()
        .and_then(|name| releases.iter().find(|r| r.name == name))
        .or_else(|| releases.iter().find(|r| r.current))
        .or_else(|| releases.first())
        .cloned();
    let (introduced, regressed) = match &selected {
        Some(release) => (
            store.release_issues(&release.name, true, 20).await?,
            store.release_issues(&release.name, false, 20).await?,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let current = store.current_release_label().await?;
    let now: Option<Node> = current.map(|(name, deployed)| {
        Text::muted(Node::list(vec![
            Node::from("Now at "),
            code(name),
            Node::from(format!(" · deployed {}", ago(deployed))),
        ]))
        .into()
    });

    let rows: Vec<TableRowEl> = releases
        .iter()
        .map(|release| {
            let chosen = selected.as_ref().is_some_and(|chosen| {
                chosen.name == release.name && chosen.environment == release.environment
            });
            release_row(&mount, release, chosen)
        })
        .collect();
    let list: Node = if releases.is_empty() {
        titled(
            "Releases",
            Text::muted(Node::list(vec![
                Node::from("No release has been reported yet. A process reports the release it runs when it boots, if it has one: set "),
                code("lighthouse.release"),
                Node::from(" (Harbor does this on every deploy) or "),
                code("app.version"),
                Node::from(". A deploy can also report one with its real deploy time."),
            ])),
        )
    } else {
        table(
            vec![
                head("Release"),
                head("Deployed").hide_on_mobile(),
                head("Reported by").hide_on_mobile(),
                head_num("Introduced"),
                head_num("Regressed"),
            ],
            rows,
            "No release has been reported yet.",
        )
    };
    let detail: Option<Node> = selected.as_ref().map(|release| {
        Grid::cols(2)
            .gap(4)
            .children((
                titled(
                    format!("Introduced in {}", release.name),
                    issues_table(
                        &mount,
                        &introduced,
                        "No issue began in this release.",
                    ),
                ),
                titled(
                    format!("Came back in {}", release.name),
                    issues_table(
                        &mount,
                        &regressed,
                        "No resolved issue came back in this release.",
                    ),
                ),
            ))
            .into()
    });
    let report = titled(
        "Report a release",
        navree_ui::Stack::gap(3).children((
            Text::muted(Node::list(vec![
                Node::from("From a deploy script, with an access token that has "),
                code("lighthouse.configure"),
                Node::from(". The label should be the one "),
                code("lighthouse.release"),
                Node::from(" gives the running processes, so issues and releases match."),
            ])),
            CodeBlock::new(
                "",
                format!(
                    "curl -X POST {}/api/releases \\\n  -H \"Authorization: Bearer $TOKEN\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\"release\": \"20261102-101500-3f2a9c1\", \"commit\": \"3f2a9c1\", \"url\": \"https://git.example.com/crm/commit/3f2a9c1\"}}'",
                    store.base_url()
                ),
            ),
            Text::muted("From the app's own code:"),
            CodeBlock::new(
                "",
                "lighthouse.report_release(ReleaseReport::new(\"20261102-101500-3f2a9c1\").commit(\"3f2a9c1\")).await?;",
            ),
        )),
    );
    let page = c.page(&viewer, "Releases").await?.body((
        Text::muted(
            "The releases Lighthouse has seen, when each was deployed, and the issues it introduced and brought back.",
        ),
        now,
        list,
        detail,
        report,
    ));
    Ok(page.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_row_marks_the_current_and_chosen_release() {
        let release = ReleaseSummary {
            name: "20261102-101500".to_owned(),
            environment: "production".to_owned(),
            commit: Some("3f2a9c1".to_owned()),
            url: None,
            notes: None,
            source: "api".to_owned(),
            deployed_at: chrono::Utc::now(),
            first_seen: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            introduced: 2,
            regressed: 0,
            current: true,
        };
        let table = navree_ui::Table::new()
            .rows(vec![release_row("/_lighthouse", &release, true)]);
        let html = navree_ui::render(&Node::from(table));
        assert!(html.contains("Current"), "{html}");
        assert!(html.contains("data-selected"), "{html}");
        assert!(html.contains("/_lighthouse/releases?release=20261102-101500"), "{html}");
    }
}
