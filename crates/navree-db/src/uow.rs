//! [`Uow`]: the unit of work, plus [`ChangeEvent`]s and [`ChangeObserver`]s.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::BoxFuture;
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, SqliteTransactionMode, Statement,
    TransactionOptions, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Db;

/// What happened to a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    /// Inserted.
    Created,
    /// Updated.
    Updated,
    /// Deleted.
    Deleted,
}

impl ChangeOp {
    /// `created`, `updated` or `deleted`.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeOp::Created => "created",
            ChangeOp::Updated => "updated",
            ChangeOp::Deleted => "deleted",
        }
    }
}

impl fmt::Display for ChangeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A change to one record, recorded in the unit of work (by [`Repo`](crate::Repo)
/// or by hand with [`Uow::record_change`]) and handed to every
/// [`ChangeObserver`] at commit.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChangeEvent {
    /// The table, e.g. `deals`.
    pub entity: String,
    /// The primary key (composite keys joined with `,`).
    pub id: String,
    /// What happened.
    pub op: ChangeOp,
    /// Before: the whole record for deletes, the changed fields for updates.
    pub old: Option<serde_json::Value>,
    /// After: the whole record for creates, the changed fields for updates.
    pub new: Option<serde_json::Value>,
}

/// Sees every unit of work's changes right before it commits, inside its
/// transaction (navree-audit writes audit rows this way, so a rolled-back
/// change leaves no audit row). An error rolls the whole unit of work back.
///
/// Register implementations as multi-bindings, e.g. with
/// [`ModuleBuilderDbExt::change_observer`](crate::ModuleBuilderDbExt::change_observer);
/// they are collected with `ctx.get_all::<dyn ChangeObserver>()`.
pub trait ChangeObserver: Send + Sync + 'static {
    /// Called once per commit with every recorded change, in order.
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>>;
}

/// A hook run after a successful commit.
type AfterCommit = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

enum TxState {
    /// No transaction yet (the unit of work is lazy).
    Idle,
    /// The open transaction.
    Open(Arc<DatabaseTransaction>),
    /// Committed or rolled back.
    Done,
}

/// The unit of work: one per HTTP request, job run or command. It carries
/// the actor and request id, opens its database transaction lazily on the
/// first [`tx`](Uow::tx), collects [`ChangeEvent`]s and after-commit hooks,
/// and commits or rolls back once.
///
/// In HTTP handlers, take it as an extractor (the `DatabaseModule` layer
/// commits it when the response status is below 400 and rolls it back
/// otherwise). Elsewhere, create one with [`Db::uow`]. Services take
/// `&Uow` as a parameter instead of having it injected.
///
/// On SQLite the transaction starts with `BEGIN IMMEDIATE`, so concurrent
/// units of work queue on the write lock (up to `busy_timeout`) instead of
/// failing when a reader later writes. On PostgreSQL the actor and request id
/// are set as `navree.actor` and `navree.request_id` for the transaction.
///
/// **Current unit of work.** While a request's handler runs (and inside
/// [`Db::transaction`] or [`scope`](Uow::scope)), [`Uow::current`] returns
/// the unit of work of the task. Code that cannot take a `&Uow` parameter
/// (`dyn Dispatcher`, notifications) uses it to write in the same
/// transaction: on SQLite a write through another pool connection would
/// wait for this transaction's write lock until `busy_timeout`.
#[derive(Clone)]
pub struct Uow {
    inner: Arc<UowInner>,
}

struct UowInner {
    db: Db,
    actor: Option<Uuid>,
    request_id: Option<String>,
    tx: tokio::sync::Mutex<TxState>,
    changes: Mutex<Vec<ChangeEvent>>,
    after_commit: Mutex<Vec<AfterCommit>>,
}

tokio::task_local! {
    /// The unit of work of the running task (see [`Uow::current`]).
    static CURRENT: Uow;
}

impl Uow {
    /// The unit of work of the current task: the request's while its
    /// handler runs, the one [`Db::transaction`] or [`scope`](Uow::scope)
    /// runs a future with; `None` elsewhere (spawned tasks do not inherit
    /// it). It may already be finished (a handler can commit early): check
    /// [`is_finished`](Uow::is_finished) before writing through it.
    pub fn current() -> Option<Uow> {
        CURRENT.try_with(Uow::clone).ok()
    }

    /// Runs `future` with this unit of work as [`Uow::current`].
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    /// Whether the unit of work has committed or rolled back (or is
    /// committing): its transaction can no longer be used.
    pub async fn is_finished(&self) -> bool {
        matches!(*self.inner.tx.lock().await, TxState::Done)
    }

    /// Whether `self` and `other` are handles to the same unit of work.
    pub fn same_as(&self, other: &Uow) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn new(db: Db, actor: Option<Uuid>, request_id: Option<String>) -> Self {
        Self {
            inner: Arc::new(UowInner {
                db,
                actor,
                request_id,
                tx: tokio::sync::Mutex::new(TxState::Idle),
                changes: Mutex::new(Vec::new()),
                after_commit: Mutex::new(Vec::new()),
            }),
        }
    }

