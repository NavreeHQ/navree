//! The desk as it happens: customers write, teammates answer and close, and the
//! clock on each conversation runs down. Every change goes to every open page over a
//! server-sent event stream.
//!
//! An event is only a nudge. A page never reads what it says: each live part of a
//! page fetches its own fragment again, so the stream can't put anything in a page
//! that the server didn't draw. A stream is for one page: it says `conversations` for
//! any change, and `thread` and `customer` only when the change is to the conversation
//! or the customer that page has open.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::HeaderMap;
use http::header::CONTENT_TYPE;
use navree::db::{Db, Executor as _};
use navree::http::response::sse::{Event as Sse, KeepAlive, Sse as SseResponse};
use navree::http::response::{IntoResponse, Response};
use navree::orm::Model as _;
use navree::prelude::BoxError;
use tokio::sync::broadcast;

use crate::data::{
    self, Agent, Attachment, Author, Conversation, Customer, CustomerEvent, Dice, EventKind,
    Message, Priority, Status, TOPICS, preview_of,
};

/// What changed: a conversation and its customer, or `0` for the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub conversation: i64,
    pub customer: i64,
}

impl Change {
    /// The clock moved: every timer on every page.
    pub const TICK: Change = Change {
        conversation: 0,
        customer: 0,
    };
}

/// What the simulator, or a test, makes happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Happening {
    /// A customer starts a new conversation.
    NewConversation,
    /// A customer writes again in a conversation.
    CustomerReply,
    /// A teammate answers a conversation of theirs.
    TeammateReply,
    /// A teammate leaves an internal note.
    TeammateNote,
    /// A teammate closes a conversation.
    TeammateClose,
}

impl Happening {
    pub fn parse(key: &str) -> Option<Happening> {
        Some(match key {
            "new" => Self::NewConversation,
            "customer" => Self::CustomerReply,
            "reply" => Self::TeammateReply,
            "note" => Self::TeammateNote,
            "close" => Self::TeammateClose,
            _ => return None,
        })
    }
}

/// What customers write when they write again.
const FOLLOW_UPS: [&str; 8] = [
    "Any news on this? We have clients booking this afternoon.",
    "Thanks. One more thing: does this also affect the staff app?",
    "I tried that and it still happens. Can someone call me?",
    "Just checking you saw my last message.",
    "That fixed it for one location, but not the other.",
    "Great, thank you! Could you also send that in writing for our records?",
    "Sorry, I sent the wrong file before. This is the right one.",
    "Our manager is asking for an update by end of day.",
];

/// What teammates answer.
const ANSWERS: [&str; 6] = [
    "Thanks for waiting. I've checked this with the team and it's fixed on our side now.",
    "I've just tried it on a test account and can see the same thing. Working on it.",
    "Good news: that's done. Let me know if anything still looks wrong.",
    "Could you tell me which location this was at, and roughly when?",
    "I've sent the confirmation to the address on your account.",
    "That's expected for bookings made before the change; new ones are right.",
];

/// What teammates note for each other.
const NOTES: [&str; 4] = [
    "Customer is on an annual contract, renewal next month. Worth a call.",
    "Same report from two other accounts today. Linking to the incident.",
    "Checked the logs: nothing failed on our side in that window.",
    "Offered a credit for the trouble; billing to apply it.",
];

