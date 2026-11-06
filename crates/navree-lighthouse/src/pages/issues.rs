//! The Issues list and an issue's page, with the forms that resolve,
//! ignore and reopen it.

use chrono::{TimeDelta, Utc};
use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_ui::charts::{Chart, Series};
use navree_ui::{
    Badge, BadgeVariant, Bars, Button, ButtonVariant, Checkbox, CodeBlock, Field, Grid, Input,
    NativeSelect, Node, Pagination, Reply, Row, Stack, TableCell, TableRowEl, Text, ToastTone,
};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    cell, cell_num, clean, code, facts, head, head_num, kind_badge, link, post_form, status_badge,
    table, titled, when, when_exact,
};
use crate::api::IssueFilterQuery;
use crate::dashboard::{LighthouseDashboardController, Viewer};
use crate::model::{IssueKind, IssueStatus, Level};
use crate::store::{IgnoreFor, IssueFilter, IssueSummary, Occurrence};

/// The Resolve form: now, or in the next release.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ResolveForm {
    in_next_release: bool,
    comment: String,
}

/// The Ignore form: forever, for a time or for a number of occurrences.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct IgnoreForm {
    duration: String,
    comment: String,
}

/// How long an ignore choice lasts (`forever`, `hours:24`, `occurrences:100`).
fn ignore_for(duration: &str) -> Result<IgnoreFor, AppError> {
    let number = |text: &str| text.parse::<i64>().ok().filter(|n| *n >= 1);
    match duration.split_once(':') {
        None if duration.is_empty() || duration == "forever" => Ok(IgnoreFor::Forever),
        Some(("hours", hours)) => number(hours)
            .filter(|hours| *hours <= 24 * 366)
            .map(|hours| IgnoreFor::Until(Utc::now() + TimeDelta::hours(hours)))
            .ok_or_else(|| AppError::bad_request("choose how long to ignore the issue")),
        Some(("occurrences", count)) => number(count)
            .map(IgnoreFor::Occurrences)
            .ok_or_else(|| AppError::bad_request("choose how long to ignore the issue")),
        _ => Err(AppError::bad_request(
            "choose how long to ignore the issue",
        )),
    }
}

// ---------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------

/// A filter control with its label.
fn field(id: &'static str, label: &'static str, control: impl Into<Node>) -> Node {
    Field::new(id, label).control(control).into()
}

/// A text filter.
fn text_filter(
    name: &'static str,
    label: &'static str,
    placeholder: &'static str,
    value: &Option<String>,
) -> Node {
    let mut input = Input::text(name).placeholder(placeholder);
    if let Some(value) = value {
        input = input.value(value.clone());
    }
    field(name, label, input)
}

/// A select of `options`, with `current` chosen; `prompt` is the empty
/// first choice, if any.
fn choice(
    name: &'static str,
    label: &'static str,
    prompt: Option<&'static str>,
    options: Vec<(String, String)>,
    current: &Option<String>,
) -> Node {
    let mut select = NativeSelect::new(name);
    if let Some(prompt) = prompt {
        select = select.prompt(prompt);
    }
    for (value, text) in options {
        select = select.option(value, text);
    }
    if let Some(current) = current {
        select = select.selected(current.clone());
    }
    field(name, label, select)
}

