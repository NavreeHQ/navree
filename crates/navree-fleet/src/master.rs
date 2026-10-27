//! [`Master`]: the `fleet` process. It runs no jobs itself: it keeps each
//! supervisor's worker processes running, balanced, paused or continued,
//! recycled and restarted; it reports its heartbeat, stores what its
//! workers ran and takes metric snapshots; and it raises long-wait alerts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_events::EventBus;
use navree_kernel::{AppContext, BoxError};
use navree_queue::QueueDriver;
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::balance::{Allocation, initial_allocation, next_allocation};
use crate::protocol::{ExitReason, WORKER_HEARTBEAT, WorkerCommand, WorkerReport, WorkerSpec};
use crate::record::{CompletedJob, MasterReport, ProcessReport, SupervisorReport};
use crate::snapshot::snapshots;
use crate::spawner::{ProcessSpawner, Spawner};
use crate::{Balance, Fleet, FleetError, FleetStatus, Supervisor};

/// Failures of one queue's workers within this window that are logged as
/// an error.
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);

/// How many failures within [`CRASH_WINDOW`] are logged as an error under
/// `navree::fleet`.
pub const CRASH_LIMIT: usize = 5;

/// How a master paces itself. The defaults suit production; tests shorten
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MasterSettings {
    /// The name the master reports (default: the host name).
    pub name: String,
    /// How often it checks for commands (pause, terminate) and starts or
    /// stops processes (default 1 s).
    pub tick: Duration,
    /// How often it stores its heartbeat and its workers' jobs and checks
    /// for long waits (default 3 s).
    pub heartbeat: Duration,
    /// How often workers report (default 2 s).
    pub worker_heartbeat: Duration,
    /// The first restart delay of a worker that failed (default 1 s); it
    /// doubles while the worker keeps failing.
    pub restart_delay: Duration,
    /// The longest restart delay (default 60 s).
    pub max_restart_delay: Duration,
}

impl Default for MasterSettings {
    fn default() -> Self {
        Self {
            name: host_name(),
            tick: Duration::from_secs(1),
            heartbeat: Duration::from_secs(3),
            worker_heartbeat: WORKER_HEARTBEAT,
            restart_delay: Duration::from_secs(1),
            max_restart_delay: Duration::from_secs(60),
        }
    }
}

/// The host name, from `HOSTNAME`, `COMPUTERNAME` or `/etc/hostname`.
pub fn host_name() -> String {
    ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|var| std::env::var(var).ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "fleet".to_owned())
}

/// What a master did, returned when it stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MasterSummary {
    /// Workers started.
    pub started: u64,
    /// Workers replaced after reaching a limit.
    pub recycled: u64,
    /// Workers that failed.
    pub crashed: u64,
    /// Job attempts its workers reported.
    pub jobs: u64,
}

/// The `fleet` process: see the [module docs](self).
///
/// ```no_run
/// # async fn demo(ctx: navree_kernel::AppContext) -> Result<(), navree_kernel::BoxError> {
/// use navree_fleet::Master;
///
/// let master = Master::from_app(&ctx).await?;
/// master.run(ctx.shutdown_token()).await?;
/// # Ok(())
/// # }
/// ```
pub struct Master {
    fleet: Arc<Fleet>,
    events: Option<Arc<EventBus>>,
    spawner: Arc<dyn Spawner>,
    settings: MasterSettings,
    supervisors: Vec<Supervisor>,
    id: String,
}

impl fmt::Debug for Master {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Master")
            .field("id", &self.id)
            .field("settings", &self.settings)
            .field("supervisors", &self.supervisors)
            .finish_non_exhaustive()
    }
}

impl Master {
    /// A master for this server's supervisors (`fleet.only`), starting
    /// workers as child processes of the running binary. Fails on the
    /// `memory` queue driver.
    pub fn new(fleet: Arc<Fleet>) -> Result<Self, FleetError> {
        if fleet.queue().options().driver == QueueDriver::Memory {
            return Err(FleetError::MemoryDriver);
        }
        let supervisors = fleet.local_supervisors()?;
        Ok(Self {
            fleet,
            events: None,
            spawner: Arc::new(ProcessSpawner::current_exe()?),
            settings: MasterSettings::default(),
            supervisors,
            id: Uuid::now_v7().to_string(),
        })
    }

