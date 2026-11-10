//! The conversations pane: the view's heading, the filters, a line that counts what is
//! waiting, and the list, which loads fifty conversations at a time as the agent scrolls
//! and merges what changes live.
//!
//! Each row is a link to its conversation, with what an agent scans for: who wrote,
//! about what, how long ago, what is new, how urgent, and how long is left to answer.

use navree::ui::*;

use crate::data::{Channel, Conversation, Priority, Sla, Status};
use crate::views::{Filters, View, ago, encode, only_first_name, thousands};

/// The list.
pub const LIST: &str = "conversation-list";
/// What the filters redraw: the count and the list.
pub const LIST_BODY: &str = "list-body";
/// The count.
pub const SUMMARY: &str = "list-summary";
/// How many conversations a page of the list holds.
pub const PAGE: u64 = 50;

/// A conversation as the list shows it.
#[derive(Clone, Debug, Default)]
pub struct ListRow {
    pub conversation: Conversation,
    pub customer: String,
    pub company: String,
    pub assignee: Option<String>,
}

/// What the line over the list says.
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    /// How many the view holds; none for the closed list, which isn't counted.
    pub total: Option<u64>,
    pub unread: u64,
    pub overdue: u64,
}

/// The heading and the filters. The filters are a form that works without script;
/// with it, typing narrows the list in place, and the address follows. `/` finds the
/// search, unless the agent turned single keys off (`keys`).
pub fn list_header(filters: &Filters, path: &str, keys: bool) -> Node {
    let mut search = Input::search("q")
        .value(filters.q.clone())
        .placeholder("Search subject, name or company")
        .attr("aria-label", "Search conversations");
    if keys {
        search = search.hotkey("/");
    }
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
                .attr("data-ui-target", LIST_BODY)
                .attr("data-ui-debounce", 250_i64)
                .children((
                    (filters.view != View::All).then(|| {
                        el("input")
                            .attr("type", "hidden")
                            .attr("name", "view")
                            .attr("value", filters.view.key())
                    }),
                    Stack::gap(2).children((
                        search,
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

/// How a clock stands, as a row marks it: `overdue`, `soon` or `running`.
pub fn sla_state(sla: Sla) -> &'static str {
    if sla.breached() {
        "overdue"
    } else if sla.at_risk() {
        "soon"
    } else {
        "running"
    }
}

/// What a row draws that moves with the clock alone: how long ago it last moved, and
/// the time left to answer with its state. A live list sends a row again when this
/// reads differently than when the server last drew it.
pub fn timed(conversation: &Conversation, now: i64) -> (String, Option<(String, &'static str)>) {
    (
        ago(conversation.last_at, now),
        Sla::of(conversation, now).map(|sla| (sla.text(), sla_state(sla))),
    )
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
                .attr("data-sla", sla_state(sla))
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

/// One row, as an item of the lazy list. `hotkey` makes a key open it.
pub fn conversation_item(
    row: &ListRow,
    filters: &Filters,
    open: Option<i64>,
    now: i64,
    hotkey: Option<&str>,
) -> LazyItem {
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
    LazyItem::new(conversation.id.to_string(), conversation.cursor(), item)
        .id(format!("conversation-{}", conversation.id))
}

/// The line that counts the view.
pub fn summary_line(summary: Summary, filters: &Filters) -> Node {
    let mut text = match summary.total {
        None => "Newest first".to_owned(),
        Some(total) => {
            let noun = if total == 1 {
                "conversation"
            } else {
                "conversations"
            };
            if filters.narrowed() {
                format!("{} {noun} match", thousands(total))
            } else {
                format!("{} {noun}", thousands(total))
            }
        }
    };
    if summary.unread > 0 {
        text.push_str(&format!(" · {} new", thousands(summary.unread)));
    }
    if summary.overdue > 0 {
        text.push_str(&format!(" · {} overdue", thousands(summary.overdue)));
    }
    Text::muted(text).into()
}

/// The count in its region: drawn again when the desk moves, and read out politely
/// when it changes.
pub fn summary_region(summary: Summary, filters: &Filters, stream: &str) -> Region {
    let query = filters.query();
    let src = if query.is_empty() {
        "/inbox/fragments/summary".to_owned()
    } else {
        format!("/inbox/fragments/summary?{query}")
    };
    Region::new(SUMMARY)
        .src(src)
        .live_from(stream.to_owned())
        .live_event("conversations")
        .children(
            el("div")
                .attr("role", "status")
                .attr("aria-live", "polite")
                .attr("aria-atomic", "true")
                .child(summary_line(summary, filters)),
        )
}

/// Where a list's rows come from: the same filters, as the view.
pub fn rows_src(filters: &Filters) -> String {
    let query = filters.query();
    if query.is_empty() {
        "/inbox/rows".to_owned()
    } else {
        format!("/inbox/rows?{query}")
    }
}

/// A page of the list, for its first draw: the rows, whether more come after, whether
/// this page starts somewhere down the list (with JavaScript off), and where a live ask
/// starts from (a [`Since`](crate::queries::Since), as text).
pub struct ListPage<'a> {
    pub rows: &'a [ListRow],
    pub more_after: bool,
    pub more_before: bool,
    pub since: String,
}

/// The list: lazy, live, and kept as the agent left it while they open conversations.
pub fn list_lazy(
    page: &ListPage<'_>,
    filters: &Filters,
    open: Option<i64>,
    now: i64,
    stream: &str,
) -> LazyList {
    let path = match open {
        Some(id) => format!("/inbox/{id}"),
        None => "/inbox".to_owned(),
    };
    let window = |param: &str, cursor: String| {
        let base = filters.href(&path);
        let sep = if base.contains('?') { '&' } else { '?' };
        format!("{base}{sep}{param}={}", encode(&cursor))
    };
    let mut list = LazyList::new(LIST, "Conversations", rows_src(filters))
        .view(filters.query())
        .items(
            page.rows
                .iter()
                .map(|row| conversation_item(row, filters, open, now, None)),
        )
        .since(page.since.clone())
        .live_from(stream.to_owned(), Some("conversations"), LazyEnd::Start)
        .max(300)
        .labels(LazyLabels {
            start: "Show newer conversations".to_owned(),
            end: "Show more conversations".to_owned(),
            loaded: "{n} more conversations".to_owned(),
            ..LazyLabels::default()
        })
        .empty(if filters.narrowed() {
            Empty::new("Nothing matches")
                .icon(icons::SEARCH)
                .description("No conversation in this view matches the search and filters.")
                .actions(
                    Button::outline("Clear the filters")
                        .href(Filters::with_view(filters.view).href("/inbox")),
                )
        } else {
            Empty::new("All caught up").icon(icons::INBOX).description(
                "Nothing is waiting in this view. New conversations show up here as they arrive.",
            )
        });
    if let Some(id) = open {
        list = list.current(id.to_string());
    }
    if page.more_after
        && let Some(last) = page.rows.last()
    {
        list = list.more_after(window("rows_after", last.conversation.cursor()));
    }
    if page.more_before
        && let Some(first) = page.rows.first()
    {
        list = list.more_before(window("rows_before", first.conversation.cursor()));
    }
    list
}
