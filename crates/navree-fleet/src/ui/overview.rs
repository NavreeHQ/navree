//! The overview: the figures, the workload of each supervisor, the last half
//! hour's jobs per minute and the newest failed jobs.

use chrono::TimeDelta;
use http::request::Parts;
use navree_http::AppError;
use navree_ui::charts::{Chart, Series};
use navree_ui::{
    AppPage, Badge, BadgeVariant, Button, Card, Grid, Item, Justify, Node, Progress, Region, Row,
    Size, Stack, Stat, Table, TableCell, TableHead, TableRowEl, Text, icons,
};
use serde::Deserialize;

use super::format::{
    breakable, count, duration, first_line, minute, number, query_value, segment, status,
};
use super::{Frame, parts};
use crate::{Fleet, Overview, OverviewSupervisor, QueueThroughput, QueueWorkload, RecentFailure};

/// `?queue=`: the queue the jobs-per-minute chart shows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Params {
    queue: Option<String>,
}

/// The page.
pub(crate) async fn page(
    fleet: &Fleet,
    query: &Params,
    parts: &mut Parts,
) -> Result<AppPage, AppError> {
    let frame = Frame::load(fleet, parts).await?;
    Ok(draw(&frame, query.queue.as_deref()))
}

/// The page for `frame`.
fn draw(frame: &Frame, queue: Option<&str>) -> AppPage {
    let overview = &frame.overview;
    let mut sections: Vec<Node> = vec![head(frame).into(), figures(overview).into()];
    if overview.supervisors.is_empty() {
        sections.push(
            Card::titled("Workload")
                .body(parts::nothing("No supervisor is configured."))
                .into(),
        );
    }
    for supervisor in &overview.supervisors {
        sections.push(supervisor_card(frame, supervisor).into());
    }
    let panels: Vec<Node> = vec![throughput(frame, queue).into(), failures(frame).into()];
    sections.push(Grid::cols(2).gap(4).children(panels).into());
    // The whole page body redraws itself, so the figures and the lists stay
    // current without a reload.
    let region = Region::new("fleet-overview")
        .src(frame.here.clone())
        .poll(parts::REFRESH)
        .children(Stack::gap(4).children(sections));
    frame.page("Overview").body(region)
}

/// Which masters run, and the button that pauses or continues them all.
fn head(frame: &Frame) -> Row {
    let overview = &frame.overview;
    let masters = if overview.masters.is_empty() {
        "No master running".to_owned()
    } else {
        format!(
            "{} \u{b7} {}",
            count(overview.masters.len(), "master", "masters"),
            overview.masters.join(", ")
        )
    };
    let (path, button) = if overview.paused.is_empty() {
        (
            "pause",
            Button::outline("Pause all")
                .icon(icons::PAUSE)
                .size(Size::Sm),
        )
    } else {
        (
            "continue",
            Button::outline("Continue all")
                .icon(icons::PLAY)
                .size(Size::Sm),
        )
    };
    let form = parts::action_form(
        frame,
        format!("{}/{path}", frame.base),
        &frame.here,
        &[],
        button,
    );
    Row::gap(3)
        .justify(Justify::Between)
        .wrap()
        .children((Text::muted(masters), form))
}

/// One figure.
fn stat(label: &'static str, value: String, note: Option<(String, bool)>) -> Node {
    let stat = Stat::new(label, value);
    match note {
        Some((text, good)) => stat.trend(text, good).into(),
        None => stat.into(),
    }
}

