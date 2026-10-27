//! [`MemoryWorkflowStore`]: runs in process memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use navree_db::Uow;
use navree_kernel::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

use super::{Lease, Release, StoreResult, WorkflowStore};
use crate::{RunQuery, RunStatus, StepKind, StepStatus, WorkflowRun, WorkflowStep};

/// Runs and steps in process memory (`workflows.store = "memory"`): lost on
/// restart, for tests and examples. Writes given a unit of work apply once
/// it commits.
#[derive(Clone, Default)]
pub struct MemoryWorkflowStore {
    state: Arc<Mutex<HashMap<Uuid, Entry>>>,
}

/// One run with its execution state and steps.
#[derive(Clone)]
struct Entry {
    run: WorkflowRun,
    /// The execution holding it: its token and when the hold runs out.
    lease: Option<(Uuid, DateTime<Utc>)>,
    /// An event resolved a wait while the run was running.
    wake_requested: bool,
    /// In the order they were reached.
    steps: Vec<WorkflowStep>,
}

impl MemoryWorkflowStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `apply` now, or once `uow` commits.
    fn write(&self, uow: Option<&Uow>, apply: impl FnOnce(&MemoryWorkflowStore) + Send + 'static) {
        match uow {
            Some(uow) => {
                let store = self.clone();
                uow.after_commit(move || {
                    Box::pin(async move {
                        apply(&store);
                    })
                });
            }
            None => apply(self),
        }
    }

    fn holds(entry: &Entry, lease: &Lease) -> bool {
        entry.run.status == RunStatus::Running
            && entry.lease.is_some_and(|(token, _)| token == lease.token)
    }
}

impl std::fmt::Debug for MemoryWorkflowStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryWorkflowStore")
            .field("runs", &self.lock().len())
            .finish()
    }
}

