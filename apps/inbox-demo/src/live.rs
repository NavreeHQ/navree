//! The desk as it happens: customers write, teammates answer and close, the clock on
//! each conversation runs down, and a snoozed conversation comes back on its day. Every
//! change goes to every open page through the app's live hub (`RealtimeModule`).
//!
//! A nudge says only that something changed. A page never reads it: each live part of
//! a page fetches its own fragment again, so a nudge can't put anything in a page that
//! the server didn't draw. The topics: `conversations` for any change, which the lists
//! hear; `conversation.<id>` and `customer.<id>` when the change is to that
//! conversation or customer, and more than its being read; and `clock` when the timers
//! move. A page listens to the lists and, with a conversation open, to that
//! conversation, its customer and the clock (see [`page_stream`]). The desk signs
//! nobody in, so the topics are public.
//!
//! **Every write takes its numbers where it commits.** A live list asks for what changed
//! since the newest revision it has seen, and an open conversation for the messages after
//! the newest it has. So a number must never become visible after a reader has seen a
//! larger one. Each write runs in one transaction (a unit of work, `BEGIN IMMEDIATE` on
//! SQLite, so one writer at a time), and inside it:
//!
//! - the desk's revision comes from the one row of `desk`, which the transaction updates
//!   (`UPDATE desk SET rev = rev + 1 RETURNING rev`): the next writer can't read it until
//!   this one has committed;
//! - a message's id is the row id SQLite gives it as it is inserted, one more than the
//!   largest, under the same write lock;
//! - counts move in SQL (`messages = messages + 1`), never from a value read earlier.
//!
//! So ids and revisions become visible in the order they commit, and a reader that moves
//! its cursor to the newest it saw can't skip one.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree::db::{Db, Executor as _, ExecutorExt as _, Statement, Uow, Value};
use navree::orm::{Expr, Model as _};
use navree::prelude::BoxError;
use navree::realtime::{Live, Topic};
use navree::ui::LiveStream;

use crate::data::{
    self, Agent, Attachment, Author, Conversation, Customer, CustomerEvent, Dice, EventKind,
    Message, Priority, Status, TOPICS, day_of, inbox_of, preview_of,
};
use crate::scale;

/// What changed: a conversation and its customer, or `0` for the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub conversation: i64,
    pub customer: i64,
    /// Only what the lists show moved (it was read): the open conversation's heading,
    /// messages and customer are as they were, so a page that has it open fetches none
    /// of them again, and nothing the agent has open in them is drawn over.
    pub lists_only: bool,
}

impl Change {
    /// The clock moved: every timer on every page.
    pub const TICK: Change = Change {
        conversation: 0,
        customer: 0,
        lists_only: false,
    };

    /// Conversation `conversation` of customer `customer` changed.
    pub const fn of(conversation: i64, customer: i64) -> Change {
        Change {
            conversation,
            customer,
            lists_only: false,
        }
    }

