//! [`WorkflowCtx`]: what a running workflow uses to take durable steps.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use futures_util::FutureExt;
use navree_contracts::Backoff;
use navree_events::Event;
use navree_kernel::{AppContext, BoxError};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::engine::Engine;
use crate::store::Lease;
use crate::{EventMatch, StepKind, StepStatus, WorkflowRun, WorkflowStep, WorkflowsError};

/// The longest step name.
const MAX_STEP_NAME_LEN: usize = 100;

/// How a step is retried (see [`WorkflowCtx::step_with`]).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct StepOptions {
    /// Attempts before the step fails for good (default 3).
    pub max_attempts: u32,
    /// The wait before each next attempt (default exponential from 10 s,
    /// capped at one hour). The run sleeps in between: no worker is held.
    pub backoff: Backoff,
}

impl Default for StepOptions {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: Backoff::Exponential {
                base_secs: 10,
                max_secs: 3_600,
            },
        }
    }
}

impl StepOptions {
    /// `attempts` attempts with the default backoff.
    pub fn attempts(attempts: u32) -> Self {
        Self {
            max_attempts: attempts.max(1),
            ..Self::default()
        }
    }

    /// The same options with `backoff`.
    pub fn backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }
}

/// Why an execution stopped before the workflow returned.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Suspension {
    /// A timer (or a step's next attempt) ends at this time.
    Sleep(DateTime<Utc>),
    /// An event wait, timing out at this time (`None`: never).
    Wait(Option<DateTime<Utc>>),
    /// The lease is gone (the run was cancelled or taken over): stop
    /// without writing.
    Lost,
    /// The store failed: stop, and let the lease run out.
    Abort(String),
}

impl Suspension {
    /// Two suspensions of one execution (branches of a `join!`): the lease
    /// problems win, else the run waits until the earliest wake-up.
    fn merge(self, other: Suspension) -> Suspension {
        use Suspension::{Abort, Lost, Sleep, Wait};
        let earliest = |a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match (self, other) {
            (Lost, _) | (_, Lost) => Lost,
            (Abort(e), _) | (_, Abort(e)) => Abort(e),
            (Sleep(a), Sleep(b)) => Sleep(a.min(b)),
            (Sleep(a), Wait(b)) | (Wait(b), Sleep(a)) => Wait(earliest(Some(a), b)),
            (Wait(a), Wait(b)) => Wait(earliest(a, b)),
        }
    }
}

/// The handle a workflow runs with: durable [`step`](WorkflowCtx::step)s,
/// [`sleep`](WorkflowCtx::sleep)s and event
/// [`wait_for`](WorkflowCtx::wait_for)s, the recorded
/// [`now`](WorkflowCtx::now), and the app.
///
/// Every call is keyed by its name and how many times that name was used
/// before in this run (`send-reminder`, `send-reminder#2`, ...). On a
/// replay, a call whose key has a stored result returns it; the first call
/// without one runs. When the workflow must wait (a timer, an event, a
/// step's next attempt), the call does not return: the execution stops,
/// the run is stored as sleeping or waiting, and a later execution replays
/// it up to the same point and goes on.
///
/// Clone it freely (into `join!` branches, for example); clones share one
/// execution.
#[derive(Clone)]
pub struct WorkflowCtx {
    inner: Arc<Inner>,
}

struct Inner {
    run: WorkflowRun,
    lease: Mutex<Lease>,
    app: AppContext,
    engine: Arc<Engine>,
    state: Mutex<State>,
    suspended: Notify,
}

#[derive(Default)]
struct State {
    steps: HashMap<String, WorkflowStep>,
    counters: HashMap<String, u32>,
    next_position: u32,
    suspension: Option<Suspension>,
}

