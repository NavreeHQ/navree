//! The list pane, drawn with navree-ui: the filter, one row for each entry, and
//! the new arrivals, which join as a region that polls and morphs.

use std::time::Duration;

use navree_ui::{
    Alert, AlertTone, Badge, BadgeVariant, Button, ButtonVariant, Empty, Input, Item, ItemVariant,
    Node, Region, Size, Stack, Text, TextVariant, el, icons,
};

use super::{Row, tight};

/// How often the list asks for new entries.
const LIVE: Duration = Duration::from_secs(2);

/// What the list shows.
pub(super) struct List {
    pub(super) q: String,
    pub(super) kind: Option<String>,
    pub(super) error: String,
    pub(super) rows: Vec<Row>,
    /// The feed to poll for newer entries; `None` on older pages and after a bad filter.
    pub(super) live_href: Option<String>,
    /// The newest entry shown: the feed asks for what is newer.
    pub(super) latest: u64,
    pub(super) next_href: Option<String>,
    /// Whether an entry is open beside it: on a phone the list gives way to it.
    pub(super) entry_open: bool,
}

/// The badge of a status: green for success, amber for a client's mistake, red for ours.
fn status_badge(status: &str) -> Option<Badge> {
    if status.is_empty() {
        return None;
    }
    let variant = match status.as_bytes().first() {
        Some(b'2') => BadgeVariant::Success,
        Some(b'4') => BadgeVariant::Warning,
        Some(b'5') => BadgeVariant::Danger,
        _ if status == "failed" => BadgeVariant::Danger,
        _ => BadgeVariant::Outline,
    };
    Some(Badge::new(status.to_owned()).variant(variant))
}

/// One entry as a row: its type and what it was, the status and how long it took,
/// then when it happened and what it did inside.
pub(super) fn item(row: Row) -> Item {
    let title: Vec<Node> = vec![
        row.prefix
            .map(|prefix| Text::muted(format!("{prefix} ")).inline().into())
            .unwrap_or(Node::Empty),
        Text::new(tight(&row.title))
            .variant(TextVariant::Code)
            .inline()
            .into(),
    ];
    let mut inside = vec![Node::from(
        el("span").attr("title", row.at.clone()).child(row.time),
    )];
    for counter in row.counters {
        inside.push(
            el("span")
                .attr("title", counter.label)
                .child(format!(" · {} {}", counter.count, counter.short))
                .into(),
        );
    }
    for flag in row.flags {
        inside.push(" ".into());
        inside.push(Badge::new(flag).variant(BadgeVariant::Warning).into());
    }
    let mut at_end: Vec<Node> = Vec::new();
    if let Some(status) = status_badge(&row.status) {
        at_end.push(status.into());
    }
    if !row.duration.is_empty() {
        at_end.push(Text::muted(row.duration).inline().into());
    }
    let item = Item::new(Node::list(title))
        .variant(if row.selected {
            ItemVariant::Muted
        } else {
            ItemVariant::Plain
        })
        .description(Node::list(inside))
        .actions(Node::list(at_end))
        .href(row.href)
        .attr("data-key", row.seq.to_string());
    if row.selected {
        item.attr("aria-current", "true")
    } else {
        item
    }
}

/// The rows of a feed answer, ready to go in at the top of the list.
pub(super) fn rows_fragment(rows: Vec<Row>) -> String {
    navree_ui::render(&Node::list(rows.into_iter().map(item).collect::<Vec<_>>()))
}

/// The filter: a form that asks the server again as the user types, and keeps the
/// entry that is open beside the list.
pub(super) fn search(action: &str, q: &str, kind: Option<&str>) -> Node {
    el("form")
        .attr("role", "search")
        .attr("method", "get")
        .attr("action", action.to_owned())
        .attr("data-ui-target", "rows")
        .children((
            kind.map(|kind| {
                el("input")
                    .attr("type", "hidden")
                    .attr("name", "type")
                    .attr("value", kind.to_owned())
            }),
            Input::search("q")
                .value(q)
                .placeholder("user:… route:… status:4xx slow")
                .attr("aria-label", "Filter")
                .autocomplete("off"),
        ))
        .into()
}

/// The list pane.
pub(super) fn pane(list: List) -> Node {
    let filtered = !list.q.is_empty() || list.kind.is_some();
    // The list is always there, so rows that arrive have somewhere to go; the
    // message under an empty one goes when the first does.
    let empty = list.rows.is_empty().then(|| {
        let message = if filtered {
            "Nothing recorded yet for this filter."
        } else {
            "Nothing recorded yet."
        };
        el("div")
            .attr("id", "rows-empty")
            .child(Empty::new(message).icon(icons::INBOX))
    });
    let rows: Vec<Node> = list.rows.into_iter().map(|row| item(row).into()).collect();
    let mut region = Region::new("rows").children((
        (!list.error.is_empty()).then(|| Alert::new(AlertTone::Danger).title(list.error)),
        el("nav")
            .attr("aria-label", "Entries, newest first")
            .child(el("div").attr("id", "rows-body").child(rows)),
        empty,
    ));
    if let Some(live) = list.live_href {
        region = region.src(live).poll(LIVE).feed(
            "rows-body",
            i64::try_from(list.latest).unwrap_or(i64::MAX),
            Some("rows-empty"),
        );
    }
    let mut pane = Stack::gap(3).attr("data-pane", "").children((
        region,
        list.next_href.map(|href| {
            Button::new("Older entries")
                .variant(ButtonVariant::Outline)
                .size(Size::Sm)
                .href(href)
        }),
    ));
    if list.entry_open {
        pane = pane.attr("data-hide-mobile", "");
    }
    pane.into()
}