/// What happens next, and who hears about it.
pub struct Hub {
    tx: broadcast::Sender<Change>,
    dice: Mutex<Dice>,
    next_message: AtomicI64,
    next_attachment: AtomicI64,
    next_event: AtomicI64,
    next_conversation: AtomicI64,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(256).0,
            dice: Mutex::new(Dice::new(4_111)),
            next_message: AtomicI64::new(1),
            next_attachment: AtomicI64::new(1),
            next_event: AtomicI64::new(1),
            next_conversation: AtomicI64::new(1),
        }
    }

    /// Tells every open stream that `change` happened.
    pub fn tell(&self, change: Change) {
        let _ = self.tx.send(change);
    }

    fn roll(&self, n: u64) -> u64 {
        self.dice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .below(n)
    }

    /// A message id no one has used.
    pub fn message_id(&self) -> i64 {
        self.next_message.fetch_add(1, Ordering::SeqCst)
    }

    /// An event id no one has used.
    pub fn event_id(&self) -> i64 {
        self.next_event.fetch_add(1, Ordering::SeqCst)
    }

    /// Creates the tables, empty.
    pub async fn create(&self, db: &Db) -> Result<(), BoxError> {
        let pool = db.pool();
        for statement in [
            "DROP TABLE IF EXISTS layouts",
            "DROP TABLE IF EXISTS customer_events",
            "DROP TABLE IF EXISTS attachments",
            "DROP TABLE IF EXISTS messages",
            "DROP TABLE IF EXISTS conversations",
            "DROP TABLE IF EXISTS customers",
            "DROP TABLE IF EXISTS agents",
            "CREATE TABLE agents (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, team TEXT NOT NULL, active BOOLEAN NOT NULL)",
            "CREATE INDEX agents_name ON agents (name, id)",
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, company TEXT NOT NULL, role TEXT NOT NULL, plan TEXT NOT NULL, lifetime_cents INTEGER NOT NULL, seats INTEGER NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL, since DATE, tags TEXT NOT NULL)",
            "CREATE TABLE conversations (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL, subject TEXT NOT NULL, channel TEXT NOT NULL, status TEXT NOT NULL, priority TEXT NOT NULL, assignee_id INTEGER, created_at INTEGER NOT NULL, last_at INTEGER NOT NULL, sla_due INTEGER, sla_from INTEGER, snoozed_until DATE, unread INTEGER NOT NULL, preview TEXT NOT NULL)",
            "CREATE INDEX conversations_status ON conversations (status, last_at, id)",
            "CREATE INDEX conversations_assignee ON conversations (assignee_id, status, last_at)",
            "CREATE INDEX conversations_customer ON conversations (customer_id, last_at)",
            "CREATE TABLE messages (id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL, kind TEXT NOT NULL, author TEXT NOT NULL, agent_id INTEGER, body TEXT NOT NULL, at INTEGER NOT NULL)",
            "CREATE INDEX messages_conversation ON messages (conversation_id, at, id)",
            "CREATE TABLE attachments (id INTEGER PRIMARY KEY, message_id INTEGER NOT NULL, conversation_id INTEGER NOT NULL, name TEXT NOT NULL, bytes INTEGER NOT NULL)",
            "CREATE INDEX attachments_conversation ON attachments (conversation_id, message_id)",
            "CREATE TABLE customer_events (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL, kind TEXT NOT NULL, at INTEGER NOT NULL, title TEXT NOT NULL, detail TEXT NOT NULL)",
            "CREATE INDEX customer_events_customer ON customer_events (customer_id, at)",
            "CREATE TABLE layouts (id INTEGER PRIMARY KEY, agent_id INTEGER NOT NULL, split TEXT NOT NULL, sizes TEXT NOT NULL)",
            "CREATE UNIQUE INDEX layouts_agent ON layouts (agent_id, split)",
        ] {
            pool.execute_script(statement).await?;
        }
        Ok(())
    }

    /// Writes the desk afresh as it stands at `now`.
    pub async fn seed(&self, db: &Db, now: i64) -> Result<(), BoxError> {
        data::Layout::query().delete(db).await?;
        CustomerEvent::query().delete(db).await?;
        Attachment::query().delete(db).await?;
        Message::query().delete(db).await?;
        Conversation::query().delete(db).await?;
        Customer::query().delete(db).await?;
        Agent::query().delete(db).await?;
        let seed = data::seed(now);
        Agent::query().insert(db, seed.agents.clone()).await?;
        Customer::query().insert(db, seed.customers.clone()).await?;
        for chunk in seed.conversations.chunks(100) {
            Conversation::query().insert(db, chunk.to_vec()).await?;
        }
        for chunk in seed.messages.chunks(100) {
            Message::query().insert(db, chunk.to_vec()).await?;
        }
        if !seed.attachments.is_empty() {
            Attachment::query()
                .insert(db, seed.attachments.clone())
                .await?;
        }
        for chunk in seed.events.chunks(100) {
            CustomerEvent::query().insert(db, chunk.to_vec()).await?;
        }
        let next = |n: usize| i64::try_from(n).unwrap_or(0) + 1;
        let max_message = seed.messages.iter().map(|m| m.id).max().unwrap_or(0);
        self.next_message.store(max_message + 1, Ordering::SeqCst);
        self.next_attachment
            .store(next(seed.attachments.len()), Ordering::SeqCst);
        self.next_event.store(next(seed.events.len()), Ordering::SeqCst);
        self.next_conversation
            .store(next(seed.conversations.len()), Ordering::SeqCst);
        Ok(())
    }

    /// One thing happens: `what` when given, otherwise whatever the dice say; in
    /// conversation `on` when given and it can happen there. Returns the change, or
    /// `None` when there was nowhere for it to happen.
    pub async fn step(
        &self,
        db: &Db,
        now: i64,
        what: Option<Happening>,
        on: Option<i64>,
    ) -> Result<Option<Change>, BoxError> {
        let what = what.unwrap_or_else(|| match self.roll(100) {
            0..=24 => Happening::NewConversation,
            25..=64 => Happening::CustomerReply,
            65..=82 => Happening::TeammateReply,
            83..=92 => Happening::TeammateNote,
            _ => Happening::TeammateClose,
        });
        let change = match what {
            Happening::NewConversation => self.new_conversation(db, now).await?,
            Happening::CustomerReply => self.customer_reply(db, now, on).await?,
            Happening::TeammateReply | Happening::TeammateNote | Happening::TeammateClose => {
                self.teammate(db, now, what, on).await?
            }
        };
        if let Some(change) = change {
            self.tell(change);
        }
        Ok(change)
    }

    async fn new_conversation(&self, db: &Db, now: i64) -> Result<Option<Change>, BoxError> {
        let customers: Vec<Customer> = Customer::query()
            .order_by("id")
            .get(db)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect();
        if customers.is_empty() {
            return Ok(None);
        }
        let customer = &customers[usize::try_from(self.roll(customers.len() as u64)).unwrap_or(0)];
        let topic = &TOPICS[usize::try_from(self.roll(TOPICS.len() as u64)).unwrap_or(0)];
        let id = self.next_conversation.fetch_add(1, Ordering::SeqCst);
        let first = &topic.lines[0];
        let message = Message {
            id: self.message_id(),
            conversation_id: id,
            kind: Author::Customer.key().to_owned(),
            author: customer.name.clone(),
            agent_id: None,
            body: first.body.to_owned(),
            at: now,
        };
        Conversation::query()
            .insert(
                db,
                [Conversation {
                    id,
                    customer_id: customer.id,
                    subject: topic.subject.to_owned(),
                    channel: topic.channel.key().to_owned(),
                    status: Status::Open.key().to_owned(),
                    priority: topic.priority.key().to_owned(),
                    assignee_id: None,
                    created_at: now,
                    last_at: now,
                    sla_due: Some(now + topic.priority.target()),
                    sla_from: Some(now),
                    snoozed_until: None,
                    unread: 1,
                    preview: preview_of(first.body),
                }],
            )
            .await?;
        Message::query().insert(db, [message.clone()]).await?;
        if let Some((name, bytes)) = first.file {
            Attachment::query()
                .insert(
                    db,
                    [Attachment {
                        id: self.next_attachment.fetch_add(1, Ordering::SeqCst),
                        message_id: message.id,
                        conversation_id: id,
                        name: name.to_owned(),
                        bytes,
                    }],
                )
                .await?;
        }
        CustomerEvent::query()
            .insert(
                db,
                [CustomerEvent {
                    id: self.event_id(),
                    customer_id: customer.id,
                    kind: EventKind::TicketOpened.key().to_owned(),
                    at: now,
                    title: format!("Wrote in: {}", topic.subject),
                    detail: format!("Through {}", topic.channel.label().to_lowercase()),
                }],
            )
            .await?;
        Ok(Some(Change {
            conversation: id,
            customer: customer.id,
        }))
    }

    /// A conversation for something to happen in: `on` if it is one of `statuses`, or
    /// one of them at random.
    async fn pick(
        &self,
        db: &Db,
        statuses: &[Status],
        on: Option<i64>,
        assigned: bool,
    ) -> Result<Option<Conversation>, BoxError> {
        let keys: Vec<&str> = statuses.iter().map(|status| status.key()).collect();
        let mut query = Conversation::query().where_(Conversation::STATUS.is_in(keys));
        if assigned {
            query = query.where_(Conversation::ASSIGNEE_ID.is_not_null());
        }
        if let Some(id) = on {
            return Ok(query.find(db, id).await?.map(|record| record.into_inner()));
        }
        let count = query.clone().count(db).await?;
        if count == 0 {
            return Ok(None);
        }
        Ok(query
            .order_by_desc("last_at")
            .order_by("id")
            .offset(self.roll(count.min(12)))
            .first(db)
            .await?
            .map(|record| record.into_inner()))
    }

    async fn customer_reply(
        &self,
        db: &Db,
        now: i64,
        on: Option<i64>,
    ) -> Result<Option<Change>, BoxError> {
        let Some(conversation) = self
            .pick(db, &[Status::Open, Status::Pending, Status::Snoozed], on, false)
            .await?
        else {
            return Ok(None);
        };
        let Some(customer) = Customer::query()
            .find(db, conversation.customer_id)
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(None);
        };
        let body =
            FOLLOW_UPS[usize::try_from(self.roll(FOLLOW_UPS.len() as u64)).unwrap_or(0)];
        Message::query()
            .insert(
                db,
                [Message {
                    id: self.message_id(),
                    conversation_id: conversation.id,
                    kind: Author::Customer.key().to_owned(),
                    author: customer.name.clone(),
                    agent_id: None,
                    body: body.to_owned(),
                    at: now,
                }],
            )
            .await?;
        let priority = Priority::parse(&conversation.priority).unwrap_or(Priority::Normal);
        // A customer's word starts the clock again, unless one is already running: the
        // first unanswered message is what the team owes.
        let (due, from) = match (Status::parse(&conversation.status), conversation.sla_due) {
            (Some(Status::Open), Some(due)) => (due, conversation.sla_from.unwrap_or(now)),
            _ => (now + priority.target(), now),
        };
        Conversation::query()
            .where_(Conversation::ID.eq(conversation.id))
            .update(
                db,
                [
                    Conversation::STATUS.set(Status::Open.key().to_owned()),
                    Conversation::UNREAD.set(conversation.unread + 1),
                    Conversation::LAST_AT.set(now),
                    Conversation::SLA_DUE.set(Some(due)),
                    Conversation::SLA_FROM.set(Some(from)),
                    Conversation::SNOOZED_UNTIL.set(None::<chrono::NaiveDate>),
                    Conversation::PREVIEW.set(preview_of(body)),
                ],
            )
            .await?;
        Ok(Some(Change {
            conversation: conversation.id,
            customer: conversation.customer_id,
        }))
    }

    async fn teammate(
        &self,
        db: &Db,
        now: i64,
        what: Happening,
        on: Option<i64>,
    ) -> Result<Option<Change>, BoxError> {
        let statuses: &[Status] = match what {
            Happening::TeammateNote => &[Status::Open, Status::Pending],
            _ => &[Status::Open],
        };
        let Some(conversation) = self.pick(db, statuses, on, true).await? else {
            return Ok(None);
        };
        let Some(agent) = Agent::query()
            .find(db, conversation.assignee_id.unwrap_or(0))
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(None);
        };
        let (kind, body, status) = match what {
            Happening::TeammateReply => (
                Author::Agent,
                ANSWERS[usize::try_from(self.roll(ANSWERS.len() as u64)).unwrap_or(0)]
                    .to_owned(),
                Status::Pending,
            ),
            Happening::TeammateNote => (
                Author::Note,
                NOTES[usize::try_from(self.roll(NOTES.len() as u64)).unwrap_or(0)].to_owned(),
                Status::parse(&conversation.status).unwrap_or(Status::Open),
            ),
            _ => (
                Author::Event,
                format!("{} closed the conversation", agent.name),
                Status::Closed,
            ),
        };
        Message::query()
            .insert(
                db,
                [Message {
                    id: self.message_id(),
                    conversation_id: conversation.id,
                    kind: kind.key().to_owned(),
                    author: agent.name.clone(),
                    agent_id: Some(agent.id),
                    body: body.clone(),
                    at: now,
                }],
            )
            .await?;
        let answered = matches!(kind, Author::Agent) || status == Status::Closed;
        let mut sets = vec![
            Conversation::STATUS.set(status.key().to_owned()),
            Conversation::LAST_AT.set(now),
        ];
        if answered {
            sets.push(Conversation::SLA_DUE.set(None::<i64>));
            sets.push(Conversation::SLA_FROM.set(None::<i64>));
            sets.push(Conversation::UNREAD.set(0_i64));
        }
        if kind == Author::Agent {
            sets.push(Conversation::PREVIEW.set(preview_of(&body)));
        }
        Conversation::query()
            .where_(Conversation::ID.eq(conversation.id))
            .update(db, sets)
            .await?;
        if status == Status::Closed {
            CustomerEvent::query()
                .insert(
                    db,
                    [CustomerEvent {
                        id: self.event_id(),
                        customer_id: conversation.customer_id,
                        kind: EventKind::TicketClosed.key().to_owned(),
                        at: now,
                        title: format!("Resolved: {}", conversation.subject),
                        detail: format!("Closed by {}", agent.name),
                    }],
                )
                .await?;
        }
        Ok(Some(Change {
            conversation: conversation.id,
            customer: conversation.customer_id,
        }))
    }

    /// Runs the simulator: something happens every `every`, give or take. It runs until
    /// the task holding it is dropped.
    pub async fn simulate(self: Arc<Self>, db: Arc<Db>, every: Duration) {
        loop {
            let jitter = Duration::from_millis(self.roll(1500));
            tokio::time::sleep(every + jitter).await;
            if let Err(error) = self.step(&db, now(), None, None).await {
                eprintln!("inbox-demo: the simulator could not take a step: {error}");
            }
        }
    }

    /// Runs the clock: every `every` the pages draw their timers again, so a countdown
    /// moves without anyone doing anything. The time is the server's.
    pub async fn tick(self: Arc<Self>, every: Duration) {
        loop {
            tokio::time::sleep(every).await;
            self.tell(Change::TICK);
        }
    }

    /// `GET /inbox/events?open=<conversation>&customer=<customer>`: the stream for one
    /// page. `conversations` for every change, `thread` and `customer` for the page's
    /// own conversation and customer, and all three for the clock.
    pub fn stream(&self, headers: &HeaderMap, open: i64, customer: i64) -> Response {
        // The runtime asks once what a failed stream answers: say it's a stream.
        if headers.contains_key("x-ui-probe") {
            return ([(CONTENT_TYPE, "text/event-stream")], "").into_response();
        }
        let rx = self.tx.subscribe();
        // A first comment goes out at once: Firefox reports a stream open only when its
        // first bytes arrive.
        let queue = VecDeque::from([Sse::default().comment("ready")]);
        let events = futures_util::stream::unfold((rx, queue), move |(mut rx, mut queue)| async move {
            loop {
                if let Some(event) = queue.pop_front() {
                    return Some((Ok::<_, Infallible>(event), (rx, queue)));
                }
                let (lists, thread, person) = match rx.recv().await {
                    Ok(change) if change == Change::TICK => (true, open > 0, false),
                    Ok(change) => (
                        true,
                        open > 0 && change.conversation == open,
                        customer > 0 && change.customer == customer,
                    ),
                    // Missed some: everything may have changed.
                    Err(broadcast::error::RecvError::Lagged(_)) => (true, open > 0, customer > 0),
                    Err(broadcast::error::RecvError::Closed) => return None,
                };
                for (yes, name) in [(lists, "conversations"), (thread, "thread"), (person, "customer")] {
                    if yes {
                        queue.push_back(Sse::default().event(name).data("changed"));
                    }
                }
            }
        });
        SseResponse::new(events)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response()
    }
}

/// Now, in unix seconds.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
