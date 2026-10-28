//! [`Uow`]: the unit of work, plus [`ChangeEvent`]s and [`ChangeObserver`]s.

use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree_kernel::BoxFuture;
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, SqliteTransactionMode, Statement,
    TransactionOptions, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::compat::SeaExecutor;
use crate::queries::{QueryCounter, log_stats};
use crate::{
    Backend, Db, DbError, Executed, Executor, QueryStats, Row, RowStream, Transaction, TxOptions,
    mysql,
};

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
///
/// Implement one of the two methods. [`observe_with`](ChangeObserver::observe_with)
/// writes through the unit of work itself (an [`Executor`] on its
/// transaction) and runs on every unit of work. [`observe`](ChangeObserver::observe)
/// is handed SeaORM's transaction and runs only in units of work on
/// SeaORM's transactions (the default while crates move off SeaORM).
pub trait ChangeObserver: Send + Sync + 'static {
    /// Called once per commit with every recorded change, in order, in a
    /// unit of work on SeaORM's transaction `tx`. By default it runs
    /// [`observe_with`](ChangeObserver::observe_with).
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        let _ = tx;
        Box::pin(async move { self.observe_with(uow, changes).await.map_err(DbErr::from) })
    }

    /// Called once per commit with every recorded change, in order. `uow`
    /// runs statements in the transaction being committed (it is an
    /// [`Executor`] until the commit ends). By default it fails: an
    /// observer that only implements [`observe`](ChangeObserver::observe)
    /// cannot run in a unit of work on navree-db's own transaction.
    fn observe_with<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        let _ = (uow, changes);
        Box::pin(async move {
            Err(DbError::unsupported(format!(
                "the change observer `{}` writes through a SeaORM transaction: implement \
                 `ChangeObserver::observe_with` to run it in a unit of work on navree-db's own transactions",
                std::any::type_name::<Self>()
            )))
        })
    }
}

/// A hook run after a successful commit.
type AfterCommit = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

/// Prepares a change event on demand: `None` when the write changed
/// nothing worth an event.
pub(crate) type PrepareChange = Box<dyn Fn() -> Result<Option<ChangeEvent>, DbErr> + Send + Sync>;

/// A recorded change: its event, or what prepares it. Writes defer the
/// event (serializing records to JSON) in an app with no change observer,
/// where only [`Uow::changes`] reads them.
enum Recorded {
    Ready(ChangeEvent),
    Deferred(PrepareChange),
}

impl Recorded {
    /// The event, prepared now if it was deferred.
    fn event(&self) -> Result<Option<ChangeEvent>, DbErr> {
        match self {
            Recorded::Ready(event) => Ok(Some(event.clone())),
            Recorded::Deferred(prepare) => prepare(),
        }
    }
}

/// The transaction of a unit of work: SeaORM's (the default while crates
/// move off SeaORM; it serves navree-db's statements too) or navree-db's
/// own ([`Db::with_native_units_of_work`]).
#[derive(Clone)]
enum UowTx {
    Sea(Arc<DatabaseTransaction>),
    Native(Arc<Transaction>),
}

impl UowTx {
    /// navree-db's statements on this transaction.
    fn executor(&self) -> TxExecutor<'_> {
        match self {
            UowTx::Native(tx) => TxExecutor::Native(tx),
            UowTx::Sea(tx) => TxExecutor::Sea(SeaExecutor::new(&**tx)),
        }
    }
}

/// An [`Executor`] on a unit of work's transaction.
enum TxExecutor<'a> {
    Native(&'a Transaction),
    Sea(SeaExecutor<'a, DatabaseTransaction>),
}

impl TxExecutor<'_> {
    fn get(&self) -> &dyn Executor {
        match self {
            TxExecutor::Native(tx) => *tx,
            TxExecutor::Sea(sea) => sea,
        }
    }
}

enum TxState {
    /// No transaction yet (the unit of work is lazy).
    Idle,
    /// The open transaction.
    Open(UowTx),
    /// Committed or rolled back.
    Done,
}

