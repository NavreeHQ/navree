//! The open conversation: its heading with what an agent does to it, everything
//! said in it day by day, and the composer.
//!
//! The heading and the messages are regions the stream refreshes. The composer is
//! neither: a new message never touches what an agent is typing.

use chrono::NaiveDate;
use navree::ui::dates::DatePicker;
use navree::ui::*;
use serde::Deserialize;

use crate::data::{
    Attachment as FileRow, Author, Channel, Conversation, Customer, Message as MessageRow,
    Priority, SAVED, Sla, Status, message_cursor,
};
use crate::views::{Filters, file_icon, file_size, thousands, written};

/// The heading's region.
pub const HEAD: &str = "thread-head";
/// The messages' region.
pub const MESSAGES: &str = "thread-messages";
/// The composer form.
pub const COMPOSER: &str = "composer";
/// The composer's text.
pub const COMPOSER_BODY: &str = "composer-body";
/// The hidden field the command palette puts a saved reply's key in.
pub const COMPOSER_SAVED: &str = "composer-saved";

/// What the composer posts: a reply or a note, or a saved reply to put in the text.
#[derive(Debug, Default, Deserialize, Validate, Form)]
#[form(id = "composer")]
pub struct ReplyForm {
    /// Reply or note
    #[validate(skip)]
    pub kind: Option<String>,
    /// Message
    #[validate(skip)]
    pub body: Option<String>,
    /// Saved reply chosen in the command palette
    #[validate(skip)]
    pub saved: Option<String>,
    /// Saved reply chosen in the composer
    #[validate(skip)]
    pub insert: Option<String>,
}

/// What the assign form posts.
#[derive(Debug, Default, Deserialize, Validate, Form)]
#[form(id = "assign")]
pub struct AssignForm {
    /// Assign to
    #[validate(skip)]
    pub agent: Option<String>,
}

/// What the snooze form posts: a day, or a quick choice.
#[derive(Debug, Default, Deserialize, Validate, Form)]
#[form(id = "snooze")]
pub struct SnoozeForm {
    /// Snooze until
    #[validate(skip)]
    pub until: Option<NaiveDate>,
    /// Quick choice
    #[validate(skip)]
    pub preset: Option<String>,
}

/// A form with nothing in it but its post: close, reopen, take it.
#[derive(Debug, Default, Deserialize, Validate, Form)]
#[form(id = "act")]
pub struct ActForm {
    /// Where it came from
    #[validate(skip)]
    pub from: Option<String>,
}

/// The latest day a conversation can be snoozed to, from today.
pub const SNOOZE_DAYS: i64 = 90;

/// Everything about the open conversation the heading needs.
pub struct Open<'a> {
    pub conversation: &'a Conversation,
    pub customer: &'a Customer,
    pub assignee: Option<(i64, String)>,
    pub filters: &'a Filters,
    pub now: i64,
    pub today: NaiveDate,
    /// The conversations either side of this one in the view: newer, then older.
    pub neighbours: (Option<i64>, Option<i64>),
}

/// A form that posts to `action`, through the runtime when it runs.
fn post_form(id: &str, action: String) -> Element {
    el("form")
        .attr("id", id.to_owned())
        .attr("method", "post")
        .attr("action", action)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
}

/// Assigning: someone found by typing, searched on the server. With no script, a link
/// to a page that lists the team.
fn assign(open: &Open<'_>) -> Popover {
    let id = open.conversation.id;
    let mut picker = Combobox::new("agent")
        .id("assign-agent")
        .search("/inbox/agents/options")
        .placeholder("Search the team")
        .clearable();
    if let Some((agent, name)) = &open.assignee {
        picker = picker.selected_choice((agent.to_string(), name.clone()));
    }
    let label = open
        .assignee
        .as_ref()
        .map_or_else(|| "Assign".to_owned(), |(_, name)| name.clone());
    Popover::new("assign-popover", label.clone())
        .label(format!("{label}, assign this conversation"))
        .variant(ButtonVariant::Outline)
        .size(Size::Sm)
        .align(PopupAlign::Start)
        .body(
            post_form("assign", format!("/inbox/{id}/assign")).child(
                Stack::gap(3).children((
                    Field::new("assign-agent-input", "Assign to").control(picker),
                    Row::gap(2).wrap().children((
                        Button::primary("Assign").submit().size(Size::Sm),
                        Button::ghost("Assign to me")
                            .submit()
                            .size(Size::Sm)
                            .attr("formaction", format!("/inbox/{id}/assign/me")),
                    )),
                    only_without_script(
                        Button::new("Choose from the whole team")
                            .variant(ButtonVariant::Link)
                            .href(format!("/inbox/{id}/assign")),
                    ),
                )),
            ),
        )
}

