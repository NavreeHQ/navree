//! [`AuditLog`]: records, lists and prunes audit entries.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navis_contracts::{AuditEntry, AuditKind, AuditSink, Clock, SystemClock};
use navis_db::{ChangeEvent, ChangeObserver, ChangeOp, Db, Page, PageQuery, Uow, paginate};
use navis_kernel::{BoxError, BoxFuture};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbErr, EntityTrait,
    QueryFilter, QueryOrder,
};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::audit_log::{self, ActiveModel};
use crate::naming::snake_case;
use crate::policy::{Policy, changed_fields};
use crate::{AuditFilter, AuditOptions, AuditRecord};

/// How many entries may wait for the background writer.
const QUEUE_CAPACITY: usize = 4096;

/// An audit problem.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    /// The background writer is overwhelmed; the entry was not recorded.
    #[error("the audit queue is full; the `{action}` entry was not recorded")]
    QueueFull {
        /// The action of the dropped entry.
        action: String,
    },
    /// A database error.
    #[error(transparent)]
    Db(#[from] DbErr),
}

/// The audit trail. Provided and exported by `AuditModule` (inject
/// `Arc<AuditLog>`); other modules reach it as `dyn AuditSink`, and it sees
/// every unit of work as a `dyn ChangeObserver`.
///
/// - Data changes recorded in a [`Uow`] (by `navis_db::Repo`) become `data`
///   entries written inside the unit of work's transaction, right before it
///   commits: a rolled-back change leaves no audit row. The action is
///   `<subject>.<created|updated|deleted>` (`deal.updated`), the subject the
///   table's subject type (`Deal`), the actor and request id those of the
///   unit of work; updates keep only the fields that changed.
/// - [`record`](AuditLog::record) (and `AuditSink::record`) writes an entry
///   on its own connection, outside any request transaction, so security
///   events (denied authorization, failed logins) survive the rollback that
///   usually follows. Entries are queued and written in order by a
///   background task (on SQLite the request's transaction holds the write
///   lock until it ends); [`flush`](AuditLog::flush) waits for the queue,
///   and graceful shutdown drains it.
/// - [`record_in`](AuditLog::record_in) writes a custom entry inside a unit
///   of work's transaction instead.
///
/// Skipped entities and fields ([`AuditOptions`]) are never stored, and
/// nothing is written while auditing is disabled.
pub struct AuditLog {
    db: Db,
    options: Arc<AuditOptions>,
    policy: Arc<Policy>,
    clock: Arc<dyn Clock>,
    queue: mpsc::Sender<Message>,
}

/// What the background writer receives.
enum Message {
    /// An entry to write.
    Entry(Box<ActiveModel>),
    /// Answer once every earlier entry is written.
    Flush(oneshot::Sender<()>),
}

impl AuditLog {
    /// An audit log writing to `db`, using the system clock. Its background
    /// writer runs on the current Tokio runtime until the log is dropped.
    ///
    /// # Panics
    ///
    /// Outside a Tokio runtime.
    pub fn new(db: Db, options: AuditOptions) -> AuditLog {
        Self::start(
            db,
            Arc::new(options),
            Arc::new(SystemClock),
            std::future::pending(),
            |writer| {
                tokio::spawn(writer);
            },
        )
    }