    /// Conversation `conversation` was read: only the lists show that.
    pub const fn read(conversation: i64, customer: i64) -> Change {
        Change {
            conversation,
            customer,
            lists_only: true,
        }
    }
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

/// The most snoozed conversations one wake-up brings back in one transaction.
const WAKE_AT_ONCE: u64 = 200;

type Result<T> = std::result::Result<T, BoxError>;

/// The desk's next revision, taken inside the writing transaction: `desk` has one row,
/// and the update holds it until the transaction ends, so no other writer reads it first.
pub async fn next_rev(uow: &Uow) -> Result<i64> {
    Ok(uow
        .fetch_scalar::<i64>(&Statement::portable(
            "UPDATE desk SET rev = rev + 1 WHERE id = 1 RETURNING rev",
            Vec::<Value>::new(),
        ))
        .await?)
}

/// The revision the desk is at: what a page drawn now says it is at. Read it before
/// the rows, so a change that commits between the two is asked for again, not missed.
pub async fn current_rev(db: &(impl navree::db::Executor + ?Sized)) -> Result<i64> {
    Ok(db
        .fetch_scalar::<i64>(&Statement::portable(
            "SELECT rev FROM desk WHERE id = 1",
            Vec::<Value>::new(),
        ))
        .await?)
}

/// `column + 1`, counted where the row is written.
pub fn plus_one(column: &'static str) -> Expr {
    Expr::raw(format!("\"{column}\" + 1"), Vec::<Value>::new())
}

/// The topic every list hears: any change to any conversation, and the clock.
pub const LISTS: &str = "conversations";

/// The topic of the clock: every timer on every page.
pub const CLOCK: &str = "clock";

/// The topic of one conversation: its heading and its messages.
pub fn conversation_topic(id: i64) -> String {
    format!("conversation.{id}")
}

/// The topic of one customer: the customer pane.
pub fn customer_topic(id: i64) -> String {
    format!("customer.{id}")
}

/// The stream a page listens on: the lists and, with a conversation open (its id and
/// its customer's), that conversation, its customer and the clock. One connection.
pub fn page_stream(open: Option<(i64, i64)>) -> LiveStream {
    let stream = LiveStream::topics([LISTS]);
    match open {
        Some((conversation, customer)) => stream.with_topics([
            conversation_topic(conversation),
            customer_topic(customer),
            CLOCK.to_owned(),
        ]),
        None => stream,
    }
}

/// The topics `change` tells: the lists always; the clock when it ticked; the
/// conversation and the customer when more than its being read changed.
pub fn topics_of(change: Change) -> Vec<String> {
    let mut topics = vec![LISTS.to_owned()];
    if change == Change::TICK {
        topics.push(CLOCK.to_owned());
    } else if !change.lists_only {
        if change.conversation > 0 {
            topics.push(conversation_topic(change.conversation));
        }
        if change.customer > 0 {
            topics.push(customer_topic(change.customer));
        }
    }
    topics
}

/// What happens next, and who hears about it.
pub struct Hub {
    /// Held while the desk is written afresh: one at a time.
    pub writing: tokio::sync::Mutex<()>,
    live: Option<Arc<Live>>,
    dice: Mutex<Dice>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    /// A hub that tells nobody (the tests of the data).
    pub fn new() -> Self {
        Self {
            writing: tokio::sync::Mutex::new(()),
            live: None,
            dice: Mutex::new(Dice::new(4_111)),
        }
    }

    /// Tells the pages through the app's live hub.
    pub fn with_live(mut self, live: Arc<Live>) -> Self {
        self.live = Some(live);
        self
    }

    /// Tells every open page that `change` happened, once it can be read: the live
    /// hub holds a nudge made in a unit of work until it commits, and drops it on a
    /// rollback, because a page told sooner would ask before the commit, miss the
    /// change, and have no reason to ask again. Outside one (the clock) it goes now.
    pub async fn tell(&self, change: Change) {
        let Some(live) = &self.live else {
            return;
        };
        let topics = topics_of(change).into_iter().map(Topic::public);
        if let Err(error) = live.nudge(topics).await {
            eprintln!("inbox-demo: the pages could not be told: {error}");
        }
    }

    fn roll(&self, n: u64) -> u64 {
        self.dice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .below(n)
    }

