//! The conversations pane: the view's heading, the filters, and the list that follows
//! the stream. Each row is a link to its conversation, with what an agent scans for:
//! who wrote, about what, how long ago, what is new, how urgent, and how long is left
//! to answer.

use navree::ui::*;

use crate::data::{Channel, Conversation, Priority, Sla, Status};
use crate::views::{Filters, View, ago, only_first_name};

/// The list's region.
pub const LIST: &str = "conversation-list";

/// A conversation as the list shows it.
#[derive(Clone, Debug, Default)]
pub struct ListRow {
    pub conversation: Conversation,
    pub customer: String,
    pub company: String,
    pub assignee: Option<String>,
}

/// The heading and the filters. The filters are a form that works without script;
/// with it, typing narrows the list in place, and the address follows.
pub fn list_header(filters: &Filters, path: &str) -> Node {
    let channels = std::iter::once(OptionItem::new("", "All channels")).chain(
        Channel::ALL
            .iter()
            .map(|channel| OptionItem::new(channel.key(), channel.label())),
    );
    let priorities = std::iter::once(OptionItem::new("", "Any priority")).chain(
        Priority::ALL
            .iter()
            .map(|priority| OptionItem::new(priority.key(), priority.label())),
    );
    Stack::gap(2)
        .children((
            Heading::h2(filters.view.label()),
            el("form")
                .attr("method", "get")
                .attr("action", path.to_owned())
                .attr("role", "search")
                .attr("aria-label", "Filter conversations")
                .attr("data-ui-target", LIST)
                .attr("data-ui-debounce", 250_i64)
                .children((
                    (filters.view != View::All).then(|| {
                        el("input")
                            .attr("type", "hidden")
                            .attr("name", "view")
                            .attr("value", filters.view.key())
                    }),
                    Stack::gap(2).children((
                        Input::search("q")
                            .value(filters.q.clone())
                            .placeholder("Search subject, name or company")
                            .attr("aria-label", "Search conversations")
                            .hotkey("/"),
                        Grid::cols(2).gap(2).children((
                            NativeSelect::new("channel")
                                .options(channels)
                                .selected(filters.channel.clone())
                                .attr("aria-label", "Channel"),
                            NativeSelect::new("priority")
                                .options(priorities)
                                .selected(filters.priority.clone())
                                .attr("aria-label", "Priority"),
                        )),
                        only_without_script(Button::outline("Filter").submit().size(Size::Sm)),
                    )),
                )),
        ))
        .into()
}

/// The badges under a row: how urgent, the clock, where it stands, what is new, and
/// whose it is.
fn badges(row: &ListRow, now: i64) -> Vec<Node> {
    let conversation = &row.conversation;
    let mut out: Vec<Node> = Vec::with_capacity(5);
    let priority = Priority::parse(&conversation.priority).unwrap_or(Priority::Normal);
    if matches!(priority, Priority::Urgent | Priority::High) {
        out.push(
            Badge::new(priority.label())
                .tone(priority.tone())
                .icon(priority.icon())
                .into(),
        );
    }
    if let Some(sla) = Sla::of(conversation, now) {
        out.push(
            Badge::new(sla.text())
                .tone(sla.tone())
                .icon(icons::CLOCK)
                .attr("data-sla", if sla.breached() { "overdue" } else if sla.at_risk() { "soon" } else { "running" })
                .into(),
        );
    }
    let status = Status::parse(&conversation.status).unwrap_or(Status::Open);
    if status != Status::Open {
        out.push(Badge::new(status.label()).tone(status.tone()).dot().into());
    }
    if conversation.unread > 0 {
        out.push(
            Badge::new(format!("{} new", conversation.unread))
                .tone(Tone::Info)
                .attr("data-unread", conversation.unread)
                .into(),
        );
    }
    out.push(match &row.assignee {
        Some(name) => Badge::new(only_first_name(name))
            .icon(icons::USER_ROUND)
            .variant(BadgeVariant::Outline)
            .into(),
        None => Badge::new("Unassigned")
            .variant(BadgeVariant::Outline)
            .into(),
    });
    out
}

