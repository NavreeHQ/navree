//! [`Uow`]: the unit of work, plus [`ChangeEvent`]s and [`ChangeObserver`]s.

use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::queries::{QueryCounter, log_stats};
use crate::{
    Backend, Db, DbError, ErrorKind, Executed, Executor, IsolationLevel, QueryStats, Row,
    RowStream, Transaction, TxOptions, mysql,
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

/// A change to one record, recorded in the unit of work (by navree-orm's
/// model writes, the recorded writes of [`record`](crate::record), or by
/// hand with [`Uow::record_change`]) and handed to every [`ChangeObserver`]
/// at commit.
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
/// ```
/// use navree_db::{ChangeEvent, ChangeObserver, DbError, Executor, Statement, Uow};
/// use navree_kernel::BoxFuture;
///
/// struct Counter;
///
/// impl ChangeObserver for Counter {
///     fn observe<'a>(&'a self, uow: &'a Uow, changes: &'a [ChangeEvent]) -> BoxFuture<'a, Result<(), DbError>> {
///         Box::pin(async move {
///             let statement = Statement::portable(
///                 "UPDATE change_counts SET total = total + ?",
///                 [(changes.len() as i64).into()],
///             );
///             uow.execute(&statement).await?;
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait ChangeObserver: Send + Sync + 'static {
    /// Called once per commit with every recorded change, in order. `uow`
    /// runs statements in the transaction being committed (it is an
    /// [`Executor`] until the commit ends).
    fn observe<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>>;
}

/// A hook run after a successful commit.
type AfterCommit = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

/// Prepares a change event on demand: `None` when the write changed
/// nothing worth an event.
pub(crate) type PrepareChange = Box<dyn Fn() -> Result<Option<ChangeEvent>, DbError> + Send + Sync>;

/// A recorded change: its event, or what prepares it. Writes defer the
/// event (serializing records to JSON) in an app with no change observer,
/// where only [`Uow::changes`] reads them.
enum Recorded {
    Ready(ChangeEvent),
    Deferred(PrepareChange),
}

impl Recorded {
    /// The event, prepared now if it was deferred.
    fn event(&self) -> Result<Option<ChangeEvent>, DbError> {
        match self {
            Recorded::Ready(event) => Ok(Some(event.clone())),
            Recorded::Deferred(prepare) => prepare(),
        }
    }
}

enum TxState {
    /// No transaction yet (the unit of work is lazy).
    Idle,
    /// The open transaction.
    Open(Arc<Transaction>),
    /// Committed or rolled back.
    Done,
}