    /// Creates the tables, empty. The indexes come after the rows: see [`Hub::seed`].
    pub async fn create(&self, db: &Db) -> Result<()> {
        let pool = db.pool();
        for statement in [
            "DROP TABLE IF EXISTS conversations_fts",
            "DROP TABLE IF EXISTS layouts",
            "DROP TABLE IF EXISTS customer_events",
            "DROP TABLE IF EXISTS attachments",
            "DROP TABLE IF EXISTS messages",
            "DROP TABLE IF EXISTS conversations",
            "DROP TABLE IF EXISTS customers",
            "DROP TABLE IF EXISTS agents",
            "DROP TABLE IF EXISTS desk",
            "CREATE TABLE desk (id INTEGER PRIMARY KEY CHECK (id = 1), rev INTEGER NOT NULL)",
            "INSERT INTO desk (id, rev) VALUES (1, 1)",
            "CREATE TABLE agents (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, team TEXT NOT NULL, active BOOLEAN NOT NULL, single_keys BOOLEAN NOT NULL)",
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, phone TEXT NOT NULL, company TEXT NOT NULL, role TEXT NOT NULL, plan TEXT NOT NULL, lifetime_paise INTEGER NOT NULL, seats INTEGER NOT NULL, country TEXT NOT NULL, city TEXT NOT NULL, since DATE, tags TEXT NOT NULL)",
            "CREATE TABLE conversations (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL, subject TEXT NOT NULL, channel TEXT NOT NULL, status TEXT NOT NULL, priority TEXT NOT NULL, assignee_id INTEGER, created_at INTEGER NOT NULL, last_at INTEGER NOT NULL, last_rev INTEGER NOT NULL DEFAULT 0, sla_due INTEGER, sla_from INTEGER, snoozed_until DATE, unread INTEGER NOT NULL, preview TEXT NOT NULL, rev INTEGER NOT NULL, messages INTEGER NOT NULL, inbox TEXT NOT NULL)",
            "CREATE TABLE messages (id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL, kind TEXT NOT NULL, author TEXT NOT NULL, agent_id INTEGER, body TEXT NOT NULL, at INTEGER NOT NULL)",
            "CREATE TABLE attachments (id INTEGER PRIMARY KEY, message_id INTEGER NOT NULL, conversation_id INTEGER NOT NULL, name TEXT NOT NULL, bytes INTEGER NOT NULL)",
            "CREATE TABLE customer_events (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL, kind TEXT NOT NULL, at INTEGER NOT NULL, title TEXT NOT NULL, detail TEXT NOT NULL)",
            "CREATE TABLE layouts (id INTEGER PRIMARY KEY, agent_id INTEGER NOT NULL, split TEXT NOT NULL, sizes TEXT NOT NULL)",
            "CREATE UNIQUE INDEX layouts_agent ON layouts (agent_id, split)",
            // The search: subjects, names and companies, with accents folded.
            "CREATE VIRTUAL TABLE conversations_fts USING fts5 (subject, customer, company, content = '', tokenize = 'unicode61 remove_diacritics 2')",
        ] {
            pool.execute_script(statement).await?;
        }
        Ok(())
    }

    /// Writes the desk afresh as it stands at `now`: the hand-written conversations,
    /// then `scale` generated ones (see [`scale::seed`]), then the indexes.
    pub async fn seed(&self, db: &Db, now: i64, scale: i64) -> Result<scale::Made> {
        self.create(db).await?;
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
        let today = data::day_of(now)
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string();
        let made = scale::seed(db, now, &today, scale).await?;
        for statement in scale::INDEXES {
            db.pool().execute_script(statement).await?;
        }
        // The seed and its indexes pass through the write-ahead log: at full size that is
        // hundreds of megabytes, which the first write after it would copy back while a
        // reader waited. Copy it now and empty the log.
        db.pool()
            .execute_script("PRAGMA wal_checkpoint(TRUNCATE)")
            .await?;
        Ok(made)
    }

    /// Runs `work` in a unit of work of its own: committed when it returns `Ok`, and
    /// rolled back, its news unsaid, when it fails.
    async fn in_uow<T, F>(db: &Db, work: impl FnOnce(Uow) -> F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let uow = db.uow(None, None).await;
        match uow.scope(work(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = uow.rollback().await;
                Err(error)
            }
        }
    }

    /// One thing happens: `what` when given, otherwise whatever the dice say; in
    /// conversation `on` when given and it can happen there. Returns the change, or
    /// `None` when there was nowhere for it to happen. It reads and writes in one unit of
    /// work, so nothing changes between what it reads and what it writes.
    pub async fn step(
        &self,
        db: &Db,
        now: i64,
        what: Option<Happening>,
        on: Option<i64>,
    ) -> Result<Option<Change>> {
        let what = what.unwrap_or_else(|| match self.roll(100) {
            0..=24 => Happening::NewConversation,
            25..=64 => Happening::CustomerReply,
            65..=82 => Happening::TeammateReply,
            83..=92 => Happening::TeammateNote,
            _ => Happening::TeammateClose,
        });
        Self::in_uow(db, |uow| async move {
            let change = match what {
                Happening::NewConversation => self.new_conversation(&uow, now).await?,
                Happening::CustomerReply => self.customer_reply(&uow, now, on).await?,
                Happening::TeammateReply | Happening::TeammateNote | Happening::TeammateClose => {
                    self.teammate(&uow, now, what, on).await?
                }
            };
            if let Some(change) = change {
                self.tell(change).await;
            }
            Ok(change)
        })
        .await
    }