    /// [`new`](Master::new) with the app's [`Fleet`] and, when the app has
    /// `EventsModule`, its event bus for long-wait events.
    pub async fn from_app(ctx: &AppContext) -> Result<Self, BoxError> {
        let fleet = ctx.get::<Fleet>().await?;
        let events = ctx.try_get::<EventBus>().await;
        Ok(Self::new(fleet)?.with_events(events))
    }

    /// Starts workers with `spawner` instead of child processes.
    #[must_use]
    pub fn with_spawner(mut self, spawner: Arc<dyn Spawner>) -> Self {
        self.spawner = spawner;
        self
    }

    /// Paces the master with `settings`.
    #[must_use]
    pub fn with_settings(mut self, settings: MasterSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Emits long-wait events on `events`.
    #[must_use]
    pub fn with_events(mut self, events: Option<Arc<EventBus>>) -> Self {
        self.events = events;
        self
    }

    /// This run's unique id (the `fleet_masters` row).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The name it reports.
    pub fn name(&self) -> &str {
        &self.settings.name
    }

    /// The supervisors it runs.
    pub fn supervisors(&self) -> &[Supervisor] {
        &self.supervisors
    }

    /// Runs until `stop` is cancelled or `fleet:terminate` is asked for,
    /// then stops every worker gracefully (each finishes its running jobs;
    /// there is no time limit) and removes its heartbeat.
    pub async fn run(self, stop: CancellationToken) -> Result<MasterSummary, FleetError> {
        let mut run = Run::new(self).await?;
        let result = run.run(stop).await;
        // Stop whatever is still running, even after an error.
        run.stop_all();
        while !run.procs.is_empty() {
            match run.reports.recv().await {
                Some((id, report)) => run.handle(id, report).await,
                None => break,
            }
        }
        if result.is_ok() {
            run.flush().await?;
            run.snapshot().await?;
        }
        run.master
            .fleet
            .store()
            .delete_master(&run.master.id)
            .await?;
        result.map(|()| run.summary)
    }
}

/// One worker process as the master tracks it.
struct Proc {
    supervisor: usize,
    key: String,
    pid: Option<u32>,
    commands: mpsc::UnboundedSender<WorkerCommand>,
    started: Instant,
    started_at: DateTime<Utc>,
    jobs: u64,
    memory_mb: Option<u64>,
    stopping: bool,
}

/// Restart pacing of one queue key.
#[derive(Default)]
struct Failures {
    consecutive: u32,
    next_spawn: Option<Instant>,
    recent: VecDeque<Instant>,
    reported: bool,
}

/// A supervisor's live state.
struct SupervisorState {
    supervisor: Supervisor,
    allocation: Allocation,
    paused: bool,
    failures: BTreeMap<String, Failures>,
}

/// A running master.
struct Run {
    master: Master,
    started_at: DateTime<Utc>,
    terminate_token: Option<String>,
    states: Vec<SupervisorState>,
    procs: BTreeMap<u64, Proc>,
    next_id: u64,
    reports_tx: mpsc::UnboundedSender<(u64, WorkerReport)>,
    reports: mpsc::UnboundedReceiver<(u64, WorkerReport)>,
    /// Finished attempts not stored yet.
    unsaved: Vec<CompletedJob>,
    /// Finished attempts since the last snapshot.
    window: Vec<CompletedJob>,
    stopping: bool,
    summary: MasterSummary,
}

impl Run {
    async fn new(master: Master) -> Result<Self, FleetError> {
        let paused = master.fleet.paused().await?;
        let states = master
            .supervisors
            .iter()
            .map(|supervisor| SupervisorState {
                allocation: initial_allocation(supervisor),
                paused: paused.contains(&supervisor.name),
                supervisor: supervisor.clone(),
                failures: BTreeMap::new(),
            })
            .collect();
        let (reports_tx, reports) = mpsc::unbounded_channel();
        Ok(Self {
            started_at: master.fleet.now(),
            terminate_token: master.fleet.terminate_token().await?,
            master,
            states,
            procs: BTreeMap::new(),
            next_id: 0,
            reports_tx,
            reports,
            unsaved: Vec::new(),
            window: Vec::new(),
            stopping: false,
            summary: MasterSummary::default(),
        })
    }

