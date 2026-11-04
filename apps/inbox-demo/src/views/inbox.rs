//! The inbox as one page: three panes, the command palette and the keys.

use chrono::NaiveDate;
use navree::ui::*;

use crate::data::{
    Agent, Attachment, Conversation, Customer, CustomerEvent, Message, Priority, SAVED, Status,
};
use crate::views::customer::{CUSTOMER, customer_body, customer_head};
use crate::views::list::{LIST_BODY, ListPage, Summary, list_header, list_lazy, summary_region};
use crate::views::thread::{
    ActForm, COMPOSER, COMPOSER_BODY, COMPOSER_SAVED, HEAD, Open, Window, composer, thread_head,
    thread_lazy,
};
use crate::views::{Filters, View};

/// The split's id: what a save says it is for.
pub const SPLIT: &str = "inbox";

/// The three panes and their rules, with nothing in them: the page draws it, and a
/// save is read against it.
pub fn layout() -> SplitPane {
    SplitPane::new(SPLIT)
        .height(SplitHeight::Screen)
        .stack_below(SplitWidth::Md)
        .save_to("/inbox/panes")
        .pane(
            Pane::new("conversations", "Conversations")
                .size(30)
                .min(22)
                .max(45),
        )
        .pane(Pane::fill("thread", "Conversation").min(30))
        .pane(
            Pane::new("customer", "Customer")
                .size(26)
                .min(18)
                .max(40)
                .collapsible()
                .collapse_below(SplitWidth::Lg),
        )
}

/// A conversation that is open, with the part of it a page shows.
pub struct OpenThread {
    pub conversation: Conversation,
    pub customer: Customer,
    pub assignee: Option<(i64, String)>,
    /// The window of messages: the newest, unless the page asked for older ones.
    pub messages: Vec<Message>,
    pub files: Vec<Attachment>,
    pub older: bool,
    pub newer: bool,
    /// The conversations either side of it in the view: newer, then older.
    pub neighbours: (Option<i64>, Option<i64>),
}

/// What the customer pane shows, loaded after the page.
pub struct CustomerView {
    pub customer: Customer,
    pub history: Vec<CustomerEvent>,
    /// Their conversations, newest first.
    pub theirs: Vec<Conversation>,
    /// How many are open, and how many of those are past due.
    pub open: u64,
    pub overdue: u64,
}

/// The page's state.
pub struct Inbox<'a> {
    pub filters: &'a Filters,
    pub list: ListPage<'a>,
    pub summary: Summary,
    pub open: Option<&'a OpenThread>,
    /// The customer pane's content, when the page draws it at once; otherwise the
    /// pane loads after the page.
    pub customer: Option<&'a CustomerView>,
    /// Which pane the page is about: the one shown when the panes stack.
    pub pane: &'a str,
    pub now: i64,
    pub today: NaiveDate,
    /// What the composer starts with.
    pub draft: &'a str,
    pub kind: &'a str,
    pub error: Option<&'a str>,
    /// The agent at the desk.
    pub me: i64,
    /// Whether they have single-key shortcuts on.
    pub keys: bool,
}

/// The page's heading: the open conversation's subject, or the view.
pub fn page_title(page: &Inbox<'_>) -> String {
    page.open.map_or_else(
        || page.filters.view.label().to_owned(),
        |thread| thread.conversation.subject.clone(),
    )
}

/// The stream a page listens to.
pub fn stream_url(open: Option<&OpenThread>) -> String {
    match open {
        Some(thread) => format!(
            "/inbox/events?open={}&customer={}",
            thread.conversation.id, thread.customer.id
        ),
        None => "/inbox/events".to_owned(),
    }
}

/// The address of the page, without the pane.
fn page_path(open: Option<&OpenThread>) -> String {
    open.map_or_else(
        || "/inbox".to_owned(),
        |thread| format!("/inbox/{}", thread.conversation.id),
    )
}