    async fn new_conversation(&self, uow: &Uow, now: i64) -> Result<Option<Change>> {
        let customers: Vec<Customer> = Customer::query()
            .where_(Customer::ID.lt(scale::FIRST_CUSTOMER))
            .order_by("id")
            .get(uow)
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
        let first = &topic.lines[0];
        let rev = next_rev(uow).await?;
        let id = Conversation::create(
            uow,
            Conversation {
                id: 0,
                customer_id: customer.id,
                subject: topic.subject.to_owned(),
                channel: topic.channel.key().to_owned(),
                status: Status::Open.key().to_owned(),
                priority: topic.priority.key().to_owned(),
                assignee_id: None,
                created_at: now,
                last_at: now,
                last_rev: rev,
                sla_due: Some(now + topic.priority.target()),
                sla_from: Some(now),
                snoozed_until: None,
                unread: 1,
                preview: preview_of(first.body),
                rev,
                messages: 1,
                inbox: "open".to_owned(),
            },
        )
        .await?
        .id;
        let message = Message::create(
            uow,
            Message {
                id: 0,
                conversation_id: id,
                kind: Author::Customer.key().to_owned(),
                author: customer.name.clone(),
                agent_id: None,
                body: first.body.to_owned(),
                at: now,
            },
        )
        .await?
        .id;
        uow.execute(&Statement::portable(
            "INSERT INTO conversations_fts (rowid, subject, customer, company) VALUES (?, ?, ?, ?)",
            [
                Value::I64(id),
                Value::String(topic.subject.to_owned()),
                Value::String(customer.name.clone()),
                Value::String(customer.company.clone()),
            ],
        ))
        .await?;
        if let Some((name, bytes)) = first.file {
            Attachment::create(
                uow,
                Attachment {
                    id: 0,
                    message_id: message,
                    conversation_id: id,
                    name: name.to_owned(),
                    bytes,
                },
            )
            .await?;
        }
        CustomerEvent::create(
            uow,
            CustomerEvent {
                id: 0,
                customer_id: customer.id,
                kind: EventKind::TicketOpened.key().to_owned(),
                at: now,
                title: format!("Wrote in: {}", topic.subject),
                detail: format!("Through {}", topic.channel.label().to_lowercase()),
            },
        )
        .await?;
        Ok(Some(Change::of(id, customer.id)))
    }

    /// A conversation for something to happen in: `on` if it is one of `statuses`, or
    /// one of them at random.
    async fn pick(
        &self,
        uow: &Uow,
        statuses: &[Status],
        on: Option<i64>,
        assigned: bool,
    ) -> Result<Option<Conversation>> {
        let keys: Vec<&str> = statuses.iter().map(|status| status.key()).collect();
        let mut query = Conversation::query().where_(Conversation::STATUS.is_in(keys.clone()));
        if assigned {
            query = query.where_(Conversation::ASSIGNEE_ID.is_not_null());
        }
        if let Some(id) = on {
            return Ok(query.find(uow, id).await?.map(|record| record.into_inner()));
        }
        // Among the dozen most recent: what a teammate or a customer is in the middle of.
        let recent: Vec<Conversation> = crate::queries::newest_first(query)
            .limit(12)
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect();
        if recent.is_empty() {
            return Ok(None);
        }
        let at = usize::try_from(self.roll(recent.len() as u64)).unwrap_or(0);
        Ok(recent.into_iter().nth(at))
    }