/// The filters of the list: a form that asks with the address, so a filtered
/// list can be linked to.
fn filters(mount: &str, filter: &IssueFilter) -> Node {
    let sort = match filter.sort {
        crate::store::IssueSort::LastSeen => "last_seen",
        crate::store::IssueSort::FirstSeen => "first_seen",
        crate::store::IssueSort::Count => "count",
    };
    let statuses: Vec<(String, String)> = IssueStatus::ALL
        .iter()
        .map(|status| (status.as_str().to_owned(), status.label().to_owned()))
        .collect();
    let kinds: Vec<(String, String)> = IssueKind::ALL
        .iter()
        .map(|kind| (kind.as_str().to_owned(), kind.label().to_owned()))
        .collect();
    let levels = vec![
        ("error".to_owned(), "Error".to_owned()),
        ("warning".to_owned(), "Warning".to_owned()),
        ("info".to_owned(), "Info".to_owned()),
    ];
    let sorts = vec![
        ("last_seen".to_owned(), "Last seen".to_owned()),
        ("first_seen".to_owned(), "First seen".to_owned()),
        ("count".to_owned(), "Most frequent".to_owned()),
    ];
    let controls: Vec<Node> = vec![
        choice("status", "Status", Some("Any status"), statuses, &filter.status),
        choice("kind", "Kind", Some("Any kind"), kinds, &filter.kind),
        choice("level", "Level", Some("Any level"), levels, &filter.level),
        text_filter("search", "Search", "Words in the title", &filter.search),
        text_filter("route", "Route", "GET /api/deals", &filter.route),
        text_filter("job", "Job", "SendInvoice", &filter.job),
        text_filter("release", "Release", "20261102-101500", &filter.release),
        text_filter("environment", "Environment", "production", &filter.environment),
        text_filter("tag", "Tag", "plan:pro", &filter.tag),
        text_filter("user", "User id", "", &filter.user),
        text_filter("workspace", "Workspace id", "", &filter.workspace),
        choice("sort", "Sort", None, sorts, &Some(sort.to_owned())),
    ];
    navree_ui::el("form")
        .attr("method", "get")
        .attr("action", format!("{mount}/issues"))
        .child(Stack::gap(3).children((
            Grid::cols(4).gap(3).children(controls),
            Row::gap(2).children(Button::primary("Filter").submit()),
        )))
        .into()
}

/// One issue of the list.
fn list_row(mount: &str, summary: &IssueSummary) -> TableRowEl {
    let issue = &summary.issue;
    let counts: Vec<u32> = summary
        .sparkline
        .iter()
        .map(|n| u32::try_from(*n).unwrap_or(0))
        .collect();
    let mut what = vec![
        kind_badge(issue.kind, issue.level).into(),
        Node::from(" "),
        link(format!("{mount}/issues/{}", issue.id), issue.title.clone()),
        Text::muted(issue.culprit.clone()).into(),
    ];
    if issue.status != IssueStatus::Open {
        what.insert(1, status_badge(issue.status).into());
    }
    TableRowEl::new(vec![
        cell(Node::list(what)),
        TableCell::new(Bars::new("Events per hour, last 24 hours", &counts)).hide_on_mobile(),
        cell_num(issue.count.to_string()),
        cell_num(issue.users.to_string()).hide_on_mobile(),
        cell_num(summary.workspaces.to_string()).hide_on_mobile(),
        cell(when(issue.first_seen)).hide_on_mobile(),
        cell(when(issue.last_seen)),
    ])
    .key(issue.id.to_string())
}

/// The list page.
pub(crate) async fn list(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    query: IssueFilterQuery,
) -> Result<Response, AppError> {
    let filter = query.filter();
    let page = c.store()?.issues(&filter).await?;
    let mount = c.mount().to_owned();
    let rows: Vec<TableRowEl> = page.data.iter().map(|s| list_row(&mount, s)).collect();
    let status = filter
        .status
        .as_deref()
        .filter(|status| !status.is_empty())
        .map(|status| format!("{} ", status.replace('_', " ")))
        .unwrap_or_default();
    let plural = if page.meta.total == 1 { "" } else { "s" };
    let summary = Text::muted(format!(
        "{} {status}issue{plural}, grouped by fingerprint.",
        page.meta.total
    ));
    let base = format!("{mount}/issues?{}", query.encoded_without_page());
    let pager: Option<Node> = (page.meta.last_page > 1).then(|| {
        Pagination::new(
            u32::try_from(page.meta.current_page).unwrap_or(1),
            u32::try_from(page.meta.last_page).unwrap_or(1),
            |n| format!("{base}&page={n}"),
        )
        .into()
    });
    let page = c.page(&viewer, "Issues").await?.body((
        summary,
        filters(&mount, &filter),
        table(
            vec![
                head("Issue"),
                head("Last 24 hours").hide_on_mobile(),
                head_num("Events"),
                head_num("Users").hide_on_mobile(),
                head_num("Workspaces").hide_on_mobile(),
                head("First seen").hide_on_mobile(),
                head("Last seen"),
            ],
            rows,
            "No issues match.",
        ),
        pager,
    ));
    Ok(page.into_response())
}

