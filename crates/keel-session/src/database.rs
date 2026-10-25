//! [`DatabaseStore`]: session records in the `sessions` table, and the
//! migration that creates it.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use keel_contracts::{BoxError, BoxFuture, Clock, SessionRecord, SessionStore, SystemClock};
use keel_db::{Db, MigrationSet};
use sea_orm::sea_query::{ColumnDef, Expr, ExprTrait, Index, OnConflict, Query, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::{Map, Value};

use crate::PRUNE_EVERY;

/// The table holding session records.
pub const SESSIONS_TABLE: &str = "sessions";

/// The migration set name of the `sessions` table.
pub(crate) const MIGRATION_SET: &str = "session";

/// Keeps session records in the `sessions` table (`id` text primary key,
/// `data` JSON text, `expires_at` timestamp), created by the `session`
/// migration set that `SessionModule` contributes. Records survive restarts
/// and are shared by every instance of the app.
///
/// Expired rows are deleted on every hundredth save, or by
/// [`prune_expired`](DatabaseStore::prune_expired). Records are written
/// outside the request's unit of work: the session layer loads them before
/// it opens and saves them after it commits.
#[derive(Clone)]
pub struct DatabaseStore {
    db: Db,
    clock: Arc<dyn Clock>,
    saves: Arc<AtomicU64>,
}

impl DatabaseStore {
    /// A store writing through `db`'s pool.
    pub fn new(db: Db) -> DatabaseStore {
        DatabaseStore {
            db,
            clock: Arc::new(SystemClock),
            saves: Arc::default(),
        }
    }

    /// The same store, deciding what is expired with `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> DatabaseStore {
        self.clock = clock;
        self
    }

    /// Deletes every expired record; returns how many.
    pub async fn prune_expired(&self) -> Result<u64, DbErr> {
        let result = self
            .db
            .conn()
            .execute(
                Query::delete()
                    .from_table(SESSIONS_TABLE)
                    .and_where(Expr::col("expires_at").lte(self.clock.now())),
            )
            .await?;
        Ok(result.rows_affected())
    }

    async fn load_record(&self, id: &str) -> Result<Option<SessionRecord>, DbErr> {
        let row = self
            .db
            .conn()
            .query_one(
                Query::select()
                    .columns(["data", "expires_at"])
                    .from(SESSIONS_TABLE)
                    .and_where(Expr::col("id").eq(id)),
            )
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let data: String = row.try_get("", "data")?;
        let expires_at: Option<DateTime<Utc>> = row.try_get("", "expires_at")?;
        match serde_json::from_str::<Map<String, Value>>(&data) {
            Ok(data) => Ok(Some(SessionRecord { data, expires_at })),
            Err(_) => {
                tracing::warn!(
                    target: "keel::session",
                    "a stored session holds invalid JSON; starting a new session"
                );
                Ok(None)
            }
        }
    }

    async fn save_record(&self, id: &str, record: SessionRecord) -> Result<(), DbErr> {
        let data = serde_json::to_string(&record.data)
            .map_err(|error| DbErr::Custom(format!("cannot encode the session: {error}")))?;
        self.db
            .conn()
            .execute(
                Query::insert()
                    .into_table(SESSIONS_TABLE)
                    .columns(["id", "data", "expires_at"])
                    .values_panic([id.into(), data.into(), record.expires_at.into()])
                    .on_conflict(
                        OnConflict::column("id")
                            .update_columns(["data", "expires_at"])
                            .to_owned(),
                    ),
            )
            .await?;
        if self.saves.fetch_add(1, Ordering::Relaxed) % PRUNE_EVERY == PRUNE_EVERY - 1
            && let Err(error) = self.prune_expired().await
        {
            tracing::warn!(target: "keel::session", %error, "could not delete expired sessions");
        }
        Ok(())
    }

    async fn delete_record(&self, id: &str) -> Result<(), DbErr> {
        self.db
            .conn()
            .execute(
                Query::delete()
                    .from_table(SESSIONS_TABLE)
                    .and_where(Expr::col("id").eq(id)),
            )
            .await
            .map(drop)
    }
}

impl SessionStore for DatabaseStore {
    fn load(&self, id: &str) -> BoxFuture<'_, Result<Option<SessionRecord>, BoxError>> {
        let id = id.to_owned();
        Box::pin(async move { Ok(self.load_record(&id).await?) })
    }

    fn save(&self, id: &str, record: SessionRecord) -> BoxFuture<'_, Result<(), BoxError>> {
        let id = id.to_owned();
        Box::pin(async move { Ok(self.save_record(&id, record).await?) })
    }

    fn delete(&self, id: &str) -> BoxFuture<'_, Result<(), BoxError>> {
        let id = id.to_owned();
        Box::pin(async move { Ok(self.delete_record(&id).await?) })
    }
}

impl fmt::Debug for DatabaseStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseStore")
            .field("db", &self.db)
            .finish_non_exhaustive()
    }
}

/// The `session` migration set: the `sessions` table.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(MIGRATION_SET).migration(CreateSessionsTable)
}

/// Creates `sessions(id, data, expires_at)` with an index on `expires_at`.
struct CreateSessionsTable;

impl MigrationName for CreateSessionsTable {
    fn name(&self) -> &str {
        "m20261025_000001_create_sessions_table"
    }
}

#[async_trait]
impl MigrationTrait for CreateSessionsTable {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SESSIONS_TABLE)
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("data").text().not_null())
                    .col(
                        ColumnDef::new("expires_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sessions_expires_at_index")
                    .table(SESSIONS_TABLE)
                    .col("expires_at")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SESSIONS_TABLE).to_owned())
            .await
    }
}
