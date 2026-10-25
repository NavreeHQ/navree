//! The records the drivers store and return: [`FailedJob`] (public) and the
//! internal [`NewJob`] and [`ReservedJob`].

use chrono::{DateTime, Utc};
use keel_contracts::JobOptions;
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
    /// The job type ([`Job::NAME`](keel_contracts::Job::NAME)).
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
