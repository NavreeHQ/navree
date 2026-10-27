//! [`MongoUow`]: the unit of work on MongoDB.

use std::borrow::Cow;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};

use navree_db::{ChangeEvent, Uow};
use navree_kernel::BoxFuture;
use navree_orm::{Access, ChangeRecorder, DocumentTarget, Executor, OrmError, Target};
use uuid::Uuid;

use crate::connection::{MongoDb, MongoExecutor};
use crate::driver::{MongoDriver, SessionState};
use crate::error::{DRIVER, MongoError, MongoResult};

/// Sees every MongoDB unit of work's changes right before it commits,
/// inside its transaction (an audit trail written as documents, say). An
/// error rolls the unit of work back.
///
/// Register implementations with
/// [`ModuleBuilderMongoExt::mongo_change_observer`](crate::ModuleBuilderMongoExt::mongo_change_observer).
/// A unit of work joined to navree-db's ([`MongoDb::begin_in`]) gives its
/// changes to navree-db's `ChangeObserver`s instead.
pub trait MongoChangeObserver: Send + Sync + 'static {
    /// Called once per commit with every recorded change, in order. Write
    /// through `uow` to write in the transaction.
    fn observe<'a>(
        &'a self,
        uow: &'a MongoUow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, MongoResult>;
}

/// A hook run after a successful commit.
type AfterCommit = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

/// Where a unit of work is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Open,
    Committing,
    Committed,
    RolledBack,
}

/// The change events of a unit of work.
#[derive(Default)]
struct Changes(Mutex<Vec<ChangeEvent>>);

