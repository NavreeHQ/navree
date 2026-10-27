//! [`Fleet`]: the service behind the commands and the dashboard.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::Clock;
use navree_queue::{FailedJob, PendingJob, PendingQuery, Queue, QueueError, QueueStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::record::{CompletedJob, MasterReport, MetricSeries, MetricSnapshot, MetricsPeriod};
use crate::redact::Redactor;
use crate::store::{FleetStore, needle};
use crate::{FleetError, FleetOptions, FleetStatus, LongWaitDetected, QueueWorkload, Supervisor};

/// A master that has not reported for this long is inactive: the dashboard
/// and `fleet:status` leave it out.
pub const MASTER_TIMEOUT: Duration = Duration::from_secs(30);

/// How far back the balancer's average runtimes look.
pub const RUNTIME_WINDOW: Duration = Duration::from_secs(5 * 60);

/// The most completed or pending jobs one listing returns.
pub const LIST_LIMIT: u64 = 500;

const PAUSED_PREFIX: &str = "paused:";
const TERMINATE_KEY: &str = "terminate";
const LONG_WAIT_PREFIX: &str = "long_wait:";

/// The longest current wait, on the dashboard's overview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongestWait {
    /// The queue.
    pub queue: String,
    /// How long its oldest job has waited, in seconds.
    pub wait_secs: u64,
}

/// The dashboard's row of figures and workload table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    /// Running, paused or inactive.
    pub status: FleetStatus,
    /// Attempts finished in the last minute.
    pub jobs_per_minute: u64,
    /// Attempts finished in the past hour.
    pub jobs_past_hour: u64,
    /// Jobs that failed in the past seven days (still in `failed_jobs`).
    pub failed_past_week: u64,
    /// The longest current wait and its queue.
    pub longest_wait: Option<LongestWait>,
    /// The paused supervisors.
    pub paused: Vec<String>,
    /// Every supervised queue's workload.
    pub workload: Vec<QueueWorkload>,
}

/// Fleet: supervisors and their state, the workload of the supervised
/// queues, failed jobs, completed jobs and metrics. Provided by
/// [`FleetModule`](crate::FleetModule).
///
/// Pausing and terminating are stored in the database, so they reach every
/// master on every server.
///
/// ```no_run
/// # async fn demo(fleet: &navree_fleet::Fleet) -> Result<(), navree_fleet::FleetError> {
/// fleet.pause(Some("exports")).await?;
/// fleet.resume(Some("exports")).await?;
/// for late in fleet.long_waits().await? {
///     println!("{} waited {} s", late.queue, late.wait_secs);
/// }
/// # Ok(())
/// # }
/// ```
pub struct Fleet {
    store: FleetStore,
    queue: Arc<Queue>,
    options: Arc<FleetOptions>,
    supervisors: Vec<Supervisor>,
    clock: Option<Arc<dyn Clock>>,
    redactor: Redactor,
}

impl fmt::Debug for Fleet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fleet")
            .field("supervisors", &self.supervisors)
            .finish_non_exhaustive()
    }
}

impl Fleet {
    pub(crate) fn new(
        store: FleetStore,
        queue: Arc<Queue>,
        options: Arc<FleetOptions>,
        clock: Option<Arc<dyn Clock>>,
        redactor: Redactor,
    ) -> Self {
        let supervisors = options.all_supervisors(queue.options());
        Self {
            store,
            queue,
            options,
            supervisors,
            clock,
            redactor,
        }
    }

    /// The `fleet` settings.
    pub fn options(&self) -> &FleetOptions {
        &self.options
    }

    /// Every configured supervisor (all servers together, `fleet.only`
    /// aside), sorted by name.
    pub fn supervisors(&self) -> &[Supervisor] {
        &self.supervisors
    }

    /// The supervisor named `name`.
    pub fn supervisor(&self, name: &str) -> Option<&Supervisor> {
        self.supervisors.iter().find(|s| s.name == name)
    }

    /// The supervisors this server runs (`fleet.only`).
    pub fn local_supervisors(&self) -> Result<Vec<Supervisor>, FleetError> {
        self.options.supervisors(self.queue.options())
    }

    /// The queue Fleet works on.
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub(crate) fn store(&self) -> &FleetStore {
        &self.store
    }

    /// The current time (the app's `dyn Clock` when it has one).
    pub fn now(&self) -> DateTime<Utc> {
        match &self.clock {
            Some(clock) => clock.now(),
            None => Utc::now(),
        }
    }

    fn now_ms(&self) -> i64 {
        self.now().timestamp_millis()
    }