/// The row of figures.
fn figures(overview: &Overview) -> Grid {
    let paused = overview.paused.len();
    let figures: Vec<Node> = vec![
        stat(
            "Status",
            status(overview.status).to_owned(),
            (paused > 0).then(|| (format!("{paused} paused"), false)),
        ),
        stat(
            "Jobs per minute",
            number(overview.jobs_per_minute),
            Some(("All queues".to_owned(), true)),
        ),
        stat(
            "Jobs past hour",
            number(overview.jobs_past_hour),
            Some((
                count(overview.supervisors.len(), "supervisor", "supervisors"),
                true,
            )),
        ),
        stat(
            "Failed past 7 days",
            number(overview.failed_past_week),
            Some(if overview.failed_total > 0 {
                ("Retry or forget".to_owned(), false)
            } else {
                ("Nothing to handle".to_owned(), true)
            }),
        ),
        stat(
            "Longest wait",
            overview
                .longest_wait
                .as_ref()
                .map_or_else(|| "0 s".to_owned(), |wait| duration(wait.wait_secs)),
            overview
                .longest_wait
                .as_ref()
                .map(|wait| (format!("Queue {}", wait.queue), false)),
        ),
    ];
    Grid::cols(5).gap(3).children(figures)
}

/// A supervisor: its queues with the processes, the backlog and the waits,
/// and the button that pauses or continues it.
fn supervisor_card(frame: &Frame, supervisor: &OverviewSupervisor) -> Card {
    let rows: Vec<TableRowEl> = supervisor
        .queues
        .iter()
        .filter_map(|queue| frame.overview.workload.iter().find(|w| &w.queue == queue))
        .map(|workload| queue_row(workload, supervisor))
        .collect();
    let table = Table::new()
        .head(vec![
            TableHead::new("Queue"),
            TableHead::new("Processes"),
            TableHead::new("Pending").numeric(),
            TableHead::new("Wait").numeric().hide_on_mobile(),
            TableHead::new("Clears in").numeric().hide_on_mobile(),
            TableHead::new("Jobs/min").numeric().hide_on_mobile(),
        ])
        .rows(rows);
    let (path, button) = if supervisor.paused {
        (
            "continue",
            Button::outline("Continue").icon(icons::PLAY).size(Size::Sm),
        )
    } else {
        (
            "pause",
            Button::outline("Pause").icon(icons::PAUSE).size(Size::Sm),
        )
    };
    let toggle = parts::action_form(
        frame,
        format!("{}/{path}", frame.base),
        &frame.here,
        &[("supervisor", supervisor.name.as_str())],
        button,
    );
    let mut title: Vec<Node> = vec![Node::from(supervisor.name.clone())];
    if supervisor.paused {
        title.push(" ".into());
        title.push(
            Badge::new("paused")
                .variant(BadgeVariant::Warning)
                .into(),
        );
    }
    Card::new()
        .title(Node::list(title))
        .description(format!(
            "{} \u{b7} {} \u{b7} {} of {} running",
            supervisor.balance, supervisor.processes_label, supervisor.processes, supervisor.slots
        ))
        .action(toggle)
        .body(table)
}

/// A queue of a supervisor.
fn queue_row(workload: &QueueWorkload, supervisor: &OverviewSupervisor) -> TableRowEl {
    let slots = supervisor.slots.max(workload.processes);
    let words = format!("{} of {slots} processes", workload.processes);
    let queue = if workload.over_threshold {
        let cell: Vec<Node> = vec![
            parts::code(&workload.queue),
            " ".into(),
            Badge::new("Waiting too long")
                .variant(BadgeVariant::Warning)
                .into(),
        ];
        Node::list(cell)
    } else {
        parts::code(&workload.queue)
    };
    let processes = Row::gap(2).children((
        Progress::new(
            u32::try_from(workload.processes).unwrap_or(u32::MAX),
            u32::try_from(slots).unwrap_or(u32::MAX).max(1),
            words.clone(),
        ),
        Text::muted(words).inline(),
    ));
    let clears = if workload.pending > 0 {
        duration(workload.time_to_clear_secs)
    } else {
        "now".to_owned()
    };
    TableRowEl::new(vec![
        TableCell::new(queue),
        TableCell::new(processes),
        TableCell::new(number(workload.pending)).numeric(),
        TableCell::new(duration(workload.oldest_wait_secs))
            .numeric()
            .hide_on_mobile(),
        TableCell::new(clears).numeric().hide_on_mobile(),
        TableCell::new(number(workload.jobs_per_minute))
            .numeric()
            .hide_on_mobile(),
    ])
    .key(workload.queue.clone())
}

