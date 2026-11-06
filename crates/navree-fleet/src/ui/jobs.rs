//! The jobs pages: the pending and the completed jobs, searched, and one
//! pending job with its payload.

use http::request::Parts;
use navree_http::AppError;
use navree_queue::{PendingJob, PendingState};
use navree_ui::{
    AppPage, Button, Card, FactCard, FactGrid, Justify, Node, Region, Row, Size, Stack, Table,
    TableCell, TableHead, TableRowEl, Text, icons,
};
use serde::Deserialize;

use super::format::{breakable, millis, number, query_value, segment, when};
use super::{Frame, parts};
use crate::dashboard::api_error;
use crate::{CompletedJob, Fleet, LIST_LIMIT};

/// `?tab=pending|completed&search=`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Params {
    tab: Option<String>,
    search: Option<String>,
}

/// A job's state in words.
fn state_name(state: PendingState) -> &'static str {
    match state {
        PendingState::Waiting => "waiting",
        PendingState::Delayed => "delayed",
        PendingState::Running => "running",
    }
}

/// The list: the pending jobs or the completed ones, whichever tab is open.
pub(crate) async fn list(
    fleet: &Fleet,
    query: &Params,
    parts: &mut Parts,
) -> Result<AppPage, AppError> {
    let frame = Frame::load(fleet, parts).await?;
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let pending: Vec<PendingJob> = fleet
        .pending_jobs(search)
        .await
        .map_err(api_error)?
        .into_iter()
        .map(|mut job| {
            job.payload = fleet.redact(&job.payload);
            job
        })
        .collect();
    let completed = fleet.completed_jobs(search).await.map_err(api_error)?;
    let completed_tab = query.tab.as_deref() == Some("completed");
    Ok(draw_list(&frame, &pending, &completed, search, completed_tab))
}

/// The list page.
fn draw_list(
    frame: &Frame,
    pending: &[PendingJob],
    completed: &[CompletedJob],
    search: Option<&str>,
    completed_tab: bool,
) -> AppPage {
    let base = format!("{}/jobs", frame.base);
    let with_search = |tab: &str| match search {
        Some(text) => format!("{base}?tab={tab}&search={}", query_value(text)),
        None => format!("{base}?tab={tab}"),
    };
    let tabs = parts::choices(
        "Jobs",
        vec![
            (
                format!("Pending ({})", number(pending.len() as u64)),
                with_search("pending"),
                !completed_tab,
            ),
            (
                format!("Completed ({})", number(completed.len() as u64)),
                with_search("completed"),
                completed_tab,
            ),
        ],
    );
    let tab = if completed_tab { "completed" } else { "pending" };
    let finder = parts::search_form(
        &base,
        "fleet-jobs",
        search.unwrap_or_default(),
        "Search by job type or queue",
        &[("tab", tab)],
    );
    let listed = if completed_tab {
        completed.len()
    } else {
        pending.len()
    };
    let table: Node = if listed == 0 {
        parts::nothing("Nothing here.")
    } else if completed_tab {
        completed_table(completed).into()
    } else {
        pending_table(frame, pending).into()
    };
    let mut sections: Vec<Node> = vec![
        Row::gap(3)
            .wrap()
            .justify(Justify::Between)
            .children((tabs, finder))
            .into(),
        Card::new().body(table).into(),
    ];
    if u64::try_from(listed).is_ok_and(|n| n >= LIST_LIMIT) {
        sections.push(
            Text::muted(format!(
                "Showing the newest {}. Search to narrow the list.",
                number(LIST_LIMIT)
            ))
            .into(),
        );
    }
    let region = Region::new("fleet-jobs")
        .src(frame.here.clone())
        .poll(parts::REFRESH)
        .children(Stack::gap(4).children(sections));
    frame.page("Jobs").body(region)
}

/// The pending jobs.
fn pending_table(frame: &Frame, jobs: &[PendingJob]) -> Table {
    let rows: Vec<TableRowEl> = jobs
        .iter()
        .map(|job| {
            TableRowEl::new(vec![
                TableCell::new(parts::link(
                    format!("{}/jobs/{}", frame.base, segment(&job.id)),
                    breakable(&job.job_type),
                )),
                TableCell::new(parts::code(&job.queue)),
                TableCell::new(parts::state_badge(state_name(job.state))),
                TableCell::new(job.attempts.to_string()).numeric(),
                TableCell::new(when(job.available_at)).hide_on_mobile(),
            ])
            .key(job.id.clone())
        })
        .collect();
    Table::new()
        .head(vec![
            TableHead::new("Type"),
            TableHead::new("Queue"),
            TableHead::new("State"),
            TableHead::new("Attempts").numeric(),
            TableHead::new("Available").hide_on_mobile(),
        ])
        .rows(rows)
}

/// The completed jobs.
fn completed_table(jobs: &[CompletedJob]) -> Table {
    let rows: Vec<TableRowEl> = jobs
        .iter()
        .enumerate()
        .map(|(at, job)| {
            TableRowEl::new(vec![
                TableCell::new(breakable(&job.job_type)),
                TableCell::new(parts::code(&job.queue)),
                TableCell::new(parts::state_badge(job.outcome.as_str())),
                TableCell::new(job.attempt.to_string()).numeric(),
                TableCell::new(millis(job.runtime_ms)).numeric(),
                TableCell::new(when(job.finished_at)).hide_on_mobile(),
            ])
            .key(format!("{at}-{}", job.job_id))
        })
        .collect();
    Table::new()
        .head(vec![
            TableHead::new("Type"),
            TableHead::new("Queue"),
            TableHead::new("Outcome"),
            TableHead::new("Attempt").numeric(),
            TableHead::new("Runtime").numeric(),
            TableHead::new("Finished").hide_on_mobile(),
        ])
        .rows(rows)
}

/// One pending job.
pub(crate) async fn show(
    fleet: &Fleet,
    id: &str,
    parts: &mut Parts,
) -> Result<AppPage, AppError> {
    let frame = Frame::load(fleet, parts).await?;
    let mut job = fleet
        .pending_job(id)
        .await
        .map_err(api_error)?
        .ok_or_else(AppError::not_found)?;
    job.payload = fleet.redact(&job.payload);
    Ok(draw_job(&frame, &job))
}

/// The page of one job.
fn draw_job(frame: &Frame, job: &PendingJob) -> AppPage {
    let list = format!("{}/jobs", frame.base);
    let facts = FactGrid::new()
        .card(FactCard::new("Id", parts::code(&job.id)))
        .card(FactCard::new("Queue", parts::code(&job.queue)))
        .card(FactCard::new(
            "State",
            parts::state_badge(state_name(job.state)),
        ))
        .card(FactCard::new(
            "Attempts",
            format!("{} of {}", job.attempts, job.options.max_attempts),
        ))
        .card(FactCard::new("Available", when(job.available_at)))
        .card(FactCard::new("Queued", when(job.created_at)));
    let payload = Card::titled("Payload").body(parts::payload(&job.payload));
    frame
        .page(job.job_type.clone())
        .crumb("Jobs", list.clone())
        .actions(
            Button::outline("Back to jobs")
                .icon(icons::ARROW_LEFT)
                .size(Size::Sm)
                .href(list),
        )
        .body(Stack::gap(4).children((facts, payload)))
}