fn query_of(filters: &Filters) -> String {
    let query = filters.query();
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

/// The heading region of the open conversation.
pub fn head_region(open: &Open<'_>) -> Region {
    Region::new(HEAD)
        .src(format!(
            "/inbox/{}/fragments/head{}",
            open.conversation.id,
            query_of(open.filters)
        ))
        .live_from(format!(
            "/inbox/events?open={}&customer={}",
            open.conversation.id, open.customer.id
        ))
        .live_event("thread")
        .children(thread_head(open))
}

/// The customer region: fetched after the page, and again when the customer changes.
pub fn customer_region(
    conversation: i64,
    customer: i64,
    filters: &Filters,
    view: Option<&CustomerView>,
    now: i64,
) -> Region {
    let region = Region::new(CUSTOMER)
        .src(format!(
            "/inbox/{conversation}/fragments/customer{}",
            query_of(filters)
        ))
        .live_from(format!(
            "/inbox/events?open={conversation}&customer={customer}"
        ))
        .live_event("customer");
    match view {
        Some(view) => region.children(customer_body(view, conversation, filters, now)),
        None => region.lazy(),
    }
}

/// The count and the list, which the filters redraw together.
pub fn list_body(page: &Inbox<'_>, stream: &str) -> Node {
    el("div")
        .attr("id", LIST_BODY)
        .children((
            summary_region(page.summary, page.filters, stream),
            list_lazy(
                &page.list,
                page.filters,
                page.open.map(|thread| thread.conversation.id),
                page.now,
                stream,
            ),
        ))
        .into()
}

/// The panes, drawn with the agent's sizes.
pub fn inbox_split(page: &Inbox<'_>, sizes: Option<&SplitSizes>) -> SplitPane {
    let path = page_path(page.open);
    let stream = stream_url(page.open);
    let mut split = layout();
    if let Some(sizes) = sizes {
        split = split.sizes(sizes);
    }
    let split = split.current(page.pane).pane_with("conversations", |pane| {
        pane.header(list_header(page.filters, &path, page.keys))
            .body(list_body(page, &stream))
    });
    match page.open {
        Some(thread) => {
            let open = Open {
                conversation: &thread.conversation,
                customer: &thread.customer,
                assignee: thread.assignee.clone(),
                filters: page.filters,
                now: page.now,
                today: page.today,
                neighbours: thread.neighbours,
                keys: page.keys,
            };
            let back = page.filters.href(&path);
            let window = Window {
                messages: &thread.messages,
                files: &thread.files,
                older: thread.older,
                newer: thread.newer,
            };
            split
                .pane_with("thread", |pane| {
                    pane.header(head_region(&open))
                        .body(thread_lazy(
                            &thread.conversation,
                            &window,
                            &stream,
                            page.now,
                            page.me,
                        ))
                        .from_end()
                        .body_label("Messages")
                        .footer(composer(
                            &thread.conversation,
                            page.draft,
                            page.kind,
                            page.error,
                            page.keys,
                        ))
                })
                .pane_with("customer", |pane| {
                    pane.header(customer_head(&thread.customer, &back)).body(customer_region(
                        thread.conversation.id,
                        thread.customer.id,
                        page.filters,
                        page.customer,
                        page.now,
                    ))
                })
        }
        None => split
            .pane_with("thread", |pane| {
                let first = page.list.rows.first().map(|row| {
                    // j opens the first; on a conversation's page, j and k are its neighbours.
                    let button = Button::outline("Open the first")
                        .href(page.filters.href(&format!("/inbox/{}", row.conversation.id)));
                    if page.keys { button.hotkey("j") } else { button }
                });
                let empty = Empty::new("Choose a conversation")
                    .icon(icons::MESSAGES_SQUARE)
                    .description(if page.keys {
                        "Pick one from the list, or press j to open the first. Press ? for every key."
                    } else {
                        "Pick one from the list."
                    });
                pane.body(match first {
                    Some(button) => empty.actions(button),
                    None => empty,
                })
            })
            .pane_with("customer", |pane| {
                pane.body(
                    Empty::new("No customer yet")
                        .icon(icons::USER_ROUND)
                        .description("Their plan, history and tags show here with a conversation."),
                )
            }),
    }
}

/// The command palette: the open conversation's actions, saved replies, and places.
pub fn palette(page: &Inbox<'_>) -> Command {
    let mut command = Command::new("palette")
        .title("Command palette")
        .placeholder("Type an action or a saved reply")
        .shortcut(true);
    if let Some(thread) = page.open {
        let id = thread.conversation.id;
        let status = Status::parse(&thread.conversation.status).unwrap_or(Status::Open);
        let mut actions = CommandGroup::new("This conversation")
            .item(
                CommandItem::run("Reply", Cmd::focus(COMPOSER_BODY))
                    .icon(icons::SEND)
                    .description("Write to the customer")
                    .not_recent(),
            )
            .item(
                CommandItem::post("Assign to me", format!("/inbox/{id}/assign/me"))
                    .icon(icons::USER_CHECK)
                    .keywords(["take", "mine"]),
            );
        actions = if status == Status::Closed {
            actions.item(
                CommandItem::post("Reopen", format!("/inbox/{id}/status/open"))
                    .icon(icons::ROTATE_CCW)
                    .not_recent(),
            )
        } else {
            actions
                .item(
                    CommandItem::post("Close", format!("/inbox/{id}/status/closed"))
                        .icon(icons::CIRCLE_CHECK)
                        .keywords(["resolve", "done"])
                        .not_recent(),
                )
                .item(
                    CommandItem::post("Mark pending", format!("/inbox/{id}/status/pending"))
                        .icon(icons::HOURGLASS)
                        .keywords(["waiting"])
                        .not_recent(),
                )
                .item(
                    CommandItem::post(
                        "Snooze until tomorrow",
                        format!("/inbox/{id}/snooze/tomorrow"),
                    )
                    .icon(icons::ALARM_CLOCK)
                    .not_recent(),
                )
        };
        for priority in [
            Priority::Urgent,
            Priority::High,
            Priority::Normal,
            Priority::Low,
        ] {
            if priority.key() != thread.conversation.priority {
                actions = actions.item(
                    CommandItem::post(
                        format!("Set priority to {}", priority.label().to_lowercase()),
                        format!("/inbox/{id}/priority/{}", priority.key()),
                    )
                    .icon(priority.icon())
                    .not_recent(),
                );
            }
        }
        command = command.group(actions);
        let mut replies = CommandGroup::new("Saved replies");
        for reply in &SAVED {
            replies = replies.item(
                CommandItem::run(
                    reply.title,
                    vec![Cmd::value(COMPOSER_SAVED, reply.key), Cmd::submit(COMPOSER)],
                )
                .id(format!("saved-{}", reply.key))
                .icon(icons::MESSAGE_SQUARE_TEXT)
                .description(crate::data::preview_of(reply.body))
                .keywords(["saved", "reply", "template"]),
            );
        }
        command = command.group(replies);
    }
    let mut places = CommandGroup::new("Go to");
    for view in View::ALL {
        places = places.item(
            CommandItem::link(view.label(), Filters::with_view(view).href("/inbox"))
                .icon(view.icon()),
        );
    }
    places = places.item(
        CommandItem::run("Keyboard shortcuts", Cmd::open("shortcuts"))
            .icon(icons::KEYBOARD)
            .not_recent(),
    );
    command.group(places)
}

/// Where the agent turns single-key shortcuts on or off.
pub const KEYS_PATH: &str = "/inbox/keys";

/// The keys, in a dialog `?` opens, and the switch that turns single keys off for the
/// agent: some people can't use them (a speech input types letters), so each person
/// chooses, and the server keeps the choice.
pub fn shortcuts(keys: bool) -> Dialog {
    let row = |keys: &[&'static str], what: &'static str| {
        TableRowEl::new((
            TableCell::new(
                Row::gap(1).children(
                    keys.iter()
                        .map(|key| Kbd::new(*key).into())
                        .collect::<Vec<Node>>(),
                ),
            ),
            TableCell::new(what),
        ))
    };
    let switch = el("form")
        .attr("id", "keys-form")
        .attr("method", "post")
        .attr("action", KEYS_PATH)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .children((
            el("input")
                .attr("type", "hidden")
                .attr("name", "from")
                .attr("value", if keys { "off" } else { "on" }),
            Button::outline(if keys {
                "Turn single keys off"
            } else {
                "Turn single keys on"
            })
            .submit()
            .size(Size::Sm),
        ));
    Dialog::new("shortcuts", "Keyboard shortcuts")
        .description(if keys {
            "Single keys work while you aren't typing in a field; e works only inside the open conversation. Turn them off if you use speech input or press keys by accident."
        } else {
            "Single keys are off for you. Ctrl+K and Esc still work."
        })
        .body(Stack::gap(4).children((
            Table::new()
                .caption("Keyboard shortcuts")
                .head((TableHead::new("Key"), TableHead::new("Does")))
                .rows([
                    row(&["j"], "Open the next, older conversation in the list"),
                    row(&["k"], "Open the newer conversation above"),
                    row(&["r"], "Write a reply"),
                    row(&["e"], "Close the open conversation, or reopen it"),
                    row(&["/"], "Search the conversations"),
                    row(&["Ctrl", "K"], "Open the command palette"),
                    row(&["?"], "Show these keys"),
                    row(&["Esc"], "Close a dialog or a menu"),
                ]),
            switch,
        )))
        .footer(Button::new("Close").closes_dialog("shortcuts"))
}

/// What the switch posts.
pub type KeysForm = ActForm;

/// The buttons beside the page's heading: the palette and the keys.
pub fn page_actions(keys: bool) -> Node {
    let shortcuts = Button::icon_only(icons::KEYBOARD, "Keyboard shortcuts")
        .variant(ButtonVariant::Ghost)
        .size(Size::Sm)
        .opens_dialog("shortcuts");
    Row::gap(2)
        .children((
            Command::trigger("palette", "Actions"),
            if keys {
                shortcuts.hotkey("?")
            } else {
                shortcuts
            },
        ))
        .into()
}

/// The page with JavaScript off that assigns a conversation: the team, searched by a
/// form, each with a button.
pub fn assign_page(conversation: &Conversation, team: &[Agent], q: &str, back: &str) -> Node {
    let rows: Vec<Node> = team
        .iter()
        .map(|agent| {
            el("li")
                .child(
                    el("form")
                        .attr("method", "post")
                        .attr("action", format!("/inbox/{}/assign", conversation.id))
                        .flag("data-ui-form", true)
                        .uses(Uses::CORE)
                        .child(
                            Item::new(agent.name.clone())
                                .description(agent.team.clone())
                                .media(Avatar::placeholder(agent.name.clone()).size(Size::Sm))
                                .actions(
                                    Button::outline("Assign")
                                        .submit()
                                        .size(Size::Sm)
                                        .named("agent", agent.id.to_string())
                                        .attr("aria-label", format!("Assign to {}", agent.name)),
                                ),
                        ),
                )
                .into()
        })
        .collect();
    Stack::gap(4)
        .children((
            Text::muted(format!("Who should answer “{}”?", conversation.subject)),
            el("form")
                .attr("method", "get")
                .attr("role", "search")
                .attr("aria-label", "Search the team")
                .child(
                    Row::gap(2).children((
                        Input::search("q")
                            .value(q.to_owned())
                            .placeholder("Search the team")
                            .attr("aria-label", "Search the team"),
                        Button::outline("Search").submit(),
                    )),
                ),
            if rows.is_empty() {
                Node::from(
                    Empty::new("Nobody matches").description("Try part of a name or a team."),
                )
            } else {
                el("ul")
                    .slot("stack")
                    .attr("data-gap", "1")
                    .child(rows)
                    .into()
            },
            Button::ghost("Back to the conversation")
                .icon(icons::ARROW_LEFT)
                .href(back.to_owned()),
        ))
        .into()
}