    async fn customer_reply(&self, uow: &Uow, now: i64, on: Option<i64>) -> Result<Option<Change>> {
        let Some(conversation) = self
            .pick(
                uow,
                &[Status::Open, Status::Pending, Status::Snoozed],
                on,
                false,
            )
            .await?
        else {
            return Ok(None);
        };
        let Some(customer) = Customer::query()
            .find(uow, conversation.customer_id)
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(None);
        };
        let body = FOLLOW_UPS[usize::try_from(self.roll(FOLLOW_UPS.len() as u64)).unwrap_or(0)];
        Message::create(
            uow,
            Message {
                id: 0,
                conversation_id: conversation.id,
                kind: Author::Customer.key().to_owned(),
                author: customer.name.clone(),
                agent_id: None,
                body: body.to_owned(),
                at: now,
            },
        )
        .await?;
        let priority = Priority::parse(&conversation.priority).unwrap_or(Priority::Normal);
        // A customer's word starts the clock again, unless one is already running: the
        // first unanswered message is what the team owes.
        let (due, from) = match (Status::parse(&conversation.status), conversation.sla_due) {
            (Some(Status::Open), Some(due)) => (due, conversation.sla_from.unwrap_or(now)),
            _ => (now + priority.target(), now),
        };
        let rev = next_rev(uow).await?;
        Conversation::query()
            .where_(Conversation::ID.eq(conversation.id))
            .update(
                uow,
                [
                    Conversation::STATUS.set(Status::Open.key().to_owned()),
                    Conversation::UNREAD.set_expr(plus_one("unread")),
                    Conversation::LAST_AT.set(now),
                    Conversation::LAST_REV.set(rev),
                    Conversation::SLA_DUE.set(Some(due)),
                    Conversation::SLA_FROM.set(Some(from)),
                    Conversation::SNOOZED_UNTIL.set(None::<chrono::NaiveDate>),
                    Conversation::PREVIEW.set(preview_of(body)),
                    Conversation::REV.set(rev),
                    Conversation::MESSAGES.set_expr(plus_one("messages")),
                    Conversation::INBOX.set("open".to_owned()),
                ],
            )
            .await?;
        Ok(Some(Change::of(conversation.id, conversation.customer_id)))
    }

    async fn teammate(
        &self,
        uow: &Uow,
        now: i64,
        what: Happening,
        on: Option<i64>,
    ) -> Result<Option<Change>> {
        let statuses: &[Status] = match what {
            Happening::TeammateNote => &[Status::Open, Status::Pending],
            _ => &[Status::Open],
        };
        let Some(conversation) = self.pick(uow, statuses, on, true).await? else {
            return Ok(None);
        };
        let Some(agent) = Agent::query()
            .find(uow, conversation.assignee_id.unwrap_or(0))
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(None);
        };
        let (kind, body, status) = match what {
            Happening::TeammateReply => (
                Author::Agent,
                ANSWERS[usize::try_from(self.roll(ANSWERS.len() as u64)).unwrap_or(0)].to_owned(),
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
        Message::create(
            uow,
            Message {
                id: 0,
                conversation_id: conversation.id,
                kind: kind.key().to_owned(),
                author: agent.name.clone(),
                agent_id: Some(agent.id),
                body: body.clone(),
                at: now,
            },
        )
        .await?;
        let answered = matches!(kind, Author::Agent) || status == Status::Closed;
        let rev = next_rev(uow).await?;
        let mut sets = vec![
            Conversation::STATUS.set(status.key().to_owned()),
            Conversation::INBOX.set(inbox_of(status.key()).to_owned()),
            Conversation::LAST_AT.set(now),
            Conversation::LAST_REV.set(rev),
            Conversation::REV.set(rev),
            Conversation::MESSAGES.set_expr(plus_one("messages")),
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
            .update(uow, sets)
            .await?;
        if status == Status::Closed {
            CustomerEvent::create(
                uow,
                CustomerEvent {
                    id: 0,
                    customer_id: conversation.customer_id,
                    kind: EventKind::TicketClosed.key().to_owned(),
                    at: now,
                    title: format!("Resolved: {}", conversation.subject),
                    detail: format!("Closed by {}", agent.name),
                },
            )
            .await?;
        }
        Ok(Some(Change::of(conversation.id, conversation.customer_id)))
    }

    /// Brings back every snoozed conversation whose day has come by `now` in the desk's
    /// zone: open again, its clock started, a line in it that says so. A few hundred to a
    /// transaction, so a writer never waits long. Returns how many came back.
    pub async fn wake(&self, db: &Db, now: i64) -> Result<usize> {
        let Some(today) = day_of(now) else {
            return Ok(0);
        };
        let mut woken = 0;
        loop {
            let done = Self::in_uow(db, |uow| async move {
                let due: Vec<Conversation> = Conversation::query()
                    .where_(Conversation::INBOX.eq("snoozed"))
                    .where_(Conversation::SNOOZED_UNTIL.lte(Some(today)))
                    .order_by("id")
                    .limit(WAKE_AT_ONCE)
                    .get(&uow)
                    .await?
                    .into_vec()
                    .into_iter()
                    .map(|record| record.into_inner())
                    .collect();
                for conversation in &due {
                    let day = conversation
                        .snoozed_until
                        .map_or_else(String::new, |day| day.format("%A, %-d %B").to_string());
                    Message::create(
                        &uow,
                        Message {
                            id: 0,
                            conversation_id: conversation.id,
                            kind: Author::Event.key().to_owned(),
                            author: "Snooze".to_owned(),
                            agent_id: None,
                            body: format!("Back from snooze: it was put aside until {day}"),
                            at: now,
                        },
                    )
                    .await?;
                    let target = Priority::parse(&conversation.priority)
                        .unwrap_or(Priority::Normal)
                        .target();
                    let rev = next_rev(&uow).await?;
                    Conversation::query()
                        .where_(Conversation::ID.eq(conversation.id))
                        .update(
                            &uow,
                            [
                                Conversation::STATUS.set(Status::Open.key().to_owned()),
                                Conversation::INBOX.set("open".to_owned()),
                                Conversation::SNOOZED_UNTIL.set(None::<chrono::NaiveDate>),
                                Conversation::LAST_AT.set(now),
                                Conversation::LAST_REV.set(rev),
                                Conversation::SLA_DUE.set(Some(now + target)),
                                Conversation::SLA_FROM.set(Some(now)),
                                Conversation::REV.set(rev),
                                Conversation::MESSAGES.set_expr(plus_one("messages")),
                            ],
                        )
                        .await?;
                    self.tell(Change::of(conversation.id, conversation.customer_id))
                        .await;
                }
                Ok(due.len())
            })
            .await?;
            woken += done;
            if (done as u64) < WAKE_AT_ONCE {
                return Ok(woken);
            }
        }
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
            self.tell(Change::TICK).await;
        }
    }

    /// Wakes snoozed conversations whose day has come, every `every`, whether or not
    /// the clock runs. Not while the desk is written afresh.
    pub async fn wake_up(self: Arc<Self>, db: Arc<Db>, every: Duration) {
        loop {
            {
                let _one = self.writing.lock().await;
                if let Err(error) = self.wake(&db, now()).await {
                    eprintln!("inbox-demo: snoozed conversations could not come back: {error}");
                }
            }
            tokio::time::sleep(every).await;
        }
    }
}

