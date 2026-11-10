//! The `micro` migration set and the tables behind the outbox, the inbox
//! and dead letters.

use std::collections::BTreeMap;

use bytes::Bytes;
use chrono::{DateTime, TimeZone, Utc};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{Db, DbError, Executor, MigrationSet, Row, Statement, Uow};
use serde::de::DeserializeOwned;

use crate::envelope::now_millis;
use crate::{Codec, Envelope};

/// Events emitted inside a unit of work, waiting to be published.
pub const OUTBOX_TABLE: &str = "micro_outbox";

/// Handled message ids (and the stored replies of `dedupe` handlers).
pub const INBOX_TABLE: &str = "micro_inbox";

/// Events that used up their attempts.
pub const DEAD_LETTERS_TABLE: &str = "micro_dead_letters";

/// The migrations `MicroModule` contributes when the app has
/// `DatabaseModule` (set `micro`).
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new("micro").migration(SchemaMigration::new(
        "m20261026_000001_create_micro_tables",
        CreateMicroTables,
    ))
}

/// Creates the outbox, the inbox and the dead letters, each with the index
/// its cleanup reads.
struct CreateMicroTables;

impl Migration for CreateMicroTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(OUTBOX_TABLE, |t| {
                t.string("id").primary();
                t.string("pattern");
                t.text("headers");
                t.binary("payload");
                t.big_integer("seq");
                t.big_integer("created_at");
                t.index(["created_at"])
                    .name("micro_outbox_created_at_index");
            })
            .await?;
        schema
            .create(INBOX_TABLE, |t| {
                t.string("id").primary();
                t.string("message_id");
                t.string("consumer");
                t.string("pattern");
                t.integer("reply_status").nullable();
                t.string("reply_content_type").nullable();
                t.binary("reply").nullable();
                t.big_integer("created_at");
                t.index(["created_at"]).name("micro_inbox_created_at_index");
            })
            .await?;
        schema
            .create(DEAD_LETTERS_TABLE, |t| {
                t.string("id").primary();
                t.string("service");
                t.string("pattern");
                t.string("handler_pattern");
                t.string("message_id");
                t.text("headers");
                t.binary("payload");
                t.big_integer("attempts");
                t.text("error");
                t.big_integer("failed_at");
                t.index(["service", "failed_at"])
                    .name("micro_dead_letters_service_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in [DEAD_LETTERS_TABLE, INBOX_TABLE, OUTBOX_TABLE] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}

/// An event that used up its attempts (or could never succeed: a payload
/// that does not decode or validate, a guard that refuses it).
#[derive(Clone, Debug, PartialEq)]
pub struct DeadLetter {
    /// The dead letter's own id (a UUID v7), for `micro:retry <id>`.
    pub id: String,
    /// The service that gave up (`micro.service`).
    pub service: String,
    /// The event's pattern (`deal.won`).
    pub pattern: String,
    /// The pattern of the handler that failed (`deal.*` for a wildcard
    /// handler).
    pub handler_pattern: String,
    /// The event's message id.
    pub message_id: String,
    /// The event's headers.
    pub headers: BTreeMap<String, String>,
    /// The encoded payload.
    pub payload: Bytes,
    /// Attempts made.
    pub attempts: u32,
    /// The last error.
    pub error: String,
    /// When the event was dead-lettered.
    pub failed_at: DateTime<Utc>,
}

impl DeadLetter {
    /// The envelope of the event, as it was received.
    pub fn envelope(&self) -> Envelope {
        let mut envelope = Envelope::new(self.pattern.clone(), self.payload.clone());
        for (name, value) in &self.headers {
            envelope.set_header(name, value.clone());
        }
        envelope
    }

    /// The payload decoded with its content type into `T`.
    pub fn decode<T: DeserializeOwned>(&self) -> Option<T> {
        let codec = Codec::from_content_type(
            self.headers
                .get(crate::headers::CONTENT_TYPE)
                .map_or("application/json", String::as_str),
        )?;
        codec.decode(&self.payload).ok()
    }

    /// The payload as JSON (`null` when it does not decode).
    pub fn payload_json(&self) -> serde_json::Value {
        self.decode().unwrap_or(serde_json::Value::Null)
    }
}

/// A stored reply of a `dedupe` handler.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StoredReply {
    pub(crate) status: u16,
    pub(crate) content_type: String,
    pub(crate) body: Bytes,
}

/// An inbox row: the message was handled (with its reply, for `dedupe`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InboxEntry {
    pub(crate) reply: Option<StoredReply>,
}

/// An event in the outbox.
#[derive(Clone, Debug)]
pub(crate) struct OutboxRow {
    pub(crate) id: String,
    pub(crate) envelope: Envelope,
}

/// SQL on the micro tables, on SQLite, PostgreSQL and MySQL.
#[derive(Clone, Debug)]
pub(crate) struct Store {
    db: Db,
}

impl Store {
    pub(crate) fn new(db: Db) -> Self {
        Self { db }
    }

    pub(crate) fn db(&self) -> &Db {
        &self.db
    }

    /// What the store's own statements run on: the writer's pool, outside
    /// the units of work of the handlers (the relay and the after-commit
    /// cleanups are their own transactions), or the connection itself when
    /// it has no pool.
    fn pool(&self) -> &dyn Executor {
        self.db.pool()
    }

    /// Writes `envelope` to the outbox in the transaction of `uow`.
    pub(crate) async fn outbox_insert(
        &self,
        uow: &Uow,
        envelope: &Envelope,
        seq: i64,
    ) -> Result<String, DbError> {
        let id = envelope
            .message_id()
            .map_or_else(|| uuid::Uuid::now_v7().to_string(), str::to_owned);
        let headers = serde_json::to_string(envelope.headers())
            .map_err(|error| DbError::invalid(error.to_string()))?;
        uow.execute(&Statement::portable(
            format!(
                "INSERT INTO {OUTBOX_TABLE} (id, pattern, headers, payload, seq, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)"
            ),
            [
                id.clone().into(),
                envelope.pattern().into(),
                headers.into(),
                envelope.payload().to_vec().into(),
                seq.into(),
                now_millis().into(),
            ],
        ))
        .await?;
        Ok(id)
    }

    /// Removes an event the relay published.
    pub(crate) async fn outbox_delete(&self, id: &str) -> Result<(), DbError> {
        self.outbox_delete_on(self.pool(), id).await
    }

    /// Outbox rows older than `before` (Unix ms), oldest first, claimed in
    /// the transaction `executor` is (`FOR UPDATE SKIP LOCKED` on PostgreSQL
    /// and MySQL).
    pub(crate) async fn outbox_due<E: Executor + ?Sized>(
        &self,
        executor: &E,
        before: i64,
        limit: u64,
    ) -> Result<Vec<OutboxRow>, DbError> {
        let lock = if executor.backend().supports_row_locks() {
            " FOR UPDATE SKIP LOCKED"
        } else {
            ""
        };
        // `limit` is a u64 and `lock` a fixed fragment; every value is a bound parameter.
        // nosemgrep: navree-sql-built-from-strings
        let rows = executor
            .fetch_all(&Statement::portable(
                format!(
                    "SELECT id, pattern, headers, payload FROM {OUTBOX_TABLE} \
                     WHERE created_at < ? ORDER BY created_at, seq, id LIMIT {limit}{lock}"
                ),
                [before.into()],
            ))
            .await?;
        rows.iter()
            .map(|row| {
                let id: String = row.get("id")?;
                let envelope = envelope_of(row)?;
                Ok(OutboxRow { id, envelope })
            })
            .collect()
    }

    /// Deletes outbox row `id` through `executor`.
    pub(crate) async fn outbox_delete_on<E: Executor + ?Sized>(
        &self,
        executor: &E,
        id: &str,
    ) -> Result<(), DbError> {
        executor
            .execute(&Statement::portable(
                format!("DELETE FROM {OUTBOX_TABLE} WHERE id = ?"),
                [id.into()],
            ))
            .await?;
        Ok(())
    }

    /// The inbox row of `message_id` for `consumer`, read through `uow`.
    pub(crate) async fn inbox_find(
        &self,
        uow: &Uow,
        consumer: &str,
        message_id: &str,
    ) -> Result<Option<InboxEntry>, DbError> {
        let row = uow
            .fetch_optional(&Statement::portable(
                format!(
                    "SELECT reply_status, reply_content_type, reply FROM {INBOX_TABLE} WHERE id = ?"
                ),
                [inbox_id(consumer, message_id).into()],
            ))
            .await?;
        row.map(|row| {
            let status: Option<i32> = row.get("reply_status")?;
            let content_type: Option<String> = row.get("reply_content_type")?;
            let body: Option<Vec<u8>> = row.get("reply")?;
            let reply = match (status, body) {
                (Some(status), Some(body)) => Some(StoredReply {
                    status: u16::try_from(status).unwrap_or(200),
                    content_type: content_type.unwrap_or_else(|| "application/json".to_owned()),
                    body: body.into(),
                }),
                _ => None,
            };
            Ok(InboxEntry { reply })
        })
        .transpose()
    }

    /// Records that `consumer` handled `message_id`, in the transaction of
    /// `uow`.
    pub(crate) async fn inbox_insert(
        &self,
        uow: &Uow,
        consumer: &str,
        message_id: &str,
        pattern: &str,
        reply: Option<&StoredReply>,
    ) -> Result<(), DbError> {
        uow.execute(&Statement::portable(
            format!(
                "INSERT INTO {INBOX_TABLE} \
                 (id, message_id, consumer, pattern, reply_status, reply_content_type, reply, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            ),
            [
                inbox_id(consumer, message_id).into(),
                message_id.into(),
                consumer.into(),
                pattern.into(),
                reply.map(|r| i32::from(r.status)).into(),
                reply.map(|r| r.content_type.clone()).into(),
                reply.map(|r| r.body.to_vec()).into(),
                now_millis().into(),
            ],
        ))
        .await?;
        Ok(())
    }

    /// Deletes inbox rows created before `before` (Unix ms).
    pub(crate) async fn inbox_prune(&self, before: i64) -> Result<u64, DbError> {
        Ok(self
            .pool()
            .execute(&Statement::portable(
                format!("DELETE FROM {INBOX_TABLE} WHERE created_at < ?"),
                [before.into()],
            ))
            .await?
            .rows_affected)
    }

    /// Stores a dead letter.
    pub(crate) async fn dead_insert(&self, letter: &DeadLetter) -> Result<(), DbError> {
        let headers = serde_json::to_string(&letter.headers)
            .map_err(|error| DbError::invalid(error.to_string()))?;
        self.pool()
            .execute(&Statement::portable(
                format!(
                    "INSERT INTO {DEAD_LETTERS_TABLE} \
                     (id, service, pattern, handler_pattern, message_id, headers, payload, attempts, error, failed_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                ),
                [
                    letter.id.clone().into(),
                    letter.service.clone().into(),
                    letter.pattern.clone().into(),
                    letter.handler_pattern.clone().into(),
                    letter.message_id.clone().into(),
                    headers.into(),
                    letter.payload.to_vec().into(),
                    i64::from(letter.attempts).into(),
                    letter.error.clone().into(),
                    letter.failed_at.timestamp_millis().into(),
                ],
            ))
            .await?;
        Ok(())
    }

    /// The dead letters of `service`, oldest first.
    pub(crate) async fn dead_list(&self, service: &str) -> Result<Vec<DeadLetter>, DbError> {
        let rows = self
            .pool()
            .fetch_all(&Statement::portable(
                format!(
                    "SELECT * FROM {DEAD_LETTERS_TABLE} WHERE service = ? ORDER BY failed_at, id"
                ),
                [service.into()],
            ))
            .await?;
        rows.iter().map(dead_letter_of).collect()
    }

    /// Dead letter `id` of `service`.
    pub(crate) async fn dead_find(
        &self,
        service: &str,
        id: &str,
    ) -> Result<Option<DeadLetter>, DbError> {
        let row = self
            .pool()
            .fetch_optional(&Statement::portable(
                format!("SELECT * FROM {DEAD_LETTERS_TABLE} WHERE service = ? AND id = ?"),
                [service.into(), id.into()],
            ))
            .await?;
        row.as_ref().map(dead_letter_of).transpose()
    }

    /// Deletes dead letter `id`; whether it existed.
    pub(crate) async fn dead_delete(&self, id: &str) -> Result<bool, DbError> {
        Ok(self
            .pool()
            .execute(&Statement::portable(
                format!("DELETE FROM {DEAD_LETTERS_TABLE} WHERE id = ?"),
                [id.into()],
            ))
            .await?
            .rows_affected
            == 1)
    }

    /// Deletes every dead letter of `service`.
    pub(crate) async fn dead_flush(&self, service: &str) -> Result<u64, DbError> {
        Ok(self
            .pool()
            .execute(&Statement::portable(
                format!("DELETE FROM {DEAD_LETTERS_TABLE} WHERE service = ?"),
                [service.into()],
            ))
            .await?
            .rows_affected)
    }
}

/// The inbox key: a message is handled once per consumer (service and
/// handler pattern), so services sharing a database keep separate rows.
fn inbox_id(consumer: &str, message_id: &str) -> String {
    format!("{consumer}|{message_id}")
}

fn envelope_of(row: &Row) -> Result<Envelope, DbError> {
    let pattern: String = row.get("pattern")?;
    let headers: String = row.get("headers")?;
    let payload: Vec<u8> = row.get("payload")?;
    let headers: BTreeMap<String, String> =
        serde_json::from_str(&headers).map_err(|error| DbError::invalid(error.to_string()))?;
    let mut envelope = Envelope::new(pattern, payload);
    for (name, value) in headers {
        envelope.set_header(&name, value);
    }
    Ok(envelope)
}

fn dead_letter_of(row: &Row) -> Result<DeadLetter, DbError> {
    let headers: String = row.get("headers")?;
    let payload: Vec<u8> = row.get("payload")?;
    let attempts: i64 = row.get("attempts")?;
    let failed_at: i64 = row.get("failed_at")?;
    Ok(DeadLetter {
        id: row.get("id")?,
        service: row.get("service")?,
        pattern: row.get("pattern")?,
        handler_pattern: row.get("handler_pattern")?,
        message_id: row.get("message_id")?,
        headers: serde_json::from_str(&headers)
            .map_err(|error| DbError::invalid(error.to_string()))?,
        payload: payload.into(),
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        error: row.get("error")?,
        failed_at: Utc
            .timestamp_millis_opt(failed_at)
            .single()
            .unwrap_or_else(Utc::now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_letters_rebuild_their_envelope() {
        let letter = DeadLetter {
            id: "d1".into(),
            service: "notifications".into(),
            pattern: "deal.won".into(),
            handler_pattern: "deal.*".into(),
            message_id: "m1".into(),
            headers: BTreeMap::from([
                ("navree-message-id".to_owned(), "m1".to_owned()),
                ("content-type".to_owned(), "application/json".to_owned()),
            ]),
            payload: Bytes::from_static(br#"{"deal_id":1}"#),
            attempts: 5,
            error: "smtp refused".into(),
            failed_at: Utc::now(),
        };
        let envelope = letter.envelope();
        assert_eq!(envelope.pattern(), "deal.won");
        assert_eq!(envelope.message_id(), Some("m1"));
        assert_eq!(letter.payload_json()["deal_id"], 1);
        assert_eq!(inbox_id("svc|deal.won", "m1"), "svc|deal.won|m1");
    }
}