/// The queue the chart shows: the one asked for, else the one that finished
/// the most jobs (the first of equals), else none.
fn pick<'a>(series: &'a [QueueThroughput], wanted: Option<&str>) -> Option<&'a QueueThroughput> {
    let total = |queue: &QueueThroughput| queue.per_minute.iter().sum::<u64>();
    wanted
        .and_then(|name| series.iter().find(|queue| queue.queue == name))
        .or_else(|| {
            series
                .iter()
                .fold(None, |best: Option<&QueueThroughput>, queue| match best {
                    Some(best) if total(best) >= total(queue) => Some(best),
                    _ => Some(queue),
                })
        })
}

/// Jobs finished per minute over the last half hour, one queue at a time.
fn throughput(frame: &Frame, queue: Option<&str>) -> Card {
    let series = &frame.overview.throughput;
    let Some(chosen) = pick(series, queue) else {
        return Card::titled("Jobs per minute").body(parts::nothing("No queue is supervised."));
    };
    let minutes = chosen.per_minute.len();
    let labels: Vec<String> = (0..minutes)
        .map(|at| {
            let ago = i64::try_from(minutes - 1 - at).unwrap_or(0);
            minute(frame.now - TimeDelta::minutes(ago))
        })
        .collect();
    let total: u64 = chosen.per_minute.iter().sum();
    let peak = chosen.per_minute.iter().copied().max().unwrap_or(0);
    let summary = if total > 0 {
        format!(
            "{}: {} jobs in {} min, peak {}/min",
            chosen.queue,
            number(total),
            minutes,
            number(peak)
        )
    } else {
        format!("{}: no jobs finished", chosen.queue)
    };
    let chart = Chart::bar("Jobs finished per minute")
        .id("fleet-throughput")
        .hide_title()
        .categories(labels)
        .series(Series::new("Jobs finished", chosen.per_minute.iter().copied()))
        .height(160);
    let mut card = Card::titled("Jobs per minute").description(summary);
    if series.len() > 1 {
        let items = series
            .iter()
            .map(|queue| {
                (
                    queue.queue.clone(),
                    format!("{}?queue={}", frame.base, query_value(&queue.queue)),
                    queue.queue == chosen.queue,
                )
            })
            .collect();
        card = card.action(parts::choices("Queue", items));
    }
    card.body(chart)
}

/// The newest failed jobs, each with Retry and Forget.
fn failures(frame: &Frame) -> Card {
    let overview = &frame.overview;
    let mut card = Card::titled("Failed jobs");
    if overview.failed_total > 0 {
        card = card.action(parts::action_form(
            frame,
            format!("{}/failed/retry", frame.base),
            &frame.here,
            &[],
            parts::small("Retry all"),
        ));
    }
    if overview.recent_failures.is_empty() {
        return card.body(parts::nothing("No failed jobs."));
    }
    let mut body: Vec<Node> = overview
        .recent_failures
        .iter()
        .map(|job| failure(frame, job).into())
        .collect();
    let shown = u64::try_from(overview.recent_failures.len()).unwrap_or(u64::MAX);
    if overview.failed_total > shown {
        body.push(
            parts::link(
                format!("{}/failed", frame.base),
                format!("View all {} failed jobs", number(overview.failed_total)),
            )
            .into(),
        );
    }
    card.body(body)
}

/// One failed job.
fn failure(frame: &Frame, job: &RecentFailure) -> Item {
    let id = segment(&job.id);
    let title: Vec<Node> = vec![
        parts::link(format!("{}/failed/{id}", frame.base), breakable(&job.job_type)).into(),
        " ".into(),
        Text::muted(format!("{}/{}", job.attempts, job.max_attempts))
            .inline()
            .into(),
    ];
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
    Item::new(Node::list(title))
        .description(first_line(&job.error).to_owned())
        .truncate()
        .actions(Row::gap(2).children((retry, forget)))
}
