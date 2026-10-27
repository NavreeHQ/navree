//! The models of the lifecycle tests, and their SQLite tables.

#![allow(dead_code)]

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use navree_db::Db;
use navree_db::sea_orm::ConnectionTrait;
use navree_orm::hooks::Flow;
use navree_orm::{ExecContext, Model, Observer, Record, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The lifecycle tables, as one script.
pub const SCHEMA: &str = r#"
CREATE TABLE tickets (
    id BLOB PRIMARY KEY,
    subject TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE audit_rows (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry TEXT NOT NULL
);
CREATE TABLE gadgets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    active BOOLEAN NOT NULL,
    archived BOOLEAN NOT NULL,
    level TEXT NOT NULL
);
CREATE TABLE probes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    reading INTEGER NOT NULL,
    note TEXT NULL,
    level TEXT NOT NULL
);
CREATE TABLE log_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    reference TEXT NOT NULL,
    total_cents INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NULL
);
"#;

/// Creates the lifecycle tables.
pub async fn create_tables(db: &Db) {
    for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        db.conn()
            .execute_unprepared(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// What model-declared observers saw, across tests: each test reads only
/// the lines of its own records (their subjects are unique).
pub static TICKET_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The lines about `subject`, taken out of [`TICKET_LOG`].
pub fn ticket_log(subject: &str) -> Vec<String> {
    let mut log = TICKET_LOG.lock().unwrap();
    let (mine, others): (Vec<String>, Vec<String>) = log
        .drain(..)
        .partition(|line| line.ends_with(&format!(" {subject}")));
    *log = others;
    mine
}

/// A support ticket: its observer is declared on the model.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, observers(TicketObserver))]
pub struct Ticket {
    pub id: Uuid,
    pub subject: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Opens new tickets, refuses empty subjects and notes what happened.
pub struct TicketObserver;

fn note(event: &str, ticket: &Record<Ticket>) {
    TICKET_LOG
        .lock()
        .unwrap()
        .push(format!("{event} {}", ticket.subject));
}

impl Observer<Ticket> for TicketObserver {
    async fn saving(&self, ticket: &mut Record<Ticket>, _ctx: &ExecContext) -> Result<Flow> {
        note("saving", ticket);
        Ok(if ticket.subject.trim().is_empty() {
            Flow::Cancel
        } else {
            Flow::Continue
        })
    }

    async fn creating(&self, ticket: &mut Record<Ticket>, _ctx: &ExecContext) -> Result<Flow> {
        if ticket.status.is_empty() {
            ticket.status = "open".into();
        }
        note("creating", ticket);
        Ok(Flow::Continue)
    }

    async fn created(&self, ticket: &Record<Ticket>, _ctx: &ExecContext) -> Result<()> {
        note("created", ticket);
        Ok(())
    }

    async fn saved(&self, ticket: &Record<Ticket>, _ctx: &ExecContext) -> Result<()> {
        note("saved", ticket);
        Ok(())
    }

    async fn custom(
        &self,
        event: &'static str,
        ticket: &mut Record<Ticket>,
        _ctx: &ExecContext,
    ) -> Result<Flow> {
        note(event, ticket);
        Ok(if ticket.status == "frozen" {
            Flow::Cancel
        } else {
            Flow::Continue
        })
    }
}

/// How serious a gadget is: no `Default`, so a missing `level` column has
/// nothing to fall back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    High,
}

/// The model the runtime global-scope tests register scopes for (scopes
/// registered at run time are process-wide, so no other test uses it).
#[derive(Model, Clone, Debug, PartialEq, Serialize)]
pub struct Gadget {
    pub id: i64,
    pub name: String,
    pub active: bool,
    pub archived: bool,
    #[model(cast = "string_enum")]
    pub level: Level,
}

impl Gadget {
    pub fn new(name: &str, active: bool, archived: bool) -> Gadget {
        Gadget {
            id: 0,
            name: name.into(),
            active,
            archived,
            level: Level::Low,
        }
    }
}

/// Gadgets that are not archived: a scope registered by value.
pub struct NotArchived;

impl navree_orm::GlobalScope<Gadget> for NotArchived {
    fn apply(&self, query: navree_orm::Builder<Gadget>) -> navree_orm::Builder<Gadget> {
        query.where_(Gadget::ARCHIVED.eq(false))
    }
}

/// A model for partial selects: `level` has no default.
#[derive(Model, Clone, Debug, PartialEq, Serialize)]
pub struct Probe {
    pub id: i64,
    pub name: String,
    pub reading: i64,
    pub note: Option<String>,
    #[model(cast = "string_enum")]
    pub level: Level,
}

/// Before this, records are old enough to prune.
pub fn prune_cutoff() -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
}

/// What `LogEntry::pruning` saw, across tests (each test uses its own
/// messages).
pub static PRUNING_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The lines of `PRUNING_LOG` starting with `prefix`, taken out.
pub fn pruned_with_prefix(prefix: &str) -> Vec<String> {
    let mut log = PRUNING_LOG.lock().unwrap();
    let (mine, others): (Vec<String>, Vec<String>) =
        log.drain(..).partition(|line| line.starts_with(prefix));
    *log = others;
    mine
}

/// A log line, pruned record by record once it is old.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, soft_deletes, prunable)]
pub struct LogEntry {
    pub id: i64,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl navree_orm::prune::Prunable for LogEntry {
    fn prunable(query: navree_orm::Builder<Self>) -> navree_orm::Builder<Self> {
        query.where_(LogEntry::CREATED_AT.lt(prune_cutoff()))
    }

    async fn pruning(&self, _ctx: &ExecContext) -> Result<()> {
        if self.message.ends_with("explode") {
            return Err(navree_orm::OrmError::Hook {
                model: "LogEntry".into(),
                message: format!("cannot archive {}", self.message),
            });
        }
        PRUNING_LOG.lock().unwrap().push(self.message.clone());
        Ok(())
    }
}

/// A measurement, pruned a chunk per statement once it is old.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(mass_prunable)]
pub struct Metric {
    pub id: i64,
    pub name: String,
    pub recorded_at: DateTime<Utc>,
}

impl navree_orm::prune::MassPrunable for Metric {
    const CHUNK_SIZE: Option<u64> = Some(2);

    fn prunable(query: navree_orm::Builder<Self>) -> navree_orm::Builder<Self> {
        query.where_(Metric::RECORDED_AT.lt(prune_cutoff()))
    }
}

/// An order: its lifecycle reaches the app as typed events.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    timestamps,
    dispatches(saving = OrderSaving, created = OrderPlaced, deleted = OrderRemoved)
)]
pub struct Order {
    pub id: i64,
    pub reference: String,
    pub total_cents: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An order was placed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderPlaced {
    pub id: i64,
    pub reference: String,
    pub total_cents: i64,
}

impl navree_events::Event for OrderPlaced {
    const NAME: &'static str = "order.placed";
}

impl From<&Record<Order>> for OrderPlaced {
    fn from(order: &Record<Order>) -> Self {
        OrderPlaced {
            id: order.id,
            reference: order.reference.clone(),
            total_cents: order.total_cents,
        }
    }
}

/// An order was removed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderRemoved {
    pub reference: String,
}

impl navree_events::Event for OrderRemoved {
    const NAME: &'static str = "order.removed";
}

impl From<&Record<Order>> for OrderRemoved {
    fn from(order: &Record<Order>) -> Self {
        OrderRemoved {
            reference: order.reference.clone(),
        }
    }
}

/// An order is about to be saved (a listener may refuse it by failing).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderSaving {
    pub reference: String,
}

impl navree_events::Event for OrderSaving {
    const NAME: &'static str = "order.saving";
}

impl From<&Record<Order>> for OrderSaving {
    fn from(order: &Record<Order>) -> Self {
        OrderSaving {
            reference: order.reference.clone(),
        }
    }
}

/// A soft-deleting model with a unique column.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, soft_deletes)]
pub struct Member {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A row an in-transaction observer writes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(table = "audit_rows")]
pub struct AuditRow {
    pub id: i64,
    pub entry: String,
}
