//! Starting workers: the [`Spawner`] trait, [`ProcessSpawner`] (child
//! processes of the app's own binary running `fleet:work`) and
//! [`InProcessSpawner`] (workers as tasks of the master's app, for tests
//! and single-process setups).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;

use navree_kernel::{AppContext, BoxFuture};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::FleetError;
use crate::protocol::{ExitReason, WorkerCommand, WorkerReport, WorkerSpec};
use crate::worker::run_worker;

/// A started worker: its process id, where to send it commands, and its
/// reports. The last report is always [`WorkerReport::Exited`]; a
/// receiver that closes without one means the worker was lost.
#[derive(Debug)]
pub struct WorkerHandle {
    /// The worker's process id (none for an in-process worker).
    pub pid: Option<u32>,
    /// Commands to the worker. Dropping it stops the worker too.
    pub commands: mpsc::UnboundedSender<WorkerCommand>,
    /// The worker's reports.
    pub reports: mpsc::UnboundedReceiver<WorkerReport>,
}

/// Starts workers for a master.
pub trait Spawner: Send + Sync + 'static {
    /// Starts one worker as `spec` describes.
    fn spawn(&self, spec: WorkerSpec) -> BoxFuture<'_, Result<WorkerHandle, FleetError>>;
}

/// Starts each worker as a child process of the app's own binary:
/// `<binary> fleet:work --supervisor .. --queue ..`, with stdin and stdout
/// piped for commands and reports and stderr inherited. Lines a worker
/// prints that are not reports (its logs) are printed by the master.
#[derive(Clone, Debug)]
pub struct ProcessSpawner {
    program: PathBuf,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
}

impl ProcessSpawner {
    /// Spawns `program` with `args` before `fleet:work` and its flags.
    pub fn new(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            envs: Vec::new(),
        }
    }

    /// Sets an environment variable for every worker (on top of the
    /// master's environment, which workers inherit).
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }

    /// The running binary (`std::env::current_exe`), with no extra
    /// arguments.
    pub fn current_exe() -> Result<Self, FleetError> {
        let program = std::env::current_exe().map_err(|error| {
            FleetError::Spawn(format!("cannot find the running binary: {error}"))
        })?;
        Ok(Self::new(program, Vec::<OsString>::new()))
    }
}

impl Spawner for ProcessSpawner {
    fn spawn(&self, spec: WorkerSpec) -> BoxFuture<'_, Result<WorkerHandle, FleetError>> {
        Box::pin(async move {
            let mut child = Command::new(&self.program)
                .args(&self.args)
                .envs(self.envs.iter().map(|(k, v)| (k, v)))
                .arg("fleet:work")
                .args(spec.to_args())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .kill_on_drop(false)
                .spawn()
                .map_err(|error| {
                    FleetError::Spawn(format!("{}: {error}", self.program.display()))
                })?;
            let pid = child.id();
            let mut stdin = child.stdin.take();
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| FleetError::Spawn("the worker has no stdout".to_owned()))?;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel::<WorkerCommand>();
            let (report_tx, report_rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                while let Some(command) = command_rx.recv().await {
                    let Some(pipe) = stdin.as_mut() else { break };
                    let line = format!("{}\n", command.as_str());
                    if pipe.write_all(line.as_bytes()).await.is_err() || pipe.flush().await.is_err()
                    {
                        break;
                    }
                }
                // Closing stdin stops the worker as well.
                drop(stdin);
            });
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                let mut said: Option<ExitReason> = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    match WorkerReport::from_line(&line) {
                        Some(WorkerReport::Exiting { reason }) => said = Some(reason),
                        Some(report) => {
                            let _ = report_tx.send(report);
                        }
                        None => println!("{line}"),
                    }
                }
                let reason = match child.wait().await {
                    Ok(status) if status.success() => said.unwrap_or(ExitReason::Stopped),
                    Ok(status) => ExitReason::Crashed {
                        error: match said {
                            Some(ExitReason::Crashed { error }) => error,
                            _ => format!("the worker exited with {status}"),
                        },
                    },
                    Err(error) => ExitReason::Crashed {
                        error: format!("cannot wait for the worker: {error}"),
                    },
                };
                let _ = report_tx.send(WorkerReport::Exited { reason });
            });
            Ok(WorkerHandle {
                pid,
                commands: command_tx,
                reports: report_rx,
            })
        })
    }
}

/// Runs each worker as a task of the master's own app (same process, same
/// database pool). For tests, and for development where one process is
/// enough. Workers report no memory.
#[derive(Clone, Debug)]
pub struct InProcessSpawner {
    ctx: AppContext,
}

impl InProcessSpawner {
    /// Workers on `ctx`'s app.
    pub fn new(ctx: AppContext) -> Self {
        Self { ctx }
    }
}

impl Spawner for InProcessSpawner {
    fn spawn(&self, spec: WorkerSpec) -> BoxFuture<'_, Result<WorkerHandle, FleetError>> {
        Box::pin(async move {
            let (command_tx, command_rx) = mpsc::unbounded_channel();
            let (report_tx, report_rx) = mpsc::unbounded_channel();
            let ctx = self.ctx.clone();
            tokio::spawn(async move {
                let (inner_tx, mut inner_rx) = mpsc::unbounded_channel();
                let forward = {
                    let report_tx = report_tx.clone();
                    tokio::spawn(async move {
                        while let Some(report) = inner_rx.recv().await {
                            if !matches!(report, WorkerReport::Exiting { .. }) {
                                let _ = report_tx.send(report);
                            }
                        }
                    })
                };
                let reason = run_worker(ctx, spec, command_rx, inner_tx, false).await;
                let _ = forward.await;
                let _ = report_tx.send(WorkerReport::Exited { reason });
            });
            Ok(WorkerHandle {
                pid: None,
                commands: command_tx,
                reports: report_rx,
            })
        })
    }
}
