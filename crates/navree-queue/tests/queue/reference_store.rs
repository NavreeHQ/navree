//! A [`QueueStore`] written only against the public trait, registered the
//! way navree-redis registers its driver (a provider named `redis`), so the
//! driver suite runs on `queue.driver = "redis"` through the same seam.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_contracts::BoxFuture;
use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_queue::{FailedJob, NewJob, QueueError, QueueStore, ReservedJob, RetryTarget};

/// A stored job and its reservation.
#[derive(Clone, Debug)]
struct Stored {
    job: NewJob,
    attempts: u32,
    reserved_by: Option<String>,
    reserved_at: Option<i64>,
}

impl Stored {
    fn reservable(&self, now: i64) -> bool {
        match self.reserved_at {
            None => self.job.available_at <= now,
            Some(at) => at.saturating_add(self.job.stale_after_ms()) <= now,
        }
    }
}

#[derive(Default)]
struct State {
    jobs: BTreeMap<String, Stored>,
    failed: Vec<FailedJob>,
}

/// The reference store: the contract of [`QueueStore`], in memory.
#[derive(Clone, Default)]
pub struct ReferenceStore {
    state: Arc<Mutex<State>>,
}

impl ReferenceStore {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many jobs (pending or reserved) the store holds.
    pub fn len(&self) -> usize {
        self.state().jobs.len()
    }
}

impl QueueStore for ReferenceStore {
    fn insert(&self, job: NewJob) -> BoxFuture<'_, Result<String, QueueError>> {
        Box::pin(async move {
            let mut state = self.state();
            if let Some(key) = &job.unique_key
                && let Some(pending) = state
                    .jobs
                    .values_mut()
                    .find(|s| s.job.unique_key.as_ref() == Some(key))
            {
                if job.blocked_by(pending.job.created_at) {
                    return Ok(pending.job.id.clone());
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
            Ok(id)
        })
    }

    fn reserve<'a>(
        &'a self,
        queue: &'a str,
        worker: &'a str,
        now: i64,
    ) -> BoxFuture<'a, Result<Option<ReservedJob>, QueueError>> {
        Box::pin(async move {
            let mut state = self.state();
            let Some(id) = state
                .jobs
                .values()
                .filter(|s| s.job.queue == queue && s.reservable(now))
                .min_by(|a, b| {
                    (a.job.available_at, &a.job.id).cmp(&(b.job.available_at, &b.job.id))
                })
                .map(|s| s.job.id.clone())
            else {
                return Ok(None);
            };
            let stored = state.jobs.get_mut(&id).expect("just found");
            stored.attempts += 1;
            stored.reserved_by = Some(worker.to_owned());
            stored.reserved_at = Some(now);
            Ok(Some(ReservedJob {
                id: stored.job.id.clone(),
                queue: stored.job.queue.clone(),
                job_type: stored.job.job_type.clone(),
                payload: stored.job.payload.clone(),
                options: stored.job.options.clone(),
                attempts: stored.attempts,
                trace_parent: stored.job.trace_parent.clone(),
            }))
        })
    }

    fn complete<'a>(&'a self, job: &'a ReservedJob) -> BoxFuture<'a, Result<(), QueueError>> {
        Box::pin(async move {
            self.state().jobs.remove(&job.id);
            Ok(())
        })
    }

    fn release<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        available_at: i64,
    ) -> BoxFuture<'a, Result<bool, QueueError>> {
        Box::pin(async move {
            let mut state = self.state();
            Ok(match state.jobs.get_mut(&job.id) {
                Some(stored) if stored.reserved_by.as_deref() == Some(worker) => {
                    stored.reserved_by = None;
                    stored.reserved_at = None;
                    stored.job.available_at = available_at;
                    true
                }
                _ => false,
            })
        })
    }

    fn fail<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        error: &'a str,
        now: i64,
    ) -> BoxFuture<'a, Result<bool, QueueError>> {
        Box::pin(async move {
            let mut state = self.state();
            let held = state
                .jobs
                .get(&job.id)
                .is_some_and(|s| s.reserved_by.as_deref() == Some(worker));
            if !held {
                return Ok(false);
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
                failed_at: chrono::DateTime::from_timestamp_millis(now).unwrap_or_default(),
            });
            Ok(true)
        })
    }

    fn failed(&self) -> BoxFuture<'_, Result<Vec<FailedJob>, QueueError>> {
        Box::pin(async move {
            let mut failed = self.state().failed.clone();
            failed.sort_by(|a, b| (b.failed_at, &b.id).cmp(&(a.failed_at, &a.id)));
            Ok(failed)
        })
    }

    fn retry<'a>(
        &'a self,
        target: &'a RetryTarget,
        now: i64,
    ) -> BoxFuture<'a, Result<u64, QueueError>> {
        Box::pin(async move {
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
            Ok(count)
        })
    }

    fn flush_failed(&self) -> BoxFuture<'_, Result<u64, QueueError>> {
        Box::pin(async move {
            let mut state = self.state();
            let count = state.failed.len() as u64;
            state.failed.clear();
            Ok(count)
        })
    }

    fn pending_count<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<u64, QueueError>> {
        Box::pin(async move {
            Ok(self
                .state()
                .jobs
                .values()
                .filter(|s| s.job.queue == queue)
                .count() as u64)
        })
    }
}

/// Registers `store` as the provider of `dyn QueueStore` named `redis`.
pub struct NamedStoreModule(pub ReferenceStore);

impl Module for NamedStoreModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let store = self.0.clone();
        m.factory(
            Factory::<dyn QueueStore>::new(move |_| {
                let store = store.clone();
                async move { Ok(Arc::new(store) as Arc<dyn QueueStore>) }
            })
            .named("redis"),
        );
    }
}