/// The status menu: every other status, posted.
fn status_menu(conversation: &Conversation) -> DropdownMenu {
    let now = Status::parse(&conversation.status).unwrap_or(Status::Open);
    let mut menu = DropdownMenu::button(
        "status-menu",
        Button::outline(now.label())
            .icon(now.icon())
            .size(Size::Sm)
            .attr("aria-label", format!("{} status, change it", now.label())),
    )
    .label("Change the status")
    .caret();
    for status in [Status::Open, Status::Pending, Status::Closed] {
        if status != now {
            menu = menu.item(
                MenuItem::post(
                    format!("Mark {}", status.label().to_lowercase()),
                    format!("/inbox/{}/status/{}", conversation.id, status.key()),
                )
                .icon(status.icon()),
            );
        }
    }
    menu
}

/// The priority menu.
fn priority_menu(conversation: &Conversation) -> DropdownMenu {
    let now = Priority::parse(&conversation.priority).unwrap_or(Priority::Normal);
    let mut menu = DropdownMenu::button(
        "priority-menu",
        Button::outline(now.label())
            .icon(now.icon())
            .size(Size::Sm)
            .attr("aria-label", format!("{} priority, change it", now.label())),
    )
    .label("Change the priority")
    .caret();
    for priority in Priority::ALL {
        let item = MenuItem::post(
            priority.label(),
            format!("/inbox/{}/priority/{}", conversation.id, priority.key()),
        )
        .icon(priority.icon());
        menu = menu.item(item.disabled(priority == now));
    }
    menu
}

/// Snoozing: a day from a calendar, or a quick choice.
fn snooze(open: &Open<'_>) -> Popover {
    let id = open.conversation.id;
    let tomorrow = open.today.succ_opt().unwrap_or(open.today);
    let last = open
        .today
        .checked_add_signed(chrono::Duration::days(SNOOZE_DAYS))
        .unwrap_or(open.today);
    Popover::new("snooze-popover", "Snooze")
        .label("Snooze, put this conversation aside until a day")
        .variant(ButtonVariant::Outline)
        .size(Size::Sm)
        .align(PopupAlign::Start)
        .body(
            post_form("snooze", format!("/inbox/{id}/snooze")).child(
                Stack::gap(3).children((
                    Row::gap(2).wrap().children((
                        Button::outline("Tomorrow")
                            .submit()
                            .size(Size::Sm)
                            .named("preset", "tomorrow"),
                        Button::outline("Next week")
                            .submit()
                            .size(Size::Sm)
                            .named("preset", "next-week"),
                    )),
                    Field::new("snooze-until", "Or until").control(
                        DatePicker::new("until")
                            .today(open.today)
                            .min(tomorrow)
                            .max(last)
                            .placeholder("Pick a day"),
                    ),
                    Button::primary("Snooze").submit().size(Size::Sm),
                )),
            ),
        )
}

/// Close, or reopen: one key does it.
fn close_or_reopen(conversation: &Conversation) -> Node {
    let closed = conversation.status == Status::Closed.key();
    let (to, label, glyph) = if closed {
        ("open", "Reopen", icons::ROTATE_CCW)
    } else {
        ("closed", "Close", icons::CIRCLE_CHECK)
    };
    post_form(
        "close-form",
        format!("/inbox/{}/status/{to}", conversation.id),
    )
    .child(
        Button::new(label)
            .submit()
            .icon(glyph)
            .size(Size::Sm)
            .hotkey("e"),
    )
    .into()
}

