//! [`DatabaseStore`]: session records in the `sessions` table, and the
//! migration that creates it.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use navree_contracts::{BoxError, BoxFuture, Clock, SessionRecord, SessionStore, SystemClock};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{Backend, Db, DbError, Executor, MigrationSet, Statement};
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
    pub async fn prune_expired(&self) -> Result<u64, DbError> {
        let statement = Statement::portable(
            format!("DELETE FROM {SESSIONS_TABLE} WHERE expires_at <= ?"),
            [self.clock.now().into()],
        );
        Ok(self.executor().execute(&statement).await?.rows_affected)
    }

    /// What the store's statements run on: the writer's pool, never the
    /// request's unit of work (a store that joined it would lose a session
    /// when the request rolls back), or the connection itself when it has
    /// no pool.
    fn executor(&self) -> &dyn Executor {
        match self.db.pool() {
            Some(pool) => pool,
            None => &self.db,
        }
    }

    async fn load_record(&self, id: &str) -> Result<Option<SessionRecord>, DbError> {
        let statement = Statement::portable(
            format!("SELECT data, expires_at FROM {SESSIONS_TABLE} WHERE id = ?"),
            [id.into()],
        );
        let Some(row) = self.executor().fetch_optional(&statement).await? else {
            return Ok(None);
        };
        let data: String = row.get("data")?;
        let expires_at: Option<DateTime<Utc>> = row.get("expires_at")?;
        match serde_json::from_str::<Map<String, Value>>(&data) {
            Ok(data) => Ok(Some(SessionRecord { data, expires_at })),
            Err(_) => {
                tracing::warn!(
                    target: "navree::session",
                    "a stored session holds invalid JSON; starting a new session"
                );
                Ok(None)
            }
        }
    }

    async fn save_record(&self, id: &str, record: SessionRecord) -> Result<(), DbError> {
        let data = serde_json::to_string(&record.data)
            .map_err(|error| DbError::invalid(format!("cannot encode the session: {error}")))?;
        let upsert = match self.executor().backend() {
            Backend::MySql => {
                "ON DUPLICATE KEY UPDATE data = VALUES(data), expires_at = VALUES(expires_at)"
            }
            _ => {
                "ON CONFLICT (id) DO UPDATE SET data = excluded.data, expires_at = excluded.expires_at"
            }
        };
        let statement = Statement::portable(
            format!(
                "INSERT INTO {SESSIONS_TABLE} (id, data, expires_at) VALUES (?, ?, ?) {upsert}"
            ),
            [id.into(), data.into(), record.expires_at.into()],
        );
        self.executor().execute(&statement).await?;
        if self.saves.fetch_add(1, Ordering::Relaxed) % PRUNE_EVERY == PRUNE_EVERY - 1
            && let Err(error) = self.prune_expired().await
        {
            tracing::warn!(target: "navree::session", %error, "could not delete expired sessions");
        }
        Ok(())
    }

    async fn delete_record(&self, id: &str) -> Result<(), DbError> {
        let statement = Statement::portable(
            format!("DELETE FROM {SESSIONS_TABLE} WHERE id = ?"),
            [id.into()],
        );
        self.executor().execute(&statement).await.map(drop)
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
    MigrationSet::new(MIGRATION_SET).migration(SchemaMigration::new(
        "m20261025_000001_create_sessions_table",
        CreateSessionsTable,
    ))
}

/// Creates `sessions(id, data, expires_at)` with an index on `expires_at`.
struct CreateSessionsTable;

impl Migration for CreateSessionsTable {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create(SESSIONS_TABLE, |t| {
                t.string("id").primary();
                t.text("data");
                t.timestamp_tz("expires_at").nullable();
                t.index(["expires_at"]).name("sessions_expires_at_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(SESSIONS_TABLE).await
    }
}
