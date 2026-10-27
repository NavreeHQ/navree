//! [`WorkflowStore`]: where runs and steps live, with the database and
//! memory drivers.

mod database;
mod memory;

use chrono::{DateTime, Utc};
use navree_db::Uow;
use navree_kernel::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

pub use database::DatabaseWorkflowStore;
pub use memory::MemoryWorkflowStore;

pub(crate) use database::{from_millis, statement};

use crate::{RunQuery, RunStatus, WorkflowRun, WorkflowStep, WorkflowsError};

/// A store result.
pub type StoreResult<T> = Result<T, WorkflowsError>;

/// An execution's hold on a run: taken by [`WorkflowStore::claim`],
/// renewed by every recorded step, ended by [`WorkflowStore::release`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lease {
    /// The run.
    pub run_id: Uuid,
    /// Identifies this execution: writes with another token are refused.
    pub token: Uuid,
    /// The current time.
    pub now: DateTime<Utc>,
    /// When the hold runs out.
    pub until: DateTime<Utc>,
}

/// How an execution ends (see [`WorkflowStore::release`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Release {
    /// The workflow returned this output.
    Completed(Value),
    /// The workflow failed with this error.
    Failed(String),
    /// A timer or a step's next attempt: wake at this time.
    Sleeping(DateTime<Utc>),
    /// An event wait, timing out at this time (`None`: never).
    Waiting(Option<DateTime<Utc>>),
    /// Run again as soon as possible.
    Pending,
}

/// Which runs [`WorkflowStore::due`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DueQuery {
    /// The current time: running runs whose lease ran out before it are
    /// due.
    pub now: DateTime<Utc>,
    /// Sleeping and waiting runs whose `wake_at` is at or before this are
    /// due.
    pub wake_before: DateTime<Utc>,
    /// Pending runs not touched since this are due (their job was lost);
    /// the store marks them touched at `now`, so the next pass waits again.
    pub stale_before: DateTime<Utc>,
    /// At most this many, oldest first.
    pub limit: u64,
}

/// Where workflow runs and their steps are stored. Two drivers ship:
/// [`DatabaseWorkflowStore`] (`workflow_runs`, `workflow_steps`) and
/// [`MemoryWorkflowStore`]; both pass the same conformance suite.
///
/// Writes that join a caller's unit of work take `Option<&Uow>`: with the
/// database driver they run in its transaction, with the memory driver they
/// apply once it commits. The engine's own writes (claims, steps, releases)
/// run on their own.
pub trait WorkflowStore: Send + Sync + 'static {
    /// Stores a new run.
    fn insert_run<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        run: &'a WorkflowRun,
    ) -> BoxFuture<'a, StoreResult<()>>;

    /// The run `id`.
    fn run(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<WorkflowRun>>>;

    /// The unfinished run of `workflow` started with deduplication key
    /// `key`.
    fn unfinished_with_key<'a>(
        &'a self,
        workflow: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, StoreResult<Option<WorkflowRun>>>;

    /// Runs passing `query`, newest first.
    fn runs<'a>(&'a self, query: &'a RunQuery) -> BoxFuture<'a, StoreResult<Vec<WorkflowRun>>>;

    /// How many runs pass `query` (its limit and offset aside).
    fn count_runs<'a>(&'a self, query: &'a RunQuery) -> BoxFuture<'a, StoreResult<u64>>;

    /// The steps of run `run_id`, in the order they were reached.
    fn steps(&self, run_id: Uuid) -> BoxFuture<'_, StoreResult<Vec<WorkflowStep>>>;

    /// Takes the run for one execution when it is ready: pending, a
    /// sleeping or waiting run whose `wake_at` has passed, or a running run
    /// whose lease ran out. Returns the run, now running (`None`: not
    /// ready, finished, unknown, or held by another execution).
    fn claim(&self, lease: Lease) -> BoxFuture<'_, StoreResult<Option<WorkflowRun>>>;

    /// Inserts or replaces a step (by run and key) and renews the lease;
    /// `false` (and nothing written) when the lease is lost: the run was
    /// cancelled, or another execution took it over. A completed step is
    /// never replaced (an event that resolved a wait wins over a timeout
    /// recorded at the same moment).
    fn save_step<'a>(
        &'a self,
        lease: &'a Lease,
        step: &'a WorkflowStep,
    ) -> BoxFuture<'a, StoreResult<bool>>;

    /// Ends the execution. A run asked to sleep or wait while an event
    /// resolved one of its waits becomes pending instead. Returns the
    /// status written, `None` when the lease was lost.
    fn release<'a>(
        &'a self,
        lease: &'a Lease,
        outcome: &'a Release,
    ) -> BoxFuture<'a, StoreResult<Option<RunStatus>>>;

    /// Runs that need an execution (see [`DueQuery`]), oldest first.
    fn due(&self, query: DueQuery) -> BoxFuture<'_, StoreResult<Vec<Uuid>>>;

    /// The open waits for event `event`, of unfinished runs.
    fn open_waits<'a>(&'a self, event: &'a str) -> BoxFuture<'a, StoreResult<Vec<WorkflowStep>>>;

    /// Resolves an open wait with `output` and wakes its run (pending when
    /// it was sleeping or waiting; flagged when it is running). `false`
    /// when the wait was no longer open.
    fn resolve_wait<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        run_id: Uuid,
        key: &'a str,
        output: &'a Value,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>>;

    /// Cancels an unfinished run; `false` when it was finished or unknown.
    fn cancel<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>>;

    /// Puts a failed run back to pending and forgets its failed steps, so
    /// they run again; `false` when it had not failed.
    fn retry<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>>;

    /// Deletes runs (with their steps) that finished before `before`;
    /// returns how many.
    fn prune(&self, before: DateTime<Utc>) -> BoxFuture<'_, StoreResult<u64>>;
}
