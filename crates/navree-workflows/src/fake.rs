//! [`WorkflowsFake`]: records the runs app code starts, for assertions in
//! tests.

use std::any::type_name;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::service::Inner;
use crate::{RunQuery, RunStatus, StartOptions, Workflow, WorkflowRun, Workflows, WorkflowsError};

/// A start [`WorkflowsFake`] recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct StartedRun {
    /// The id the start returned.
    pub id: Uuid,
    /// [`Workflow::NAME`].
    pub workflow: &'static str,
    /// [`Workflow::VERSION`].
    pub version: u32,
    /// The input type.
    pub type_name: &'static str,
    /// The input, as JSON.
    pub input: Value,
    /// The start's options.
    pub options: StartOptions,
    /// Whether it was cancelled since.
    pub cancelled: bool,
}

#[derive(Default)]
pub(crate) struct FakeState {
    started: Mutex<Vec<StartedRun>>,
}

impl FakeState {
    fn lock(&self) -> MutexGuard<'_, Vec<StartedRun>> {
        self.started.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn record_start<W: Workflow>(
        &self,
        input: W,
        options: StartOptions,
    ) -> Result<Uuid, WorkflowsError> {
        let input = serde_json::to_value(&input).map_err(|error| {
            WorkflowsError::payload(
                &format!("the input of workflow `{}` cannot be stored", W::NAME),
                error,
            )
        })?;
        let mut started = self.lock();
        if let Some(key) = &options.key
            && let Some(existing) = started.iter().find(|run| {
                run.workflow == W::NAME && run.options.key.as_ref() == Some(key) && !run.cancelled
            })
        {
            return Ok(existing.id);
        }
        let id = Uuid::now_v7();
        started.push(StartedRun {
            id,
            workflow: W::NAME,
            version: W::VERSION,
            type_name: type_name::<W>(),
            input,
            options,
            cancelled: false,
        });
        Ok(id)
    }

    pub(crate) fn cancel(&self, id: Uuid) -> Option<Result<WorkflowRun, WorkflowsError>> {
        let mut started = self.lock();
        let run = started.iter_mut().find(|run| run.id == id)?;
        if run.cancelled {
            return Some(Err(WorkflowsError::conflict(
                crate::error::codes::WORKFLOW_RUN_FINISHED,
                "The workflow run has already finished (cancelled).",
            )));
        }
        run.cancelled = true;
        Some(Ok(as_run(run)))
    }

    pub(crate) fn run(&self, id: Uuid) -> Option<WorkflowRun> {
        self.lock().iter().find(|run| run.id == id).map(as_run)
    }

    pub(crate) fn runs(&self, query: &RunQuery) -> Vec<WorkflowRun> {
        let mut runs: Vec<WorkflowRun> = self
            .lock()
            .iter()
            .map(as_run)
            .filter(|run| query.matches(run))
            .collect();
        runs.reverse();
        runs
    }
}

/// A recorded start as a run: pending, or cancelled.
fn as_run(started: &StartedRun) -> WorkflowRun {
    let now = Utc::now();
    WorkflowRun {
        id: started.id,
        workflow: started.workflow.to_owned(),
        version: started.version,
        status: if started.cancelled {
            RunStatus::Cancelled
        } else {
            RunStatus::Pending
        },
        input: started.input.clone(),
        output: None,
        error: None,
        workspace_id: started.options.workspace_id,
        key: started.options.key.clone(),
        wake_at: None,
        created_at: now,
        updated_at: now,
        finished_at: started.cancelled.then_some(now),
    }
}

/// Records the runs app code starts instead of running them, like
/// `QueueFake` for jobs. Install its [`Workflows`] in the test app, run the
/// code, then assert on what was started.
///
/// ```
/// use navree_kernel::BoxError;
/// use navree_workflows::{Workflow, WorkflowCtx, WorkflowsFake};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Onboarding {
///     user_id: u64,
/// }
///
/// impl Workflow for Onboarding {
///     const NAME: &'static str = "onboarding";
///     type Output = ();
///     async fn run(self, _ctx: WorkflowCtx) -> Result<(), BoxError> {
///         Ok(())
///     }
/// }
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), BoxError> {
/// let fake = WorkflowsFake::new();
/// // TestApp::builder().override_value(std::sync::Arc::new(fake.workflows()))
/// let workflows = fake.workflows();
/// workflows.spawn(Onboarding { user_id: 7 }).await?;
///
/// fake.assert_started::<Onboarding>(|input| input.user_id == 7);
/// fake.assert_started_times::<Onboarding>(1);
/// # Ok(())
/// # }
/// ```
///
/// Reads through its `Workflows` see each recorded start as a pending run
/// (cancelled once cancelled); nothing executes, `tick` and `deliver` wake
/// nothing, and `retry` is a 409.
#[derive(Clone, Default)]
pub struct WorkflowsFake {
    state: Arc<FakeState>,
}

impl std::fmt::Debug for WorkflowsFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowsFake")
            .field("started", &self.state.lock().len())
            .finish()
    }
}

impl WorkflowsFake {
    /// A fake with nothing recorded.
    pub fn new() -> Self {
        Self::default()
    }

    /// The [`Workflows`] to install: `override_value(Arc::new(fake.workflows()))`.
    pub fn workflows(&self) -> Workflows {
        Workflows {
            inner: Inner::Fake(self.state.clone()),
        }
    }

    /// Every recorded start, oldest first.
    pub fn started_runs(&self) -> Vec<StartedRun> {
        self.state.lock().clone()
    }

    /// The inputs of the recorded starts of `W`, oldest first.
    pub fn started<W: Workflow>(&self) -> Vec<W> {
        self.state
            .lock()
            .iter()
            .filter(|run| run.type_name == type_name::<W>())
            .filter_map(|run| serde_json::from_value(run.input.clone()).ok())
            .collect()
    }

    /// Panics unless a start of `W` passes `pred`.
    pub fn assert_started<W: Workflow>(&self, pred: impl Fn(&W) -> bool) {
        let started = self.started::<W>();
        assert!(
            started.iter().any(pred),
            "expected a start of workflow `{}` matching the predicate; {} start(s) of it were \
             recorded",
            W::NAME,
            started.len()
        );
    }

    /// Panics unless `W` was started exactly `times` times.
    pub fn assert_started_times<W: Workflow>(&self, times: usize) {
        let count = self.started::<W>().len();
        assert_eq!(
            count,
            times,
            "expected workflow `{}` to be started {times} time(s), it was started {count} time(s)",
            W::NAME
        );
    }

    /// Panics when `W` was started.
    pub fn assert_not_started<W: Workflow>(&self) {
        self.assert_started_times::<W>(0);
    }

    /// Panics when anything was started.
    pub fn assert_nothing_started(&self) {
        let started = self.started_runs();
        assert!(
            started.is_empty(),
            "expected no workflow starts, got: {:?}",
            started.iter().map(|run| run.workflow).collect::<Vec<_>>()
        );
    }

    /// The ids of the cancelled runs.
    pub fn cancelled(&self) -> Vec<Uuid> {
        self.state
            .lock()
            .iter()
            .filter(|run| run.cancelled)
            .map(|run| run.id)
            .collect()
    }

    /// Panics unless run `id` was cancelled.
    pub fn assert_cancelled(&self, id: Uuid) {
        assert!(
            self.cancelled().contains(&id),
            "expected workflow run {id} to be cancelled"
        );
    }

    /// Forgets everything recorded.
    pub fn clear(&self) {
        self.state.lock().clear();
    }
}