// ---------------------------------------------------------------------
// One issue
// ---------------------------------------------------------------------

/// Adds a fact when there is a value for it.
fn push_text(
    rows: &mut Vec<(&'static str, Node)>,
    label: &'static str,
    value: &Option<String>,
    as_code: bool,
) {
    if let Some(value) = value {
        let node = if as_code {
            code(value.clone())
        } else {
            Node::from(value.clone())
        };
        rows.push((label, node));
    }
}

/// The facts of the latest stored event.
fn latest_facts(c: &LighthouseDashboardController, mount: &str, o: &Occurrence) -> Node {
    let mut rows: Vec<(&'static str, Node)> = vec![("When", Node::from(when_exact(o.at)))];
    if let Some(method) = &o.method {
        let target = o.route.clone().or_else(|| o.path.clone()).unwrap_or_default();
        let status = o.status.map(|s| format!(" → {s}")).unwrap_or_default();
        rows.push(("Request", code(format!("{method} {target}{status}"))));
    }
    push_text(&mut rows, "Job", &o.job_id, true);
    push_text(&mut rows, "Request id", &o.request_id, true);
    if let Some(trace_id) = &o.trace_id {
        let node = match c.trace_link(Some(trace_id)) {
            Some(href) => navree_ui::el("a")
                .href(href)
                .attr("rel", "noopener noreferrer")
                .child(code(trace_id.clone()))
                .into(),
            None => code(trace_id.clone()),
        };
        rows.push(("Trace id", node));
    }
    push_text(&mut rows, "User", &o.user_id, true);
    push_text(&mut rows, "Workspace", &o.workspace_id, true);
    push_text(&mut rows, "Location", &o.location, true);
    push_text(&mut rows, "Release", &o.release, false);
    push_text(&mut rows, "Environment", &o.environment, false);
    let server = [o.server.clone(), o.process.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    if !server.is_empty() {
        rows.push(("Server", Node::from(server)));
    }
    if let Some(sample) = o.sample_id {
        rows.push((
            "Trace",
            link(format!("{mount}/traces/{sample}"), "Open the trace"),
        ));
    }
    facts(rows)
}

/// What the occurrences table says about where one happened.
fn where_of(o: &Occurrence) -> String {
    match (&o.method, &o.job_id) {
        (Some(method), _) => format!(
            "{method} {}",
            o.route.clone().or_else(|| o.path.clone()).unwrap_or_default()
        ),
        (None, Some(job)) => format!("job {job}"),
        (None, None) => o.process.clone().unwrap_or_default(),
    }
}

/// The forms that change an issue's status.
fn status_forms(
    mount: &str,
    viewer: &Viewer,
    id: Uuid,
    status: IssueStatus,
    comment: &Option<String>,
) -> Node {
    let csrf = viewer.csrf.as_deref();
    if status != IssueStatus::Open {
        let mut parts: Vec<Node> = vec![Button::new("Reopen").submit().into()];
        if let Some(comment) = comment {
            parts.push(Text::muted(comment.clone()).into());
        }
        return titled(
            "Status",
            post_form(
                format!("{mount}/issues/{id}/reopen"),
                csrf,
                Row::gap(3).children(parts),
            ),
        );
    }
    let resolve = post_form(
        format!("{mount}/issues/{id}/resolve"),
        csrf,
        Row::gap(3).wrap().children((
            Checkbox::new("in_next_release").label("In the next release"),
            Input::text("comment").placeholder("Comment (optional)"),
            Button::primary("Resolve").submit(),
        )),
    );
    let ignore = post_form(
        format!("{mount}/issues/{id}/ignore"),
        csrf,
        Row::gap(3).wrap().children((
            NativeSelect::new("duration")
                .option("forever", "Ignore forever")
                .option("hours:1", "Ignore for an hour")
                .option("hours:24", "Ignore for a day")
                .option("occurrences:100", "Ignore for 100 more events"),
            Input::text("comment").placeholder("Comment (optional)"),
            Button::new("Ignore").variant(ButtonVariant::Outline).submit(),
        )),
    );
    titled("Status", Stack::gap(3).children((resolve, ignore)))
}

/// An issue's page.
pub(crate) async fn detail(
    c: &LighthouseDashboardController,
    viewer: Viewer,
    id: Uuid,
) -> Result<Response, AppError> {
    let store = c.store()?;
    let detail = store.issue_detail(id).await?;
    let occurrences = store.occurrences(id, 1, 25).await?;
    let mount = c.mount().to_owned();
    let issue = &detail.issue;

    let mut badges: Vec<Node> = vec![
        Badge::new(issue.level.as_str())
            .tone(super::level_tone(issue.level))
            .into(),
        kind_badge(issue.kind, issue.level).into(),
        status_badge(issue.status).into(),
    ];
    if let Some(at) = detail.regressed_at {
        badges.push(
            Badge::new(format!("regressed {}", when(at)))
                .variant(BadgeVariant::Warning)
                .into(),
        );
    }
    let mut meta = format!(
        "{} · {} events · {} users · {} workspaces · first seen {}",
        issue.culprit,
        issue.count,
        issue.users,
        detail.workspaces,
        when(issue.first_seen)
    );
    if let Some(release) = &detail.first_release {
        meta.push_str(&format!(" in {release}"));
    }
    meta.push_str(&format!(" · last seen {}", when(issue.last_seen)));
    if let Some(release) = &detail.last_release {
        meta.push_str(&format!(" in {release}"));
    }

    // The message, its causes and the backtrace.
    let mut message: Vec<Node> = vec![CodeBlock::new("", detail.message.clone()).into()];
    if detail.chain.len() > 1 {
        message.push(Text::new("Caused by").into());
        message.push(
            CodeBlock::new("", detail.chain[1..].join("\n")).into(),
        );
    }
    if let Some(backtrace) = detail.latest.as_ref().and_then(|l| l.backtrace.clone()) {
        message.push(Text::new("Backtrace").into());
        message.push(CodeBlock::new("", backtrace).into());
    }
    let latest: Node = match &detail.latest {
        Some(latest) => latest_facts(c, &mount, latest),
        None => Text::muted("No stored event (sampling, or pruned).").into(),
    };
    let context: Option<Node> = detail.latest.as_ref().map(|latest| {
        titled(
            "Context",
            CodeBlock::new(
                "json",
                serde_json::to_string_pretty(&latest.context).unwrap_or_default(),
            ),
        )
    });

    let chart: Node = if detail.timeline.is_empty() {
        Text::muted("No events in the last 7 days.").into()
    } else {
        Chart::bar("Events per hour over the last 7 days")
            .hide_title()
            .times(detail.timeline.iter().map(|point| point.at.naive_utc()))
            .series(Series::new(
                "Events",
                detail.timeline.iter().map(|point| point.count),
            ))
            .height(120)
            .into()
    };

    let rows: Vec<TableRowEl> = occurrences
        .data
        .iter()
        .map(|o| {
            TableRowEl::new(vec![
                cell(when_exact(o.at)),
                cell(code(where_of(o))),
                cell(o.user_id.clone().map(code).unwrap_or(Node::Empty)).hide_on_mobile(),
                cell(o.release.clone().unwrap_or_default()).hide_on_mobile(),
                cell(o.server.clone().unwrap_or_default()).hide_on_mobile(),
                cell(match o.sample_id {
                    Some(sample) => link(format!("{mount}/traces/{sample}"), "Trace"),
                    None => Node::Empty,
                }),
            ])
            .key(o.id.to_string())
        })
        .collect();

    let alerts: Option<Node> = (!detail.alerts.is_empty()).then(|| {
        let rows: Vec<TableRowEl> = detail
            .alerts
            .iter()
            .map(|alert| {
                TableRowEl::new(vec![
                    cell(alert.kind.replace('_', " ")),
                    cell(alert.channel.clone()),
                    cell(super::delivery_badge(&alert.status)),
                    cell_num(alert.attempts.to_string()),
                    cell(when(alert.created_at)),
                    cell(alert.error.clone().unwrap_or_default()),
                ])
                .key(alert.id.to_string())
            })
            .collect();
        titled(
            "Alerts sent",
            table(
                vec![
                    head("Alert"),
                    head("Channel"),
                    head("Status"),
                    head_num("Attempts"),
                    head("Raised"),
                    head("Error"),
                ],
                rows,
                "No alerts.",
            ),
        )
    });

    let forms: Option<Node> = viewer
        .can_resolve
        .then(|| status_forms(&mount, &viewer, id, issue.status, &detail.comment));
    let title = issue.title.clone();
    let page = c
        .page(&viewer, title)
        .await?
        .crumb("Issues", format!("{mount}/issues"))
        .body((
            navree_ui::Row::gap(2).wrap().children(badges),
            Text::muted(meta),
            forms,
            Grid::cols(2).gap(4).children((
                titled("Message", Stack::gap(3).children(message)),
                titled("Latest event", latest),
            )),
            context,
            titled("Events over the last 7 days", chart),
            titled(
                "Stored events",
                table(
                    vec![
                        head("When"),
                        head("Where"),
                        head("User").hide_on_mobile(),
                        head("Release").hide_on_mobile(),
                        head("Server").hide_on_mobile(),
                        head("Trace"),
                    ],
                    rows,
                    "No stored events.",
                ),
            ),
            alerts,
        ));
    Ok(page.into_response())
}

// ---------------------------------------------------------------------
// The forms
// ---------------------------------------------------------------------

/// Back to the issue, with a message.
fn back(c: &LighthouseDashboardController, id: Uuid, text: &'static str) -> Reply {
    Reply::redirect(format!("{}/issues/{id}", c.mount())).toast(ToastTone::Success, text)
}

/// Resolves the issue.
pub(crate) async fn resolve(
    c: &LighthouseDashboardController,
    id: Uuid,
    form: ResolveForm,
) -> Result<Reply, AppError> {
    c.store()?
        .resolve(id, form.in_next_release, clean(form.comment))
        .await?;
    Ok(back(
        c,
        id,
        if form.in_next_release {
            "Resolved in the next release."
        } else {
            "Resolved."
        },
    ))
}

/// Ignores the issue.
pub(crate) async fn ignore(
    c: &LighthouseDashboardController,
    id: Uuid,
    form: IgnoreForm,
) -> Result<Reply, AppError> {
    let how = ignore_for(&form.duration)?;
    c.store()?.ignore(id, how, clean(form.comment)).await?;
    Ok(back(c, id, "Ignored."))
}

/// Opens the issue again.
pub(crate) async fn reopen(
    c: &LighthouseDashboardController,
    id: Uuid,
) -> Result<Reply, AppError> {
    c.store()?.reopen(id).await?;
    Ok(back(c, id, "Reopened."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_choices_become_durations() {
        assert_eq!(ignore_for("forever").unwrap(), IgnoreFor::Forever);
        assert_eq!(ignore_for("").unwrap(), IgnoreFor::Forever);
        assert_eq!(
            ignore_for("occurrences:100").unwrap(),
            IgnoreFor::Occurrences(100)
        );
        assert!(matches!(
            ignore_for("hours:24").unwrap(),
            IgnoreFor::Until(_)
        ));
        for bad in ["hours:0", "hours:x", "occurrences:-3", "weeks:2", "hours"] {
            assert!(ignore_for(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_filter_form_asks_with_the_address() {
        let html = navree_ui::render(&filters("/_lighthouse", &IssueFilter::default()));
        assert!(html.contains(r#"method="get""#), "{html}");
        assert!(html.contains(r#"action="/_lighthouse/issues""#), "{html}");
        assert!(!html.contains("_csrf"), "a read needs no token: {html}");
    }
}
