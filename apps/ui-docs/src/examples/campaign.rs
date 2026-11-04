//! The example app's own views, drawn from its seed: what `/ui/` shows under
//! "Build anything". The code beside each preview is the app's source, read from
//! its files, so the website shows what the app runs.

use campaign_demo::data::{self, Event, Kind, Recipient, Status};
use campaign_demo::views::dashboard::{Counts, FeedEntry, Period, activity_feed, funnel};
use campaign_demo::views::recipient::recipient_sheet;
use campaign_demo::views::recipients::{RecipientRow, recipients_table};
use navree::ui::*;

use crate::catalog::{self, Example};

/// The moment the previews are drawn at, fixed so the files don't change:
/// 4 November 2026, 12:00 in London.
const NOW: i64 = 1_793_793_600;

const RECIPIENTS_RS: &str = include_str!("../../../campaign-demo/src/views/recipients.rs");
const RECIPIENT_RS: &str = include_str!("../../../campaign-demo/src/views/recipient.rs");
const DASHBOARD_RS: &str = include_str!("../../../campaign-demo/src/views/dashboard.rs");

pub fn all() -> Vec<Example> {
    vec![
        Example::new(
            "build-anything",
            "recipients",
            "The recipients table",
            catalog::Kind::Single(recipients),
            items(
                RECIPIENTS_RS,
                &["pub struct RecipientRow", "pub fn recipients_table"],
            ),
        )
        .start()
        .height(560)
        .streaming("on"),
        Example::new(
            "build-anything",
            "recipient",
            "A recipient in a sheet",
            catalog::Kind::Single(recipient),
            items(
                RECIPIENT_RS,
                &[
                    "pub fn recipient_sheet",
                    "pub fn recipient_detail",
                    "pub fn recipient_timeline",
                ],
            ),
        )
        .start()
        .show()
        .height(900),
        Example::new(
            "build-anything",
            "activity",
            "Recent activity, live",
            catalog::Kind::Single(activity),
            items(
                DASHBOARD_RS,
                &["pub fn activity_feed", "pub fn activity_item"],
            ),
        )
        .start()
        .height(560)
        .streaming("on"),
        Example::new(
            "build-anything",
            "funnel",
            "A funnel of plain SVG elements",
            catalog::Kind::Single(funnel_card),
            items(DASHBOARD_RS, &["fn stage", "pub fn funnel"]),
        )
        .start()
        .height(320),
    ]
}

/// Top-level items of `source` as written, each from its attributes to its
/// closing brace, one after another.
pub fn items(source: &str, starts: &[&str]) -> String {
    let source = source.replace("\r\n", "\n");
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for start in starts {
        let at = lines
            .iter()
            .position(|line| {
                line.starts_with(&format!("{start}("))
                    || line.starts_with(&format!("{start} {{"))
                    || line.starts_with(&format!("{start}<"))
            })
            .unwrap_or_else(|| panic!("no `{start}` at the start of a line"));
        let mut first = at;
        while first > 0 && lines[first - 1].starts_with("#[") {
            first -= 1;
        }
        let end = (at..lines.len())
            .find(|line| lines[*line] == "}")
            .unwrap_or_else(|| panic!("`{start}` doesn't end"));
        out.push(lines[first..=end].join("\n"));
    }
    out.join("\n\n")
}

fn seeded() -> (Vec<Recipient>, Vec<Event>) {
    data::seed(NOW)
}

fn recipients() -> Node {
    let (people, _) = seeded();
    let mut rows: Vec<RecipientRow> = people.iter().cloned().map(RecipientRow::from).collect();
    rows.sort_by(|a, b| b.last_activity.cmp(&a.last_activity).then(b.id.cmp(&a.id)));
    rows.truncate(10);
    let counts: Vec<(Status, u64)> = Status::ALL
        .iter()
        .map(|status| {
            let n = people.iter().filter(|p| p.status == status.key()).count();
            (*status, n as u64)
        })
        .collect();
    let query = TableQuery::<RecipientRow>::parse("/recipients", "per=10");
    let page = TablePage::new(rows, people.len() as u64);
    let today = data::day_of(NOW).unwrap_or_default();
    recipients_table(&query, &page, &counts, today).into()
}

/// Someone who went all the way, whose copy a proxy opened too: the timeline
/// shows every step and the notice for what it set aside.
fn chosen(people: &[Recipient], events: &[Event]) -> Recipient {
    let mine = |person: &Recipient| {
        events
            .iter()
            .filter(|event| event.recipient_id == person.id)
            .count()
    };
    people
        .iter()
        .filter(|person| person.status == Status::Converted.key())
        .find(|person| {
            events
                .iter()
                .any(|event| event.recipient_id == person.id && event.kind == Kind::ProxyOpen.key())
                && mine(person) <= 7
        })
        .or_else(|| people.first())
        .cloned()
        .unwrap_or_default()
}

fn recipient() -> Node {
    let (people, events) = seeded();
    let person = chosen(&people, &events);
    let mine: Vec<Event> = events
        .into_iter()
        .filter(|event| event.recipient_id == person.id)
        .collect();
    recipient_sheet(&person, &mine).into()
}

/// What readers did in the 24 hours to `NOW`, newest first.
fn entries() -> Vec<FeedEntry> {
    let (people, events) = seeded();
    let since = Period::Day.since(NOW);
    let mut out: Vec<FeedEntry> = events
        .into_iter()
        .filter(|event| event.at >= since)
        .filter(|event| {
            Kind::parse(&event.kind) != Kind::Sent && Kind::parse(&event.kind) != Kind::Delivered
        })
        .map(|event| FeedEntry {
            name: people
                .iter()
                .find(|person| person.id == event.recipient_id)
                .map_or_else(String::new, |person| person.name.clone()),
            event,
        })
        .collect();
    out.sort_by_key(|entry| std::cmp::Reverse(entry.event.id));
    out
}

/// The feed as the page first draws it, and after it the rows a stream would
/// bring, which the site's own script shows one by one: the preview has no
/// server to stream from.
fn activity() -> Node {
    let all = entries();
    let (later, shown): (Vec<FeedEntry>, Vec<FeedEntry>) = {
        let mut later: Vec<FeedEntry> = all
            .iter()
            .filter(|entry| Kind::parse(&entry.event.kind) != Kind::ProxyOpen)
            .take(8)
            .cloned()
            .collect();
        let held: Vec<i64> = later.iter().map(|entry| entry.event.id).collect();
        let shown = all
            .into_iter()
            .filter(|entry| !held.contains(&entry.event.id))
            .take(40)
            .collect();
        later.reverse();
        (later, shown)
    };
    let total = shown.len() as u64 + later.len() as u64;
    Node::list((
        activity_feed(&shown, Period::Day, total),
        el("template")
            .attr("data-preview-replay", "activity-list")
            .child(ActivityFeed::rows(
                later
                    .iter()
                    .map(campaign_demo::views::dashboard::activity_item),
                data::OFFSET,
            )),
    ))
}

fn funnel_card() -> Node {
    let (people, events) = seeded();
    funnel(&Counts::of(&people, &events)).into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_code_shown_is_the_apps_own() {
        let code = super::items(super::RECIPIENTS_RS, &["pub struct RecipientRow"]);
        assert!(code.starts_with("#[derive(Clone, TableRow)]"), "{code}");
        assert!(code.ends_with('}'));
        assert!(code.contains("pub last_activity: Option<NaiveDate>,"));
        let code = super::items(super::DASHBOARD_RS, &["fn stage", "pub fn funnel"]);
        assert!(code.contains("el(\"svg\")"), "{code}");
    }
}