/// The unit of work: one per HTTP request, job run or command. It carries
/// the actor and request id, opens its database transaction lazily on the
/// first [`tx`](Uow::tx) (or statement, as an [`Executor`]), collects
/// [`ChangeEvent`]s and after-commit hooks, and commits or rolls back once.
///
/// In HTTP handlers, take it as an extractor (the `DatabaseModule` layer
/// commits it when the response status is below 400 and rolls it back
/// otherwise). Elsewhere, create one with [`Db::uow`]. Services take
/// `&Uow` as a parameter instead of having it injected.
///
/// **Two transactions, one unit of work.** A unit of work runs on SeaORM's
/// transaction by default, which [`tx`](Uow::tx) hands out and navree-db's
/// statements ([`Executor`]) share; or, on a `Db` made with
/// [`Db::with_native_units_of_work`], on navree-db's own transaction,
/// reached only through [`Executor`], [`savepoint`](Uow::savepoint) and
/// [`set_local`](Uow::set_local). Everything below holds for both.
///
/// **Per database.** On SQLite the transaction starts with `BEGIN
/// IMMEDIATE`, so concurrent units of work queue on the write lock (up to
/// `busy_timeout`) instead of failing when a reader later writes. On
/// PostgreSQL the actor and request id are set as `navree.actor` and
/// `navree.request_id` for the transaction (`set_config(.., true)`), which
/// triggers and row-level security policies read with `current_setting`.
/// (Both this and the MySQL variables below can be turned off with
/// `database.transaction_settings = false`, which saves those statements.)
/// On MySQL and MariaDB they are the session variables `@navree_actor` and
/// `@navree_request_id` (text, `NULL` when absent): set right after `BEGIN`
/// and cleared right before `COMMIT` or `ROLLBACK`, because a session
/// variable outlives the transaction and the connection goes back to the
/// pool. Read them in triggers (`SET NEW.updated_by = @navree_actor` for a
/// text column, `UNHEX(REPLACE(@navree_actor, '-', ''))` for a `BINARY(16)`
/// UUID column, on MySQL and MariaDB alike); outside a unit of work they are
/// `NULL`.
///
/// MySQL has no row-level security, so nothing in the database itself
/// keeps one workspace's rows from another's: on MySQL, workspace isolation
/// is query scoping (every query of a workspace-scoped repository filters
/// on `workspace_id`), and only PostgreSQL can also enforce it with
/// policies. Rows are locked with `SELECT ... FOR UPDATE` on PostgreSQL
/// and MySQL ([`lock_for_update`](Uow::lock_for_update)). A deadlock
/// (error 1213) makes MySQL roll the whole transaction back at once, and
/// whatever runs through it afterwards commits on its own, beyond the reach
/// of [`rollback`](Uow::rollback): return the error instead of carrying on.
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
    impersonator: Option<Uuid>,
    request_id: Option<String>,
    /// The workspace the work is done in, once one was entered.
    workspace: Mutex<Option<Uuid>>,
    tx: tokio::sync::Mutex<TxState>,
    /// The transaction while change observers run inside the commit (the
    /// unit of work already counts as finished).
    observing: Mutex<Option<UowTx>>,
    /// MySQL session variables [`Uow::set_local`] set, cleared before the
    /// transaction ends.
    session_variables: Mutex<Vec<String>>,
    changes: Mutex<Vec<Recorded>>,
    after_commit: Mutex<Vec<AfterCommit>>,
    /// Whether anything was written through the unit of work.
    modified: AtomicBool,
    /// Its statements: how many, how long, which repeat.
    queries: Mutex<QueryCounter>,
}

tokio::task_local! {
    /// The unit of work of the running task (see [`Uow::current`]).
    static CURRENT: Uow;
}

