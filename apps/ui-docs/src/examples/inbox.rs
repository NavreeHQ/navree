//! The support inbox's own views, drawn from its seed: what `/ui/` and the inbox's
//! showcase page show. The code beside each preview is the app's source, read from its
//! files, so the website shows what the app runs.

use inbox_demo::data::{self, Agent, Attachment, Conversation, Customer, CustomerEvent, Message};
use inbox_demo::views::inbox::{
    CustomerView, Inbox, OpenThread, inbox_split, page_actions, palette, shortcuts,
};
use inbox_demo::views::list::{ListPage, ListRow, Summary, conversation_item};
use inbox_demo::views::thread::message_item;
use inbox_demo::views::{Filters, View, ViewCounts, shell};
use navree::ui::*;

use crate::catalog::{self, Example};

/// The moment the previews are drawn at, fixed so the files don't change:
/// 4 November 2026, 12:00 in London.
const NOW: i64 = 1_793_793_600;

const INBOX_RS: &str = include_str!("../../../inbox-demo/src/views/inbox.rs");
const LIST_RS: &str = include_str!("../../../inbox-demo/src/views/list.rs");
const THREAD_RS: &str = include_str!("../../../inbox-demo/src/views/thread.rs");
const QUERIES_RS: &str = include_str!("../../../inbox-demo/src/queries.rs");

pub fn all() -> Vec<Example> {
    vec![
        Example::new(
            "build-anything",
            "inbox-panes",
            "The support inbox in three panes",
            catalog::Kind::Single(panes),
            [
                items(INBOX_RS, &["pub fn layout", "pub fn inbox_split"]),
                items(LIST_RS, &["pub fn list_lazy"]),
                items(THREAD_RS, &["pub fn thread_lazy"]),
                items(QUERIES_RS, &["pub async fn list_page"]),
            ]
            .join("\n\n"),
        )
        .fill()
        .width(1440)
        .height(860)
        .streaming("on"),
        Example::new(
            "build-anything",
            "inbox-thread",
            "A conversation of 100 messages, opened at its newest",
            catalog::Kind::Single(long_thread),
            [
                items(THREAD_RS, &["pub fn thread_lazy"]),
                items(QUERIES_RS, &["pub async fn thread_window"]),
            ]
            .join(
                "

",
            ),
        )
        .fill()
        .width(1440)
        .height(860)
        .streaming("on"),
        Example::new(
            "build-anything",
            "inbox-phone",
            "The inbox on a phone",
            catalog::Kind::Single(phone),
            items(INBOX_RS, &["pub fn layout"]),
        )
        .fill()
        .width(390)
        .height(780)
        .streaming("on"),
        Example::new(
            "build-anything",
            "inbox-palette",
            "The command palette's actions and saved replies",
            catalog::Kind::Single(palette_open),
            items(INBOX_RS, &["pub fn palette"]),
        )
        .show()
        .height(620),
    ]
}

/// Top-level items of `source` as written, each from its attributes to its closing
/// brace, one after another.
fn items(source: &str, starts: &[&str]) -> String {
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
        while first > 0
            && (lines[first - 1].starts_with("#[") || lines[first - 1].starts_with("///"))
        {
            first -= 1;
        }
        let end = (at..lines.len())
            .find(|line| lines[*line] == "}")
            .unwrap_or_else(|| panic!("`{start}` doesn't end"));
        out.push(lines[first..=end].join("\n"));
    }
    out.join("\n\n")
}

/// The hand-written desk, as the app seeds it at `NOW`.
struct Desk {
    agents: Vec<Agent>,
    customers: Vec<Customer>,
    conversations: Vec<Conversation>,
    messages: Vec<Message>,
    files: Vec<Attachment>,
    events: Vec<CustomerEvent>,
}

fn desk() -> Desk {
    let seed = data::seed(NOW);
    Desk {
        agents: seed.agents,
        customers: seed.customers,
        conversations: seed.conversations,
        messages: seed.messages,
        files: seed.attachments,
        events: seed.events,
    }
}

/// The open list, newest activity first, as rows.
fn rows(desk: &Desk) -> Vec<ListRow> {
    let mut open: Vec<&Conversation> = desk
        .conversations
        .iter()
        .filter(|c| c.inbox == "open")
        .collect();
    open.sort_by_key(|c| std::cmp::Reverse((c.last_at, c.id)));
    open.into_iter()
        .take(50)
        .map(|conversation| {
            let customer = desk
                .customers
                .iter()
                .find(|c| c.id == conversation.customer_id);
            ListRow {
                customer: customer.map_or_else(String::new, |c| c.name.clone()),
                company: customer.map_or_else(String::new, |c| c.company.clone()),
                assignee: conversation
                    .assignee_id
                    .and_then(|id| desk.agents.iter().find(|a| a.id == id))
                    .map(|a| a.name.clone()),
                conversation: conversation.clone(),
            }
        })
        .collect()
}