    /// An audit log whose background writer is started by `spawn` and
    /// stops (after writing what is queued) when `stop` resolves; later
    /// entries are written directly. The module spawns it on the app's
    /// task tracker and stops it at shutdown.
    pub(crate) fn start(
        db: Db,
        options: Arc<AuditOptions>,
        clock: Arc<dyn Clock>,
        stop: impl Future<Output = ()> + Send + 'static,
        spawn: impl FnOnce(BoxFuture<'static, ()>),
    ) -> AuditLog {
        let (queue, receiver) = mpsc::channel(QUEUE_CAPACITY);
        spawn(Box::pin(write_until(db.clone(), receiver, stop)));
        AuditLog {
            policy: Arc::new(Policy::new(&options)),
            db,
            options,
            clock,
            queue,
        }
    }

    /// The options.
    pub fn options(&self) -> &AuditOptions {
        &self.options
    }

    /// Records `entry` on the log's own connection, outside any unit of
    /// work (queued; see [`flush`](AuditLog::flush)). The time and id are
    /// taken now. Fails only when the queue is full.
    pub async fn record(&self, entry: AuditEntry) -> Result<(), AuditError> {
        if !self.policy.enabled {
            return Ok(());
        }
        let row = self.row(entry);
        match self.queue.try_send(Message::Entry(Box::new(row))) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(Message::Entry(row))) => {
                let action = action_of(&row);
                tracing::warn!(target: "navis::audit", %action, "the audit queue is full; entry dropped");
                Err(AuditError::QueueFull { action })
            }
            // The writer has stopped (the app is shutting down): write directly.
            Err(mpsc::error::TrySendError::Closed(Message::Entry(row))) => {
                insert(self.db.conn(), vec![*row]).await?;
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    /// Records `entry` inside the transaction of `uow`, so it commits or
    /// rolls back with the work. The actor and request id default to those
    /// of the unit of work.
    pub async fn record_in(&self, uow: &Uow, mut entry: AuditEntry) -> Result<(), DbErr> {
        if !self.policy.enabled {
            return Ok(());
        }
        if entry.actor_id.is_none() {
            entry.actor_id = uow.actor();
        }
        if entry.request_id.is_none() {
            entry.request_id = uow.request_id().map(str::to_owned);
        }
        let row = self.row(entry);
        let tx = uow.tx().await?;
        insert(&*tx, vec![row]).await
    }

    /// Waits until every entry queued by [`record`](AuditLog::record) so far
    /// is written. On SQLite, do not call it while holding an open unit of
    /// work's transaction (the writer needs the write lock).
    pub async fn flush(&self) {
        let (done, wait) = oneshot::channel();
        if self.queue.send(Message::Flush(done)).await.is_ok() {
            let _ = wait.await;
        }
    }

    /// One page of entries matching `filter`, newest first.
    pub async fn list(
        &self,
        filter: &AuditFilter,
        page: PageQuery,
    ) -> Result<Page<AuditRecord>, DbErr> {
        let select = filter
            .apply(audit_log::Entity::find())
            .order_by_desc(audit_log::Column::CreatedAt)
            .order_by_desc(audit_log::Column::Id);
        paginate(select, self.db.conn(), page).await
    }

    /// Deletes entries older than `days` days; returns how many.
    pub async fn prune(&self, days: u32) -> Result<u64, DbErr> {
        let cutoff = self.clock.now() - TimeDelta::days(i64::from(days));
        self.prune_before(cutoff).await
    }

    /// Deletes entries recorded before `cutoff`; returns how many.
    pub async fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<u64, DbErr> {
        let result = audit_log::Entity::delete_many()
            .filter(audit_log::Column::CreatedAt.lt(cutoff))
            .exec(self.db.conn())
            .await?;
        Ok(result.rows_affected)
    }

    /// The row for `entry`, without skipped fields, stamped now.
    fn row(&self, entry: AuditEntry) -> ActiveModel {
        let owner = entry.subject_type.clone();
        let strip = |v: Value| self.policy.strip(owner.as_deref(), v);
        ActiveModel {
            id: Set(Uuid::now_v7()),
            kind: Set(kind_str(entry.kind).to_owned()),
            action: Set(entry.action),
            actor_id: Set(entry.actor_id),
            subject_type: Set(entry.subject_type),
            subject_id: Set(entry.subject_id),
            old_values: Set(entry.old_values.map(strip)),
            new_values: Set(entry.new_values.map(strip)),
            request_id: Set(entry.request_id),
            ip: Set(entry.ip),
            user_agent: Set(entry.user_agent),
            metadata: Set(strip(entry.metadata)),
            created_at: Set(self.clock.now()),
        }
    }

    /// The `data` row for one change, or `None` when it is not audited.
    fn data_row(&self, change: &ChangeEvent, uow: &Uow) -> Option<ActiveModel> {
        let table = change.entity.as_str();
        if self.policy.skips_entity(table) {
            return None;
        }
        let (old, new) = match change.op {
            ChangeOp::Updated => {
                let (old, new) = changed_fields(
                    change.old.clone().unwrap_or(Value::Null),
                    change.new.clone().unwrap_or(Value::Null),
                );
                if is_empty(&old) && is_empty(&new) {
                    return None;
                }
                (Some(old), Some(new))
            }
            ChangeOp::Created | ChangeOp::Deleted => (change.old.clone(), change.new.clone()),
        };
        let strip = |v: Value| self.policy.strip(Some(table), v);
        let subject_type = self.policy.subject_type(table);
        Some(ActiveModel {
            id: Set(Uuid::now_v7()),
            kind: Set(kind_str(AuditKind::Data).to_owned()),
            action: Set(format!("{}.{}", snake_case(&subject_type), change.op)),
            actor_id: Set(uow.actor()),
            subject_type: Set(Some(subject_type)),
            subject_id: Set(Some(change.id.clone())),
            old_values: Set(old.map(strip)),
            new_values: Set(new.map(strip)),
            request_id: Set(uow.request_id().map(str::to_owned)),
            ip: Set(None),
            user_agent: Set(None),
            metadata: Set(Value::Object(Default::default())),
            created_at: Set(self.clock.now()),
        })
    }
}

impl ChangeObserver for AuditLog {
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            if !self.policy.enabled {
                return Ok(());
            }
            let rows: Vec<ActiveModel> = changes
                .iter()
                .filter_map(|change| self.data_row(change, uow))
                .collect();
            insert(tx, rows).await
        })
    }
}

