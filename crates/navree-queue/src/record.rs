//! The records the drivers store and return: [`FailedJob`] (public) and the
//! internal [`NewJob`] and [`ReservedJob`].

use chrono::{DateTime, Utc};
use navree_contracts::JobOptions;
use serde::Serialize;
use serde_json::Value;

/// A job that used up its attempts (or could not run at all), as kept in
/// `failed_jobs` until it is retried or flushed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FailedJob {
    /// The job id (the id `dispatch` returned).
    pub id: String,
    /// The queue it was on.
    pub queue: String,
    /// The job type ([`Job::NAME`](navree_contracts::Job::NAME)).
    pub job_type: String,
    /// The job value as JSON.
    pub payload: Value,
    /// The options it was dispatched with.
    pub options: JobOptions,
    /// The error of the last attempt.
    pub error: String,
    /// How many attempts ran.
    pub attempts: u32,
    /// When it failed.
    pub failed_at: DateTime<Utc>,
}

/// Where a job that is not finished yet stands ([`PendingJob::state`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingState {
    /// Available now and waiting for a worker.
    Waiting,
    /// Available later: dispatched with a delay, or released for a retry
    /// after its backoff.
    Delayed,
    /// Reserved by a worker (a crashed worker's reservation counts until
    /// another worker takes the job over).
    Running,
}

/// A job that is not finished yet, as [`Queue::pending_jobs`](crate::Queue::pending_jobs)
/// lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PendingJob {
    /// The job id.
    pub id: String,
    /// The queue it is on.
    pub queue: String,
    /// The job type ([`Job::NAME`](navree_contracts::Job::NAME)).
    pub job_type: String,
    /// The job value as JSON.
    pub payload: Value,
    /// The options it was dispatched with.
    pub options: JobOptions,
    /// Attempts started so far (a running job's current one included).
    pub attempts: u32,
    /// Waiting, delayed or running.
    pub state: PendingState,
    /// When it is (or was) available to workers.
    pub available_at: DateTime<Utc>,
    /// When a worker reserved it, while it runs.
    pub reserved_at: Option<DateTime<Utc>>,
    /// When it was queued (or last retried from the failed jobs).
    pub created_at: DateTime<Utc>,
}

/// Which pending jobs [`Queue::pending_jobs`](crate::Queue::pending_jobs)
/// lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingQuery {
    /// Only jobs of this queue.
    pub queue: Option<String>,
    /// Only jobs whose type or queue contains this text (case-insensitive).
    pub search: Option<String>,
    /// At most this many (default [`PendingQuery::DEFAULT_LIMIT`]).
    pub limit: Option<u64>,
}

impl PendingQuery {
    /// How many jobs a query without a limit returns.
    pub const DEFAULT_LIMIT: u64 = 500;

    /// The limit to apply.
    pub(crate) fn limit(&self) -> u64 {
        self.limit.unwrap_or(Self::DEFAULT_LIMIT)
    }

    /// The lowercased search text, if any.
    pub(crate) fn needle(&self) -> Option<String> {
        self.search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
    }

    /// Whether a job of `queue` and `job_type` matches.
    pub(crate) fn matches(&self, queue: &str, job_type: &str) -> bool {
        if self.queue.as_deref().is_some_and(|q| q != queue) {
            return false;
        }
        match self.needle() {
            Some(needle) => {
                queue.to_lowercase().contains(&needle) || job_type.to_lowercase().contains(&needle)
            }
            None => true,
        }
    }
}

/// One queue's unfinished jobs at a moment, from [`Queue::status`](crate::Queue::status).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueueStatus {
    /// The queue.
    pub queue: String,
    /// Jobs available now, waiting for a worker.
    pub waiting: u64,
    /// Jobs available later (delayed, or released for a retry).
    pub delayed: u64,
    /// Jobs reserved by a worker.
    pub running: u64,
    /// When the oldest waiting job became available (`None`: none waits).
    pub oldest_available_at: Option<DateTime<Utc>>,
    /// When the status was taken (the queue's clock).
    pub at: DateTime<Utc>,
}

impl QueueStatus {
    /// Every job that is not finished yet: waiting, delayed and running.
    pub fn pending(&self) -> u64 {
        self.waiting + self.delayed + self.running
    }