/// The conversation the previews open: one with a note, a file and a few turns.
fn chosen(desk: &Desk) -> Conversation {
    desk.conversations
        .iter()
        .filter(|c| c.inbox == "open")
        .max_by_key(|c| {
            let notes = desk
                .messages
                .iter()
                .filter(|m| m.conversation_id == c.id && m.kind == "note")
                .count();
            let files = desk
                .files
                .iter()
                .filter(|f| f.conversation_id == c.id)
                .count();
            (
                files.min(1),
                notes.min(1),
                desk.messages
                    .iter()
                    .filter(|m| m.conversation_id == c.id)
                    .count(),
            )
        })
        .cloned()
        .unwrap_or_default()
}

/// Everything a preview draws, owned, so the views can borrow it.
struct Scene {
    desk: Desk,
    rows: Vec<ListRow>,
    thread: OpenThread,
    view: CustomerView,
    summary: Summary,
    filters: Filters,
}

fn scene() -> Scene {
    let desk = desk();
    let rows = rows(&desk);
    let conversation = chosen(&desk);
    let customer = desk
        .customers
        .iter()
        .find(|c| c.id == conversation.customer_id)
        .cloned()
        .unwrap_or_default();
    let at = rows
        .iter()
        .position(|row| row.conversation.id == conversation.id);
    let neighbours = (
        at.and_then(|at| at.checked_sub(1))
            .map(|at| rows[at].conversation.id),
        at.and_then(|at| rows.get(at + 1))
            .map(|row| row.conversation.id),
    );
    let messages: Vec<Message> = desk
        .messages
        .iter()
        .filter(|m| m.conversation_id == conversation.id)
        .cloned()
        .collect();
    let files: Vec<Attachment> = desk
        .files
        .iter()
        .filter(|f| f.conversation_id == conversation.id)
        .cloned()
        .collect();
    let mut history: Vec<CustomerEvent> = desk
        .events
        .iter()
        .filter(|e| e.customer_id == customer.id)
        .cloned()
        .collect();
    history.sort_by_key(|e| e.at);
    let mut theirs: Vec<Conversation> = desk
        .conversations
        .iter()
        .filter(|c| c.customer_id == customer.id)
        .cloned()
        .collect();
    theirs.sort_by_key(|c| std::cmp::Reverse(c.last_at));
    let view = CustomerView {
        open: theirs.iter().filter(|c| c.inbox == "open").count() as u64,
        overdue: 0,
        customer: customer.clone(),
        history,
        theirs,
    };
    let assignee = conversation
        .assignee_id
        .and_then(|id| desk.agents.iter().find(|a| a.id == id))
        .map(|a| (a.id, a.name.clone()));
    let thread = OpenThread {
        conversation,
        customer,
        assignee,
        messages,
        files,
        older: false,
        newer: false,
        neighbours,
    };
    let open: Vec<&Conversation> = desk
        .conversations
        .iter()
        .filter(|c| c.inbox == "open")
        .collect();
    let summary = Summary {
        total: Some(open.len() as u64),
        unread: open.iter().filter(|c| c.unread > 0).count() as u64,
        overdue: open
            .iter()
            .filter(|c| data::Sla::of(c, NOW).is_some_and(data::Sla::breached))
            .count() as u64,
    };
    Scene {
        desk,
        rows,
        thread,
        view,
        summary,
        filters: Filters::default(),
    }
}

