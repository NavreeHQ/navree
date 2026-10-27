//! [`Workflows`]: start, inspect, cancel and retry durable runs.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::{Page, PageQuery, Uow};
use serde_json::Value;
use uuid::Uuid;

use crate::engine::{Engine, Execution};
use crate::error::codes;
use crate::fake::FakeState;
use crate::{
    RunDetails, RunQuery, StartOptions, Workflow, WorkflowDefinition, WorkflowRun, WorkflowsError,
};

/// Durable workflows: start runs, read them with their steps, cancel and
/// retry them. Resolve it from the app (`ctx.get::<Workflows>()`) or take
/// it as a dependency.
///
/// ```ignore
/// let run_id = workflows.start(&uow, Onboarding { user_id }).await?;
/// ```
///
/// A run starts once the unit of work commits. With a job dispatcher
/// (`QueueModule`) every execution is a `workflows.run` job; without one,
/// runs execute inline (right after the start commits, and inside the
/// emit of an event that resolves a wait) and timers are woken by
/// `workflows:tick`.
#[derive(Clone)]
pub struct Workflows {
    pub(crate) inner: Inner,
}

#[derive(Clone)]
pub(crate) enum Inner {
    Engine(Arc<Engine>),
    Fake(Arc<FakeState>),
}

impl std::fmt::Debug for Workflows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match &self.inner {
            Inner::Engine(_) => "engine",
            Inner::Fake(_) => "fake",
        };
        f.debug_struct("Workflows")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

impl Workflows {
    pub(crate) fn new(engine: Arc<Engine>) -> Self {
        Self {
            inner: Inner::Engine(engine),
        }
    }

