//! The engine behind [`Workflows`](crate::Workflows): starts runs, executes
//! them until they finish or suspend, wakes them, and delivers events to
//! their waits.

use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use navree_contracts::{Clock, Dispatcher, JobEnvelope, JobOptions};
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, BoxFuture, WeakAppContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::automation::{AutomationStore, Catalog, TemplateRegistry};
use crate::ctx::{Suspension, WorkflowCtx, delta, panic_message};
use crate::jobs::RunWorkflow;
use crate::store::{DueQuery, Lease, Release, WorkflowStore};
use crate::{
    EventMatch, RunStatus, StartOptions, WorkflowDefinition, WorkflowRegistry, WorkflowRun,
    WorkflowsError, WorkflowsOptions,
};

/// How long a `workflows.run` job may take (one execution, until the run
/// finishes or suspends).
pub(crate) const RUN_JOB_TIMEOUT_SECS: u64 = 3_600;

/// How late a timer may be before `workflows:tick` wakes it, when a job
/// dispatcher already queued its wake-up.
const TICK_GRACE: Duration = Duration::from_secs(60);

/// At most this many runs per tick.
pub(crate) const TICK_LIMIT: u64 = 500;

/// What [`Engine::execute`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
    /// The run was not ready (or another execution holds it).
    Skipped,
    /// The execution ended with the run in this status.
    Ran(RunStatus),
    /// The run was cancelled or taken over while it ran.
    Lost,
}

pub(crate) struct Engine {
    pub(crate) store: Arc<dyn WorkflowStore>,
    pub(crate) automations: Arc<dyn AutomationStore>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) options: WorkflowsOptions,
    pub(crate) registry: WorkflowRegistry,
    pub(crate) catalog: Catalog,
    pub(crate) templates: TemplateRegistry,
    /// Event names a code workflow may wait for.
    pub(crate) events: BTreeSet<String>,
    pub(crate) db: Option<Db>,
    app: WeakAppContext,
    warned_inline: AtomicBool,
}