impl AuditSink for AuditLog {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move { AuditLog::record(self, entry).await.map_err(Into::into) })
    }
}

impl fmt::Debug for AuditLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditLog")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// Writes queued entries in order until every sender is gone or `stop`
/// resolves, then the entries still queued; later entries are written
/// directly by [`AuditLog::record`].
async fn write_until(
    db: Db,
    mut receiver: mpsc::Receiver<Message>,
    stop: impl Future<Output = ()>,
) {
    tokio::pin!(stop);
    loop {
        tokio::select! {
            message = receiver.recv() => match message {
                Some(message) => handle(&db, message).await,
                None => return,
            },
            () = &mut stop => break,
        }
    }
    receiver.close();
    while let Some(message) = receiver.recv().await {
        handle(&db, message).await;
    }
}

async fn handle(db: &Db, message: Message) {
    match message {
        Message::Entry(row) => {
            let action = action_of(&row);
            if let Err(error) = insert(db.conn(), vec![*row]).await {
                tracing::warn!(target: "navis::audit", %action, %error, "could not write an audit entry");
            }
        }
        Message::Flush(done) => {
            let _ = done.send(());
        }
    }
}

async fn insert<C: ConnectionTrait>(conn: &C, rows: Vec<ActiveModel>) -> Result<(), DbErr> {
    audit_log::Entity::insert_many(rows)
        .exec_without_returning(conn)
        .await?;
    Ok(())
}

fn action_of(row: &ActiveModel) -> String {
    match &row.action {
        sea_orm::ActiveValue::Set(action) | sea_orm::ActiveValue::Unchanged(action) => {
            action.clone()
        }
        sea_orm::ActiveValue::NotSet => String::new(),
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.is_empty(),
        Value::Null => true,
        _ => false,
    }
}

/// The stored name of a kind.
pub(crate) fn kind_str(kind: AuditKind) -> &'static str {
    match kind {
        AuditKind::Data => "data",
        AuditKind::Auth => "auth",
        AuditKind::Access => "access",
        AuditKind::Security => "security",
        AuditKind::Custom => "custom",
    }
}