impl WorkflowCtx {
    pub(crate) fn new(
        run: WorkflowRun,
        lease: Lease,
        steps: Vec<WorkflowStep>,
        app: AppContext,
        engine: Arc<Engine>,
    ) -> Self {
        let next_position = steps.iter().map(|s| s.position).max().unwrap_or(0) + 1;
        let steps = steps.into_iter().map(|s| (s.key.clone(), s)).collect();
        Self {
            inner: Arc::new(Inner {
                run,
                lease: Mutex::new(lease),
                app,
                engine,
                state: Mutex::new(State {
                    steps,
                    next_position,
                    ..State::default()
                }),
                suspended: Notify::new(),
            }),
        }
    }

    /// The run's id.
    pub fn run_id(&self) -> Uuid {
        self.inner.run.id
    }

    /// The workflow's name.
    pub fn workflow(&self) -> &str {
        &self.inner.run.workflow
    }

    /// The version the run started on: branch on it in an implementation
    /// that resumes older runs (`MIN_VERSION`).
    pub fn version(&self) -> u32 {
        self.inner.run.version
    }

    /// The workspace the run belongs to, if any.
    pub fn workspace_id(&self) -> Option<Uuid> {
        self.inner.run.workspace_id
    }

    /// The app, to resolve services (`ctx.app().get::<Mailer>().await?`).
    pub fn app(&self) -> &AppContext {
        &self.inner.app
    }