/// One row. `hotkey` is `j` for the row after the open one and `k` for the one before.
pub fn conversation_row(
    row: &ListRow,
    filters: &Filters,
    open: Option<i64>,
    now: i64,
    hotkey: Option<&str>,
) -> Node {
    let conversation = &row.conversation;
    let channel = Channel::parse(&conversation.channel).unwrap_or(Channel::Chat);
    let mut item = Item::new(row.customer.clone())
        .href(filters.href(&format!("/inbox/{}", conversation.id)))
        .media(icon_labelled(channel.icon(), channel.label()))
        .aside(ago(conversation.last_at, now))
        .description(conversation.subject.clone())
        .footer(badges(row, now))
        .current(open == Some(conversation.id))
        .unread(conversation.unread > 0)
        .truncate();
    if let Some(key) = hotkey {
        item = item.hotkey(key);
    }
    el("li")
        .attr("id", format!("conversation-{}", conversation.id))
        .attr("data-key", conversation.id)
        .child(item)
        .into()
}

/// What the list holds right now: a line that counts it, and the rows.
pub fn list_body(rows: &[ListRow], filters: &Filters, open: Option<i64>, now: i64) -> Node {
    let at = open.and_then(|id| rows.iter().position(|row| row.conversation.id == id));
    let key_for = |index: usize| match at {
        Some(at) if index + 1 == at => Some("k"),
        Some(at) if index == at + 1 => Some("j"),
        None if index == 0 => Some("j"),
        _ => None,
    };
    let unread: i64 = rows.iter().map(|row| row.conversation.unread).sum();
    let overdue = rows
        .iter()
        .filter(|row| Sla::of(&row.conversation, now).is_some_and(Sla::breached))
        .count();
    let mut summary = format!(
        "{} conversation{}",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    );
    if unread > 0 {
        summary.push_str(&format!(" · {unread} new"));
    }
    if overdue > 0 {
        summary.push_str(&format!(" · {overdue} overdue"));
    }
    if rows.is_empty() {
        let empty = if filters.narrowed() {
            Empty::new("Nothing matches")
                .icon(icons::SEARCH)
                .description("No conversation in this view matches the search and filters.")
                .actions(Button::outline("Clear the filters").href(Filters::with_view(filters.view).href("/inbox")))
        } else {
            Empty::new("All caught up")
                .icon(icons::INBOX)
                .description("Nothing is waiting in this view. New conversations show up here as they arrive.")
        };
        return Stack::gap(2)
            .children((Text::muted(summary).attr("id", "list-summary"), empty))
            .into();
    }
    Stack::gap(2)
        .children((
            Text::muted(summary).attr("id", "list-summary"),
            el("ul")
                .slot("stack")
                .attr("data-gap", "1")
                .attr("aria-label", "Conversations")
                .children(
                    rows.iter()
                        .enumerate()
                        .map(|(index, row)| conversation_row(row, filters, open, now, key_for(index)))
                        .collect::<Vec<Node>>(),
                ),
        ))
        .into()
}

/// The list in its region: fetched again when the stream says the desk moved.
pub fn list_region(
    rows: &[ListRow],
    filters: &Filters,
    open: Option<i64>,
    now: i64,
    stream: &str,
) -> Region {
    let query = filters.query();
    let base = match open {
        Some(id) => format!("/inbox/fragments/list?open={id}"),
        None => "/inbox/fragments/list".to_owned(),
    };
    let src = match (query.is_empty(), open.is_some()) {
        (true, _) => base,
        (false, true) => format!("{base}&{query}"),
        (false, false) => format!("{base}?{query}"),
    };
    Region::new(LIST)
        .src(src)
        .live_from(stream.to_owned())
        .live_event("conversations")
        .children(list_body(rows, filters, open, now))
}