/// The unit of work: one per HTTP request, job run or command. It carries
/// the actor and request id, opens its database transaction lazily on the
/// first statement (it is an [`Executor`]), collects [`ChangeEvent`]s and
/// after-commit hooks, and commits or rolls back once.
///
/// In HTTP handlers, take it as an extractor (the `DatabaseModule` layer
/// commits it when the response status is below 400 and rolls it back
/// otherwise). Elsewhere, create one with [`Db::uow`]. Services take
/// `&Uow` as a parameter instead of having it injected; navree-orm's
/// models run on it (`Deal::create(&uow, ..)`), and so do navree-db's
/// statements (`uow.fetch_all(&statement)`), savepoints
/// ([`savepoint`](Uow::savepoint)) and transaction-local settings
/// ([`set_local`](Uow::set_local)).
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
/// is query scoping (every query of a workspace-scoped model filters on
/// `workspace_id`), and only PostgreSQL can also enforce it with policies.
/// Rows are locked with `SELECT ... FOR UPDATE` on PostgreSQL and MySQL
/// ([`lock_for_update`](Uow::lock_for_update)). A deadlock (error 1213)
/// makes MySQL roll the whole transaction back at once, and whatever runs
/// through it afterwards commits on its own, beyond the reach of
/// [`rollback`](Uow::rollback): return the error instead of carrying on.
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
    /// The isolation level the transaction begins at, when one was chosen.
    isolation: Mutex<Option<IsolationLevel>>,
    tx: tokio::sync::Mutex<TxState>,
    /// The transaction while change observers run inside the commit (the
    /// unit of work already counts as finished).
    observing: Mutex<Option<Arc<Transaction>>>,
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
                isolation: Mutex::new(None),
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

    /// Chooses the isolation level the unit of work's transaction begins at,
    /// on PostgreSQL and MySQL, in place of the server's or the pool's own
    /// (`database.postgres.isolation_level`, `database.mysql.isolation_level`).
    /// SQLite's transactions are serializable whatever is chosen. It must come
    /// before the first statement: once the transaction has begun, or the unit
    /// of work has finished, it is refused, so a level is never silently not
    /// the one asked for.
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
    /// use navree_db::IsolationLevel;
    ///
    /// let uow = db.uow(None, None).await;
    /// uow.set_isolation(IsolationLevel::ReadCommitted).await?;
    /// // every statement through `uow` now runs in a READ COMMITTED transaction
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidStatement`] when the transaction has begun or the
    /// unit of work has finished.
    pub async fn set_isolation(&self, level: IsolationLevel) -> Result<(), DbError> {
        let state = self.inner.tx.lock().await;
        if !matches!(*state, TxState::Idle) {
            return Err(DbError::new(
                ErrorKind::InvalidStatement,
                "a unit of work's isolation level is chosen before its first statement",
            ));
        }
        *lock(&self.inner.isolation) = Some(level);
        Ok(())
    }

    /// The isolation level chosen with [`set_isolation`](Uow::set_isolation),
    /// or `None` for the server's or the pool's own.
    pub fn isolation(&self) -> Option<IsolationLevel> {
        *lock(&self.inner.isolation)
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
    /// ORM's writes, recorded writes and statements through `execute` do
    /// it); see [`has_modified_records`](Uow::has_modified_records).
    pub fn mark_modified(&self) {
        self.inner.modified.store(true, Ordering::Relaxed);
    }

    /// Whether anything was written through the unit of work (also after it
    /// committed). With read replicas and `sticky` (the default), the
    /// task's reads then go to the writer, so it reads its own writes.
    pub fn has_modified_records(&self) -> bool {
        self.inner.modified.load(Ordering::Relaxed)
    }

    /// The transaction for statements: the one observers run in during
    /// the commit, else the open one, else a new one.
    async fn transaction(&self) -> Result<Arc<Transaction>, DbError> {
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
                let tx = Arc::new(self.begin().await?);
                *state = TxState::Open(tx.clone());
                Ok(tx)
            }
        }
    }

    /// A savepoint inside the unit of work's transaction (begun if need
    /// be): a nested level that commits or rolls back on its own, for work
    /// that may fail without failing the unit of work.
    ///
    /// ```no_run
    /// # async fn demo(uow: navree_db::Uow) -> Result<(), navree_db::DbError> {
    /// use navree_db::{Executor, Statement};
    ///
    /// let savepoint = uow.savepoint().await?;
    /// match savepoint.execute(&Statement::text("UPDATE deals SET stage = 'won'")).await {
    ///     Ok(_) => savepoint.commit().await?,
    ///     Err(_) => savepoint.rollback().await?,
    /// }
    /// # Ok(()) }
    /// ```
    pub async fn savepoint(&self) -> Result<Transaction, DbError> {
        self.transaction().await?.savepoint().await
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
    /// # async fn main() -> Result<(), navree_db::DbError> {
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
    pub(crate) fn record_with(&self, prepare: PrepareChange) -> Result<(), DbError> {
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

    /// [`record_with`](Uow::record_with) for a closure (the record
    /// statements of [`crate::record`]).
    pub(crate) fn record_lazily(
        &self,
        prepare: impl Fn() -> Result<Option<ChangeEvent>, DbError> + Send + Sync + 'static,
    ) -> Result<(), DbError> {
        self.record_with(Box::new(prepare))
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
    /// Committing a finished unit of work does nothing.
    ///
    /// The unit of work counts as finished as soon as `commit` starts:
    /// observers write through the unit of work (an [`Executor`] on the
    /// transaction being committed until the commit ends). Observers may
    /// still register [`after_commit`](Uow::after_commit) hooks; they run
    /// after the ones registered before the commit. On MySQL the session
    /// variables are cleared after the observers, right before `COMMIT`;
    /// if that fails, the unit of work rolls back and the error is
    /// returned.
    pub async fn commit(&self) -> Result<(), DbError> {
        // Boxed, as `begin`: the commit's future is deep.
        let committing: BoxFuture<'_, Result<bool, DbError>> = Box::pin(self.commit_tx());
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
    async fn ending(
        &self,
        current: TxState,
        changes: &[Recorded],
    ) -> Result<Option<Arc<Transaction>>, DbError> {
        Ok(match current {
            TxState::Done | TxState::Idle if changes.is_empty() => None,
            TxState::Done => None,
            TxState::Open(tx) => Some(tx),
            TxState::Idle => Some(Arc::new(self.begin().await?)),
        })
    }

    /// The first half of [`commit`](Uow::commit): marks the unit of work
    /// finished, runs the observers and commits the transaction. `false`
    /// when it was already finished (its hooks are dropped then, as after a
    /// failed commit).
    async fn commit_tx(&self) -> Result<bool, DbError> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        let changes = std::mem::take(&mut *lock(&self.inner.changes));
        if matches!(current, TxState::Done) {
            return Ok(false);
        }
        self.log_query_stats();
        let Some(tx) = self.ending(current, &changes).await? else {
            return Ok(true);
        };
        if let Err(error) = self.observe(&tx, &changes).await {
            self.end_session_or_warn(&tx).await;
            rollback_or_warn(&tx, "a change observer error").await;
            return Err(error);
        }
        if let Err(error) = self.end_session(&tx).await {
            rollback_or_warn(&tx, "a failure to clear the session variables").await;
            return Err(error);
        }
        tx.commit_ref().await?;
        Ok(true)
    }

    /// Ends the unit of work as a commit would, except that the transaction
    /// rolls back: the [`ChangeObserver`]s run inside it, it rolls back,
    /// then the after-commit hooks run. The transaction-per-test strategy:
    /// nothing a test wrote stays, and what a commit triggers (jobs, mail,
    /// broadcasts) still happens, so the test can assert it. Hooks run
    /// after the rollback and see what other connections see. An observer
    /// error is returned after the rollback, and the hooks are dropped.
    /// Ending a finished unit of work does nothing.
    pub async fn rollback_as_committed(&self) -> Result<(), DbError> {
        // Boxed, as `begin`: ending the transaction's future is deep.
        let ending: BoxFuture<'_, Result<(), DbError>> = Box::pin(self.end_as_committed());
        ending.await
    }

    /// [`rollback_as_committed`](Uow::rollback_as_committed), unboxed.
    async fn end_as_committed(&self) -> Result<(), DbError> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        let changes = std::mem::take(&mut *lock(&self.inner.changes));
        if matches!(current, TxState::Done) {
            return Ok(());
        }
        if let Some(tx) = self.ending(current, &changes).await? {
            let observed = self.observe(&tx, &changes).await;
            self.end_session_or_warn(&tx).await;
            tx.rollback_ref().await?;
            if let Err(error) = observed {
                lock(&self.inner.after_commit).clear();
                return Err(error);
            }
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
    /// session variables are cleared first.
    pub async fn rollback(&self) -> Result<(), DbError> {
        // Boxed, as `begin`: ending the transaction's future is deep.
        let ending: BoxFuture<'_, Result<(), DbError>> = Box::pin(self.roll_back());
        ending.await
    }

    /// [`rollback`](Uow::rollback), unboxed.
    async fn roll_back(&self) -> Result<(), DbError> {
        let current = std::mem::replace(&mut *self.inner.tx.lock().await, TxState::Done);
        lock(&self.inner.changes).clear();
        lock(&self.inner.after_commit).clear();
        if !matches!(current, TxState::Done) {
            self.log_query_stats();
        }
        match current {
            TxState::Open(tx) => {
                self.end_session_or_warn(&tx).await;
                tx.rollback_ref().await
            }
            TxState::Idle | TxState::Done => Ok(()),
        }
    }

    /// Whether the backend supports `SELECT ... FOR UPDATE` (and `SKIP
    /// LOCKED`): true on PostgreSQL and MySQL, false on SQLite, where
    /// `BEGIN IMMEDIATE` already serialises writers.
    pub fn lock_for_update(&self) -> bool {
        self.inner.db.backend() != Backend::Sqlite
    }

    /// Begins the unit of work's transaction: `BEGIN IMMEDIATE` on SQLite,
    /// the actor and request id set right after `BEGIN` elsewhere. Boxed:
    /// units of work are awaited deep inside handlers, jobs and commands,
    /// and the drivers' futures are deep; a boxed future keeps every
    /// caller's future type shallow.
    fn begin(&self) -> BoxFuture<'_, Result<Transaction, DbError>> {
        Box::pin(async move {
            let db = &self.inner.db;
            let backend = db.backend();
            let options = match (backend, self.isolation()) {
                (Backend::Sqlite, _) => TxOptions::new().immediate(),
                (Backend::Postgres | Backend::MySql, Some(level)) => {
                    TxOptions::new().isolation(level)
                }
                (Backend::Postgres | Backend::MySql, None) => TxOptions::new(),
            };
            let tx = db.begin_with(options).await?;
            if let Some(statement) = self.settings_statement(backend) {
                // A failure drops `tx`, which rolls it back.
                tx.execute(&statement).await?;
            }
            Ok(tx)
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

    /// Runs the change observers on `changes`, in `tx`, when the app has
    /// any.
    async fn observe(&self, tx: &Arc<Transaction>, changes: &[Recorded]) -> Result<(), DbError> {
        if changes.is_empty() {
            return Ok(());
        }
        let observers = self.inner.db.observers().await?;
        if observers.is_empty() {
            return Ok(());
        }
        let mut events = Vec::with_capacity(changes.len());
        for recorded in changes {
            events.extend(recorded.event()?);
        }
        let _observing = Observing::new(self, tx.clone());
        for observer in observers {
            observer.observe(self, &events).await?;
        }
        Ok(())
    }

    /// The statement that clears what the unit of work set for the
    /// session, right before the transaction ends. Only MySQL needs it: its
    /// user variables belong to the connection, which goes back to the
    /// pool; PostgreSQL's `set_config(.., true)` ends with the
    /// transaction.
    fn end_session_sql(&self) -> Option<String> {
        if self.inner.db.backend() != Backend::MySql {
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

    async fn end_session(&self, tx: &Transaction) -> Result<(), DbError> {
        if let Some(sql) = self.end_session_sql() {
            tx.execute_script(&sql).await?;
        }
        Ok(())
    }

    async fn end_session_or_warn(&self, tx: &Transaction) {
        if let Err(error) = self.end_session(tx).await {
            tracing::warn!(target: "navree_db::uow", %error, "clearing the unit of work's session variables failed");
        }
    }
}

/// The transaction change observers run in, for the commit's duration.
struct Observing<'a> {
    uow: &'a Uow,
}

impl<'a> Observing<'a> {
    fn new(uow: &'a Uow, tx: Arc<Transaction>) -> Self {
        *lock(&uow.inner.observing) = Some(tx);
        Observing { uow }
    }
}

impl Drop for Observing<'_> {
    fn drop(&mut self) {
        *lock(&self.uow.inner.observing) = None;
    }
}

/// Rolls `tx` back after `cause` stopped its commit.
async fn rollback_or_warn(tx: &Transaction, cause: &str) {
    if let Err(error) = tx.rollback_ref().await {
        tracing::warn!(target: "navree_db::uow", %error, cause, "rollback failed");
    }
}

/// Statements in the unit of work's transaction (begun on the first one);
/// `execute` and `execute_script` count as writes
/// ([`has_modified_records`](Uow::has_modified_records)).
impl Executor for Uow {
    fn backend(&self) -> Backend {
        self.inner.db.backend()
    }

    fn execute<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            self.mark_modified();
            tx.execute(statement).await
        })
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        Box::pin(async move { self.transaction().await?.fetch_all(statement).await })
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        Box::pin(async move { self.transaction().await?.fetch_optional(statement).await })
    }

    fn fetch<'a>(
        &'a self,
        statement: &'a crate::Statement,
    ) -> BoxFuture<'a, Result<RowStream, DbError>> {
        Box::pin(async move { self.transaction().await?.fetch(statement).await })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move {
            let tx = self.transaction().await?;
            self.mark_modified();
            tx.execute_script(sql).await
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