    /// Every supervised queue, in supervisor and priority order, once each.
    pub fn queues(&self) -> Vec<String> {
        let mut queues: Vec<String> = Vec::new();
        for supervisor in &self.supervisors {
            for queue in &supervisor.queues {
                if !queues.contains(queue) {
                    queues.push(queue.clone());
                }
            }
        }
        queues
    }

    fn known(&self, supervisor: &str) -> Result<(), FleetError> {
        if self.supervisor(supervisor).is_some() {
            Ok(())
        } else {
            Err(FleetError::UnknownSupervisor(supervisor.to_owned()))
        }
    }

    // ------------------------------------------------- pause and terminate

    /// Pauses one supervisor, or every supervisor (`None`), on every
    /// server: its workers finish the jobs they run and take no new ones.
    /// Jobs keep queueing.
    pub async fn pause(&self, supervisor: Option<&str>) -> Result<(), FleetError> {
        let now = self.now_ms();
        let names: Vec<String> = match supervisor {
            Some(name) => {
                self.known(name)?;
                vec![name.to_owned()]
            }
            None => self.supervisors.iter().map(|s| s.name.clone()).collect(),
        };
        for name in names {
            self.store
                .set_state(&format!("{PAUSED_PREFIX}{name}"), "1", now)
                .await?;
        }
        Ok(())
    }

    /// Continues one paused supervisor, or every one (`None`).
    pub async fn resume(&self, supervisor: Option<&str>) -> Result<(), FleetError> {
        match supervisor {
            Some(name) => {
                self.known(name)?;
                self.store
                    .delete_state(&format!("{PAUSED_PREFIX}{name}"))
                    .await
            }
            None => self.store.delete_state_prefix(PAUSED_PREFIX).await,
        }
    }