/// The time the clock is held at, in unix seconds; 0 when it runs.
static HELD: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Holds the clock at `at` (unix seconds) for every page and everything the
/// app writes: for a test that compares two runs of the app.
pub fn hold_clock(at: i64) {
    HELD.store(at, std::sync::atomic::Ordering::Relaxed);
}

/// Now, in unix seconds.
pub fn now() -> i64 {
    match HELD.load(std::sync::atomic::Ordering::Relaxed) {
        0 => chrono::Utc::now().timestamp(),
        held => held,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_change_tells_the_lists_and_only_the_parts_it_touched() {
        // The topics replace the page streams' filters.
        assert_eq!(
            topics_of(Change::of(7, 3)),
            ["conversations", "conversation.7", "customer.3"]
        );
        assert_eq!(
            topics_of(Change::read(7, 3)),
            ["conversations"],
            "being read changes only what the lists show"
        );
        assert_eq!(topics_of(Change::TICK), ["conversations", "clock"]);
        assert_eq!(
            page_stream(Some((7, 3))).href().as_str(),
            "/broadcasting/stream?topics=conversations,conversation.7,customer.3,clock"
        );
        assert_eq!(
            page_stream(None).href().as_str(),
            "/broadcasting/stream?topics=conversations"
        );
    }
}
