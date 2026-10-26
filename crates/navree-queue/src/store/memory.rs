//! The memory driver: jobs in process memory, with the database driver's
//! semantics.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::record::{FailedJob, NewJob, ReservedJob, RetryTarget, from_millis, stale_after_ms};

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
        let held = state
            .jobs
            .get(&job.id)
            .is_some_and(|s| s.reserved_by.as_deref() == Some(worker));
        if !held {
            return false;
        }
        state.jobs.remove(&job.id);
        state.failed.push(FailedJob {
            id: job.id.clone(),
            queue: job.queue.clone(),
            job_type: job.job_type.clone(),
            payload: job.payload.clone(),
            options: job.options.clone(),
            error: error.to_owned(),
            attempts: job.attempts,
            failed_at: from_millis(now),
        });
        true
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