    fn engine(&self) -> Result<&Arc<Engine>, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => Ok(engine),
            Inner::Fake(_) => Err(WorkflowsError::Other(
                "WorkflowsFake records starts and runs nothing".to_owned(),
            )),
        }
    }

    /// The definition of `W`, or [`WorkflowsError::Unregistered`].
    pub fn definition<W: Workflow>(&self) -> Result<WorkflowDefinition, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => engine
                .registry
                .of::<W>()
                .cloned()
                .ok_or_else(|| WorkflowsError::unregistered(W::NAME, std::any::type_name::<W>())),
            Inner::Fake(_) => Ok(WorkflowDefinition::of::<W>()),
        }
    }

    /// Every registered workflow, by name, newest version first.
    pub fn definitions(&self) -> Vec<WorkflowDefinition> {
        match &self.inner {
            Inner::Engine(engine) => engine.registry.iter().cloned().collect(),
            Inner::Fake(_) => Vec::new(),
        }
    }

    /// Starts a run of `input`'s workflow, on its newest version, when `uow`
    /// commits; returns the run's id.
    pub async fn start<W: Workflow>(&self, uow: &Uow, input: W) -> Result<Uuid, WorkflowsError> {
        self.start_with(uow, input, StartOptions::default()).await
    }

    /// [`start`](Workflows::start) with a workspace, a deduplication key
    /// or a delay. With a key, an unfinished run of the same workflow with
    /// that key is returned instead of a new one.
    pub async fn start_with<W: Workflow>(
        &self,
        uow: &Uow,
        input: W,
        options: StartOptions,
    ) -> Result<Uuid, WorkflowsError> {
        self.start_in(Some(uow), input, options).await
    }

    /// Starts a run outside an explicit unit of work: inside the current
    /// one (a handler's) when there is one, at once otherwise.
    pub async fn spawn<W: Workflow>(&self, input: W) -> Result<Uuid, WorkflowsError> {
        self.spawn_with(input, StartOptions::default()).await
    }

    /// [`spawn`](Workflows::spawn) with options.
    pub async fn spawn_with<W: Workflow>(
        &self,
        input: W,
        options: StartOptions,
    ) -> Result<Uuid, WorkflowsError> {
        let current = match Uow::current() {
            Some(uow) if !uow.is_finished().await => Some(uow),
            _ => None,
        };
        self.start_in(current.as_ref(), input, options).await
    }

    async fn start_in<W: Workflow>(
        &self,
        uow: Option<&Uow>,
        input: W,
        options: StartOptions,
    ) -> Result<Uuid, WorkflowsError> {
        let engine = match &self.inner {
            Inner::Engine(engine) => engine,
            Inner::Fake(fake) => return fake.record_start(input, options),
        };
        let definition = self.definition::<W>()?;
        let input = serde_json::to_value(input).map_err(|error| {
            WorkflowsError::payload(
                &format!("the input of workflow `{}` cannot be stored", W::NAME),
                error,
            )
        })?;
        let (run, created) = engine.insert_run(uow, &definition, input, &options).await?;
        if created {
            engine.wake_after(uow, run.id, run.wake_at).await?;
        }
        Ok(run.id)
    }

    /// The run `id`.
    pub async fn run(&self, id: Uuid) -> Result<WorkflowRun, WorkflowsError> {
        let run = match &self.inner {
            Inner::Engine(engine) => engine.store.run(id).await?,
            Inner::Fake(fake) => fake.run(id),
        };
        run.ok_or(WorkflowsError::UnknownRun(id))
    }

    /// The run `id` with its steps.
    pub async fn details(&self, id: Uuid) -> Result<RunDetails, WorkflowsError> {
        let run = self.run(id).await?;
        let steps = match &self.inner {
            Inner::Engine(engine) => engine.store.steps(id).await?,
            Inner::Fake(_) => Vec::new(),
        };
        Ok(RunDetails { run, steps })
    }

    /// Runs passing `query`, newest first.
    pub async fn runs(&self, query: &RunQuery) -> Result<Vec<WorkflowRun>, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.store.runs(query).await,
            Inner::Fake(fake) => Ok(fake.runs(query)),
        }
    }

    /// One page of the runs passing `query` (its limit and offset are the
    /// page's).
    pub async fn page(
        &self,
        query: &RunQuery,
        page: PageQuery,
    ) -> Result<Page<WorkflowRun>, WorkflowsError> {
        let query = RunQuery {
            limit: page.per_page,
            offset: page.offset(),
            ..query.clone()
        };
        let (data, total) = match &self.inner {
            Inner::Engine(engine) => (
                engine.store.runs(&query).await?,
                engine.store.count_runs(&query).await?,
            ),
            Inner::Fake(fake) => {
                let all = fake.runs(&RunQuery {
                    limit: u64::MAX,
                    offset: 0,
                    ..query.clone()
                });
                let total = all.len() as u64;
                let skip = usize::try_from(query.offset).unwrap_or(usize::MAX);
                let take = usize::try_from(query.limit).unwrap_or(usize::MAX);
                (all.into_iter().skip(skip).take(take).collect(), total)
            }
        };
        Ok(Page::new(data, page, total))
    }

    /// Cancels an unfinished run: it never runs again, and an execution
    /// under way stops at its next step. 409 when it already finished.
    pub async fn cancel(&self, id: Uuid) -> Result<WorkflowRun, WorkflowsError> {
        if let Inner::Fake(fake) = &self.inner {
            return fake
                .cancel(id)
                .unwrap_or(Err(WorkflowsError::UnknownRun(id)));
        }
        let engine = self.engine()?;
        let run = self.run(id).await?;
        if !engine.store.cancel(None, id, engine.now()).await? {
            return Err(WorkflowsError::conflict(
                codes::WORKFLOW_RUN_FINISHED,
                format!("The workflow run has already finished ({}).", run.status),
            ));
        }
        tracing::info!(target: "navree::workflows", run_id = %id, "workflow run cancelled");
        self.run(id).await
    }

    /// Runs a failed run again from its failed step (completed steps are
    /// replayed, failed ones run again with fresh attempts). 409 unless it
    /// failed.
    pub async fn retry(&self, id: Uuid) -> Result<WorkflowRun, WorkflowsError> {
        let run = self.run(id).await?;
        let Inner::Engine(engine) = &self.inner else {
            return Err(WorkflowsError::conflict(
                codes::WORKFLOW_RUN_NOT_FAILED,
                format!("Only a failed workflow run can be retried (this one is {}).", run.status),
            ));
        };
        if !engine.store.retry(None, id, engine.now()).await? {
            return Err(WorkflowsError::conflict(
                codes::WORKFLOW_RUN_NOT_FAILED,
                format!("Only a failed workflow run can be retried (this one is {}).", run.status),
            ));
        }
        engine.wake(id, None).await?;
        self.run(id).await
    }

    /// Executes run `id` once, here: from where it stopped until it
    /// finishes or waits (what the `workflows.run` job does).
    pub async fn execute(&self, id: Uuid) -> Result<Execution, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.execute(id).await,
            Inner::Fake(_) => Ok(Execution::Skipped),
        }
    }

    /// Wakes (or queues) every run that is due: timers that ended, waits
    /// that timed out, executions whose worker died and starts whose job
    /// was lost. What the `workflows:tick` schedule runs every minute;
    /// returns how many runs it woke.
    pub async fn tick(&self) -> Result<usize, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.tick().await,
            Inner::Fake(_) => Ok(0),
        }
    }

    /// Delivers event `name` with its JSON payload to the waits it matches
    /// and the automations it triggers (what the listener of
    /// `m.workflow_event::<E>()` does for each emitted `E`); returns how
    /// many runs it woke or started.
    pub async fn deliver(&self, name: &str, payload: &Value) -> Result<usize, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.deliver(name, payload).await,
            Inner::Fake(_) => Ok(0),
        }
    }

    /// Deletes runs that finished before `before` (with their steps);
    /// returns how many.
    pub async fn prune(&self, before: DateTime<Utc>) -> Result<u64, WorkflowsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.store.prune(before).await,
            Inner::Fake(_) => Ok(0),
        }
    }

    /// The current time on the clock runs use (the app's `dyn Clock`).
    pub fn now(&self) -> DateTime<Utc> {
        match &self.inner {
            Inner::Engine(engine) => engine.now(),
            Inner::Fake(_) => Utc::now(),
        }
    }
}