    async fn run(&mut self, stop: CancellationToken) -> Result<(), FleetError> {
        let settings = self.master.settings.clone();
        tracing::info!(
            target: "navree::fleet",
            master = %self.master.id,
            name = %settings.name,
            supervisors = self.states.len(),
            "fleet master started"
        );
        self.reconcile().await;
        self.heartbeat().await?;
        let mut ticks = tokio::time::interval(settings.tick);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let now = Instant::now();
        let mut last_heartbeat = now;
        let mut last_balance = now;
        let mut last_snapshot = now;
        let cooldown = self.master.fleet.options().balance_cooldown();
        let snapshot_every = self.master.fleet.options().snapshot_interval();
        loop {
            if self.stopping && self.procs.is_empty() {
                return Ok(());
            }
            tokio::select! {
                () = stop.cancelled(), if !self.stopping => {
                    tracing::info!(target: "navree::fleet", master = %self.master.id, "fleet master stopping");
                    self.stop_all();
                }
                Some((id, report)) = self.reports.recv() => {
                    self.handle(id, report).await;
                }
                _ = ticks.tick() => {
                    if self.stopping {
                        continue;
                    }
                    self.check_commands().await?;
                    if self.stopping {
                        continue;
                    }
                    if last_balance.elapsed() >= cooldown {
                        last_balance = Instant::now();
                        self.balance().await?;
                    }
                    self.reconcile().await;
                    if last_heartbeat.elapsed() >= settings.heartbeat {
                        last_heartbeat = Instant::now();
                        self.heartbeat().await?;
                        self.flush().await?;
                        self.long_waits().await?;
                    }
                    if last_snapshot.elapsed() >= snapshot_every {
                        last_snapshot = Instant::now();
                        self.snapshot().await?;
                        self.master.fleet.trim().await?;
                    }
                }
            }
        }
    }

    /// Pause, continue and terminate, from the shared state.
    async fn check_commands(&mut self) -> Result<(), FleetError> {
        let token = self.master.fleet.terminate_token().await?;
        if token.is_some() && token != self.terminate_token {
            tracing::info!(target: "navree::fleet", master = %self.master.id, "fleet:terminate asked; stopping");
            self.stop_all();
            return Ok(());
        }
        let paused: BTreeSet<String> = self.master.fleet.paused().await?;
        for (index, state) in self.states.iter_mut().enumerate() {
            let now_paused = paused.contains(&state.supervisor.name);
            if now_paused == state.paused {
                continue;
            }
            state.paused = now_paused;
            let command = if now_paused {
                WorkerCommand::Pause
            } else {
                WorkerCommand::Continue
            };
            tracing::info!(
                target: "navree::fleet",
                supervisor = %state.supervisor.name,
                "supervisor {}",
                if now_paused { "paused" } else { "continued" }
            );
            for proc in self.procs.values().filter(|p| p.supervisor == index) {
                let _ = proc.commands.send(command);
            }
        }
        Ok(())
    }

    /// One `auto` balancing step per supervisor.
    async fn balance(&mut self) -> Result<(), FleetError> {
        let shift = self.master.fleet.options().balance_max_shift;
        for state in &mut self.states {
            if state.supervisor.balance != Balance::Auto {
                continue;
            }
            let workload = self
                .master
                .fleet
                .workload_of(&state.supervisor.queues, &[])
                .await?;
            let next = next_allocation(&state.supervisor, &state.allocation, &workload, shift);
            if next != state.allocation {
                tracing::debug!(
                    target: "navree::fleet",
                    supervisor = %state.supervisor.name,
                    allocation = ?next,
                    "rebalanced"
                );
                state.allocation = next;
            }
        }
        Ok(())
    }

