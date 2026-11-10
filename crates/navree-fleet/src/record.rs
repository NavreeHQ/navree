//! What Fleet records and reports: [`CompletedJob`], [`MasterReport`],
//! [`SupervisorReport`], [`ProcessReport`], [`FleetStatus`] and the metric
//! types.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_queue::AttemptOutcome;
use serde::{Deserialize, Serialize};

use crate::Balance;

/// A finished job attempt, as workers report it and the dashboard lists
/// it (kept for `fleet.trim.recent_minutes`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedJob {
    /// The job id.
    pub job_id: String,
    /// The queue it ran from.
    pub queue: String,
    /// The job type (`Job::NAME`).
    pub job_type: String,
    /// The attempt (1 for the first).
    pub attempt: u32,
    /// How the attempt ended.
    pub outcome: AttemptOutcome,
    /// How long it ran, in milliseconds.
    pub runtime_ms: u64,
    /// When it finished.
    pub finished_at: DateTime<Utc>,
}

/// Fleet as a whole: running, paused or inactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FleetStatus {
    /// At least one master reported in the last 30 seconds and not every
    /// supervisor is paused.
    Running,
    /// Masters are running and every supervisor is paused.
    Paused,
    /// No master has reported for 30 seconds.
    Inactive,
}

impl FleetStatus {
    /// `running`, `paused` or `inactive`.
    pub fn as_str(self) -> &'static str {
        match self {
            FleetStatus::Running => "running",
            FleetStatus::Paused => "paused",
            FleetStatus::Inactive => "inactive",
        }
    }
}

impl fmt::Display for FleetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One worker process of a supervisor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessReport {
    /// The queue it works (the supervisor's queues joined with commas with
    /// `balance = "off"`).
    pub queue: String,
    /// Its process id (none for an in-process worker).
    pub pid: Option<u32>,
    /// Its resident memory in MB, as last reported.
    pub memory_mb: Option<u64>,
    /// Jobs it has run.
    pub jobs: u64,
    /// When it started.
    pub started_at: DateTime<Utc>,
}

/// One supervisor of a master.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorReport {
    /// The supervisor name.
    pub name: String,
    /// Its queues, in priority order.
    pub queues: Vec<String>,
    /// Its balancing strategy.
    pub balance: Balance,
    /// Its processes at most.
    pub max_processes: usize,
    /// Whether it is paused.
    pub paused: bool,
    /// Its worker processes.
    pub processes: Vec<ProcessReport>,
}

/// A running master (`fleet` process), as its last heartbeat stored it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterReport {
    /// A unique id for this run of the master.
    pub id: String,
    /// The server's host name.
    pub name: String,
    /// The master's process id.
    pub pid: u32,
    /// `running`, or `paused` when every supervisor it runs is paused.
    pub status: FleetStatus,
    /// Its supervisors.
    pub supervisors: Vec<SupervisorReport>,
    /// When it started.
    pub started_at: DateTime<Utc>,
    /// Its last heartbeat.
    pub heartbeat_at: DateTime<Utc>,
}

impl MasterReport {
    /// Its worker processes, all supervisors together.
    pub fn process_count(&self) -> usize {
        self.supervisors.iter().map(|s| s.processes.len()).sum()
    }
}

/// What a metric snapshot row describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MetricKind {
    /// A queue.
    Queue,
    /// A job type.
    Job,
}

impl MetricKind {
    /// `queue` or `job`.
    pub fn as_str(self) -> &'static str {
        match self {
            MetricKind::Queue => "queue",
            MetricKind::Job => "job",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "queue" => Some(MetricKind::Queue),
            "job" => Some(MetricKind::Job),
            _ => None,
        }
    }
}

/// One snapshot of a queue or a job type: what ran since the previous
/// snapshot of the same master.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricSnapshot {
    /// When the snapshot was taken.
    pub at: DateTime<Utc>,
    /// Queue or job type.
    pub kind: MetricKind,
    /// The queue or job type.
    pub name: String,
    /// Attempts that finished.
    pub processed: u64,
    /// Attempts that failed (released for a retry or failed for good).
    pub failed: u64,
    /// Their average runtime, in milliseconds.
    pub runtime_avg_ms: u64,
    /// Their 95th percentile runtime, in milliseconds.
    pub runtime_p95_ms: u64,
    /// The queue's pending jobs at the snapshot (queues only).
    pub pending: Option<u64>,
    /// How long the queue's oldest job had waited, in milliseconds (queues
    /// only).
    pub wait_ms: Option<u64>,
}

/// A chart's period on the dashboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MetricsPeriod {
    /// The last hour, per minute (the default).
    #[default]
    Hour,
    /// The last day, per 15 minutes.
    Day,
    /// The last week, per hour.
    Week,
}

impl MetricsPeriod {
    /// How far back the period goes.
    pub fn span(self) -> chrono::TimeDelta {
        match self {
            MetricsPeriod::Hour => chrono::TimeDelta::hours(1),
            MetricsPeriod::Day => chrono::TimeDelta::days(1),
            MetricsPeriod::Week => chrono::TimeDelta::weeks(1),
        }
    }

    /// The width of one point.
    pub fn bucket(self) -> chrono::TimeDelta {
        match self {
            MetricsPeriod::Hour => chrono::TimeDelta::minutes(1),
            MetricsPeriod::Day => chrono::TimeDelta::minutes(15),
            MetricsPeriod::Week => chrono::TimeDelta::hours(1),
        }
    }
}

/// One point of a [`MetricSeries`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricPoint {
    /// The start of the bucket.
    pub at: DateTime<Utc>,
    /// Attempts finished in the bucket (every master).
    pub processed: u64,
    /// Failed attempts in the bucket.
    pub failed: u64,
    /// Attempts per minute over the bucket.
    pub throughput_per_minute: f64,
    /// Average runtime, in milliseconds.
    pub runtime_avg_ms: u64,
    /// The largest 95th percentile runtime of the bucket's snapshots, in
    /// milliseconds.
    pub runtime_p95_ms: u64,
    /// The longest wait of the bucket, in seconds (queues only).
    pub wait_secs: Option<f64>,
}

/// The chart data of one queue or job type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricSeries {
    /// Queue or job type.
    pub kind: MetricKind,
    /// The queue or job type.
    pub name: String,
    /// One point per bucket that has data, oldest first.
    pub points: Vec<MetricPoint>,
}
