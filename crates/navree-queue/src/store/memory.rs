//! The memory driver: jobs in process memory, with the database driver's
//! semantics.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::QueueError;
use crate::batch::{BatchRow, BatchStep, Finish, Finished};
use crate::record::{FailedJob, NewJob, ReservedJob, RetryTarget, from_millis, stale_after_ms};
use crate::store::PrepareCallback;

/// A stored job.
#[derive(Clone, Debug)]
struct Stored {
    job: NewJob,
    attempts: u32,
    reserved_by: Option<String>,
    /// Unix milliseconds.
    reserved_at: Option<i64>,
}

impl Stored {
    /// Whether a worker may take the job at `now`: unreserved and due, or
    /// its reservation is stale.
    fn reservable(&self, now: i64) -> bool {
        match self.reserved_at {
            None => self.job.available_at <= now,
            Some(at) => at.saturating_add(stale_after_ms(&self.job.options)) <= now,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    /// By id (UUID v7, so in dispatch order).
    jobs: BTreeMap<String, Stored>,
    /// Oldest first.
    failed: Vec<FailedJob>,
    /// By id.
    batches: BTreeMap<String, BatchRow>,
}

/// Jobs in memory.
#[derive(Debug, Default)]
pub(crate) struct MemoryStore {
    state: Mutex<State>,
}

impl MemoryStore {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores `job`; returns its id, or the id of the pending unique job
    /// that takes its place.
    pub(crate) fn insert(&self, job: NewJob) -> String {
        let mut state = self.state();
        if let Some(key) = &job.unique_key
            && let Some(pending) = state
                .jobs
                .values_mut()
                .find(|s| s.job.unique_key.as_ref() == Some(key))
        {
            if job.blocked_by(pending.job.created_at) {
                return pending.job.id.clone();
            }
            pending.job.unique_key = None;
        }
        let id = job.id.clone();
        state.jobs.insert(
            id.clone(),
            Stored {
                job,
                attempts: 0,
                reserved_by: None,
                reserved_at: None,
            },
        );
        id
    }

    pub(crate) fn reserve(&self, queue: &str, worker: &str, now: i64) -> Option<ReservedJob> {
        let mut state = self.state();
        let id = state
            .jobs
            .values()
            .filter(|s| s.job.queue == queue && s.reservable(now))
            .min_by(|a, b| (a.job.available_at, &a.job.id).cmp(&(b.job.available_at, &b.job.id)))
            .map(|s| s.job.id.clone())?;
        let stored = state.jobs.get_mut(&id)?;
        stored.attempts += 1;
        stored.reserved_by = Some(worker.to_owned());
        stored.reserved_at = Some(now);
        Some(ReservedJob {
            id: stored.job.id.clone(),
            queue: stored.job.queue.clone(),
            job_type: stored.job.job_type.clone(),
            payload: stored.job.payload.clone(),
            options: stored.job.options.clone(),
            attempts: stored.attempts,
            membership: stored.job.membership.clone(),
        })
    }

    pub(crate) fn complete(&self, id: &str) {
        self.state().jobs.remove(id);
    }

    pub(crate) fn release(&self, id: &str, worker: &str, available_at: i64) -> bool {
        let mut state = self.state();
        match state.jobs.get_mut(id) {
            Some(stored) if stored.reserved_by.as_deref() == Some(worker) => {
                stored.reserved_by = None;
                stored.reserved_at = None;
                stored.job.available_at = available_at;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn fail(&self, job: &ReservedJob, worker: &str, error: &str, now: i64) -> bool {
        let mut state = self.state();
        if !state.held(job, worker) {
            return false;
        }
        state.jobs.remove(&job.id);
        state.failed.push(failed_job(job, error, now));
        true
    }

    /// Stores a batch with its jobs (and, for a batch with no jobs, the
    /// callbacks already due).
    pub(crate) fn insert_batch(&self, row: BatchRow, jobs: Vec<NewJob>) {
        let mut state = self.state();
        state.batches.insert(row.id.clone(), row);
        for job in jobs {
            state.insert_new(job);
        }
    }

    /// Finishes a reserved job of a batch (one of its jobs or callbacks):
    /// removes it (into the failed jobs when it failed), updates the batch
    /// and queues the callbacks that became due.
    pub(crate) fn finish_batch_job(
        &self,
        job: &ReservedJob,
        worker: &str,
        finish: Finish,
        error: &str,
        now: i64,
        prepare: &PrepareCallback<'_>,
    ) -> Result<Finished, QueueError> {
        let mut state = self.state();
        if !state.held(job, worker) {
            return Ok(Finished::Lost);
        }
        let batch = job
            .membership
            .batch_id
            .as_ref()
            .and_then(|id| state.batches.get(id).cloned());
        let step = match batch {
            Some(mut row) => {
                let was_settled = row.settled();
                let due = if job.membership.callback {
                    row.callback_finished(finish == Finish::Failed);
                    Vec::new()
                } else {
                    row.job_finished(&job.id, finish, now)
                };
                let callbacks = due
                    .into_iter()
                    .map(|envelope| prepare(envelope, &row))
                    .collect::<Result<Vec<_>, _>>()?;
                for callback in callbacks {
                    state.insert_new(callback);
                }
                let settled_now = !was_settled && row.settled();
                state.batches.insert(row.id.clone(), row.clone());
                Some(BatchStep {
                    batch: row,
                    settled_now,
                })
            }
            None => None,
        };
        state.jobs.remove(&job.id);
        if finish == Finish::Failed {
            state.failed.push(failed_job(job, error, now));
        }
        Ok(Finished::Done(step))
    }

    pub(crate) fn find_batch(&self, id: &str) -> Option<BatchRow> {
        self.state().batches.get(id).cloned()
    }

    /// Newest first, at most `limit`.
    pub(crate) fn batches(&self, limit: usize) -> Vec<BatchRow> {
        let mut batches: Vec<BatchRow> = self.state().batches.values().cloned().collect();
        batches.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
        batches.truncate(limit);
        batches
    }

    /// Cancels batch `id` at `now` (unless it finished); the batch
    /// afterwards, `None` when there is none.
    pub(crate) fn cancel_batch(&self, id: &str, now: i64) -> Option<BatchRow> {
        let mut state = self.state();
        let row = state.batches.get_mut(id)?;
        row.cancel(now);
        Some(row.clone())
    }

    /// Deletes the batches that settled before `before`; returns how many.
    pub(crate) fn prune_batches(&self, before: i64) -> u64 {
        let mut state = self.state();
        let count = state.batches.len();
        state
            .batches
            .retain(|_, row| !(row.settled() && row.finished_at.is_some_and(|at| at < before)));
        (count - state.batches.len()) as u64
    }

    /// How many stored jobs do the work of operation `operation_id`.
    pub(crate) fn count_for_operation(&self, operation_id: &str) -> u64 {
        self.state()
            .jobs
            .values()
            .filter(|s| s.job.membership.operation_id.as_deref() == Some(operation_id))
            .count() as u64
    }

    /// Newest first.
    pub(crate) fn failed(&self) -> Vec<FailedJob> {
        let mut failed = self.state().failed.clone();
        failed.sort_by(|a, b| (b.failed_at, &b.id).cmp(&(a.failed_at, &a.id)));
        failed
    }

    pub(crate) fn retry(&self, target: &RetryTarget, now: i64) -> u64 {
        let mut state = self.state();
        let (retried, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.failed)
            .into_iter()
            .partition(|failed| match target {
                RetryTarget::All => true,
                RetryTarget::Id(id) => &failed.id == id,
            });
        state.failed = kept;
        let count = retried.len() as u64;
        for failed in retried {
            state.jobs.insert(
                failed.id.clone(),
                Stored {
                    job: failed.requeued(now),
                    attempts: 0,
                    reserved_by: None,
                    reserved_at: None,
                },
            );
        }
        count
    }

    pub(crate) fn flush_failed(&self) -> u64 {
        let mut state = self.state();
        let count = state.failed.len() as u64;
        state.failed.clear();
        count
    }

    pub(crate) fn pending_count(&self, queue: &str) -> u64 {
        self.state()
            .jobs
            .values()
            .filter(|s| s.job.queue == queue)
            .count() as u64
    }
}

impl State {
    /// Whether `worker` holds the reservation of `job`.
    fn held(&self, job: &ReservedJob, worker: &str) -> bool {
        self.jobs
            .get(&job.id)
            .is_some_and(|s| s.reserved_by.as_deref() == Some(worker))
    }

    /// Stores a job that is not unique (batch jobs and callbacks).
    fn insert_new(&mut self, job: NewJob) {
        self.jobs.insert(
            job.id.clone(),
            Stored {
                job,
                attempts: 0,
                reserved_by: None,
                reserved_at: None,
            },
        );
    }
}

fn failed_job(job: &ReservedJob, error: &str, now: i64) -> FailedJob {
    FailedJob {
        id: job.id.clone(),
        queue: job.queue.clone(),
        job_type: job.job_type.clone(),
        payload: job.payload.clone(),
        options: job.options.clone(),
        error: error.to_owned(),
        attempts: job.attempts,
        failed_at: from_millis(now),
    }
}