    /// How long the oldest waiting job has waited (zero when none waits).
    pub fn oldest_wait(&self) -> std::time::Duration {
        self.oldest_available_at
            .and_then(|since| (self.at - since).to_std().ok())
            .unwrap_or_default()
    }
}

/// Which failed jobs [`Queue::retry`](crate::Queue::retry) puts back on
/// their queues. Converts from a string: `"all"` is [`All`](RetryTarget::All),
/// anything else a job id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryTarget {
    /// Every failed job.
    All,
    /// The failed job with this id.
    Id(String),
}

impl From<&str> for RetryTarget {
    fn from(value: &str) -> Self {
        if value == "all" {
            RetryTarget::All
        } else {
            RetryTarget::Id(value.to_owned())
        }
    }
}

impl From<String> for RetryTarget {
    fn from(value: String) -> Self {
        RetryTarget::from(value.as_str())
    }
}

impl FailedJob {
    /// The job as it goes back on its queue at `now`: same id, attempts
    /// start over, not unique.
    pub(crate) fn requeued(&self, now: i64) -> NewJob {
        NewJob {
            id: self.id.clone(),
            queue: self.queue.clone(),
            job_type: self.job_type.clone(),
            payload: self.payload.clone(),
            options: self.options.clone(),
            available_at: now,
            created_at: now,
            unique_key: None,
            unique_for_ms: None,
            trace_parent: None,
        }
    }
}

/// A job ready to be stored.
#[derive(Clone, Debug)]
pub(crate) struct NewJob {
    pub(crate) id: String,
    pub(crate) queue: String,
    pub(crate) job_type: String,
    pub(crate) payload: Value,
    pub(crate) options: JobOptions,
    /// Unix milliseconds.
    pub(crate) available_at: i64,
    /// Unix milliseconds.
    pub(crate) created_at: i64,
    /// Set for unique jobs: a pending job with the same key (dispatched
    /// within [`unique_for_ms`](NewJob::unique_for_ms)) takes its place.
    pub(crate) unique_key: Option<String>,
    /// How long a pending unique job blocks its duplicates (`None`: while
    /// it is pending).
    pub(crate) unique_for_ms: Option<i64>,
    /// The W3C `traceparent` of the span that dispatched the job, so its
    /// attempts continue that trace.
    pub(crate) trace_parent: Option<String>,
}

impl NewJob {
    /// Whether a pending duplicate created at `created_at` (Unix
    /// milliseconds) still blocks this job.
    pub(crate) fn blocked_by(&self, created_at: i64) -> bool {
        self.unique_for_ms
            .is_none_or(|window| self.created_at < created_at.saturating_add(window))
    }
}

/// A job a worker reserved.
#[derive(Clone, Debug)]
pub(crate) struct ReservedJob {
    pub(crate) id: String,
    pub(crate) queue: String,
    pub(crate) job_type: String,
    pub(crate) payload: Value,
    pub(crate) options: JobOptions,
    /// Including the attempt that is starting.
    pub(crate) attempts: u32,
    /// The `traceparent` recorded at dispatch (see [`NewJob::trace_parent`]).
    pub(crate) trace_parent: Option<String>,
}

/// How long a reservation of a job without a time limit (`timeout_secs`
/// 0) lasts before it counts as abandoned: one day.
pub(crate) const UNLIMITED_RESERVATION_SECS: u64 = 86_400;

/// How long after it was taken a reservation counts as abandoned, in
/// milliseconds: the job's timeout plus
/// [`STALE_RESERVATION_GRACE_SECS`](crate::STALE_RESERVATION_GRACE_SECS).
pub(crate) fn stale_after_ms(options: &JobOptions) -> i64 {
    let timeout = match options.timeout_secs {
        0 => UNLIMITED_RESERVATION_SECS,
        secs => secs,
    };
    let secs = timeout.saturating_add(crate::STALE_RESERVATION_GRACE_SECS);
    i64::try_from(secs.saturating_mul(1000)).unwrap_or(i64::MAX)
}

/// Unix milliseconds → `DateTime<Utc>` (the epoch for out-of-range values).
pub(crate) fn from_millis(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}
