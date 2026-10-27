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
            membership: Membership::default(),
        }
    }
}

/// What a job belongs to: a batch (as one of its jobs or as one of its
/// callbacks) and an operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Membership {
    /// The batch the job belongs to.
    pub(crate) batch_id: Option<String>,
    /// Whether the job is a `then`, `catch` or `finally` callback of its
    /// batch rather than one of its jobs.
    pub(crate) callback: bool,
    /// The operation the job does the work of.
    pub(crate) operation_id: Option<String>,
}

impl Membership {
    /// One of the jobs of batch `batch_id`.
    pub(crate) fn batch_job(batch_id: &str, operation_id: Option<&str>) -> Self {
        Self {
            batch_id: Some(batch_id.to_owned()),
            callback: false,
            operation_id: operation_id.map(str::to_owned),
        }
    }

    /// A callback of batch `batch_id`.
    pub(crate) fn batch_callback(batch_id: &str, operation_id: Option<&str>) -> Self {
        Self {
            batch_id: Some(batch_id.to_owned()),
            callback: true,
            operation_id: operation_id.map(str::to_owned),
        }
    }

    /// One of a batch's jobs (not a callback).
    pub(crate) fn in_batch(&self) -> Option<&str> {
        match (&self.batch_id, self.callback) {
            (Some(batch), false) => Some(batch),
            _ => None,
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
    /// The batch and operation the job belongs to.
    pub(crate) membership: Membership,
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
    /// The batch and operation the job belongs to.
    pub(crate) membership: Membership,
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
