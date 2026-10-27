//! Test helpers: [`FakeSpawner`], a [`Spawner`] whose workers are puppets
//! the test drives, for checking what a [`Master`](crate::Master) decides.

use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::BoxFuture;
use tokio::sync::mpsc;

use crate::FleetError;
use crate::protocol::{ExitReason, WorkerCommand, WorkerReport, WorkerSpec};
use crate::spawner::{Spawner, WorkerHandle};

/// One fake worker.
#[derive(Debug)]
struct FakeWorker {
    spec: WorkerSpec,
    commands: Arc<Mutex<Vec<WorkerCommand>>>,
    reports: mpsc::UnboundedSender<WorkerReport>,
    exited: Arc<Mutex<bool>>,
}

/// A [`Spawner`] that starts no process: each worker records the commands
/// it receives and exits (as [`ExitReason::Stopped`]) when told to stop.
/// The test makes workers report, crash or reach a limit with
/// [`report`](FakeSpawner::report) and [`exit`](FakeSpawner::exit).
///
/// ```
/// # use std::sync::Arc;
/// use navree_fleet::testing::FakeSpawner;
///
/// let spawner = Arc::new(FakeSpawner::new());
/// // master.with_spawner(spawner.clone()) ...
/// assert_eq!(spawner.spawned().len(), 0);
/// ```
#[derive(Debug, Default)]
pub struct FakeSpawner {
    workers: Mutex<Vec<FakeWorker>>,
    failures: Mutex<u32>,
}

impl FakeSpawner {
    /// A spawner with no workers yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn workers(&self) -> std::sync::MutexGuard<'_, Vec<FakeWorker>> {
        self.workers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The spec of every worker started so far, in order (index = worker
    /// number).
    pub fn spawned(&self) -> Vec<WorkerSpec> {
        self.workers().iter().map(|w| w.spec.clone()).collect()
    }

    /// The numbers of the workers that have not exited.
    pub fn running(&self) -> Vec<usize> {
        self.workers()
            .iter()
            .enumerate()
            .filter(|(_, w)| !*w.exited.lock().unwrap_or_else(PoisonError::into_inner))
            .map(|(i, _)| i)
            .collect()
    }

    /// The specs of the workers that have not exited.
    pub fn running_specs(&self) -> Vec<WorkerSpec> {
        let running = self.running();
        let workers = self.workers();
        running.iter().map(|i| workers[*i].spec.clone()).collect()
    }

    /// The commands worker `index` received.
    pub fn commands(&self, index: usize) -> Vec<WorkerCommand> {
        self.workers()[index]
            .commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Worker `index` reports `report`.
    pub fn report(&self, index: usize, report: WorkerReport) {
        let _ = self.workers()[index].reports.send(report);
    }

    /// Worker `index` exits with `reason`.
    pub fn exit(&self, index: usize, reason: ExitReason) {
        let workers = self.workers();
        let worker = &workers[index];
        let mut exited = worker.exited.lock().unwrap_or_else(PoisonError::into_inner);
        if !*exited {
            *exited = true;
            let _ = worker.reports.send(WorkerReport::Exited { reason });
        }
    }

    /// The next `count` spawns fail.
    pub fn fail_next(&self, count: u32) {
        *self.failures.lock().unwrap_or_else(PoisonError::into_inner) = count;
    }
}

impl Spawner for FakeSpawner {
    fn spawn(&self, spec: WorkerSpec) -> BoxFuture<'_, Result<WorkerHandle, FleetError>> {
        Box::pin(async move {
            {
                let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
                if *failures > 0 {
                    *failures -= 1;
                    return Err(FleetError::Spawn(
                        "the fake spawner was told to fail".to_owned(),
                    ));
                }
            }
            let (command_tx, mut command_rx) = mpsc::unbounded_channel();
            let (report_tx, report_rx) = mpsc::unbounded_channel();
            let commands = Arc::new(Mutex::new(Vec::new()));
            let exited = Arc::new(Mutex::new(false));
            {
                let commands = commands.clone();
                let exited = exited.clone();
                let reports = report_tx.clone();
                tokio::spawn(async move {
                    loop {
                        let command = command_rx.recv().await;
                        if let Some(command) = command {
                            commands
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push(command);
                        }
                        if matches!(command, Some(WorkerCommand::Stop) | None) {
                            let mut exited = exited.lock().unwrap_or_else(PoisonError::into_inner);
                            if !*exited {
                                *exited = true;
                                let _ = reports.send(WorkerReport::Exited {
                                    reason: ExitReason::Stopped,
                                });
                            }
                            return;
                        }
                    }
                });
            }
            self.workers().push(FakeWorker {
                spec,
                commands,
                reports: report_tx,
                exited,
            });
            Ok(WorkerHandle {
                pid: None,
                commands: command_tx,
                reports: report_rx,
            })
        })
    }
}
