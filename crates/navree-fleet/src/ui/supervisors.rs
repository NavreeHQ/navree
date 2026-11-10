//! The supervisors page: each master that reported lately, its supervisors
//! and their processes, with the button that pauses or continues each
//! supervisor.

use http::request::Parts;
use navree_http::AppError;
use navree_ui::{
    Alert, AlertTone, AppPage, Badge, BadgeVariant, Button, Card, Heading, Justify, Node, Region,
    Row, Size, Stack, Table, TableCell, TableHead, TableRowEl, Text, icons,
};

use super::format::{ago, count, number, when};
use super::{Frame, parts};
use crate::dashboard::api_error;
use crate::{Fleet, MasterReport, ProcessReport, SupervisorReport};

/// The page.
pub(crate) async fn page(fleet: &Fleet, parts: &mut Parts) -> Result<AppPage, AppError> {
    let frame = Frame::load(fleet, parts).await?;
    let masters = fleet.masters().await.map_err(api_error)?;
    Ok(draw(&frame, &masters))
}

/// The page for `frame`.
fn draw(frame: &Frame, masters: &[MasterReport]) -> AppPage {
    let mut sections: Vec<Node> = Vec::new();
    if masters.is_empty() {
        sections.push(
            Alert::new(AlertTone::Neutral)
                .title("No master has reported in the last 30 seconds")
                .body("Start one with the fleet command.")
                .into(),
        );
    }
    for master in masters {
        sections.push(master_card(frame, master).into());
    }
    let region = Region::new("fleet-supervisors")
        .src(frame.here.clone())
        .poll(parts::REFRESH)
        .children(Stack::gap(4).children(sections));
    frame.page("Supervisors").body(region)
}

/// One master: when it started and reported, and its supervisors.
fn master_card(frame: &Frame, master: &MasterReport) -> Card {
    let title: Vec<Node> = vec![
        Node::from(master.name.clone()),
        " ".into(),
        parts::state_badge(master.status.as_str()).into(),
    ];
    let facts: Vec<Node> = vec![
        format!("pid {} \u{b7} started ", master.pid).into(),
        when(master.started_at),
        format!(", last report {}", ago(frame.now, master.heartbeat_at)).into(),
    ];
    let blocks: Vec<Node> = master
        .supervisors
        .iter()
        .map(|supervisor| supervisor_block(frame, supervisor).into())
        .collect();
    Card::new()
        .title(Node::list(title))
        .description(Node::list(facts))
        .body(Stack::gap(6).children(blocks))
}

/// One supervisor of a master and its processes.
fn supervisor_block(frame: &Frame, supervisor: &SupervisorReport) -> Stack {
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
    let mut name: Vec<Node> = vec![Node::from(supervisor.name.clone())];
    if supervisor.paused {
        name.push(" ".into());
        name.push(Badge::new("paused").variant(BadgeVariant::Warning).into());
    }
    let heading = Row::gap(3).justify(Justify::Between).wrap().children((
        Stack::gap(1).children((
            Heading::h3(Node::list(name)),
            Text::muted(format!(
                "{} \u{b7} {} \u{b7} up to {}",
                supervisor.queues.join(", "),
                supervisor.balance,
                count(supervisor.max_processes, "process", "processes")
            )),
        )),
        toggle,
    ));
    let processes: Node = if supervisor.processes.is_empty() {
        parts::nothing("No process is running.")
    } else {
        Table::new()
            .head(vec![
                TableHead::new("Queue"),
                TableHead::new("PID").numeric(),
                TableHead::new("Memory").numeric().hide_on_mobile(),
                TableHead::new("Jobs").numeric(),
                TableHead::new("Started").hide_on_mobile(),
            ])
            .rows(
                supervisor
                    .processes
                    .iter()
                    .map(|process| process_row(frame, process))
                    .collect::<Vec<_>>(),
            )
            .into()
    };
    Stack::gap(3).children((heading, processes))
}

/// One process.
fn process_row(frame: &Frame, process: &ProcessReport) -> TableRowEl {
    TableRowEl::new(vec![
        TableCell::new(parts::code(&process.queue)),
        TableCell::new(
            process
                .pid
                .map_or_else(|| "in process".to_owned(), |pid| pid.to_string()),
        )
        .numeric(),
        TableCell::new(
            process
                .memory_mb
                .map_or_else(String::new, |mb| format!("{} MB", number(mb))),
        )
        .numeric()
        .hide_on_mobile(),
        TableCell::new(number(process.jobs)).numeric(),
        TableCell::new(ago(frame.now, process.started_at)).hide_on_mobile(),
    ])
}
