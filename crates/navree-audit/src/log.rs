//! [`AuditLog`]: records, lists and prunes audit entries.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, Clock, SystemClock};
use navree_db::{
    ChangeEvent, ChangeObserver, ChangeOp, Db, DbError, Executor, Page, PageQuery, Uow,
    prune_time_series, record,
};
use navree_kernel::{BoxError, BoxFuture};
use navree_orm::list::{ListQuery, ListQueryExt as _, ListResource as _};
use navree_orm::{OrmError, Record};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::naming::snake_case;
use crate::policy::{AUDIT_TABLE, Policy, changed_fields};
use crate::{AuditFilter, AuditList, AuditOptions, AuditRecord};

/// How many entries may wait for the background writer.
const QUEUE_CAPACITY: usize = 4096;

/// An audit problem.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuditError {
    /// The background writer is overwhelmed; the entry was not recorded.
    #[error("the audit queue is full; the `{action}` entry was not recorded")]
    QueueFull {
        /// The action of the dropped entry.
        action: String,
    },
    /// A database error.
    #[error(transparent)]
    Db(#[from] DbError),
}

/// The audit trail. Provided and exported by `AuditModule` (inject
/// `Arc<AuditLog>`); other modules reach it as `dyn AuditSink`, and it sees
/// every unit of work as a `dyn ChangeObserver`.
///
/// - Data changes recorded in a [`Uow`] (by navree-orm models saved in it,
///   or navree-db's recorded record writes) become `data`
///   entries written inside the unit of work's transaction, right before it
///   commits ([`ChangeObserver::observe`]): a rolled-back change leaves
///   no audit row. The action is
///   `<subject>.<created|updated|deleted>` (`deal.updated`), the subject the
///   table's subject type (`Deal`), the actor, impersonator and request id
///   those of the unit of work; updates keep only the fields that changed.
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
    Entry(Box<AuditRecord>),
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
    /// taken now, and an entry without an impersonator gets the one of the
    /// task's unit of work ([`Uow::current`]), if any. Fails only when the
    /// queue is full.
    pub async fn record(&self, mut entry: AuditEntry) -> Result<(), AuditError> {
        if !self.policy.enabled {
            return Ok(());
        }
        if entry.impersonator_id.is_none() {
            entry.impersonator_id = Uow::current().and_then(|uow| uow.impersonator());
        }
        let row = self.row(entry);
        match self.queue.try_send(Message::Entry(Box::new(row))) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(Message::Entry(row))) => {
                let action = row.action;
                tracing::warn!(target: "navree::audit", %action, "the audit queue is full; entry dropped");
                Err(AuditError::QueueFull { action })
            }
            // The writer has stopped (the app is shutting down): write directly.
            Err(mpsc::error::TrySendError::Closed(Message::Entry(row))) => {
                insert(own_connection(&self.db), &[*row]).await?;
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    /// Records `entry` inside the transaction of `uow`, so it commits or
    /// rolls back with the work. The actor, impersonator and request id
    /// default to those of the unit of work.
    pub async fn record_in(&self, uow: &Uow, mut entry: AuditEntry) -> Result<(), DbError> {
        if !self.policy.enabled {
            return Ok(());
        }
        if entry.actor_id.is_none() {
            entry.actor_id = uow.actor();
        }
        if entry.impersonator_id.is_none() {
            entry.impersonator_id = uow.impersonator();
        }
        if entry.request_id.is_none() {
            entry.request_id = uow.request_id().map(str::to_owned);
        }
        let row = self.row(entry);
        insert(uow, &[row]).await
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

    /// One page of entries matching `filter`, newest first, read through
    /// the ORM on the log's database: the task's unit of work when its
    /// transaction is open, else the connection pool.
    pub async fn list(
        &self,
        filter: &AuditFilter,
        page: PageQuery,
    ) -> Result<Page<AuditRecord>, OrmError> {
        let page = filter
            .query()
            .order_by_desc(AuditRecord::CREATED_AT)
            .order_by_desc(AuditRecord::ID)
            .paginate(&self.db, page)
            .await?;
        Ok(page.map(Record::into_inner))
    }

    /// One page of the entries a list query asks for (read against
    /// [`AuditList`]), newest first: what `GET /api/audit` answers.
    pub(crate) async fn list_query(&self, list: &ListQuery) -> Result<Page<AuditRecord>, OrmError> {
        let spec = &AuditList::SPEC;
        let action = AuditFilter {
            action: list.eq("action").map(str::to_owned),
            ..AuditFilter::default()
        };
        let page = action
            .query()
            .list(list, spec)?
            .paginate(&self.db, list.page_request(spec))
            .await?;
        Ok(page.map(Record::into_inner))
    }

    /// Deletes entries older than `days` days; returns how many.
    pub async fn prune(&self, days: u32) -> Result<u64, DbError> {
        let cutoff = self.clock.now() - TimeDelta::days(i64::from(days));
        self.prune_before(cutoff).await
    }

    /// Deletes entries recorded before `cutoff`; returns how many. On
    /// TimescaleDB, where `audit_logs` is a hypertable, the weeks entirely
    /// before `cutoff` go as whole chunks (see [`prune_time_series`]),
    /// in a unit of work of its own.
    pub async fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<u64, DbError> {
        let uow = self.db.uow(None, None).await;
        match prune_time_series(&uow, AUDIT_TABLE, "created_at", cutoff).await {
            Ok(pruned) => {
                uow.commit().await?;
                Ok(pruned)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::audit", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    /// The row for `entry`, without skipped fields, stamped now.
    fn row(&self, entry: AuditEntry) -> AuditRecord {
        let owner = entry.subject_type.clone();
        let strip = |v: Value| self.policy.strip(owner.as_deref(), v);
        AuditRecord {
            id: Uuid::now_v7(),
            kind: kind_str(entry.kind).to_owned(),
            action: entry.action,
            actor_id: entry.actor_id,
            impersonator_id: entry.impersonator_id,
            subject_type: entry.subject_type,
            subject_id: entry.subject_id,
            old_values: entry.old_values.map(strip),
            new_values: entry.new_values.map(strip),
            request_id: entry.request_id,
            ip: entry.ip,
            user_agent: entry.user_agent,
            metadata: strip(entry.metadata),
            created_at: self.clock.now(),
        }
    }

    /// The `data` row for one change, or `None` when it is not audited.
    fn data_row(&self, change: &ChangeEvent, uow: &Uow) -> Option<AuditRecord> {
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
        Some(AuditRecord {
            id: Uuid::now_v7(),
            kind: kind_str(AuditKind::Data).to_owned(),
            action: format!("{}.{}", snake_case(&subject_type), change.op),
            actor_id: uow.actor(),
            impersonator_id: uow.impersonator(),
            subject_type: Some(subject_type),
            subject_id: Some(change.id.clone()),
            old_values: old.map(strip),
            new_values: new.map(strip),
            request_id: uow.request_id().map(str::to_owned),
            ip: None,
            user_agent: None,
            metadata: Value::Object(Default::default()),
            created_at: self.clock.now(),
        })
    }
}

/// Writes the `data` entries through the unit of work being committed.
impl ChangeObserver for AuditLog {
    fn observe<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            if !self.policy.enabled {
                return Ok(());
            }
            let rows: Vec<AuditRecord> = changes
                .iter()
                .filter_map(|change| self.data_row(change, uow))
                .collect();
            insert(uow, &rows).await
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
            let action = row.action.clone();
            if let Err(error) = insert(own_connection(db), &[*row]).await {
                tracing::warn!(target: "navree::audit", %action, %error, "could not write an audit entry");
            }
        }
        Message::Flush(done) => {
            let _ = done.send(());
        }
    }
}

/// Where entries written outside a unit of work go: the writer's pool,
/// never the task's unit of work (an entry that joined it would be rolled
/// back with the request), or the connection itself when it has no pool.
fn own_connection(db: &Db) -> &dyn Executor {
    db.pool()
}

/// Inserts `rows` through `exec`: the log's own connection, or the unit of
/// work it is handed. A plain insert records no change events, so audit
/// rows are never audited themselves.
async fn insert<E: Executor + ?Sized>(exec: &E, rows: &[AuditRecord]) -> Result<(), DbError> {
    for row in rows {
        record::insert(exec, row).await?;
    }
    Ok(())
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