impl Engine {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        app: &AppContext,
        store: Arc<dyn WorkflowStore>,
        automations: Arc<dyn AutomationStore>,
        clock: Arc<dyn Clock>,
        options: WorkflowsOptions,
        registry: WorkflowRegistry,
        catalog: Catalog,
        templates: TemplateRegistry,
        events: BTreeSet<String>,
        db: Option<Db>,
    ) -> Self {
        Self {
            store,
            automations,
            clock,
            options,
            registry,
            catalog,
            templates,
            events,
            db,
            app: app.downgrade(),
            warned_inline: AtomicBool::new(false),
        }
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    pub(crate) fn app(&self) -> Result<AppContext, WorkflowsError> {
        self.app
            .upgrade()
            .ok_or_else(|| WorkflowsError::Other("the app has shut down".to_owned()))
    }

    /// Whether a workflow may wait for event `name`.
    pub(crate) fn waits_for(&self, name: &str) -> bool {
        self.events.contains(name)
    }

    /// Stores a new run of `definition` (in `uow`, or at once) and returns
    /// it; the caller wakes it. An unfinished run with the same key is
    /// returned instead of a new one.
    pub(crate) async fn insert_run(
        &self,
        uow: Option<&Uow>,
        definition: &WorkflowDefinition,
        input: Value,
        options: &StartOptions,
    ) -> Result<(WorkflowRun, bool), WorkflowsError> {
        if let Some(key) = &options.key
            && let Some(existing) = self.store.unfinished_with_key(definition.name, key).await?
        {
            return Ok((existing, false));
        }
        let now = self.now();
        let wake_at = options.delay.map(|d| now + delta(d));
        let run = WorkflowRun {
            id: Uuid::now_v7(),
            workflow: definition.name.to_owned(),
            version: definition.version,
            status: if wake_at.is_some() {
                RunStatus::Sleeping
            } else {
                RunStatus::Pending
            },
            input,
            output: None,
            error: None,
            workspace_id: options.workspace_id,
            key: options.key.clone(),
            wake_at,
            created_at: now,
            updated_at: now,
            finished_at: None,
        };
        self.store.insert_run(uow, &run).await?;
        tracing::debug!(
            target: "navree::workflows",
            run_id = %run.id,
            workflow = %run.workflow,
            version = run.version,
            "workflow run started"
        );
        Ok((run, true))
    }

    /// Wakes `run` once `uow` commits (or now, without one).
    pub(crate) async fn wake_after(
        self: &Arc<Self>,
        uow: Option<&Uow>,
        run_id: Uuid,
        at: Option<DateTime<Utc>>,
    ) -> Result<(), WorkflowsError> {
        match uow {
            Some(uow) => {
                let engine = self.clone();
                uow.after_commit(move || {
                    Box::pin(async move {
                        if let Err(error) = engine.wake(run_id, at).await {
                            tracing::error!(
                                target: "navree::workflows",
                                %run_id,
                                %error,
                                "cannot wake a workflow run; workflows:tick will"
                            );
                        }
                    })
                });
                Ok(())
            }
            None => self.wake(run_id, at).await,
        }
    }

    /// Gets `run_id` executed at `at` (now when `None` or past): with a job
    /// dispatcher, a `workflows.run` job (delayed until `at`); without one,
    /// an execution right here for runs due now, and nothing for later ones
    /// (`workflows:tick` wakes them).
    pub(crate) fn wake(
        self: &Arc<Self>,
        run_id: Uuid,
        at: Option<DateTime<Utc>>,
    ) -> BoxFuture<'_, Result<(), WorkflowsError>> {
        Box::pin(async move {
            let app = self.app()?;
            let now = self.now();
            let delay = at
                .filter(|at| *at > now)
                .map(|at| (at - now).to_std().unwrap_or_default());
            if let Some(dispatcher) = app.try_get::<dyn Dispatcher>().await {
                let envelope = self.run_envelope(run_id, delay)?;
                dispatcher
                    .dispatch_envelope(envelope)
                    .await
                    .map_err(|error| {
                        WorkflowsError::Other(format!(
                            "cannot queue workflow run {run_id}: {error}"
                        ))
                    })?;
                return Ok(());
            }
            if delay.is_some() {
                return Ok(());
            }
            if !self.warned_inline.swap(true, Ordering::Relaxed) {
                tracing::debug!(
                    target: "navree::workflows",
                    "no job dispatcher: workflow runs execute inline, and timers wait for \
                     workflows:tick"
                );
            }
            self.execute(run_id).await.map(|_| ())
        })
    }

    /// The `workflows.run` job for `run_id`.
    fn run_envelope(
        &self,
        run_id: Uuid,
        delay: Option<Duration>,
    ) -> Result<JobEnvelope, WorkflowsError> {
        let payload = serde_json::to_value(RunWorkflow { run_id })
            .map_err(|error| WorkflowsError::payload("the run job cannot be queued", error))?;
        Ok(JobEnvelope {
            job_type: <RunWorkflow as navree_contracts::Job>::NAME.to_owned(),
            payload,
            options: JobOptions {
                queue: self.options.queue.clone(),
                timeout_secs: RUN_JOB_TIMEOUT_SECS,
                ..JobOptions::default()
            },
            delay_secs: delay.map(|d| d.as_secs() + u64::from(d.subsec_nanos() > 0)),
            unique_key: None,
        })
    }

    /// Executes run `run_id` once: claims it, replays it up to where it
    /// stopped, runs it on until it finishes or must wait, stores the
    /// outcome and wakes it again when needed.
    pub(crate) fn execute(
        self: &Arc<Self>,
        run_id: Uuid,
    ) -> BoxFuture<'_, Result<Execution, WorkflowsError>> {
        Box::pin(async move {
            let app = self.app()?;
            let now = self.now();
            let lease = Lease {
                run_id,
                token: Uuid::new_v4(),
                now,
                until: now + delta(self.options.lease()),
            };
            let Some(run) = self.store.claim(lease).await? else {
                return Ok(Execution::Skipped);
            };
            let Some(definition) = self.registry.for_run(&run.workflow, run.version).cloned()
            else {
                return self.unresolved(&run, lease).await;
            };
            let steps = self.store.steps(run_id).await?;
            let ctx = WorkflowCtx::new(run.clone(), lease, steps, app, self.clone());
            let future = (definition.run)(run.input.clone(), ctx.clone());
            let outcome = tokio::select! {
                biased;
                () = ctx.suspended() => None,
                result = AssertUnwindSafe(future).catch_unwind() => Some(result),
            };
            let release = match outcome {
                None => match ctx.suspension() {
                    Some(Suspension::Sleep(at)) => Release::Sleeping(at),
                    Some(Suspension::Wait(at)) => Release::Waiting(at),
                    Some(Suspension::Lost) => return Ok(Execution::Lost),
                    Some(Suspension::Abort(error)) => {
                        tracing::error!(
                            target: "navree::workflows",
                            %run_id,
                            %error,
                            "workflow storage failed; the run resumes once its lease runs out"
                        );
                        return Err(WorkflowsError::Other(error));
                    }
                    None => Release::Pending,
                },
                Some(Ok(Ok(output))) => Release::Completed(output),
                Some(Ok(Err(error))) => Release::Failed(error.to_string()),
                Some(Err(panic)) => Release::Failed(format!(
                    "the workflow panicked: {}",
                    panic_message(panic.as_ref())
                )),
            };
            let mut lease = ctx.lease();
            lease.now = self.now();
            let Some(status) = self.store.release(&lease, &release).await? else {
                return Ok(Execution::Lost);
            };
            self.after_release(&run, status, &release).await;
            Ok(Execution::Ran(status))
        })
    }

    /// Logs the outcome and wakes the run when it must go on.
    async fn after_release(self: &Arc<Self>, run: &WorkflowRun, status: RunStatus, release: &Release) {
        let next = match (status, release) {
            (RunStatus::Pending, _) => Some(None),
            (RunStatus::Sleeping, Release::Sleeping(at)) => Some(Some(*at)),
            (RunStatus::Waiting, Release::Waiting(Some(at))) => Some(Some(*at)),
            _ => None,
        };
        match status {
            RunStatus::Completed => tracing::info!(
                target: "navree::workflows",
                run_id = %run.id,
                workflow = %run.workflow,
                "workflow run completed"
            ),
            RunStatus::Failed => tracing::warn!(
                target: "navree::workflows",
                run_id = %run.id,
                workflow = %run.workflow,
                error = match release {
                    Release::Failed(error) => error.as_str(),
                    _ => "",
                },
                "workflow run failed"
            ),
            _ => {}
        }
        if let Some(at) = next
            && let Err(error) = self.wake(run.id, at).await
        {
            tracing::error!(
                target: "navree::workflows",
                run_id = %run.id,
                %error,
                "cannot wake a workflow run; workflows:tick will"
            );
        }
    }

    /// A claimed run no registered implementation resumes: a newer version
    /// (or an unknown workflow) waits for a deploy that has it; an older
    /// one fails.
    async fn unresolved(
        self: &Arc<Self>,
        run: &WorkflowRun,
        mut lease: Lease,
    ) -> Result<Execution, WorkflowsError> {
        lease.now = self.now();
        let oldest = self
            .registry
            .iter()
            .filter(|d| d.name == run.workflow)
            .map(|d| d.min_version)
            .min();
        let release = match oldest {
            Some(oldest) if run.version < oldest => Release::Failed(format!(
                "no registered implementation of workflow `{}` resumes version {} (the oldest \
                 resumes version {oldest}): register the old implementation again, or cancel \
                 the run",
                run.workflow, run.version
            )),
            _ => {
                tracing::warn!(
                    target: "navree::workflows",
                    run_id = %run.id,
                    workflow = %run.workflow,
                    version = run.version,
                    "no implementation of this workflow version is registered here; it is \
                     tried again after the lease"
                );
                Release::Sleeping(lease.now + delta(self.options.lease()))
            }
        };
        let status = self.store.release(&lease, &release).await?;
        Ok(status.map_or(Execution::Lost, Execution::Ran))
    }

    /// Delivers event `name` (its JSON) to the open waits it matches and
    /// to the automations it triggers; returns how many runs it woke or
    /// started.
    pub(crate) async fn deliver(
        self: &Arc<Self>,
        name: &str,
        payload: &Value,
    ) -> Result<usize, WorkflowsError> {
        let mut woken = 0;
        let now = self.now();
        for wait in self.store.open_waits(name).await? {
            let filter: EventMatch = wait
                .filter
                .clone()
                .and_then(|filter| serde_json::from_value(filter).ok())
                .unwrap_or_default();
            if !filter.matches(payload) {
                continue;
            }
            if let Some(trigger) = &filter.trigger
                && !self.trigger_accepts(trigger, payload)
            {
                continue;
            }
            let output = json!({ "event": payload });
            if self
                .store
                .resolve_wait(None, wait.run_id, &wait.key, &output, now)
                .await?
            {
                woken += 1;
                if let Err(error) = self.wake(wait.run_id, None).await {
                    tracing::error!(
                        target: "navree::workflows",
                        run_id = %wait.run_id,
                        %error,
                        "cannot wake a workflow run; workflows:tick will"
                    );
                }
            }
        }
        if self.options.automations {
            woken += crate::automation::enroll(self, name, payload).await?;
        }
        Ok(woken)
    }

    /// Whether the trigger of an automation wait accepts `payload`.
    fn trigger_accepts(&self, trigger: &crate::matcher::TriggerRef, payload: &Value) -> bool {
        let Some(entry) = self.catalog.trigger(&trigger.kind) else {
            return false;
        };
        if let Some(subject) = &trigger.subject
            && entry.subject_of(payload).as_ref() != Some(subject)
        {
            return false;
        }
        (entry.matches)(&trigger.config, payload).unwrap_or(false)
    }

    /// Wakes (or queues) every run that is due now; returns how many.
    pub(crate) async fn tick(self: &Arc<Self>) -> Result<usize, WorkflowsError> {
        let now = self.now();
        let dispatching = self.app()?.try_get::<dyn Dispatcher>().await.is_some();
        let wake_before = if dispatching {
            now - delta(TICK_GRACE)
        } else {
            now
        };
        let due = self
            .store
            .due(DueQuery {
                now,
                wake_before,
                stale_before: now - delta(self.options.lease()),
                limit: TICK_LIMIT,
            })
            .await?;
        let count = due.len();
        for run_id in due {
            if let Err(error) = self.wake(run_id, None).await {
                tracing::error!(
                    target: "navree::workflows",
                    %run_id,
                    %error,
                    "cannot wake a due workflow run"
                );
            }
        }
        Ok(count)
    }
}
