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

/// A row an in-transaction observer writes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(table = "audit_rows")]
pub struct AuditRow {
    pub id: i64,
    pub entry: String,
}