    /// The paused supervisors.
    pub async fn paused(&self) -> Result<BTreeSet<String>, FleetError> {
        Ok(self
            .store
            .state_prefix(PAUSED_PREFIX)
            .await?
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix(PAUSED_PREFIX).map(str::to_owned))
            .collect())
    }

    /// Whether `supervisor` is paused.
    pub async fn is_paused(&self, supervisor: &str) -> Result<bool, FleetError> {
        Ok(self.paused().await?.contains(supervisor))
    }

    /// Asks every running master, on every server, to stop gracefully (its
    /// workers finish their jobs), for its process manager to start it
    /// again: run it after a deploy. Masters started afterwards keep
    /// running.
    pub async fn terminate(&self) -> Result<(), FleetError> {
        let token = uuid::Uuid::now_v7().to_string();
        self.store
            .set_state(TERMINATE_KEY, &token, self.now_ms())
            .await
    }

    /// The token of the last `terminate`: a master stops when it changes.
    pub(crate) async fn terminate_token(&self) -> Result<Option<String>, FleetError> {
        self.store.state(TERMINATE_KEY).await
    }

    // -------------------------------------------------------------- masters

    /// The masters that reported in the last 30 seconds, oldest first.
    pub async fn masters(&self) -> Result<Vec<MasterReport>, FleetError> {
        let since = self.now() - TimeDelta::from_std(MASTER_TIMEOUT).unwrap_or_default();
        self.store.masters(since.timestamp_millis()).await
    }

    /// Running (a master reported in the last 30 seconds and not every
    /// supervisor is paused), paused or inactive.
    pub async fn status(&self) -> Result<FleetStatus, FleetError> {
        Ok(status_of(&self.masters().await?))
    }

    // ------------------------------------------------------------- workload

    /// Each supervised queue's workload: its pending jobs and oldest wait,
    /// the average runtime of its jobs over the last five minutes, the
    /// workload (the time it would take to clear on one process), its
    /// processes on the running masters and its throughput. The numbers
    /// the `auto` balancer works from.
    pub async fn workload(&self) -> Result<Vec<QueueWorkload>, FleetError> {
        let masters = self.masters().await?;
        self.workload_of(&self.queues(), &masters).await
    }

    /// The workload of `queues`, with processes counted on `masters`.
    pub(crate) async fn workload_of(
        &self,
        queues: &[String],
        masters: &[MasterReport],
    ) -> Result<Vec<QueueWorkload>, FleetError> {
        let now = self.now();
        let window = TimeDelta::from_std(RUNTIME_WINDOW).unwrap_or_default();
        let runtimes = self
            .store
            .queue_runtimes((now - window).timestamp_millis())
            .await?;
        let last_minute = self
            .store
            .queue_runtimes((now - TimeDelta::minutes(1)).timestamp_millis())
            .await?;
        let mut out = Vec::with_capacity(queues.len());
        for queue in queues {
            let status = self.queue.status(queue).await?;
            out.push(self.workload_from(&status, &runtimes, &last_minute, masters));
        }
        Ok(out)
    }

    fn workload_from(
        &self,
        status: &QueueStatus,
        runtimes: &std::collections::BTreeMap<String, (u64, u64)>,
        last_minute: &std::collections::BTreeMap<String, (u64, u64)>,
        masters: &[MasterReport],
    ) -> QueueWorkload {
        let queue = status.queue.as_str();
        let avg = runtimes.get(queue).map_or(0, |(_, avg)| *avg);
        let mut workload = QueueWorkload::new(
            queue,
            status.waiting,
            status.running,
            status.oldest_wait().as_secs(),
            avg,
        );
        workload.delayed = status.delayed;
        workload.pending = status.pending();
        workload.jobs_per_minute = last_minute.get(queue).map_or(0, |(n, _)| *n);
        workload.processes = masters
            .iter()
            .flat_map(|m| &m.supervisors)
            .flat_map(|s| &s.processes)
            .filter(|p| p.queue.split(',').any(|q| q == queue))
            .count();
        workload.long_wait_threshold_secs = self.options.long_wait.get(queue).copied();
        workload.compute();
        workload
    }

    /// The queues whose oldest available job has waited longer than their
    /// `fleet.long_wait` threshold, right now. Queues without a threshold
    /// are never listed.
    pub async fn long_waits(&self) -> Result<Vec<LongWaitDetected>, FleetError> {
        let mut late = Vec::new();
        for (queue, threshold) in &self.options.long_wait {
            let waited = self.queue.status(queue).await?.oldest_wait().as_secs();
            if waited > *threshold {
                late.push(LongWaitDetected {
                    queue: queue.clone(),
                    wait_secs: waited,
                    threshold_secs: *threshold,
                });
            }
        }
        Ok(late)
    }

    /// Claims the long-wait alert of `queue` for now: true at most once per
    /// `fleet.long_wait_cooldown_minutes`, across every master.
    pub(crate) async fn claim_long_wait(&self, queue: &str) -> Result<bool, FleetError> {
        let cooldown = i64::try_from(self.options.long_wait_cooldown_minutes)
            .unwrap_or(i64::MAX / 60_000)
            .saturating_mul(60_000);
        self.store
            .claim(&format!("{LONG_WAIT_PREFIX}{queue}"), self.now_ms(), cooldown)
            .await
    }

    /// The dashboard's overview: status, throughput, failures, the longest
    /// wait and the workload table.
    pub async fn overview(&self) -> Result<Overview, FleetError> {
        let now = self.now();
        let masters = self.masters().await?;
        let workload = self.workload_of(&self.queues(), &masters).await?;
        let week_ago = now - TimeDelta::days(7);
        let failed_past_week = self
            .queue
            .failed()
            .await?
            .iter()
            .filter(|job| job.failed_at >= week_ago)
            .count() as u64;
        let longest_wait = workload
            .iter()
            .filter(|w| w.oldest_wait_secs > 0)
            .max_by_key(|w| w.oldest_wait_secs)
            .map(|w| LongestWait {
                queue: w.queue.clone(),
                wait_secs: w.oldest_wait_secs,
            });
        Ok(Overview {
            status: status_of(&masters),
            jobs_per_minute: self
                .store
                .count_jobs((now - TimeDelta::minutes(1)).timestamp_millis())
                .await?,
            jobs_past_hour: self
                .store
                .count_jobs((now - TimeDelta::hours(1)).timestamp_millis())
                .await?,
            failed_past_week,
            longest_wait,
            paused: self.paused().await?.into_iter().collect(),
            workload,
        })
    }

    // ------------------------------------------------------------ failed jobs

    /// Every failed job, newest first, whose type, queue, error or payload
    /// contains `search` (case-insensitive).
    pub async fn failed_jobs(&self, search: Option<&str>) -> Result<Vec<FailedJob>, FleetError> {
        let mut failed = self.queue.failed().await?;
        failed.sort_by(|a, b| b.failed_at.cmp(&a.failed_at).then(b.id.cmp(&a.id)));
        let Some(needle) = needle(search) else {
            return Ok(failed);
        };
        Ok(failed
            .into_iter()
            .filter(|job| {
                job.job_type.to_lowercase().contains(&needle)
                    || job.queue.to_lowercase().contains(&needle)
                    || job.error.to_lowercase().contains(&needle)
                    || job.payload.to_string().to_lowercase().contains(&needle)
            })
            .collect())
    }

    /// The failed job with id `id`.
    pub async fn failed_job(&self, id: &str) -> Result<Option<FailedJob>, FleetError> {
        Ok(self.queue.failed_job(id).await?)
    }

    /// Puts failed job `id` back on its queue.
    pub async fn retry(&self, id: &str) -> Result<(), FleetError> {
        if self.queue.failed_job(id).await?.is_none() {
            return Err(FleetError::FailedJobNotFound { id: id.to_owned() });
        }
        self.queue.retry(id).await?;
        Ok(())
    }

    /// Retries every failed job matching `search` (every failed job
    /// without one); returns how many.
    pub async fn retry_all(&self, search: Option<&str>) -> Result<u64, FleetError> {
        if needle(search).is_none() {
            return Ok(self.queue.retry("all").await?);
        }
        let mut retried = 0;
        for job in self.failed_jobs(search).await? {
            retried += self.queue.retry(job.id.as_str()).await?;
        }
        Ok(retried)
    }

    /// Deletes failed job `id` without retrying it.
    pub async fn forget(&self, id: &str) -> Result<(), FleetError> {
        match self.queue.forget(id).await {
            Ok(()) => Ok(()),
            Err(QueueError::FailedJobNotFound { id }) => Err(FleetError::FailedJobNotFound { id }),
            Err(error) => Err(error.into()),
        }
    }

    // ------------------------------------------------------------------ jobs

    /// Pending jobs (waiting, delayed and running), oldest first, whose
    /// type or queue contains `search`; at most 500.
    pub async fn pending_jobs(&self, search: Option<&str>) -> Result<Vec<PendingJob>, FleetError> {
        let query = PendingQuery {
            search: search.map(str::to_owned),
            limit: Some(LIST_LIMIT),
            ..PendingQuery::default()
        };
        Ok(self.queue.pending_jobs(&query).await?)
    }

    /// The pending job with id `id`.
    pub async fn pending_job(&self, id: &str) -> Result<Option<PendingJob>, FleetError> {
        Ok(self.queue.pending_job(id).await?)
    }

    /// Attempts finished within `fleet.trim.recent_minutes`, newest first,
    /// whose type or queue contains `search`; at most 500.
    pub async fn completed_jobs(&self, search: Option<&str>) -> Result<Vec<CompletedJob>, FleetError> {
        let minutes = i64::try_from(self.options.trim.recent_minutes).unwrap_or(i64::MAX / 60_000);
        let since = self.now() - TimeDelta::minutes(minutes);
        self.store
            .jobs(since.timestamp_millis(), search, LIST_LIMIT)
            .await
    }

    /// Stores finished attempts (a master does this for its workers).
    pub async fn record(&self, master: Option<&str>, jobs: &[CompletedJob]) -> Result<(), FleetError> {
        self.store.insert_jobs(master, jobs).await
    }

    /// Stores metric snapshots (a master does this every
    /// `fleet.snapshot_secs`).
    pub async fn record_snapshots(
        &self,
        master: Option<&str>,
        snapshots: &[MetricSnapshot],
    ) -> Result<(), FleetError> {
        self.store.insert_metrics(master, snapshots).await
    }

    /// Chart series for every queue and job type over `period`.
    pub async fn metrics(&self, period: MetricsPeriod) -> Result<Vec<MetricSeries>, FleetError> {
        let now = self.now();
        let rows = self
            .store
            .metrics((now - period.span()).timestamp_millis())
            .await?;
        Ok(crate::snapshot::series(period, now, &rows))
    }

    /// Deletes completed jobs older than `fleet.trim.recent_minutes`,
    /// snapshots older than `fleet.trim.metrics_days` and masters that
    /// stopped reporting an hour ago.
    pub async fn trim(&self) -> Result<(), FleetError> {
        let now = self.now();
        let minutes = i64::try_from(self.options.trim.recent_minutes).unwrap_or(i64::MAX / 60_000);
        let days = i64::try_from(self.options.trim.metrics_days).unwrap_or(i64::MAX / 86_400_000);
        self.store
            .delete_jobs_before((now - TimeDelta::minutes(minutes)).timestamp_millis())
            .await?;
        self.store
            .delete_metrics_before((now - TimeDelta::days(days)).timestamp_millis())
            .await?;
        self.store
            .delete_masters_before((now - TimeDelta::hours(1)).timestamp_millis())
            .await?;
        Ok(())
    }

    /// `payload` with values whose names contain a `log.redact` fragment
    /// replaced by `[redacted]`.
    pub fn redact(&self, payload: &Value) -> Value {
        self.redactor.redact(payload)
    }
}

/// Running, paused (every master's supervisors paused) or inactive (none).
fn status_of(masters: &[MasterReport]) -> FleetStatus {
    if masters.is_empty() {
        FleetStatus::Inactive
    } else if masters.iter().all(|m| m.status == FleetStatus::Paused) {
        FleetStatus::Paused
    } else {
        FleetStatus::Running
    }
}