/// Why a unit of work on navree-db's own transaction has no SeaORM one.
fn no_sea_transaction() -> DbErr {
    DbErr::Custom(
        "this unit of work runs on navree-db's own transaction, which has no SeaORM transaction to hand out: \
         run statements through it as an `Executor`"
            .to_owned(),
    )
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

    /// Whether its transaction has begun and is not finished.
    pub async fn has_open_transaction(&self) -> bool {
        matches!(*self.inner.tx.lock().await, TxState::Open(_))
    }

    /// Whether `self` and `other` are handles to the same unit of work.
    pub fn same_as(&self, other: &Uow) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn new(db: Db, actor: Option<Uuid>, request_id: Option<String>) -> Self {
        Self::impersonated(db, actor, None, request_id)
    }

    pub(crate) fn impersonated(
        db: Db,
        actor: Option<Uuid>,
        impersonator: Option<Uuid>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            inner: Arc::new(UowInner {
                db,
                actor,
                impersonator,
                request_id,
                workspace: Mutex::new(None),
                tx: tokio::sync::Mutex::new(TxState::Idle),
                observing: Mutex::new(None),
                session_variables: Mutex::new(Vec::new()),
                changes: Mutex::new(Vec::new()),
                after_commit: Mutex::new(Vec::new()),
                modified: AtomicBool::new(false),
                queries: Mutex::new(QueryCounter::default()),
            }),
        }
    }

    /// The statements the unit of work ran on its connection while it was
    /// [current](Uow::current): how many, their total time, and the ones
    /// that ran more than once. A summary is logged at debug level (target
    /// `navree_db::uow`) when it finishes, with a warning for a statement
    /// that ran ten times or more.
    pub fn query_stats(&self) -> QueryStats {
        lock(&self.inner.queries).stats()
    }

    /// The total time of the unit of work's statements so far (see
    /// [`query_stats`](Uow::query_stats)).
    pub fn total_query_duration(&self) -> Duration {
        lock(&self.inner.queries).total()
    }

    /// Starts the total time of the unit of work's statements again from
    /// zero (the statement count stays).
    pub fn reset_total_query_duration(&self) {
        lock(&self.inner.queries).reset_total();
    }

    /// Lets the [`Db::when_querying_for_longer_than`] handlers that ran
    /// for this unit of work run again.
    pub fn allow_duration_handlers_again(&self) {
        lock(&self.inner.queries).allow_handlers_again();
    }

    /// Counts a statement; returns the total time so far.
    pub(crate) fn count_query(&self, sql: &str, elapsed: Duration) -> Duration {
        lock(&self.inner.queries).count(sql, elapsed)
    }

    /// Whether duration handler `id` may run for this unit of work.
    pub(crate) fn claim_duration_handler(&self, id: usize) -> bool {
        lock(&self.inner.queries).claim(id)
    }

    /// Logs the statistics of the finished unit of work.
    fn log_query_stats(&self) {
        log_stats(&self.query_stats(), self.request_id());
    }

    /// Notes that something was written through the unit of work (the
    /// ORM's writes, [`Repo`](crate::Repo) and recorded changes do it);
    /// see [`has_modified_records`](Uow::has_modified_records).
    pub fn mark_modified(&self) {
        self.inner.modified.store(true, Ordering::Relaxed);
    }

    /// Whether anything was written through the unit of work (also after it
    /// committed). With read replicas and `sticky` (the default), the
    /// task's reads then go to the writer, so it reads its own writes.
    pub fn has_modified_records(&self) -> bool {
        self.inner.modified.load(Ordering::Relaxed)
    }

    /// The SeaORM transaction, begun on the first call. Drop the returned
    /// `Arc` before the unit of work commits (at the end of the handler).
    /// Fails for a unit of work on navree-db's own transaction
    /// ([`Db::with_native_units_of_work`]): run statements through it as an
    /// [`Executor`] instead.
    pub async fn tx(&self) -> Result<Arc<DatabaseTransaction>, DbErr> {
        let mut state = self.inner.tx.lock().await;
        match &*state {
            TxState::Open(UowTx::Sea(tx)) => Ok(tx.clone()),
            TxState::Open(UowTx::Native(_)) => Err(no_sea_transaction()),
            TxState::Done => Err(DbErr::Custom(
                "the unit of work is already committed or rolled back".to_owned(),
            )),
            TxState::Idle if self.inner.db.native_units_of_work() => Err(no_sea_transaction()),
            TxState::Idle => {
                let tx = Arc::new(self.begin_sea().await?);
                *state = TxState::Open(UowTx::Sea(tx.clone()));
                Ok(tx)
            }
        }
    }

    /// The SeaORM transaction when it is open, without beginning one
    /// (unlike [`tx`](Uow::tx)): `None` before the first `tx`, once the
    /// unit of work is finished, and for a unit of work on navree-db's own
    /// transaction. See [`Db::reader`].
    pub async fn open_tx(&self) -> Option<Arc<DatabaseTransaction>> {
        match &*self.inner.tx.lock().await {
            TxState::Open(UowTx::Sea(tx)) => Some(tx.clone()),
            TxState::Open(UowTx::Native(_)) | TxState::Idle | TxState::Done => None,
        }
    }

    /// The transaction for navree-db's statements: the one observers run
    /// in during the commit, else the open one, else a new one.
    async fn transaction(&self) -> Result<UowTx, DbError> {
        if let Some(tx) = lock(&self.inner.observing).clone() {
            return Ok(tx);
        }
        let mut state = self.inner.tx.lock().await;
        match &*state {
            TxState::Open(tx) => Ok(tx.clone()),
            TxState::Done => Err(DbError::done(
                "the unit of work is already committed or rolled back",
            )),
            TxState::Idle => {
                let tx = self.begin().await?;
                *state = TxState::Open(tx.clone());
                Ok(tx)
            }
        }
    }

    /// A savepoint inside the unit of work's transaction (begun if need
    /// be): a nested level that commits or rolls back on its own, for work
    /// that may fail without failing the unit of work. Only on navree-db's
    /// own transaction; on SeaORM's, use `uow.tx().await?.begin()`.
    pub async fn savepoint(&self) -> Result<Transaction, DbError> {
        match self.transaction().await? {
            UowTx::Native(tx) => tx.savepoint().await,
            UowTx::Sea(_) => Err(DbError::unsupported(
                "this unit of work runs on a SeaORM transaction: nest one with `uow.tx().await?.begin()`",
            )),
        }
    }

    /// Sets `name` to `value` for the rest of the transaction (begun if
    /// need be), for triggers, row-level security policies and views to
    /// read: `set_config(name, value, true)` on PostgreSQL (read with
    /// `current_setting(name, true)`; `name` takes a dot, e.g.
    /// `navree.workspace`), the session variable `@name` on MySQL (cleared
    /// before the transaction ends; dots become underscores). SQLite has
    /// neither: `false`, nothing set.
    ///
    /// ```no_run
    /// # async fn demo(uow: navree_db::Uow, workspace: uuid::Uuid) -> Result<(), navree_db::DbError> {
    /// uow.set_local("navree.workspace", &workspace.to_string()).await?;
    /// # Ok(()) }
    /// ```
    pub async fn set_local(&self, name: &str, value: &str) -> Result<bool, DbError> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return Err(DbError::invalid(format!(
                "`{name}` is not a setting name (letters, digits, `_` and `.`)"
            )));
        }
        let backend = Executor::backend(self);
        let statement = match backend {
            Backend::Postgres => crate::Statement::new(
                Backend::Postgres,
                "SELECT set_config($1, $2, true)",
                [name.into(), value.into()],
            ),
            Backend::MySql => {
                let variable = name.replace('.', "_");
                let statement = crate::Statement::new(
                    Backend::MySql,
                    format!("SET @{variable} = ?"),
                    [value.into()],
                );
                let mut variables = lock(&self.inner.session_variables);
                if !variables.contains(&variable) {
                    variables.push(variable);
                }
                statement
            }
            Backend::Sqlite => return Ok(false),
        };
        Executor::execute(self, &statement).await?;
        Ok(true)
    }

    /// The user the work is done for, if any.
    pub fn actor(&self) -> Option<Uuid> {
        self.inner.actor
    }

    /// The user impersonating the [`actor`](Uow::actor), if the work is
    /// done in an impersonation session (`navree_contracts::Impersonator`).
    /// The audit trail records it on the work's entries.
    pub fn impersonator(&self) -> Option<Uuid> {
        self.inner.impersonator
    }

    /// The request id, if any.
    pub fn request_id(&self) -> Option<&str> {
        self.inner.request_id.as_deref()
    }

    /// The workspace the work is done in: set when a workspace is entered
    /// (navree-workspace does it for every workspace request and for
    /// `Workspaces::enter`), `None` before. Workspace-scoped ORM models read
    /// it to scope their queries and fill new records.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), sea_orm::DbErr> {
    /// # let dir = tempfile::tempdir().unwrap();
    /// # let url = format!("sqlite://{}?mode=rwc", dir.path().join("w.db").display());
    /// use navree_db::{Db, DbOptions};
    ///
    /// let db = Db::connect(&DbOptions { url, ..DbOptions::default() }).await?;
    /// let uow = db.uow(None, None).await;
    /// assert_eq!(uow.workspace(), None);
    /// let acme = uuid::Uuid::now_v7();
    /// uow.set_workspace(Some(acme));
    /// assert_eq!(uow.workspace(), Some(acme));
    /// # Ok(()) }
    /// ```
    pub fn workspace(&self) -> Option<Uuid> {
        *lock(&self.inner.workspace)
    }

    /// Records the workspace the work is done in (`None` leaves it). This
    /// only remembers the id: the database-side settings of a workspace
    /// (PostgreSQL's `navree.workspace`, the `search_path`) are applied by
    /// whoever enters it.
    pub fn set_workspace(&self, workspace: Option<Uuid>) {
        *lock(&self.inner.workspace) = workspace;
    }

    /// The database.
    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    /// Records a change for the [`ChangeObserver`]s (the unit of work then
    /// [has modified records](Uow::has_modified_records)).
    pub fn record_change(&self, ev: ChangeEvent) {
        self.mark_modified();
        lock(&self.inner.changes).push(Recorded::Ready(ev));
    }

    /// Records the change `prepare` describes: prepared now when the app
    /// has a [`ChangeObserver`] (an error is returned, as the write's own),
    /// when [`changes`](Uow::changes) asks otherwise.
    pub(crate) fn record_with(&self, prepare: PrepareChange) -> Result<(), DbErr> {
        if self.inner.db.observed() {
            if let Some(event) = prepare()? {
                self.record_change(event);
            }
        } else {
            self.mark_modified();
            lock(&self.inner.changes).push(Recorded::Deferred(prepare));
        }
        Ok(())
    }

    /// The changes recorded so far (cleared by commit and rollback). A
    /// deferred event that cannot be prepared (its record does not
    /// serialize) is left out, with a warning.
    pub fn changes(&self) -> Vec<ChangeEvent> {
        lock(&self.inner.changes)
            .iter()
            .filter_map(|recorded| match recorded.event() {
                Ok(event) => event,
                Err(error) => {
                    tracing::warn!(target: "navree_db::uow", %error, "cannot prepare a change event");
                    None
                }
            })
            .collect()
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
    /// observers write through the transaction they are given (or through
    /// the unit of work, an [`Executor`] until the commit ends), and
    /// [`tx`](Uow::tx) fails from then on. Observers may still register
    /// [`after_commit`](Uow::after_commit) hooks; they run after the ones
    /// registered before the commit. On MySQL the session variables are
    /// cleared after the observers, right before `COMMIT`; if that fails, the
    /// unit of work rolls back and the error is returned.
    pub async fn commit(&self) -> Result<(), DbErr> {
        // Boxed, as `begin`: the commit's future is deep.
        let committing: BoxFuture<'_, Result<bool, DbErr>> = Box::pin(self.commit_tx());
        let committed = committing.await;
        // Taken after the observers ran, so hooks they registered run too.
        let hooks = std::mem::take(&mut *lock(&self.inner.after_commit));
        if committed? {
            for hook in hooks {
                hook().await;
            }
        }
        Ok(())
    }

    /// The transaction to end: the open one; a new one when changes were
    /// recorded without one (their observers still run); `None` when there
    /// is nothing to end.
    async fn ending(&self, current: TxState, changes: &[Recorded]) -> Result<Option<UowTx>, DbErr> {
        Ok(match current {
            TxState::Done | TxState::Idle if changes.is_empty() => None,
            TxState::Done => None,
            TxState::Open(tx) => Some(tx),
            TxState::Idle => Some(self.begin().await?),
        })
    }

    /// The first half of [`commit`](Uow::commit): marks the unit of work
    /// finished, runs the observers and commits the transaction. `false`
    /// when it was already finished (its hooks are dropped then, as after a
    /// failed commit).
    async fn commit_tx(&self) -> Result<bool, DbErr> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        let changes = std::mem::take(&mut *lock(&self.inner.changes));
        if matches!(current, TxState::Done) {
            return Ok(false);
        }
        self.log_query_stats();
        let Some(tx) = self.ending(current, &changes).await? else {
            return Ok(true);
        };
        match tx {
            UowTx::Sea(tx) => self.commit_sea(tx, &changes).await,
            UowTx::Native(tx) => self.commit_native(tx, &changes).await,
        }
    }

    /// Commits SeaORM's transaction.
    async fn commit_sea(
        &self,
        tx: Arc<DatabaseTransaction>,
        changes: &[Recorded],
    ) -> Result<bool, DbErr> {
        // Nobody can take a new handle once the unit of work is finished; a
        // handle still held means the transaction cannot be committed.
        if Arc::strong_count(&tx) > 1 {
            // Rolled back when the last holder drops it.
            self.end_session_sea_or_warn(&tx).await;
            return Err(still_in_use("commit"));
        }
        if let Err(error) = self.observe_sea(&tx, changes).await {
            self.end_session_sea_or_warn(&tx).await;
            rollback_or_warn(sole(tx, "commit")?, "a change observer error").await;
            return Err(error);
        }
        if let Err(error) = self.end_session_sea(&tx).await {
            rollback_or_warn(
                sole(tx, "commit")?,
                "a failure to clear the session variables",
            )
            .await;
            return Err(error);
        }
        sole(tx, "commit")?.commit().await.map(|()| true)
    }

    /// Commits navree-db's own transaction.
    async fn commit_native(
        &self,
        tx: Arc<Transaction>,
        changes: &[Recorded],
    ) -> Result<bool, DbErr> {
        let observed = self.observe_native(&tx, changes).await;
        if let Err(error) = observed {
            self.end_session_native_or_warn(&tx).await;
            if let Err(rollback) = tx.rollback_ref().await {
                tracing::warn!(target: "navree_db::uow", error = %rollback, cause = "a change observer error", "rollback failed");
            }
            return Err(error.into());
        }
        if let Err(error) = self.end_session_native(&tx).await {
            if let Err(rollback) = tx.rollback_ref().await {
                tracing::warn!(target: "navree_db::uow", error = %rollback, cause = "a failure to clear the session variables", "rollback failed");
            }
            return Err(error.into());
        }
        tx.commit_ref().await.map_err(DbErr::from)?;
        Ok(true)
    }

    /// Ends the unit of work as a commit would, except that the transaction
    /// rolls back: the [`ChangeObserver`]s run inside it, it rolls back,
    /// then the after-commit hooks run. The transaction-per-test strategy:
    /// nothing a test wrote stays, and what a commit triggers (jobs, mail,
    /// broadcasts) still happens, so the test can assert it. Hooks run
    /// after the rollback and see what other connections see. Fails like
    /// [`commit`](Uow::commit) when an `Arc` from [`tx`](Uow::tx) is still
    /// held (the transaction then rolls back when the last holder drops it,
    /// and the hooks are dropped); an observer error is returned after the
    /// rollback, and the hooks are dropped. Ending a finished unit of work
    /// does nothing.
    pub async fn rollback_as_committed(&self) -> Result<(), DbErr> {
        // Boxed, as `begin`: ending the transaction's future is deep.
        let ending: BoxFuture<'_, Result<(), DbErr>> = Box::pin(self.end_as_committed());
        ending.await
    }

    /// [`rollback_as_committed`](Uow::rollback_as_committed), unboxed.
    async fn end_as_committed(&self) -> Result<(), DbErr> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        let changes = std::mem::take(&mut *lock(&self.inner.changes));
        if matches!(current, TxState::Done) {
            return Ok(());
        }
        match self.ending(current, &changes).await? {
            Some(UowTx::Sea(tx)) => {
                if Arc::strong_count(&tx) > 1 {
                    self.end_session_sea_or_warn(&tx).await;
                    lock(&self.inner.after_commit).clear();
                    return Err(still_in_use("end"));
                }
                let observed = self.observe_sea(&tx, &changes).await;
                self.end_session_sea_or_warn(&tx).await;
                sole(tx, "end")?.rollback().await?;
                if let Err(error) = observed {
                    lock(&self.inner.after_commit).clear();
                    return Err(error);
                }
            }
            Some(UowTx::Native(tx)) => {
                let observed = self.observe_native(&tx, &changes).await;
                self.end_session_native_or_warn(&tx).await;
                tx.rollback_ref().await?;
                if let Err(error) = observed {
                    lock(&self.inner.after_commit).clear();
                    return Err(error.into());
                }
            }
            None => {}
        }
        // Taken after the observers ran, so hooks they registered run too.
        let hooks = std::mem::take(&mut *lock(&self.inner.after_commit));
        for hook in hooks {
            hook().await;
        }
        Ok(())
    }

    /// Rolls back and drops recorded changes and after-commit hooks.
    /// Rolling back a finished unit of work does nothing. On MySQL the
    /// session variables are cleared first, also when the transaction is
    /// still held elsewhere (it then rolls back when the last holder drops
    /// it).
    pub async fn rollback(&self) -> Result<(), DbErr> {
        // Boxed, as `begin`: ending the transaction's future is deep.
        let ending: BoxFuture<'_, Result<(), DbErr>> = Box::pin(self.roll_back());
        ending.await
    }

    /// [`rollback`](Uow::rollback), unboxed.
    async fn roll_back(&self) -> Result<(), DbErr> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        lock(&self.inner.changes).clear();
        lock(&self.inner.after_commit).clear();
        if !matches!(current, TxState::Done) {
            self.log_query_stats();
        }
        match current {
            TxState::Open(UowTx::Sea(tx)) => {
                self.end_session_sea_or_warn(&tx).await;
                match Arc::try_unwrap(tx) {
                    Ok(tx) => tx.rollback().await,
                    // Still held elsewhere: rolled back when the last holder drops it.
                    Err(_) => Ok(()),
                }
            }
            TxState::Open(UowTx::Native(tx)) => {
                self.end_session_native_or_warn(&tx).await;
                tx.rollback_ref().await.map_err(DbErr::from)
            }
            TxState::Idle | TxState::Done => Ok(()),
        }
    }

    /// Whether the backend supports `SELECT ... FOR UPDATE` (and `SKIP
    /// LOCKED`): true on PostgreSQL and MySQL, false on SQLite, where
    /// `BEGIN IMMEDIATE` already serialises writers.
    pub fn lock_for_update(&self) -> bool {
        self.inner.db.backend() != DbBackend::Sqlite
    }

    /// Begins the unit of work's transaction: navree-db's own or SeaORM's,
    /// as its `Db` says. Boxed: units of work are awaited deep inside
    /// handlers, jobs and commands, and the drivers' futures are deep; a
    /// boxed future keeps every caller's future type shallow.
    fn begin(&self) -> BoxFuture<'_, Result<UowTx, DbErr>> {
        Box::pin(async move {
            if self.inner.db.native_units_of_work() {
                Ok(UowTx::Native(Arc::new(
                    self.begin_native().await.map_err(DbErr::from)?,
                )))
            } else {
                Ok(UowTx::Sea(Arc::new(self.begin_sea().await?)))
            }
        })
    }

    /// The settings statement that follows `BEGIN`: the actor and request
    /// id (none on SQLite, or with `database.transaction_settings` off).
    fn settings_statement(&self, backend: Backend) -> Option<crate::Statement> {
        if !self.inner.db.transaction_settings() {
            return None;
        }
        match backend {
            Backend::Postgres => Some(crate::Statement::new(
                Backend::Postgres,
                "SELECT set_config('navree.actor', $1, true), set_config('navree.request_id', $2, true)",
                [
                    self.inner
                        .actor
                        .map(|a| a.to_string())
                        .unwrap_or_default()
                        .into(),
                    self.inner.request_id.clone().unwrap_or_default().into(),
                ],
            )),
            Backend::MySql => Some(crate::Statement::new(
                Backend::MySql,
                mysql::SET_UOW_VARIABLES,
                [
                    self.inner.actor.map(|a| a.to_string()).into(),
                    self.inner.request_id.clone().into(),
                ],
            )),
            Backend::Sqlite => None,
        }
    }

    /// Begins navree-db's own transaction: `BEGIN IMMEDIATE` on SQLite, the
    /// actor and request id set right after `BEGIN` elsewhere.
    async fn begin_native(&self) -> Result<Transaction, DbError> {
        let db = &self.inner.db;
        let backend = Executor::backend(db);
        let options = match backend {
            Backend::Sqlite => TxOptions::new().immediate(),
            Backend::Postgres | Backend::MySql => TxOptions::new(),
        };
        let tx = db.begin_with(options).await?;
        if let Some(statement) = self.settings_statement(backend) {
            // A failure drops `tx`, which rolls it back.
            tx.execute(&statement).await?;
        }
        Ok(tx)
    }

    /// Begins SeaORM's transaction.
    async fn begin_sea(&self) -> Result<DatabaseTransaction, DbErr> {
        let db = &self.inner.db;
        db.run_before_starting_transaction();
        let backend = match db.backend() {
            // Nothing to set: `database.transaction_settings` is off.
            DbBackend::Postgres | DbBackend::MySql if !db.transaction_settings() => {
                return db.conn().begin().await;
            }
            backend => backend,
        };
        match backend {
            DbBackend::Sqlite => {
                db.conn()
                    .begin_with_options(TransactionOptions {
                        sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                        ..TransactionOptions::default()
                    })
                    .await
            }
            DbBackend::Postgres | DbBackend::MySql => {
                let tx = db.conn().begin().await?;
                let statement = self
                    .settings_statement(crate::engine::compat::backend_of(backend))
                    .expect("PostgreSQL and MySQL set the actor and request id");
                let (sql, values) = statement.into_parts();
                // A failure drops `tx`, which rolls it back.
                tx.execute_raw(Statement::from_sql_and_values(
                    backend,
                    sql,
                    values.iter().map(crate::engine::compat::to_sea_value),
                ))
                .await?;
                Ok(tx)
            }
            _ => db.conn().begin().await,
        }
    }

    /// The change events of `changes`, when the app has observers to see
    /// them.
    async fn events_for_observers(
        &self,
        changes: &[Recorded],
    ) -> Result<Option<(Vec<ChangeEvent>, &[Arc<dyn ChangeObserver>])>, DbErr> {
        if changes.is_empty() {
            return Ok(None);
        }
        let observers = self.inner.db.observers().await?;
        if observers.is_empty() {
            return Ok(None);
        }
        let mut events = Vec::with_capacity(changes.len());
        for recorded in changes {
            events.extend(recorded.event()?);
        }
        Ok(Some((events, observers)))
    }

    async fn observe_sea(
        &self,
        tx: &Arc<DatabaseTransaction>,
        changes: &[Recorded],
    ) -> Result<(), DbErr> {
        let Some((events, observers)) = self.events_for_observers(changes).await? else {
            return Ok(());
        };
        let _observing = Observing::new(self, UowTx::Sea(tx.clone()));
        for observer in observers {
            observer.observe(tx, self, &events).await?;
        }
        Ok(())
    }

    async fn observe_native(
        &self,
        tx: &Arc<Transaction>,
        changes: &[Recorded],
    ) -> Result<(), DbError> {
        let Some((events, observers)) = self.events_for_observers(changes).await? else {
            return Ok(());
        };
        let _observing = Observing::new(self, UowTx::Native(tx.clone()));
        for observer in observers {
            observer.observe_with(self, &events).await?;
        }
        Ok(())
    }

    /// The statement that clears what the unit of work set for the
    /// session, right before the transaction ends. Only MySQL needs it: its
    /// user variables belong to the connection, which goes back to the
    /// pool; PostgreSQL's `set_config(.., true)` ends with the
    /// transaction.
    fn end_session_sql(&self) -> Option<String> {
        if self.inner.db.backend() != DbBackend::MySql {
            return None;
        }
        let extra = std::mem::take(&mut *lock(&self.inner.session_variables));
        let mut sql = if self.inner.db.transaction_settings() {
            mysql::CLEAR_UOW_VARIABLES.to_owned()
        } else if extra.is_empty() {
            return None;
        } else {
            "SET ".to_owned()
        };
        for (i, variable) in extra.iter().enumerate() {
            if i > 0 || sql.len() > "SET ".len() {
                sql.push_str(", ");
            }
            sql.push('@');
            sql.push_str(variable);
            sql.push_str(" = NULL");
        }
        Some(sql)
    }

    async fn end_session_sea(&self, tx: &DatabaseTransaction) -> Result<(), DbErr> {
        if let Some(sql) = self.end_session_sql() {
            tx.execute_unprepared(&sql).await?;
        }
        Ok(())
    }

    async fn end_session_sea_or_warn(&self, tx: &DatabaseTransaction) {
        if let Err(error) = self.end_session_sea(tx).await {
            tracing::warn!(target: "navree_db::uow", %error, "clearing the unit of work's session variables failed");
        }
    }

    async fn end_session_native(&self, tx: &Transaction) -> Result<(), DbError> {
        if let Some(sql) = self.end_session_sql() {
            tx.execute_script(&sql).await?;
        }
        Ok(())
    }

    async fn end_session_native_or_warn(&self, tx: &Transaction) {
        if let Err(error) = self.end_session_native(tx).await {
            tracing::warn!(target: "navree_db::uow", %error, "clearing the unit of work's session variables failed");
        }
    }
}