impl Scene {
    fn inbox(&self, pane: &'static str) -> Inbox<'_> {
        Inbox {
            filters: &self.filters,
            list: ListPage {
                rows: &self.rows,
                more_after: true,
                more_before: false,
                since: 1,
            },
            summary: self.summary,
            open: Some(&self.thread),
            customer: Some(&self.view),
            pane,
            now: NOW,
            today: data::day_of(NOW).unwrap_or_default(),
            draft: "",
            kind: "reply",
            error: None,
        }
    }
}

/// The page with the chosen conversation open, about `pane`: its body in the shell,
/// and what the stream would bring, which the site's own script plays one by one: the
/// preview has no server to stream from.
fn page(pane: &'static str) -> Node {
    let scene = scene();
    let desk = &scene.desk;
    let inbox = scene.inbox(pane);
    let me = desk.agents.first().cloned().unwrap_or_default();
    let counts = ViewCounts {
        all: scene.summary.total.unwrap_or(0),
        mine: desk
            .conversations
            .iter()
            .filter(|c| c.inbox == "open" && c.assignee_id == Some(me.id))
            .count() as u64,
        unassigned: desk
            .conversations
            .iter()
            .filter(|c| c.status == "open" && c.assignee_id.is_none())
            .count() as u64,
        snoozed: desk
            .conversations
            .iter()
            .filter(|c| c.inbox == "snoozed")
            .count() as u64,
    };
    let body = shell(&me, &desk.agents, counts, View::All)
        .page(
            "All open",
            format!("/inbox/{}", scene.thread.conversation.id),
        )
        .actions(page_actions())
        .body((inbox_split(&inbox, None), palette(&inbox), shortcuts()))
        .to_view()
        .body_node()
        .clone();
    let list_items = format!(
        "conversation-list-{}",
        navree::ui::assets::content_hash(b"")
    );
    let thread_items = format!(
        "thread-messages-{}",
        navree::ui::assets::content_hash(
            format!("conversation-{}", scene.thread.conversation.id).as_bytes()
        )
    );
    // A customer nobody has answered writes in, at the top of the list.
    let newcomer = desk
        .conversations
        .iter()
        .filter(|c| c.inbox == "closed")
        .max_by_key(|c| c.id)
        .cloned()
        .map(|c| {
            let customer = desk.customers.iter().find(|x| x.id == c.customer_id);
            let row = ListRow {
                customer: customer.map_or_else(String::new, |x| x.name.clone()),
                company: customer.map_or_else(String::new, |x| x.company.clone()),
                assignee: None,
                conversation: Conversation {
                    id: 9_001,
                    last_at: NOW,
                    unread: 1,
                    status: "open".to_owned(),
                    inbox: "open".to_owned(),
                    assignee_id: None,
                    sla_from: Some(NOW),
                    sla_due: Some(NOW + 3_600),
                    ..c
                },
            };
            Node::from(conversation_item(&row, &scene.filters, None, NOW, None))
        });
    // And the customer of the open conversation writes again, at its end.
    let later = Message {
        id: 9_000_001,
        conversation_id: scene.thread.conversation.id,
        kind: "customer".to_owned(),
        author: scene.thread.customer.name.clone(),
        agent_id: None,
        body: "Thanks! One more thing: does this also change the staff app?".to_owned(),
        at: NOW,
    };
    Node::list((
        body,
        el("template")
            .attr("data-preview-replay", list_items)
            .child(newcomer),
        el("template")
            .attr("data-preview-replay", thread_items.clone())
            .child(
                message_item(&later, &[], NOW)
                    .attr("data-preview-into", thread_items)
                    .attr("data-preview-at", "end"),
            ),
    ))
}

fn panes() -> Node {
    page("thread")
}

/// A generated conversation of 100 messages, as the full-size desk holds it: the page
/// draws the newest thirty with a link to earlier ones, and the replay adds the thirty
/// before them at the top, as scrolling up does.
fn long_thread() -> Node {
    let mut scene = scene();
    let (conversation, messages) = inbox_demo::scale::generated(1_008, NOW);
    let newest = messages[messages.len() - 30..].to_vec();
    let older = messages[messages.len() - 38..messages.len() - 30].to_vec();
    let name = messages
        .iter()
        .find(|m| m.kind == "customer")
        .map_or_else(String::new, |m| m.author.clone());
    scene.thread = OpenThread {
        customer: Customer {
            id: conversation.customer_id,
            name: name.clone(),
            email: format!("{}@example.org", name.to_lowercase().replace(' ', ".")),
            company: "Sage Physio".to_owned(),
            role: "Practice manager".to_owned(),
            plan: "growth".to_owned(),
            lifetime_cents: 1_253_100,
            seats: 9,
            country: "NL".to_owned(),
            city: "Delft".to_owned(),
            since: data::day_of(NOW - 400 * 86_400),
            tags: "annual,multi-location".to_owned(),
        },
        assignee: conversation.assignee_id.map(|id| {
            (
                id,
                data::AGENTS[usize::try_from(id - 1).unwrap_or(0)]
                    .0
                    .to_owned(),
            )
        }),
        conversation,
        messages: newest,
        files: Vec::new(),
        older: true,
        newer: false,
        neighbours: (None, None),
    };
    scene.view.customer = scene.thread.customer.clone();
    scene.view.history.clear();
    scene.view.theirs = vec![scene.thread.conversation.clone()];
    let inbox = scene.inbox("thread");
    let desk = &scene.desk;
    let me = desk.agents.first().cloned().unwrap_or_default();
    let body = shell(&me, &desk.agents, ViewCounts::default(), View::Closed)
        .page("Closed", "/inbox/1008")
        .actions(page_actions())
        .body((inbox_split(&inbox, None), shortcuts()))
        .to_view()
        .body_node()
        .clone();
    let thread_items = format!(
        "thread-messages-{}",
        navree::ui::assets::content_hash(b"conversation-1008")
    );
    Node::list((
        body,
        el("template")
            .attr("data-preview-replay", thread_items.clone())
            .child(
                older
                    .iter()
                    .rev()
                    .map(|m| {
                        Node::from(
                            message_item(m, &[], NOW)
                                .attr("data-preview-into", thread_items.clone()),
                        )
                    })
                    .collect::<Vec<Node>>(),
            ),
    ))
}

fn phone() -> Node {
    page("thread")
}

/// The command palette as Ctrl+K opens it over a conversation: its actions, the saved
/// replies, and the places to go.
fn palette_open() -> Node {
    let scene = scene();
    palette(&scene.inbox("thread")).into()
}
