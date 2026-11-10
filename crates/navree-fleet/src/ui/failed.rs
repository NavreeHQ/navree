//! The failed jobs pages: the list, searched, with Retry and Forget for each
//! job and for all of them, and one failed job with its error and payload.

use http::request::Parts;
use navree_http::AppError;
use navree_queue::FailedJob;
use navree_ui::{
    AppPage, Button, Card, CodeBlock, FactCard, FactGrid, Justify, Node, Region, Row, Size, Stack,
    Table, TableCell, TableHead, TableRowEl,
};
use serde::Deserialize;

use super::format::{breakable, count, first_line, number, segment, when};
use super::{Frame, parts};
use crate::Fleet;
use crate::dashboard::api_error;

/// `?search=`: the failed jobs whose type, queue, error or payload contain it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Params {
    search: Option<String>,
}

/// The list.
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
    let jobs = fleet.failed_jobs(search).await.map_err(api_error)?;
    Ok(draw_list(&frame, &jobs, search))
}

/// The list page.
fn draw_list(frame: &Frame, jobs: &[FailedJob], search: Option<&str>) -> AppPage {
    let list = format!("{}/failed", frame.base);
    let finder = parts::search_form(
        &list,
        "fleet-failed",
        search.unwrap_or_default(),
        "Search by type, queue, error or payload",
        &[],
    );
    let retry_all: Option<Node> = (!jobs.is_empty()).then(|| {
        let label = match search {
            Some(_) => format!("Retry {} matching", number(jobs.len() as u64)),
            None => "Retry all".to_owned(),
        };
        let fields: Vec<(&'static str, &str)> =
            search.map(|text| ("search", text)).into_iter().collect();
        parts::action_form(
            frame,
            format!("{list}/retry"),
            &frame.here,
            &fields,
            Button::outline(label).size(Size::Sm),
        )
        .into()
    });
    let table: Node = if jobs.is_empty() {
        parts::nothing(if search.is_some() {
            "No failed job matches."
        } else {
            "No failed jobs."
        })
    } else {
        let rows: Vec<TableRowEl> = jobs.iter().map(|job| row(frame, job)).collect();
        Table::new()
            .head(vec![
                TableHead::new("Type"),
                TableHead::new("Queue"),
                TableHead::new("Attempts").numeric(),
                TableHead::new("Failed").hide_on_mobile(),
                TableHead::new("Error").hide_on_mobile(),
                TableHead::new("Actions"),
            ])
            .rows(rows)
            .into()
    };
    let sections: Vec<Node> = vec![
        Row::gap(3)
            .wrap()
            .justify(Justify::Between)
            .children((finder, retry_all))
            .into(),
        Card::new().body(table).into(),
    ];
    let region = Region::new("fleet-failed")
        .src(frame.here.clone())
        .poll(parts::REFRESH)
        .children(Stack::gap(4).children(sections));
    frame.page("Failed jobs").body(region)
}

/// One failed job in the list.
fn row(frame: &Frame, job: &FailedJob) -> TableRowEl {
    let id = segment(&job.id);
    let retry = parts::action_form(
        frame,
        format!("{}/failed/{id}/retry", frame.base),
        &frame.here,
        &[],
        parts::small("Retry"),
    );
    let forget = parts::action_form(
        frame,
        format!("{}/failed/{id}/forget", frame.base),
        &frame.here,
        &[],
        parts::small_quiet("Forget"),
    );
    TableRowEl::new(vec![
        TableCell::new(parts::link(
            format!("{}/failed/{id}", frame.base),
            breakable(&job.job_type),
        )),
        TableCell::new(parts::code(&job.queue)),
        TableCell::new(job.attempts.to_string()).numeric(),
        TableCell::new(when(job.failed_at)).hide_on_mobile(),
        TableCell::new(first_line(&job.error).to_owned()).hide_on_mobile(),
        TableCell::new(Row::gap(2).children((retry, forget))),
    ])
    .key(job.id.clone())
}

/// One failed job.
pub(crate) async fn show(fleet: &Fleet, id: &str, parts: &mut Parts) -> Result<AppPage, AppError> {
    let frame = Frame::load(fleet, parts).await?;
    let mut job = fleet
        .failed_job(id)
        .await
        .map_err(api_error)?
        .ok_or_else(AppError::not_found)?;
    job.payload = fleet.redact(&job.payload);
    Ok(draw_job(&frame, &job))
}

/// The page of one failed job: what failed, when, the error, the payload, and
/// the two things to do with it.
fn draw_job(frame: &Frame, job: &FailedJob) -> AppPage {
    let list = format!("{}/failed", frame.base);
    let id = segment(&job.id);
    let forget = parts::action_form(
        frame,
        format!("{list}/{id}/forget"),
        &list,
        &[],
        Button::destructive("Forget").size(Size::Sm),
    );
    let retry = parts::action_form(
        frame,
        format!("{list}/{id}/retry"),
        &list,
        &[],
        Button::primary("Retry").size(Size::Sm),
    );
    let facts = FactGrid::new()
        .card(FactCard::new("Id", parts::code(&job.id)))
        .card(FactCard::new("Queue", parts::code(&job.queue)))
        .card(FactCard::new(
            "Attempts",
            count(
                usize::try_from(job.attempts).unwrap_or(usize::MAX),
                "attempt",
                "attempts",
            ),
        ))
        .card(FactCard::new("Failed", when(job.failed_at)));
    let error = Card::titled("Error").body(CodeBlock::new("", job.error.clone()));
    let payload = Card::titled("Payload").body(parts::payload(&job.payload));
    frame
        .page(job.job_type.clone())
        .crumb("Failed jobs", list)
        .actions(Row::gap(2).children((forget, retry)))
        .body(Stack::gap(4).children((facts, error, payload)))
}