/// The heading: the subject, who and where from, what stands, and what to do.
pub fn thread_head(open: &Open<'_>) -> Node {
    let conversation = open.conversation;
    let channel = Channel::parse(&conversation.channel).unwrap_or(Channel::Chat);
    let status = Status::parse(&conversation.status).unwrap_or(Status::Open);
    let priority = Priority::parse(&conversation.priority).unwrap_or(Priority::Normal);
    let here = format!("/inbox/{}", conversation.id);
    let mut customer_link = open.filters.href(&here);
    customer_link.push_str(if customer_link.contains('?') {
        "&pane=customer"
    } else {
        "?pane=customer"
    });
    let mut badges: Vec<Node> = vec![
        Badge::new(status.label()).tone(status.tone()).dot().into(),
        Badge::new(priority.label())
            .tone(priority.tone())
            .icon(priority.icon())
            .into(),
        Badge::new(channel.label())
            .icon(channel.icon())
            .variant(BadgeVariant::Outline)
            .into(),
    ];
    if let Some(sla) = Sla::of(conversation, open.now) {
        badges.push(
            Badge::new(sla.text())
                .tone(sla.tone())
                .icon(icons::CLOCK)
                .attr("id", "thread-sla")
                .into(),
        );
    }
    if let Some(day) = conversation.snoozed_until {
        badges.push(
            Badge::new(format!("Until {}", day.format("%-d %b")))
                .tone(Tone::Warning)
                .icon(icons::ALARM_CLOCK)
                .into(),
        );
    }
    let filters = open.filters;
    let step = |id: Option<i64>, glyph, label: &str, key: &str| {
        let button = Button::icon_only(glyph, label.to_owned())
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .hotkey(key);
        match id {
            Some(id) => button.href(filters.href(&format!("/inbox/{id}"))),
            None => button.disabled(true),
        }
    };
    Stack::gap(2)
        .children((
            Row::gap(1).justify(Justify::Between).children((
                SplitPane::stacked_only(
                    Button::ghost("Conversations")
                        .icon(icons::ARROW_LEFT)
                        .size(Size::Sm)
                        .href(open.filters.href("/inbox")),
                ),
                el("span"),
                Row::gap(1).children((
                    step(
                        open.neighbours.0,
                        icons::CHEVRON_UP,
                        "Newer conversation",
                        "k",
                    ),
                    step(
                        open.neighbours.1,
                        icons::CHEVRON_DOWN,
                        "Older conversation",
                        "j",
                    ),
                )),
            )),
            Heading::h2(conversation.subject.clone()).id("thread-subject"),
            Text::muted(format!(
                "{} at {} · #{} · {} message{}",
                open.customer.name,
                open.customer.company,
                conversation.id,
                thousands(u64::try_from(conversation.messages).unwrap_or(0)),
                if conversation.messages == 1 { "" } else { "s" }
            )),
            Row::gap(2).wrap().children(badges),
            Row::gap(2).wrap().children((
                assign(open),
                status_menu(conversation),
                priority_menu(conversation),
                snooze(open),
                close_or_reopen(conversation),
                SplitPane::toggle(
                    Button::ghost("Customer")
                        .icon(icons::PANEL_RIGHT)
                        .size(Size::Sm)
                        .href(customer_link),
                    "customer",
                ),
            )),
        ))
        .into()
}

/// The text of a message as paragraphs.
fn paragraphs(body: &str) -> Vec<Node> {
    body.split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| el("p").attr("dir", "auto").child(line.to_owned()).into())
        .collect()
}

/// One message: the customer's on one side, the team's on the other, notes set apart,
/// and what happened as a line between them.
fn message(message: &MessageRow, files: &[&FileRow], now: i64) -> Node {
    let author = Author::parse(&message.kind);
    if author == Author::Event {
        return el("div")
            .attr("id", format!("message-{}", message.id))
            .attr("data-author", "event")
            .child(Marker::new(
                MarkerVariant::Separator,
                format!("{} · {}", message.body, written(message.at, now)),
            ))
            .into();
    }
    let side = if author == Author::Customer {
        BubbleSide::Start
    } else {
        BubbleSide::End
    };
    let mut bubble = Bubble::new(side, paragraphs(&message.body));
    let who = match author {
        Author::Note => {
            bubble = bubble.note();
            Row::gap(1).align(Align::Center).children((
                icon(icons::LOCK),
                Text::new(format!("Internal note · {}", message.author)).inline(),
            ))
        }
        _ => Row::gap(1).children(Text::new(message.author.clone()).inline()),
    };
    let chips: Vec<Node> = files
        .iter()
        .map(|file| {
            Attachment::new(file.name.clone())
                .meta(file_size(file.bytes))
                .icon(file_icon(&file.name))
                .href(format!("/inbox/{}/files/{}", file.conversation_id, file.id))
                .into()
        })
        .collect();
    el("article")
        .attr("id", format!("message-{}", message.id))
        .attr("data-key", message.id)
        .attr("data-author", author.key())
        .attr(
            "aria-label",
            match author {
                Author::Note => format!("Internal note by {}", message.author),
                _ => format!("Message from {}", message.author),
            },
        )
        .child(
            Message::new(side)
                .avatar(Avatar::placeholder(message.author.clone()).size(Size::Sm))
                .header(who)
                .content((
                    bubble,
                    (!chips.is_empty()).then(|| Row::gap(2).wrap().children(chips)),
                ))
                .footer(el("time").child(written(message.at, now))),
        )
        .into()
}