/// The transaction change observers run in, for the commit's duration.
struct Observing<'a> {
    uow: &'a Uow,
}

impl<'a> Observing<'a> {
    fn new(uow: &'a Uow, tx: UowTx) -> Self {
        *lock(&uow.inner.observing) = Some(tx);
        Observing { uow }
    }
}

impl Drop for Observing<'_> {
    fn drop(&mut self) {
        *lock(&self.uow.inner.observing) = None;
    }
}

/// Statements in the unit of work's transaction (begun on the first one).
/// On a unit of work on SeaORM's transaction they run in that transaction;
/// `execute` and `execute_script` count as writes
/// ([`has_modified_records`](Uow::has_modified_records)).
impl Executor for Uow {
    fn backend(&self) -> Backend {
        crate::engine::compat::backend_of(self.inner.db.backend())
    }

    fn execute<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            self.mark_modified();
            tx.executor().get().execute(statement).await
        })
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            tx.executor().get().fetch_all(statement).await
        })
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            tx.executor().get().fetch_optional(statement).await
        })
    }

    fn fetch<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<RowStream, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            match &tx {
                // SeaORM's own stream, so rows arrive as the database sends
                // them on either transaction.
                UowTx::Sea(sea) => {
                    let statement = SeaExecutor::new(&**sea).statement(statement)?;
                    Ok(crate::engine::compat::stream_on_transaction(
                        Arc::clone(sea),
                        statement,
                    ))
                }
                UowTx::Native(_) => tx.executor().get().fetch(statement).await,
            }
        })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            self.mark_modified();
            tx.executor().get().execute_script(sql).await
        })
    }
}

impl fmt::Debug for Uow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Uow")
            .field("actor", &self.inner.actor)
            .field("impersonator", &self.inner.impersonator)
            .field("request_id", &self.inner.request_id)
            .field("workspace", &self.workspace())
            .field("changes", &lock(&self.inner.changes).len())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The error of a unit of work whose SeaORM transaction is still held when
/// it would `action` (commit, end) it.
fn still_in_use(action: &str) -> DbErr {
    DbErr::Custom(format!(
        "cannot {action} the unit of work: its transaction is still in use; drop every `Arc<DatabaseTransaction>` from `Uow::tx` first (it rolls back when the last one is dropped)"
    ))
}

/// The transaction out of its `Arc`, when nothing else holds it.
fn sole(tx: Arc<DatabaseTransaction>, action: &str) -> Result<DatabaseTransaction, DbErr> {
    Arc::try_unwrap(tx).map_err(|_| still_in_use(action))
}

/// Rolls `tx` back after `cause` stopped its commit.
async fn rollback_or_warn(tx: DatabaseTransaction, cause: &str) {
    if let Err(error) = tx.rollback().await {
        tracing::warn!(target: "navree_db::uow", %error, cause, "rollback failed");
    }
}