    /// Runs `f` once for this run and stores its result: replays return
    /// the stored value without calling `f`. An error is retried later with
    /// the default [`StepOptions`] (3 attempts, the run sleeping in
    /// between); after the last attempt the step returns
    /// [`WorkflowsError::StepFailed`], on this execution and every replay.
    ///
    /// ```ignore
    /// let account = ctx
    ///     .step("create-account", || async move { billing.create_account(user_id).await })
    ///     .await?;
    /// ```
    ///
    /// The result is stored as JSON, so `T` must serialize and read back.
    /// A step that dies with its process (or panics) runs again: keep its
    /// effects idempotent, or use an idempotency key such as
    /// `format!("{}:{}", ctx.run_id(), "charge")`.
    pub async fn step<T, E, F, Fut>(&self, name: &str, f: F) -> Result<T, WorkflowsError>
    where
        T: Serialize + DeserializeOwned + Send,
        E: Into<BoxError>,
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        self.step_with(name, StepOptions::default(), f).await
    }

    /// [`step`](WorkflowCtx::step) with explicit retry options.
    pub async fn step_with<T, E, F, Fut>(
        &self,
        name: &str,
        options: StepOptions,
        f: F,
    ) -> Result<T, WorkflowsError>
    where
        T: Serialize + DeserializeOwned + Send,
        E: Into<BoxError>,
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        check_step_name(name)?;
        let key = self.next_key(name);
        self.run_step(key, options, f).await
    }

    /// The step behind [`step_with`](WorkflowCtx::step_with), under an
    /// explicit key (automation nodes use their ids).
    pub(crate) async fn run_step<T, E, F, Fut>(
        &self,
        key: String,
        options: StepOptions,
        f: F,
    ) -> Result<T, WorkflowsError>
    where
        T: Serialize + DeserializeOwned + Send,
        E: Into<BoxError>,
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        let attempts = match self.recorded(&key) {
            Some(step) => match step.status {
                StepStatus::Completed => {
                    return decode(&key, step.output.unwrap_or(Value::Null));
                }
                StepStatus::Failed => {
                    return Err(WorkflowsError::StepFailed {
                        step: key,
                        attempts: step.attempts,
                        message: step.error.unwrap_or_default(),
                    });
                }
                StepStatus::Retrying => {
                    if let Some(at) = step.wake_at
                        && at > self.clock()
                    {
                        return self.suspend(Suspension::Sleep(at)).await;
                    }
                    step.attempts
                }
                StepStatus::Waiting => step.attempts,
            },
            None => 0,
        };
        let attempt = attempts.saturating_add(1);
        let outcome = AssertUnwindSafe(f()).catch_unwind().await;
        let error = match outcome {
            Ok(Ok(value)) => {
                let output = serde_json::to_value(&value).map_err(|error| {
                    WorkflowsError::payload(
                        &format!("the result of step `{key}` cannot be stored"),
                        error,
                    )
                })?;
                let mut step = self.new_step(&key, StepKind::Step, StepStatus::Completed);
                step.output = Some(output);
                step.attempts = attempt;
                self.record(step).await;
                return Ok(value);
            }
            Ok(Err(error)) => error.into().to_string(),
            Err(panic) => format!("the step panicked: {}", panic_message(panic.as_ref())),
        };
        let mut step = self.new_step(&key, StepKind::Step, StepStatus::Failed);
        step.attempts = attempt;
        step.error = Some(error.clone());
        if attempt < options.max_attempts {
            let at = self.clock() + delta(options.backoff.delay_for(attempt));
            step.status = StepStatus::Retrying;
            step.wake_at = Some(at);
            self.record(step).await;
            tracing::warn!(
                target: "navree::workflows",
                run_id = %self.run_id(),
                step = %key,
                attempt,
                %error,
                "workflow step failed; it runs again later"
            );
            return self.suspend(Suspension::Sleep(at)).await;
        }
        self.record(step).await;
        Err(WorkflowsError::StepFailed {
            step: key,
            attempts: attempt,
            message: error,
        })
    }

    /// Waits `duration` (measured on the app's clock), durably: the run
    /// sleeps without holding a worker and resumes after it.
    pub async fn sleep(&self, duration: Duration) {
        let key = self.next_key("$sleep");
        self.sleep_keyed(key, duration).await;
    }

    /// Waits until `at` (on the app's clock), durably.
    pub async fn sleep_until(&self, at: DateTime<Utc>) {
        let key = self.next_key("$sleep");
        self.timer(key, at).await;
    }

    /// [`sleep`](WorkflowCtx::sleep) under an explicit key.
    pub(crate) async fn sleep_keyed(&self, key: String, duration: Duration) {
        let at = match self.recorded(&key) {
            Some(step) => step.wake_at.unwrap_or_else(|| self.clock()),
            None => self.clock() + delta(duration),
        };
        self.timer(key, at).await;
    }

    async fn timer(&self, key: String, at: DateTime<Utc>) {
        let recorded = self.recorded(&key);
        if recorded
            .as_ref()
            .is_some_and(|s| s.status == StepStatus::Completed)
        {
            return;
        }
        let at = recorded.and_then(|s| s.wake_at).unwrap_or(at);
        if at <= self.clock() {
            let mut step = self.new_step(&key, StepKind::Sleep, StepStatus::Completed);
            step.wake_at = Some(at);
            self.record(step).await;
            return;
        }
        if self.recorded(&key).is_none() {
            let mut step = self.new_step(&key, StepKind::Sleep, StepStatus::Waiting);
            step.wake_at = Some(at);
            self.record(step).await;
        }
        self.suspend(Suspension::Sleep(at)).await
    }

    /// Waits for an event of type `E` that passes `filter`, durably, for
    /// at most `timeout` (`None`: no limit). Returns the event, or `None`
    /// once the timeout passed.
    ///
    /// ```ignore
    /// let done = ctx
    ///     .wait_for::<CourseCompleted>(
    ///         EventMatch::field("user_id", input.user_id),
    ///         Duration::from_secs(14 * 86_400),
    ///     )
    ///     .await?;
    /// ```
    ///
    /// Only events emitted after the wait began count. `E` must be
    /// registered with `m.workflow_event::<E>()`; otherwise this returns
    /// [`WorkflowsError::UnregisteredEvent`].
    pub async fn wait_for<E: Event>(
        &self,
        filter: EventMatch,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<Option<E>, WorkflowsError> {
        if !self.inner.engine.waits_for(E::NAME) {
            return Err(WorkflowsError::unregistered_event(
                E::NAME,
                std::any::type_name::<E>(),
            ));
        }
        let key = self.next_key(&format!("$wait:{}", E::NAME));
        match self
            .wait_keyed(key, E::NAME, filter, timeout.into())
            .await?
        {
            Some(payload) => serde_json::from_value(payload).map(Some).map_err(|error| {
                WorkflowsError::payload(&format!("the event `{}` cannot be read", E::NAME), error)
            }),
            None => Ok(None),
        }
    }

    /// [`wait_for`](WorkflowCtx::wait_for) by event name, returning the
    /// event's JSON.
    pub async fn wait_for_event(
        &self,
        event: &str,
        filter: EventMatch,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<Option<Value>, WorkflowsError> {
        if !self.inner.engine.waits_for(event) {
            return Err(WorkflowsError::UnregisteredEvent {
                name: event.to_owned(),
                type_name: "(by name)".to_owned(),
                short: "E".to_owned(),
            });
        }
        let key = self.next_key(&format!("$wait:{event}"));
        self.wait_keyed(key, event, filter, timeout.into()).await
    }

    /// The wait behind [`wait_for_event`](WorkflowCtx::wait_for_event),
    /// under an explicit key.
    pub(crate) async fn wait_keyed(
        &self,
        key: String,
        event: &str,
        filter: EventMatch,
        timeout: Option<Duration>,
    ) -> Result<Option<Value>, WorkflowsError> {
        let deadline = match self.recorded(&key) {
            Some(step) if step.status == StepStatus::Completed => {
                // `{"event": payload}` when an event came; null on timeout.
                return Ok(step
                    .output
                    .and_then(|mut output| output.get_mut("event").map(Value::take)));
            }
            Some(step) => step.wake_at,
            None => {
                let deadline = timeout.map(|t| self.clock() + delta(t));
                let mut step = self.new_step(&key, StepKind::Wait, StepStatus::Waiting);
                step.wake_at = deadline;
                step.event = Some(event.to_owned());
                step.filter = Some(serde_json::to_value(&filter).map_err(|error| {
                    WorkflowsError::payload("the wait's filter cannot be stored", error)
                })?);
                self.record(step).await;
                deadline
            }
        };
        if let Some(at) = deadline
            && at <= self.clock()
        {
            let mut step = self
                .recorded(&key)
                .unwrap_or_else(|| self.new_step(&key, StepKind::Wait, StepStatus::Waiting));
            step.status = StepStatus::Completed;
            step.output = Some(Value::Null);
            step.updated_at = self.clock();
            self.record(step).await;
            return Ok(None);
        }
        self.suspend(Suspension::Wait(deadline)).await
    }

    /// The current time on the app's clock, recorded the first time: a
    /// replay sees the same instant. Use it instead of `Utc::now()` in
    /// workflow code.
    pub async fn now(&self) -> DateTime<Utc> {
        let key = self.next_key("$now");
        if let Some(step) = self.recorded(&key)
            && let Some(Ok(at)) = step.output.map(serde_json::from_value::<DateTime<Utc>>)
        {
            return at;
        }
        let now = self.clock();
        let mut step = self.new_step(&key, StepKind::Value, StepStatus::Completed);
        step.output = Some(json!(now));
        self.record(step).await;
        now
    }

    /// The engine running this execution.
    pub(crate) fn engine(&self) -> Arc<Engine> {
        self.inner.engine.clone()
    }

    /// The execution's lease, as last renewed.
    pub(crate) fn lease(&self) -> Lease {
        *self
            .inner
            .lease
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The suspension, once the execution stopped.
    pub(crate) fn suspension(&self) -> Option<Suspension> {
        self.state().suspension.clone()
    }

    /// Resolves once the execution has stopped (see
    /// [`suspend`](Self::suspend)).
    pub(crate) async fn suspended(&self) {
        self.inner.suspended.notified().await;
    }

    /// Stops the execution: notes why, wakes the engine and never returns.
    async fn suspend<T>(&self, suspension: Suspension) -> T {
        {
            let mut state = self.state();
            let merged = match state.suspension.take() {
                Some(previous) => previous.merge(suspension),
                None => suspension,
            };
            state.suspension = Some(merged);
        }
        self.inner.suspended.notify_one();
        std::future::pending().await
    }

    /// The key of the next call named `name`.
    fn next_key(&self, name: &str) -> String {
        let mut state = self.state();
        let count = state.counters.entry(name.to_owned()).or_insert(0);
        *count += 1;
        if *count == 1 {
            name.to_owned()
        } else {
            format!("{name}#{count}")
        }
    }

    fn recorded(&self, key: &str) -> Option<WorkflowStep> {
        self.state().steps.get(key).cloned()
    }

    /// A step record for `key`, keeping the position and creation time of
    /// a stored one.
    fn new_step(&self, key: &str, kind: StepKind, status: StepStatus) -> WorkflowStep {
        let now = self.clock();
        let mut state = self.state();
        let (position, created_at) = match state.steps.get(key) {
            Some(existing) => (existing.position, existing.created_at),
            None => {
                let position = state.next_position;
                state.next_position += 1;
                (position, now)
            }
        };
        WorkflowStep {
            run_id: self.run_id(),
            key: key.to_owned(),
            position,
            kind,
            status,
            output: None,
            error: None,
            attempts: 0,
            wake_at: None,
            event: None,
            filter: None,
            created_at,
            updated_at: now,
        }
    }

    /// Stores `step` (renewing the lease); stops the execution when the
    /// lease is lost or the store fails.
    async fn record(&self, step: WorkflowStep) {
        let lease = {
            let mut lease = self
                .inner
                .lease
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let now = self.clock();
            lease.now = now;
            lease.until = now + delta(self.inner.engine.options.lease());
            *lease
        };
        match self.inner.engine.store.save_step(&lease, &step).await {
            Ok(true) => {
                self.state().steps.insert(step.key.clone(), step);
            }
            Ok(false) => self.suspend(Suspension::Lost).await,
            Err(error) => self.suspend(Suspension::Abort(error.to_string())).await,
        }
    }

    fn clock(&self) -> DateTime<Utc> {
        self.inner.engine.clock.now()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl std::fmt::Debug for WorkflowCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowCtx")
            .field("run_id", &self.inner.run.id)
            .field("workflow", &self.inner.run.workflow)
            .field("version", &self.inner.run.version)
            .finish_non_exhaustive()
    }
}

/// A step name: 1 to 100 characters, no `#`, not starting with `$`.
fn check_step_name(name: &str) -> Result<(), WorkflowsError> {
    if name.is_empty()
        || name.len() > MAX_STEP_NAME_LEN
        || name.starts_with('$')
        || name.contains('#')
    {
        return Err(WorkflowsError::InvalidStepName(name.to_owned()));
    }
    Ok(())
}

fn decode<T: DeserializeOwned>(key: &str, output: Value) -> Result<T, WorkflowsError> {
    serde_json::from_value(output).map_err(|error| {
        WorkflowsError::payload(
            &format!("the stored result of step `{key}` cannot be read"),
            error,
        )
    })
}

/// `duration` as a chrono delta (saturating).
pub(crate) fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}

/// The message of a caught panic.
pub(crate) fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn suspensions_merge_to_the_earliest_wake() {
        use Suspension::{Abort, Lost, Sleep, Wait};
        assert_eq!(Sleep(at(5)).merge(Sleep(at(3))), Sleep(at(3)));
        assert_eq!(Sleep(at(5)).merge(Wait(None)), Wait(Some(at(5))));
        assert_eq!(Wait(Some(at(9))).merge(Sleep(at(5))), Wait(Some(at(5))));
        assert_eq!(Wait(None).merge(Wait(None)), Wait(None));
        assert_eq!(Sleep(at(1)).merge(Lost), Lost);
        assert_eq!(Abort("x".into()).merge(Sleep(at(1))), Abort("x".into()));
    }

    #[test]
    fn step_names_are_checked() {
        assert!(check_step_name("send-welcome").is_ok());
        for bad in ["", "$sleep", "a#2", &"x".repeat(101)] {
            assert!(check_step_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn step_options_build() {
        let options = StepOptions::attempts(0).backoff(Backoff::Fixed { secs: 5 });
        assert_eq!(options.max_attempts, 1);
        assert_eq!(options.backoff, Backoff::Fixed { secs: 5 });
        assert_eq!(StepOptions::default().max_attempts, 3);
    }
}