impl WorkflowStore for MemoryWorkflowStore {
    fn insert_run<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        run: &'a WorkflowRun,
    ) -> BoxFuture<'a, StoreResult<()>> {
        let run = run.clone();
        self.write(uow, move |store| {
            store.lock().insert(
                run.id,
                Entry {
                    run,
                    lease: None,
                    wake_requested: false,
                    steps: Vec::new(),
                },
            );
        });
        Box::pin(async { Ok(()) })
    }

    fn run(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<WorkflowRun>>> {
        let run = self.lock().get(&id).map(|entry| entry.run.clone());
        Box::pin(async move { Ok(run) })
    }

    fn unfinished_with_key<'a>(
        &'a self,
        workflow: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, StoreResult<Option<WorkflowRun>>> {
        let run = self
            .lock()
            .values()
            .find(|entry| {
                entry.run.workflow == workflow
                    && entry.run.key.as_deref() == Some(key)
                    && !entry.run.status.is_finished()
            })
            .map(|entry| entry.run.clone());
        Box::pin(async move { Ok(run) })
    }

    fn runs<'a>(&'a self, query: &'a RunQuery) -> BoxFuture<'a, StoreResult<Vec<WorkflowRun>>> {
        let mut runs: Vec<WorkflowRun> = self
            .lock()
            .values()
            .filter(|entry| query.matches(&entry.run))
            .map(|entry| entry.run.clone())
            .collect();
        // UUID v7 ids sort by creation time.
        runs.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        let offset = usize::try_from(query.offset).unwrap_or(usize::MAX);
        let limit = usize::try_from(query.effective_limit()).unwrap_or(usize::MAX);
        let runs = runs.into_iter().skip(offset).take(limit).collect();
        Box::pin(async move { Ok(runs) })
    }

    fn steps(&self, run_id: Uuid) -> BoxFuture<'_, StoreResult<Vec<WorkflowStep>>> {
        let steps = self
            .lock()
            .get(&run_id)
            .map(|entry| entry.steps.clone())
            .unwrap_or_default();
        Box::pin(async move { Ok(steps) })
    }

    fn claim(&self, lease: Lease) -> BoxFuture<'_, StoreResult<Option<WorkflowRun>>> {
        let mut state = self.lock();
        let claimed = state.get_mut(&lease.run_id).and_then(|entry| {
            let run = &entry.run;
            let ready = match run.status {
                RunStatus::Pending => true,
                RunStatus::Sleeping | RunStatus::Waiting => {
                    run.wake_at.is_some_and(|at| at <= lease.now)
                }
                RunStatus::Running => entry.lease.is_none_or(|(_, until)| until < lease.now),
                RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled => false,
            };
            if !ready {
                return None;
            }
            let before = entry.run.clone();
            entry.run.status = RunStatus::Running;
            entry.run.updated_at = lease.now;
            entry.lease = Some((lease.token, lease.until));
            entry.wake_requested = false;
            Some(before)
        });
        drop(state);
        Box::pin(async move { Ok(claimed) })
    }

    fn save_step<'a>(
        &'a self,
        lease: &'a Lease,
        step: &'a WorkflowStep,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        let mut state = self.lock();
        let saved = match state.get_mut(&lease.run_id) {
            Some(entry) if Self::holds(entry, lease) => {
                entry.lease = Some((lease.token, lease.until));
                match entry.steps.iter_mut().find(|s| s.key == step.key) {
                    Some(existing) => {
                        let position = existing.position;
                        let created_at = existing.created_at;
                        *existing = step.clone();
                        existing.position = position;
                        existing.created_at = created_at;
                    }
                    None => entry.steps.push(step.clone()),
                }
                true
            }
            _ => false,
        };
        drop(state);
        Box::pin(async move { Ok(saved) })
    }

    fn release<'a>(
        &'a self,
        lease: &'a Lease,
        outcome: &'a Release,
    ) -> BoxFuture<'a, StoreResult<Option<RunStatus>>> {
        let mut state = self.lock();
        let status = match state.get_mut(&lease.run_id) {
            Some(entry) if Self::holds(entry, lease) => {
                let run = &mut entry.run;
                let woken = entry.wake_requested;
                run.updated_at = lease.now;
                run.wake_at = None;
                run.status = match outcome {
                    Release::Completed(output) => {
                        run.output = Some(output.clone());
                        run.error = None;
                        run.finished_at = Some(lease.now);
                        RunStatus::Completed
                    }
                    Release::Failed(error) => {
                        run.error = Some(error.clone());
                        run.finished_at = Some(lease.now);
                        RunStatus::Failed
                    }
                    Release::Pending => RunStatus::Pending,
                    Release::Sleeping(_) | Release::Waiting(_) if woken => RunStatus::Pending,
                    Release::Sleeping(at) => {
                        run.wake_at = Some(*at);
                        RunStatus::Sleeping
                    }
                    Release::Waiting(at) => {
                        run.wake_at = *at;
                        RunStatus::Waiting
                    }
                };
                entry.lease = None;
                entry.wake_requested = false;
                Some(entry.run.status)
            }
            _ => None,
        };
        drop(state);
        Box::pin(async move { Ok(status) })
    }

    fn due(
        &self,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        limit: u64,
    ) -> BoxFuture<'_, StoreResult<Vec<Uuid>>> {
        let mut due: Vec<(DateTime<Utc>, Uuid)> = self
            .lock()
            .values()
            .filter_map(|entry| {
                let run = &entry.run;
                match run.status {
                    RunStatus::Sleeping | RunStatus::Waiting => {
                        run.wake_at.filter(|at| *at <= now).map(|at| (at, run.id))
                    }
                    RunStatus::Running => entry
                        .lease
                        .filter(|(_, until)| *until < now)
                        .map(|(_, until)| (until, run.id)),
                    RunStatus::Pending => {
                        (run.updated_at <= stale_before).then_some((run.updated_at, run.id))
                    }
                    _ => None,
                }
            })
            .collect();
        due.sort();
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let ids = due.into_iter().take(limit).map(|(_, id)| id).collect();
        Box::pin(async move { Ok(ids) })
    }

    fn open_waits<'a>(&'a self, event: &'a str) -> BoxFuture<'a, StoreResult<Vec<WorkflowStep>>> {
        let waits = self
            .lock()
            .values()
            .filter(|entry| !entry.run.status.is_finished())
            .flat_map(|entry| entry.steps.iter())
            .filter(|step| {
                step.kind == StepKind::Wait
                    && step.status == StepStatus::Waiting
                    && step.event.as_deref() == Some(event)
            })
            .cloned()
            .collect();
        Box::pin(async move { Ok(waits) })
    }

    fn resolve_wait<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        run_id: Uuid,
        key: &'a str,
        payload: &'a Value,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        let open = self.lock().get(&run_id).is_some_and(|entry| {
            !entry.run.status.is_finished()
                && entry
                    .steps
                    .iter()
                    .any(|s| s.key == key && s.status == StepStatus::Waiting)
        });
        if open {
            let key = key.to_owned();
            let payload = payload.clone();
            self.write(uow, move |store| {
                let mut state = store.lock();
                let Some(entry) = state.get_mut(&run_id) else {
                    return;
                };
                let Some(step) = entry
                    .steps
                    .iter_mut()
                    .find(|s| s.key == key && s.status == StepStatus::Waiting)
                else {
                    return;
                };
                step.status = StepStatus::Completed;
                step.output = Some(payload);
                step.updated_at = now;
                match entry.run.status {
                    RunStatus::Waiting | RunStatus::Sleeping => {
                        entry.run.status = RunStatus::Pending;
                        entry.run.wake_at = None;
                        entry.run.updated_at = now;
                    }
                    RunStatus::Running => entry.wake_requested = true,
                    _ => {}
                }
            });
        }
        Box::pin(async move { Ok(open) })
    }

    fn cancel<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        let open = self
            .lock()
            .get(&id)
            .is_some_and(|entry| !entry.run.status.is_finished());
        if open {
            self.write(uow, move |store| {
                if let Some(entry) = store.lock().get_mut(&id)
                    && !entry.run.status.is_finished()
                {
                    entry.run.status = RunStatus::Cancelled;
                    entry.run.finished_at = Some(now);
                    entry.run.updated_at = now;
                    entry.run.wake_at = None;
                    entry.lease = None;
                }
            });
        }
        Box::pin(async move { Ok(open) })
    }

    fn retry<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        let failed = self
            .lock()
            .get(&id)
            .is_some_and(|entry| entry.run.status == RunStatus::Failed);
        if failed {
            self.write(uow, move |store| {
                if let Some(entry) = store.lock().get_mut(&id)
                    && entry.run.status == RunStatus::Failed
                {
                    entry.run.status = RunStatus::Pending;
                    entry.run.error = None;
                    entry.run.finished_at = None;
                    entry.run.updated_at = now;
                    entry.steps.retain(|s| s.status != StepStatus::Failed);
                }
            });
        }
        Box::pin(async move { Ok(failed) })
    }

    fn prune(&self, before: DateTime<Utc>) -> BoxFuture<'_, StoreResult<u64>> {
        let mut state = self.lock();
        let len = state.len();
        state.retain(|_, entry| entry.run.finished_at.is_none_or(|at| at >= before));
        let pruned = u64::try_from(len - state.len()).unwrap_or(u64::MAX);
        drop(state);
        Box::pin(async move { Ok(pruned) })
    }
}