    /// The transaction, begun on the first call. Drop the returned `Arc`
    /// before the unit of work commits (at the end of the handler).
    pub async fn tx(&self) -> Result<Arc<DatabaseTransaction>, DbErr> {
        let mut state = self.inner.tx.lock().await;
        match &*state {
            TxState::Open(tx) => Ok(tx.clone()),
            TxState::Done => Err(DbErr::Custom(
                "the unit of work is already committed or rolled back".to_owned(),
            )),
            TxState::Idle => {
                let tx = Arc::new(self.begin().await?);
                *state = TxState::Open(tx.clone());
                Ok(tx)
            }
        }
    }

    /// The user the work is done for, if any.
    pub fn actor(&self) -> Option<Uuid> {
        self.inner.actor
    }

    /// The request id, if any.
    pub fn request_id(&self) -> Option<&str> {
        self.inner.request_id.as_deref()
    }

    /// The database.
    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    /// Records a change for the [`ChangeObserver`]s.
    pub fn record_change(&self, ev: ChangeEvent) {
        lock(&self.inner.changes).push(ev);
    }

    /// The changes recorded so far (cleared by commit and rollback).
    pub fn changes(&self) -> Vec<ChangeEvent> {
        lock(&self.inner.changes).clone()
    }

    /// Runs `f` after a successful commit (dispatch jobs, send mail,
    /// broadcast); dropped on rollback. Hooks run in registration order.
    pub fn after_commit(&self, f: impl FnOnce() -> BoxFuture<'static, ()> + Send + 'static) {
        lock(&self.inner.after_commit).push(Box::new(f));
    }

    /// Runs the [`ChangeObserver`]s inside the transaction, commits, then runs
    /// the after-commit hooks. An observer error rolls back and is returned.
    /// Fails if an `Arc` from [`tx`](Uow::tx) is still held. Committing a
    /// finished unit of work does nothing.
    ///
    /// The unit of work counts as finished as soon as `commit` starts:
    /// observers write through the transaction they are given, and
    /// [`tx`](Uow::tx) fails from then on.
    pub async fn commit(&self) -> Result<(), DbErr> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        let changes = std::mem::take(&mut *lock(&self.inner.changes));
        let hooks = std::mem::take(&mut *lock(&self.inner.after_commit));
        let tx = match current {
            TxState::Done => return Ok(()),
            TxState::Open(tx) => Some(tx),
            TxState::Idle if !changes.is_empty() => Some(Arc::new(self.begin().await?)),
            TxState::Idle => None,
        };
        if let Some(tx) = tx {
            let tx = Arc::try_unwrap(tx).map_err(|_| {
                DbErr::Custom(
                    "cannot commit the unit of work: its transaction is still in use; drop every `Arc<DatabaseTransaction>` from `Uow::tx` first (it rolls back when the last one is dropped)"
                        .to_owned(),
                )
            })?;
            if let Err(error) = self.observe(&tx, &changes).await {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(target: "navree_db::uow", error = %rollback, "rollback after a change observer error failed");
                }
                return Err(error);
            }
            tx.commit().await?;
        }
        for hook in hooks {
            hook().await;
        }
        Ok(())
    }

    /// Rolls back and drops recorded changes and after-commit hooks.
    /// Rolling back a finished unit of work does nothing.
    pub async fn rollback(&self) -> Result<(), DbErr> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        lock(&self.inner.changes).clear();
        lock(&self.inner.after_commit).clear();
        match current {
            TxState::Open(tx) => match Arc::try_unwrap(tx) {
                Ok(tx) => tx.rollback().await,
                // Still held elsewhere: rolled back when the last holder drops it.
                Err(_) => Ok(()),
            },
            TxState::Idle | TxState::Done => Ok(()),
        }
    }

    /// Whether the backend supports `SELECT ... FOR UPDATE` (and `SKIP
    /// LOCKED`): false on SQLite, where `BEGIN IMMEDIATE` already serialises
    /// writers.
    pub fn lock_for_update(&self) -> bool {
        self.inner.db.backend() != DbBackend::Sqlite
    }

    async fn begin(&self) -> Result<DatabaseTransaction, DbErr> {
        let db = &self.inner.db;
        match db.backend() {
            DbBackend::Sqlite => {
                db.conn()
                    .begin_with_options(TransactionOptions {
                        sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                        ..TransactionOptions::default()
                    })
                    .await
            }
            DbBackend::Postgres => {
                let tx = db.conn().begin().await?;
                tx.execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT set_config('navree.actor', $1, true), set_config('navree.request_id', $2, true)",
                    [
                        self.inner
                            .actor
                            .map(|a| a.to_string())
                            .unwrap_or_default()
                            .into(),
                        self.inner.request_id.clone().unwrap_or_default().into(),
                    ],
                ))
                .await?;
                Ok(tx)
            }
            _ => db.conn().begin().await,
        }
    }

    async fn observe(
        &self,
        tx: &DatabaseTransaction,
        changes: &[ChangeEvent],
    ) -> Result<(), DbErr> {
        if changes.is_empty() {
            return Ok(());
        }
        for observer in self.inner.db.observers().await? {
            observer.observe(tx, self, changes).await?;
        }
        Ok(())
    }
}

impl fmt::Debug for Uow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Uow")
            .field("actor", &self.inner.actor)
            .field("request_id", &self.inner.request_id)
            .field("changes", &lock(&self.inner.changes).len())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