/// One message as an item of the lazy list.
pub fn message_item(item: &MessageRow, files: &[FileRow], now: i64) -> LazyItem {
    let mine: Vec<&FileRow> = files
        .iter()
        .filter(|file| file.message_id == item.id)
        .collect();
    LazyItem::new(
        item.id.to_string(),
        message_cursor(item.id),
        message(item, &mine, now),
    )
}

/// The newest messages of a conversation and the window of them a page holds: older
/// ones load as the agent scrolls up, and new ones arrive at the end.
pub struct Window<'a> {
    pub messages: &'a [MessageRow],
    pub files: &'a [FileRow],
    pub older: bool,
    pub newer: bool,
}

/// The messages, oldest first: a lazy list that starts at the newest.
pub fn thread_lazy(
    conversation: &Conversation,
    window: &Window<'_>,
    stream: &str,
    now: i64,
) -> LazyList {
    let id = conversation.id;
    let mut list = LazyList::new(MESSAGES, "Messages", format!("/inbox/{id}/messages"))
        .order(LazyOrder::Ascending)
        .view(format!("conversation-{id}"))
        .gap(4)
        .items(
            window
                .messages
                .iter()
                .map(|item| message_item(item, window.files, now)),
        )
        .since(window.messages.last().map_or(0, |last| last.id).to_string())
        .live_from(stream.to_owned(), Some("thread"), LazyEnd::End)
        .max(400)
        .labels(LazyLabels {
            start: "Show earlier messages".to_owned(),
            end: "Show newer messages".to_owned(),
            loaded: "{n} more messages".to_owned(),
            ..LazyLabels::default()
        });
    if window.older
        && let Some(first) = window.messages.first()
    {
        list = list.more_before(format!("/inbox/{id}?older={}", message_cursor(first.id)));
    }
    if window.newer
        && let Some(last) = window.messages.last()
    {
        list = list.more_after(format!("/inbox/{id}?newer={}", message_cursor(last.id)));
    }
    list
}

/// The composer: a reply or an internal note, saved replies to start from, and Send.
/// `draft` is text to start with: a saved reply put in with no script.
pub fn composer(conversation: &Conversation, draft: &str, kind: &str, error: Option<&str>) -> Node {
    let id = conversation.id;
    let note = kind == "note";
    let saved = Popover::new("saved-replies", "Saved replies")
        .label("Put a saved reply in the message")
        .variant(ButtonVariant::Ghost)
        .size(Size::Sm)
        .side(Side::Top)
        .align(PopupAlign::Start)
        .body(
            Stack::gap(1).children(
                SAVED
                    .iter()
                    .map(|reply| {
                        Button::ghost(reply.title)
                            .submit()
                            .size(Size::Sm)
                            .named("insert", reply.key)
                            .into()
                    })
                    .collect::<Vec<Node>>(),
            ),
        );
    post_form(COMPOSER, format!("/inbox/{id}/reply"))
        .attr("aria-label", "Write to the customer")
        .child(
            Stack::gap(2).children((
                ToggleGroup::single("kind", "Send as")
                    .item(ToggleItem::new("reply", "Reply").icon(icons::SEND))
                    .item(ToggleItem::new("note", "Internal note").icon(icons::LOCK))
                    .value(if note { "note" } else { "reply" })
                    .size(Size::Sm),
                Field::new(COMPOSER_BODY, "Message")
                    .error_opt(error.map(str::to_owned))
                    .control(
                        Textarea::new("body")
                            .value(draft.to_owned())
                            .rows(3)
                            .placeholder("Write a reply, or press r from anywhere")
                            .attr("maxlength", 5000_i64)
                            .hotkey("r"),
                    ),
                el("input")
                    .attr("type", "hidden")
                    .attr("name", "saved")
                    .attr("id", COMPOSER_SAVED)
                    .attr("value", "")
                    .settable(),
                Row::gap(2).justify(Justify::Between).wrap().children((
                    saved,
                    Button::primary("Send")
                        .submit()
                        .icon(icons::SEND)
                        .size(Size::Sm),
                )),
            )),
        )
        .into()
}