impl ChangeRecorder for Changes {
    fn record_change(&self, event: ChangeEvent) {
        lock(&self.0).push(event);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The unit of work on MongoDB: a session with a transaction, the actor and
/// request id it works for, the change events of the model writes in it,
/// and hooks to run after it commits. It mirrors navree-db's `Uow`.
///
/// Models run on it like on a connection (`Event::create(&uow, event)`),
/// in its transaction; inside [`scope`](MongoUow::scope), a plain
/// [`MongoDb`] runs in it too. [`commit`](MongoUow::commit) shows the
/// changes to the app's [`MongoChangeObserver`]s inside the transaction,
/// commits, then runs the after-commit hooks; [`rollback`](MongoUow::rollback)
/// (or dropping every handle) aborts. [`MongoDb::transaction`] retries the
/// whole work on a write conflict.
///
/// Transactions need a replica set or a sharded cluster (a single-node
/// replica set does): on a standalone server the first write fails, saying
/// so.
///
/// ```no_run
/// # use navree_mongo::MongoDb;
/// # async fn demo(mongo: MongoDb) -> Result<(), navree_mongo::MongoError> {
/// let uow = mongo.unit_of_work(None, Some("req-42".into())).await?;
/// // ... Event::create(&uow, ..).await?; Counter::query().increment(&uow, ..).await?;
/// uow.after_commit(|| Box::pin(async { /* notify */ }));
/// uow.commit().await?;
/// # Ok(()) }
/// ```
#[derive(Clone)]
pub struct MongoUow {
    inner: Arc<Inner>,
}

struct Inner {
    db: MongoDb,
    state: Arc<SessionState>,
    driver: Arc<MongoDriver>,
    actor: Option<Uuid>,
    request_id: Option<String>,
    joined: Option<Uow>,
    phase: Mutex<Phase>,
    changes: Arc<Changes>,
    after_commit: Mutex<Vec<AfterCommit>>,
}

tokio::task_local! {
    /// The MongoDB unit of work of the running task.
    static CURRENT: MongoUow;
}

impl MongoUow {
    /// Starts a session and its transaction.
    pub(crate) async fn begin(
        db: &MongoDb,
        actor: Option<Uuid>,
        request_id: Option<String>,
        joined: Option<Uow>,
    ) -> MongoResult<MongoUow> {
        let mut session = db.client().start_session().await?;
        session.start_transaction().await?;
        let state = SessionState::new(session);
        state.transaction.store(true, Ordering::Relaxed);
        let uow = MongoUow {
            inner: Arc::new(Inner {
                db: db.clone(),
                driver: db.session_driver(state.clone()),
                state,
                actor,
                request_id,
                joined: joined.clone(),
                phase: Mutex::new(Phase::Open),
                changes: Arc::new(Changes::default()),
                after_commit: Mutex::new(Vec::new()),
            }),
        };
        if let Some(sql) = joined {
            // Commits once the database's unit of work has; when that rolls
            // back, the hook is dropped and the transaction aborted with
            // the last handle.
            let committer = uow.clone();
            sql.after_commit(move || {
                Box::pin(async move {
                    if let Err(error) = committer.commit().await {
                        tracing::error!(
                            target: "navree_mongo::uow",
                            %error,
                            "the MongoDB transaction of a unit of work did not commit after the database's did"
                        );
                    }
                })
            });
        }
        Ok(uow)
    }

    /// The MongoDB unit of work of the current task: the one a
    /// [`scope`](Self::scope) runs a future with; `None` elsewhere.
    pub fn current() -> Option<MongoUow> {
        CURRENT.try_with(MongoUow::clone).ok()
    }

    /// Runs `future` with this unit of work as [`current`](Self::current),
    /// so a plain [`MongoDb`] of the same connection runs in it.
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    /// The connection.
    pub fn db(&self) -> &MongoDb {
        &self.inner.db
    }

    /// The user the work is done for, if any.
    pub fn actor(&self) -> Option<Uuid> {
        self.inner.actor
    }

    /// The request id, if any.
    pub fn request_id(&self) -> Option<&str> {
        self.inner.request_id.as_deref()
    }

    /// navree-db's unit of work this one joined, if any.
    pub fn joined(&self) -> Option<&Uow> {
        self.inner.joined.as_ref()
    }

    /// Whether `self` and `other` are handles to the same unit of work.
    pub fn same_as(&self, other: &MongoUow) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Records a change for the observers (model writes record theirs).
    pub fn record_change(&self, event: ChangeEvent) {
        match &self.inner.joined {
            Some(uow) => uow.record_change(event),
            None => self.inner.changes.record_change(event),
        }
    }

    /// The changes recorded so far (a joined unit of work records them in
    /// navree-db's).
    pub fn changes(&self) -> Vec<ChangeEvent> {
        match &self.inner.joined {
            Some(uow) => uow.changes(),
            None => lock(&self.inner.changes.0).clone(),
        }
    }

    /// Runs `hook` after a successful commit (dispatch jobs, send mail,
    /// broadcast); dropped on rollback. Hooks run in registration order.
    pub fn after_commit(&self, hook: impl FnOnce() -> BoxFuture<'static, ()> + Send + 'static) {
        lock(&self.inner.after_commit).push(Box::new(hook));
    }

    /// Whether the unit of work has committed or rolled back.
    pub fn is_finished(&self) -> bool {
        matches!(
            *lock(&self.inner.phase),
            Phase::Committed | Phase::RolledBack
        )
    }

    /// Whether an operation in it failed with a transient transaction
    /// error (a write conflict): the work can be run again in a new unit
    /// of work.
    pub fn had_transient_error(&self) -> bool {
        self.inner.state.transient.load(Ordering::Relaxed)
    }

    /// Shows the changes to the app's [`MongoChangeObserver`]s inside the
    /// transaction (not for a joined unit of work: navree-db's observers
    /// see them), commits, then runs the after-commit hooks. An observer's
    /// error, or a failed commit, rolls back and is returned. Committing a
    /// finished unit of work does nothing.
    pub async fn commit(&self) -> MongoResult {
        {
            let mut phase = lock(&self.inner.phase);
            match *phase {
                Phase::Open => *phase = Phase::Committing,
                Phase::Committing => return Err(MongoError::Finished("committing")),
                Phase::Committed | Phase::RolledBack => return Ok(()),
            }
        }
        let changes = std::mem::take(&mut *lock(&self.inner.changes.0));
        if self.inner.joined.is_none()
            && !changes.is_empty()
            && let Some(app) = self.inner.db.app()
        {
            let observers = app.get_all::<dyn MongoChangeObserver>().await?;
            for observer in observers {
                if let Err(error) = observer.observe(self, &changes).await {
                    self.abort(Phase::RolledBack).await;
                    return Err(error);
                }
            }
        }
        let committed = {
            let mut session = self.inner.state.session.lock().await;
            let mut result = session.commit_transaction().await;
            // The commit's outcome is unknown after a network error: try
            // once more (committing is idempotent).
            if let Err(error) = &result
                && error.contains_label(mongodb::error::UNKNOWN_TRANSACTION_COMMIT_RESULT)
            {
                result = session.commit_transaction().await;
            }
            result
        };
        match committed {
            Ok(()) => {
                *lock(&self.inner.phase) = Phase::Committed;
                let hooks = std::mem::take(&mut *lock(&self.inner.after_commit));
                for hook in hooks {
                    hook().await;
                }
                Ok(())
            }
            Err(error) => {
                self.abort(Phase::RolledBack).await;
                Err(error.into())
            }
        }
    }

    /// Aborts the transaction and drops the recorded changes and hooks.
    /// Rolling back a finished unit of work does nothing.
    pub async fn rollback(&self) -> MongoResult {
        {
            let phase = *lock(&self.inner.phase);
            if matches!(phase, Phase::Committed | Phase::RolledBack) {
                return Ok(());
            }
        }
        let aborted = self
            .inner
            .state
            .session
            .lock()
            .await
            .abort_transaction()
            .await;
        *lock(&self.inner.phase) = Phase::RolledBack;
        lock(&self.inner.changes.0).clear();
        lock(&self.inner.after_commit).clear();
        aborted.map_err(MongoError::from)
    }

    /// Aborts after a failure, keeping the first error.
    async fn abort(&self, phase: Phase) {
        let result = self
            .inner
            .state
            .session
            .lock()
            .await
            .abort_transaction()
            .await;
        if let Err(error) = result {
            tracing::debug!(target: "navree_mongo::uow", %error, "aborting the transaction after a failure failed");
        }
        *lock(&self.inner.phase) = phase;
        lock(&self.inner.after_commit).clear();
    }

    /// Where the next operation runs: the session's driver, recording
    /// change events here (or in the joined unit of work). A document
    /// target borrows nothing, so it outlives this handle.
    pub(crate) fn document_target(&self) -> Result<Target<'static>, OrmError> {
        let driver = self.mongo_driver()?;
        let target = DocumentTarget::new(driver, self.inner.db.app());
        Ok(Target::Document(match &self.inner.joined {
            Some(uow) => target.with_uow(uow.clone()),
            None => target.recording_changes(self.inner.changes.clone()),
        }))
    }
}

impl Executor for MongoUow {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        self.document_target()
    }
}

impl MongoExecutor for MongoUow {
    fn mongo_driver(&self) -> Result<Arc<MongoDriver>, OrmError> {
        match *lock(&self.inner.phase) {
            Phase::Open | Phase::Committing => Ok(self.inner.driver.clone()),
            Phase::Committed => Err(finished("committed")),
            Phase::RolledBack => Err(finished("rolled back")),
        }
    }
}

fn finished(what: &str) -> OrmError {
    OrmError::Driver {
        driver: Cow::Borrowed(DRIVER),
        message: format!("the MongoDB unit of work is already {what}: open another one"),
    }
}

impl std::fmt::Debug for MongoUow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoUow")
            .field("actor", &self.inner.actor)
            .field("request_id", &self.inner.request_id)
            .field("joined", &self.inner.joined.is_some())
            .field("phase", &*lock(&self.inner.phase))
            .finish()
    }
}
