//! [`FleetGauges`]: Fleet's numbers on `GET /metrics`, read from the
//! database when Prometheus scrapes (a navree-observe `MetricsCollector`).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::TimeDelta;
use navree_kernel::BoxFuture;
use navree_observe::MetricsCollector;

use crate::fleet::RUNTIME_WINDOW;
use crate::snapshot::percentile;
use crate::{Fleet, FleetError};

/// The gauges, by name.
pub const QUEUE_PENDING: &str = "fleet_queue_pending";
/// The oldest job's wait per queue, in seconds.
pub const QUEUE_WAIT: &str = "fleet_queue_wait_seconds";
/// Attempts per queue in the last minute.
pub const QUEUE_THROUGHPUT: &str = "fleet_queue_throughput_per_minute";
/// The 95th percentile runtime per job type over the last five minutes, in
/// seconds.
pub const JOB_RUNTIME: &str = "fleet_job_runtime_seconds";
/// Worker processes per supervisor and queue, on every running master.
pub const PROCESSES: &str = "fleet_processes";
/// Jobs in `failed_jobs`.
pub const FAILED_JOBS: &str = "fleet_failed_jobs";

/// Sets Fleet's gauges on every scrape. Label sets that disappear (a job
/// type that stopped running, a stopped process) are set to zero.
pub(crate) struct FleetGauges {
    fleet: Arc<Fleet>,
    seen: Mutex<Seen>,
}

#[derive(Default)]
struct Seen {
    jobs: BTreeSet<String>,
    processes: BTreeSet<(String, String)>,
}

impl FleetGauges {
    pub(crate) fn new(fleet: Arc<Fleet>) -> Self {
        Self {
            fleet,
            seen: Mutex::new(Seen::default()),
        }
    }

    async fn refresh(&self) -> Result<(), FleetError> {
        let fleet = &self.fleet;
        for w in fleet.workload().await? {
            let queue = w.queue.clone();
            metrics::gauge!(QUEUE_PENDING, "queue" => queue.clone()).set(w.pending as f64);
            metrics::gauge!(QUEUE_WAIT, "queue" => queue.clone()).set(w.oldest_wait_secs as f64);
            metrics::gauge!(QUEUE_THROUGHPUT, "queue" => queue).set(w.jobs_per_minute as f64);
        }
        let since = fleet.now() - TimeDelta::from_std(RUNTIME_WINDOW).unwrap_or_default();
        let runtimes = fleet
            .store()
            .job_runtimes(since.timestamp_millis())
            .await?;
        let mut processes: std::collections::BTreeMap<(String, String), usize> =
            std::collections::BTreeMap::new();
        for master in fleet.masters().await? {
            for supervisor in &master.supervisors {
                for process in &supervisor.processes {
                    *processes
                        .entry((supervisor.name.clone(), process.queue.clone()))
                        .or_default() += 1;
                }
            }
        }
        let failed = fleet.queue().failed().await?.len();
        metrics::gauge!(FAILED_JOBS).set(failed as f64);

        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let jobs: BTreeSet<String> = runtimes.keys().cloned().collect();
        for (job, mut values) in runtimes {
            let p95 = percentile(&mut values, 95);
            metrics::gauge!(JOB_RUNTIME, "job" => job).set(p95 as f64 / 1000.0);
        }
        for gone in seen.jobs.difference(&jobs) {
            metrics::gauge!(JOB_RUNTIME, "job" => gone.clone()).set(0.0);
        }
        let current: BTreeSet<(String, String)> = processes.keys().cloned().collect();
        for ((supervisor, queue), count) in processes {
            metrics::gauge!(PROCESSES, "supervisor" => supervisor, "queue" => queue)
                .set(count as f64);
        }
        for (supervisor, queue) in seen.processes.difference(&current) {
            metrics::gauge!(PROCESSES, "supervisor" => supervisor.clone(), "queue" => queue.clone())
                .set(0.0);
        }
        seen.jobs = jobs;
        seen.processes = current;
        Ok(())
    }
}

impl MetricsCollector for FleetGauges {
    fn collect(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Err(error) = self.refresh().await {
                tracing::warn!(target: "navree::fleet", %error, "cannot read Fleet's metrics");
            }
        })
    }
}