    /// Starts and stops processes so each queue has its allocation.
    async fn reconcile(&mut self) {
        if self.stopping {
            return;
        }
        for index in 0..self.states.len() {
            let allocation = self.states[index].allocation.clone();
            for (key, wanted) in allocation {
                let live: Vec<u64> = self
                    .procs
                    .iter()
                    .filter(|(_, p)| p.supervisor == index && p.key == key && !p.stopping)
                    .map(|(id, _)| *id)
                    .collect();
                if live.len() > wanted {
                    // Stop the youngest first.
                    for id in live.iter().rev().take(live.len() - wanted) {
                        if let Some(proc) = self.procs.get_mut(id) {
                            proc.stopping = true;
                            let _ = proc.commands.send(WorkerCommand::Stop);
                        }
                    }
                } else {
                    for _ in live.len()..wanted {
                        let waiting = self.states[index]
                            .failures
                            .get(&key)
                            .and_then(|f| f.next_spawn)
                            .is_some_and(|at| at > Instant::now());
                        if waiting || !self.spawn(index, &key).await {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Starts one worker for `key` of supervisor `index`; false when it
    /// could not be started (counted as a failure).
    async fn spawn(&mut self, index: usize, key: &str) -> bool {
        let state = &self.states[index];
        let supervisor = &state.supervisor;
        let spec = WorkerSpec {
            supervisor: supervisor.name.clone(),
            queues: key.split(',').map(str::to_owned).collect(),
            concurrency: supervisor.concurrency,
            max_jobs: supervisor.max_jobs,
            max_memory_mb: supervisor.max_memory_mb,
            max_time_secs: supervisor.max_time_secs,
            paused: state.paused,
            heartbeat_ms: u64::try_from(self.master.settings.worker_heartbeat.as_millis())
                .unwrap_or(2000),
        };
        match self.master.spawner.spawn(spec).await {
            Ok(handle) => {
                let id = self.next_id;
                self.next_id += 1;
                let tx = self.reports_tx.clone();
                let mut reports = handle.reports;
                tokio::spawn(async move {
                    let mut exited = false;
                    while let Some(report) = reports.recv().await {
                        exited |= matches!(report, WorkerReport::Exited { .. });
                        if tx.send((id, report)).is_err() {
                            return;
                        }
                    }
                    if !exited {
                        let _ = tx.send((
                            id,
                            WorkerReport::Exited {
                                reason: ExitReason::Crashed {
                                    error: "the worker stopped reporting".to_owned(),
                                },
                            },
                        ));
                    }
                });
                self.procs.insert(
                    id,
                    Proc {
                        supervisor: index,
                        key: key.to_owned(),
                        pid: handle.pid,
                        commands: handle.commands,
                        started: Instant::now(),
                        started_at: self.master.fleet.now(),
                        jobs: 0,
                        memory_mb: None,
                        stopping: false,
                    },
                );
                self.summary.started += 1;
                true
            }
            Err(error) => {
                self.failed(index, key, Duration::ZERO, &error.to_string());
                false
            }
        }
    }

    async fn handle(&mut self, id: u64, report: WorkerReport) {
        match report {
            WorkerReport::Started { pid } => {
                if let Some(proc) = self.procs.get_mut(&id) {
                    proc.pid = Some(pid);
                }
            }
            WorkerReport::Heartbeat { jobs, memory_mb } => {
                if let Some(proc) = self.procs.get_mut(&id) {
                    proc.jobs = jobs;
                    proc.memory_mb = memory_mb.or(proc.memory_mb);
                }
            }
            WorkerReport::Jobs { jobs } => {
                self.summary.jobs += jobs.len() as u64;
                self.window.extend(jobs.iter().cloned());
                self.unsaved.extend(jobs);
            }
            WorkerReport::Exiting { .. } => {}
            WorkerReport::Exited { reason } => {
                let Some(proc) = self.procs.remove(&id) else {
                    return;
                };
                let name = self.states[proc.supervisor].supervisor.name.clone();
                if proc.stopping || self.stopping {
                    tracing::debug!(target: "navree::fleet", supervisor = %name, queue = %proc.key, "worker stopped");
                    return;
                }
                match reason {
                    ExitReason::Crashed { error } => {
                        self.failed(proc.supervisor, &proc.key, proc.started.elapsed(), &error);
                    }
                    reason => {
                        if reason.is_recycle() {
                            self.summary.recycled += 1;
                        }
                        tracing::info!(
                            target: "navree::fleet",
                            supervisor = %name,
                            queue = %proc.key,
                            jobs = proc.jobs,
                            "worker {reason}; starting a fresh one"
                        );
                        if let Some(f) = self.states[proc.supervisor].failures.get_mut(&proc.key) {
                            f.consecutive = 0;
                            f.next_spawn = None;
                        }
                        self.reconcile().await;
                    }
                }
            }
        }
    }

    /// A worker of `key` failed after running `lived`: back off and log.
    fn failed(&mut self, index: usize, key: &str, lived: Duration, error: &str) {
        self.summary.crashed += 1;
        let settings = &self.master.settings;
        let state = &mut self.states[index];
        let failures = state.failures.entry(key.to_owned()).or_default();
        failures.consecutive = if lived >= CRASH_WINDOW {
            1
        } else {
            failures.consecutive.saturating_add(1)
        };
        let factor = 2u32.saturating_pow(failures.consecutive.saturating_sub(1).min(16));
        let delay = settings
            .restart_delay
            .saturating_mul(factor)
            .min(settings.max_restart_delay);
        let now = Instant::now();
        failures.next_spawn = Some(now + delay);
        failures.recent.push_back(now);
        while failures
            .recent
            .front()
            .is_some_and(|at| now.duration_since(*at) > CRASH_WINDOW)
        {
            failures.recent.pop_front();
            failures.reported = false;
        }
        tracing::warn!(
            target: "navree::fleet",
            supervisor = %state.supervisor.name,
            queue = %key,
            restart_in_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            error,
            "worker failed"
        );
        if failures.recent.len() >= CRASH_LIMIT && !failures.reported {
            failures.reported = true;
            tracing::error!(
                target: "navree::fleet",
                supervisor = %state.supervisor.name,
                queue = %key,
                failures = failures.recent.len(),
                error,
                "worker failed {} times within a minute",
                failures.recent.len()
            );
        }
    }

    fn stop_all(&mut self) {
        self.stopping = true;
        for proc in self.procs.values_mut() {
            if !proc.stopping {
                proc.stopping = true;
                let _ = proc.commands.send(WorkerCommand::Stop);
            }
        }
    }

    fn report(&self) -> MasterReport {
        let supervisors: Vec<SupervisorReport> = self
            .states
            .iter()
            .enumerate()
            .map(|(index, state)| SupervisorReport {
                name: state.supervisor.name.clone(),
                queues: state.supervisor.queues.clone(),
                balance: state.supervisor.balance,
                max_processes: state.supervisor.max_processes,
                paused: state.paused,
                processes: self
                    .procs
                    .values()
                    .filter(|p| p.supervisor == index && !p.stopping)
                    .map(|p| ProcessReport {
                        queue: p.key.clone(),
                        pid: p.pid,
                        memory_mb: p.memory_mb,
                        jobs: p.jobs,
                        started_at: p.started_at,
                    })
                    .collect(),
            })
            .collect();
        let all_paused = !supervisors.is_empty() && supervisors.iter().all(|s| s.paused);
        MasterReport {
            id: self.master.id.clone(),
            name: self.master.settings.name.clone(),
            pid: std::process::id(),
            status: if all_paused {
                FleetStatus::Paused
            } else {
                FleetStatus::Running
            },
            supervisors,
            started_at: self.started_at,
            heartbeat_at: self.master.fleet.now(),
        }
    }

    async fn heartbeat(&self) -> Result<(), FleetError> {
        self.master.fleet.store().save_master(&self.report()).await
    }

    /// Stores the attempts reported since the last flush.
    async fn flush(&mut self) -> Result<(), FleetError> {
        if self.unsaved.is_empty() {
            return Ok(());
        }
        let jobs = std::mem::take(&mut self.unsaved);
        self.master.fleet.record(Some(&self.master.id), &jobs).await
    }

    /// Stores one snapshot per supervised queue and per job type that ran.
    async fn snapshot(&mut self) -> Result<(), FleetError> {
        let mut statuses = Vec::new();
        let mut seen = BTreeSet::new();
        for state in &self.states {
            for queue in &state.supervisor.queues {
                if seen.insert(queue.clone()) {
                    statuses.push(self.master.fleet.queue().status(queue).await?);
                }
            }
        }
        let jobs = std::mem::take(&mut self.window);
        let rows = snapshots(self.master.fleet.now(), &jobs, &statuses);
        self.master
            .fleet
            .record_snapshots(Some(&self.master.id), &rows)
            .await
    }

    /// Emits `LongWaitDetected` for each queue over its threshold, at most
    /// once per cooldown across masters.
    async fn long_waits(&self) -> Result<(), FleetError> {
        if self.master.fleet.options().long_wait.is_empty() {
            return Ok(());
        }
        for late in self.master.fleet.long_waits().await? {
            if !self.master.fleet.claim_long_wait(&late.queue).await? {
                continue;
            }
            tracing::warn!(
                target: "navree::fleet",
                queue = %late.queue,
                wait_secs = late.wait_secs,
                threshold_secs = late.threshold_secs,
                "queue is behind: its oldest job has waited longer than its threshold"
            );
            if let Some(events) = &self.master.events
                && let Err(error) = events.emit(late).await
            {
                tracing::warn!(target: "navree::fleet", %error, "a long-wait listener failed");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_name_is_never_empty() {
        assert!(!host_name().is_empty());
        let settings = MasterSettings::default();
        assert_eq!(settings.tick, Duration::from_secs(1));
        assert_eq!(settings.max_restart_delay, Duration::from_secs(60));
    }
}
