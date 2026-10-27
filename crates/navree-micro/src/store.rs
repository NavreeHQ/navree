//! The `micro` migration set and the tables behind the outbox, the inbox
//! and dead letters.

use std::collections::BTreeMap;

use bytes::Bytes;
use chrono::{DateTime, TimeZone, Utc};
use navree_db::{Db, MigrationSet, Uow};
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, QueryResult, Statement, Value};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
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
    MigrationSet::new("micro").migration(CreateMicroTables)
}

struct CreateMicroTables;

impl MigrationName for CreateMicroTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_micro_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateMicroTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(OUTBOX_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("pattern").string().not_null())
                    .col(ColumnDef::new("headers").text().not_null())
                    .col(ColumnDef::new("payload").blob().not_null())
                    .col(ColumnDef::new("seq").big_integer().not_null())
                    .col(ColumnDef::new("created_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("micro_outbox_created_at_index")
                    .table(OUTBOX_TABLE)
                    .col("created_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(INBOX_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("message_id").string().not_null())
                    .col(ColumnDef::new("consumer").string().not_null())
                    .col(ColumnDef::new("pattern").string().not_null())
                    .col(ColumnDef::new("reply_status").integer().null())
                    .col(ColumnDef::new("reply_content_type").string().null())
                    .col(ColumnDef::new("reply").blob().null())
                    .col(ColumnDef::new("created_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("micro_inbox_created_at_index")
                    .table(INBOX_TABLE)
                    .col("created_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(DEAD_LETTERS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("service").string().not_null())
                    .col(ColumnDef::new("pattern").string().not_null())
                    .col(ColumnDef::new("handler_pattern").string().not_null())
                    .col(ColumnDef::new("message_id").string().not_null())
                    .col(ColumnDef::new("headers").text().not_null())
                    .col(ColumnDef::new("payload").blob().not_null())
                    .col(ColumnDef::new("attempts").big_integer().not_null())
                    .col(ColumnDef::new("error").text().not_null())
                    .col(ColumnDef::new("failed_at").big_integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("micro_dead_letters_service_index")
                    .table(DEAD_LETTERS_TABLE)
                    .col("service")
                    .col("failed_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [DEAD_LETTERS_TABLE, INBOX_TABLE, OUTBOX_TABLE] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
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

/// SQL on the micro tables, for SQLite and PostgreSQL.
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

    fn backend(&self) -> DbBackend {
        self.db.backend()
    }

    /// Writes `envelope` to the outbox in the transaction of `uow`.
    pub(crate) async fn outbox_insert(
        &self,
        uow: &Uow,
        envelope: &Envelope,
        seq: i64,
    ) -> Result<String, DbErr> {
        let id = envelope
            .message_id()
            .map_or_else(|| uuid::Uuid::now_v7().to_string(), str::to_owned);
        let headers = serde_json::to_string(envelope.headers())
            .map_err(|error| DbErr::Custom(error.to_string()))?;
        let tx = uow.tx().await?;
        tx.execute_raw(statement(
            self.backend(),
            &format!(
                "INSERT INTO {OUTBOX_TABLE} (id, pattern, headers, payload, seq, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)"
            ),
            vec![
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
    pub(crate) async fn outbox_delete(&self, id: &str) -> Result<(), DbErr> {
        self.db
            .conn()
            .execute_raw(statement(
                self.backend(),
                &format!("DELETE FROM {OUTBOX_TABLE} WHERE id = ?"),
                vec![id.into()],
            ))
            .await?;
        Ok(())
    }

    /// Outbox rows older than `before` (Unix ms), oldest first, claimed in
    /// `tx` (`FOR UPDATE SKIP LOCKED` on PostgreSQL).
    pub(crate) async fn outbox_due<C: ConnectionTrait>(
        &self,
        conn: &C,
        before: i64,
        limit: u64,
    ) -> Result<Vec<OutboxRow>, DbErr> {
        let lock = if self.backend() == DbBackend::Postgres {
            " FOR UPDATE SKIP LOCKED"
        } else {
            ""
        };
        let rows = conn
            .query_all_raw(statement(
                self.backend(),
                &format!(
                    "SELECT id, pattern, headers, payload FROM {OUTBOX_TABLE} \
                     WHERE created_at < ? ORDER BY created_at, seq, id LIMIT {limit}{lock}"
                ),
                vec![before.into()],
            ))
            .await?;
        rows.iter()
            .map(|row| {
                let id: String = row.try_get("", "id")?;
                let envelope = envelope_of(row)?;
                Ok(OutboxRow { id, envelope })
            })
            .collect()
    }

    /// Deletes outbox row `id` through `conn`.
    pub(crate) async fn outbox_delete_on<C: ConnectionTrait>(
        &self,
        conn: &C,
        id: &str,
    ) -> Result<(), DbErr> {
        conn.execute_raw(statement(
            self.backend(),
            &format!("DELETE FROM {OUTBOX_TABLE} WHERE id = ?"),
            vec![id.into()],
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
    ) -> Result<Option<InboxEntry>, DbErr> {
        let tx = uow.tx().await?;
        let row = tx
            .query_one_raw(statement(
                self.backend(),
                &format!(
                    "SELECT reply_status, reply_content_type, reply FROM {INBOX_TABLE} WHERE id = ?"
                ),
                vec![inbox_id(consumer, message_id).into()],
            ))
            .await?;
        row.map(|row| {
            let status: Option<i32> = row.try_get("", "reply_status")?;
            let content_type: Option<String> = row.try_get("", "reply_content_type")?;
            let body: Option<Vec<u8>> = row.try_get("", "reply")?;
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
    ) -> Result<(), DbErr> {
        let tx = uow.tx().await?;
        tx.execute_raw(statement(
            self.backend(),
            &format!(
                "INSERT INTO {INBOX_TABLE} \
                 (id, message_id, consumer, pattern, reply_status, reply_content_type, reply, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            ),
            vec![
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
    pub(crate) async fn inbox_prune(&self, before: i64) -> Result<u64, DbErr> {
        Ok(self
            .db
            .conn()
            .execute_raw(statement(
                self.backend(),
                &format!("DELETE FROM {INBOX_TABLE} WHERE created_at < ?"),
                vec![before.into()],
            ))
            .await?
            .rows_affected())
    }

    /// Stores a dead letter.
    pub(crate) async fn dead_insert(&self, letter: &DeadLetter) -> Result<(), DbErr> {
        let headers = serde_json::to_string(&letter.headers)
            .map_err(|error| DbErr::Custom(error.to_string()))?;
        self.db
            .conn()
            .execute_raw(statement(
                self.backend(),
                &format!(
                    "INSERT INTO {DEAD_LETTERS_TABLE} \
                     (id, service, pattern, handler_pattern, message_id, headers, payload, attempts, error, failed_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                ),
                vec![
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
    pub(crate) async fn dead_list(&self, service: &str) -> Result<Vec<DeadLetter>, DbErr> {
        let rows = self
            .db
            .conn()
            .query_all_raw(statement(
                self.backend(),
                &format!(
                    "SELECT * FROM {DEAD_LETTERS_TABLE} WHERE service = ? ORDER BY failed_at, id"
                ),
                vec![service.into()],
            ))
            .await?;
        rows.iter().map(dead_letter_of).collect()
    }

    /// Dead letter `id` of `service`.
    pub(crate) async fn dead_find(
        &self,
        service: &str,
        id: &str,
    ) -> Result<Option<DeadLetter>, DbErr> {
        let row = self
            .db
            .conn()
            .query_one_raw(statement(
                self.backend(),
                &format!("SELECT * FROM {DEAD_LETTERS_TABLE} WHERE service = ? AND id = ?"),
                vec![service.into(), id.into()],
            ))
            .await?;
        row.as_ref().map(dead_letter_of).transpose()
    }

    /// Deletes dead letter `id`; whether it existed.
    pub(crate) async fn dead_delete(&self, id: &str) -> Result<bool, DbErr> {
        Ok(self
            .db
            .conn()
            .execute_raw(statement(
                self.backend(),
                &format!("DELETE FROM {DEAD_LETTERS_TABLE} WHERE id = ?"),
                vec![id.into()],
            ))
            .await?
            .rows_affected()
            == 1)
    }

    /// Deletes every dead letter of `service`.
    pub(crate) async fn dead_flush(&self, service: &str) -> Result<u64, DbErr> {
        Ok(self
            .db
            .conn()
            .execute_raw(statement(
                self.backend(),
                &format!("DELETE FROM {DEAD_LETTERS_TABLE} WHERE service = ?"),
                vec![service.into()],
            ))
            .await?
            .rows_affected())
    }
}

/// The inbox key: a message is handled once per consumer (service and
/// handler pattern), so services sharing a database keep separate rows.
fn inbox_id(consumer: &str, message_id: &str) -> String {
    format!("{consumer}|{message_id}")
}

fn envelope_of(row: &QueryResult) -> Result<Envelope, DbErr> {
    let pattern: String = row.try_get("", "pattern")?;
    let headers: String = row.try_get("", "headers")?;
    let payload: Vec<u8> = row.try_get("", "payload")?;
    let headers: BTreeMap<String, String> =
        serde_json::from_str(&headers).map_err(|error| DbErr::Custom(error.to_string()))?;
    let mut envelope = Envelope::new(pattern, payload);
    for (name, value) in headers {
        envelope.set_header(&name, value);
    }
    Ok(envelope)
}

fn dead_letter_of(row: &QueryResult) -> Result<DeadLetter, DbErr> {
    let headers: String = row.try_get("", "headers")?;
    let payload: Vec<u8> = row.try_get("", "payload")?;
    let attempts: i64 = row.try_get("", "attempts")?;
    let failed_at: i64 = row.try_get("", "failed_at")?;
    Ok(DeadLetter {
        id: row.try_get("", "id")?,
        service: row.try_get("", "service")?,
        pattern: row.try_get("", "pattern")?,
        handler_pattern: row.try_get("", "handler_pattern")?,
        message_id: row.try_get("", "message_id")?,
        headers: serde_json::from_str(&headers)
            .map_err(|error| DbErr::Custom(error.to_string()))?,
        payload: payload.into(),
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        error: row.try_get("", "error")?,
        failed_at: Utc
            .timestamp_millis_opt(failed_at)
            .single()
            .unwrap_or_else(Utc::now),
    })
}

/// `?` placeholders, numbered for PostgreSQL.
pub(crate) fn statement(backend: DbBackend, sql: &str, values: Vec<Value>) -> Statement {
    let sql = if backend == DbBackend::Postgres {
        let mut numbered = String::with_capacity(sql.len() + 8);
        let mut n = 0;
        for c in sql.chars() {
            if c == '?' {
                n += 1;
                numbered.push('$');
                numbered.push_str(&n.to_string());
            } else {
                numbered.push(c);
            }
        }
        numbered
    } else {
        sql.to_owned()
    };
    Statement::from_sql_and_values(backend, sql, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_placeholders_are_numbered() {
        let statement = statement(
            DbBackend::Postgres,
            "a = ? AND b = ?",
            vec![1.into(), 2.into()],
        );
        assert_eq!(statement.sql, "a = $1 AND b = $2");
        let sqlite = super::statement(DbBackend::Sqlite, "a = ?", vec![1.into()]);
        assert_eq!(sqlite.sql, "a = ?");
    }

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
