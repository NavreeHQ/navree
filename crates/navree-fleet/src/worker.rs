//! One Fleet worker: `Queue::work_supervised` with its supervisor's
//! concurrency and limits, paused and stopped by its master, reporting its
//! jobs, its job count and its memory.

use std::io::{BufRead, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use navree_kernel::{AppContext, BoxError};
use navree_queue::{Attempt, Queue, Supervision, WorkHooks, WorkOptions};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::protocol::{ExitReason, WorkerCommand, WorkerReport, WorkerSpec};
use crate::recorder::JobRecorder;

/// Pauses the worker and counts its jobs.
#[derive(Debug, Default)]
struct FleetHooks {
    paused: AtomicBool,
    jobs: AtomicU64,
}

impl WorkHooks for FleetHooks {
    fn may_reserve(&self) -> bool {
        !self.paused.load(Ordering::SeqCst)
    }

    fn attempted(&self, _attempt: &Attempt) {
        self.jobs.fetch_add(1, Ordering::Relaxed);
    }
}

/// The resident memory of this process, in MB.
pub(crate) fn resident_memory_mb() -> Option<u64> {
    memory_stats::memory_stats().map(|stats| (stats.physical_mem / (1024 * 1024)) as u64)
}

/// Runs one worker until it is stopped, reaches a limit or fails; returns
/// why it ended. `measure_memory` is false for in-process workers, whose
/// process memory is not theirs.
pub(crate) async fn run_worker(
    ctx: AppContext,
    spec: WorkerSpec,
    mut commands: mpsc::UnboundedReceiver<WorkerCommand>,
    reports: mpsc::UnboundedSender<WorkerReport>,
    measure_memory: bool,
) -> ExitReason {
    let queue = match ctx.get::<Queue>().await {
        Ok(queue) => queue,
        Err(error) => {
            let reason = ExitReason::Crashed {
                error: error.to_string(),
            };
            let _ = reports.send(WorkerReport::Exiting {
                reason: reason.clone(),
            });
            return reason;
        }
    };
    let recorder = ctx.try_get::<JobRecorder>().await;
    if let Some(recorder) = &recorder {
        recorder.arm();
    }
    let hooks = Arc::new(FleetHooks::default());
    hooks.paused.store(spec.paused, Ordering::SeqCst);
    let stop = CancellationToken::new();
    let options = WorkOptions {
        queues: spec.queues.clone(),
        max_jobs: (spec.max_jobs > 0).then_some(spec.max_jobs),
        ..WorkOptions::default()
    };
    let supervision = Supervision {
        workers: Some(spec.concurrency.max(1)),
        stop: stop.clone(),
        hooks: hooks.clone(),
    };
    let work = {
        let queue = queue.clone();
        tokio::spawn(async move { queue.work_supervised(options, supervision).await })
    };
    tokio::pin!(work);

    let far = Duration::from_secs(60 * 60 * 24 * 365);
    let deadline = Instant::now()
        + if spec.max_time_secs > 0 {
            Duration::from_secs(spec.max_time_secs)
        } else {
            far
        };
    let max_time = tokio::time::sleep_until(deadline);
    tokio::pin!(max_time);
    let mut ticks = tokio::time::interval(spec.heartbeat());
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut reason: Option<ExitReason> = None;
    let mut listening = true;
    let report = |jobs: &[crate::CompletedJob]| {
        if !jobs.is_empty() {
            let _ = reports.send(WorkerReport::Jobs {
                jobs: jobs.to_vec(),
            });
        }
    };

    let result = loop {
        tokio::select! {
            result = &mut work => break result,
            command = commands.recv(), if listening => match command {
                Some(WorkerCommand::Pause) => hooks.paused.store(true, Ordering::SeqCst),
                Some(WorkerCommand::Continue) => hooks.paused.store(false, Ordering::SeqCst),
                Some(WorkerCommand::Stop) | None => {
                    // A closed channel (the master went away) stops too.
                    listening = command.is_some();
                    reason.get_or_insert(ExitReason::Stopped);
                    stop.cancel();
                }
            },
            () = &mut max_time, if reason.is_none() && spec.max_time_secs > 0 => {
                reason = Some(ExitReason::MaxTime);
                stop.cancel();
            }
            _ = ticks.tick() => {
                if let Some(recorder) = &recorder {
                    report(&recorder.drain());
                }
                let memory_mb = if measure_memory { resident_memory_mb() } else { None };
                let _ = reports.send(WorkerReport::Heartbeat {
                    jobs: hooks.jobs.load(Ordering::Relaxed),
                    memory_mb,
                });
                if reason.is_none()
                    && spec.max_memory_mb > 0
                    && memory_mb.is_some_and(|mb| mb > spec.max_memory_mb)
                {
                    reason = Some(ExitReason::MaxMemory);
                    stop.cancel();
                }
            }
        }
    };
    if let Some(recorder) = &recorder {
        report(&recorder.drain());
    }
    let _ = reports.send(WorkerReport::Heartbeat {
        jobs: hooks.jobs.load(Ordering::Relaxed),
        memory_mb: if measure_memory {
            resident_memory_mb()
        } else {
            None
        },
    });
    let reason = match result {
        Ok(Ok(_stats)) => reason.unwrap_or(if ctx.shutdown_token().is_cancelled() {
            ExitReason::Stopped
        } else {
            ExitReason::MaxJobs
        }),
        Ok(Err(error)) => ExitReason::Crashed {
            error: error.to_string(),
        },
        Err(join) => ExitReason::Crashed {
            error: format!("the worker task failed: {join}"),
        },
    };
    let _ = reports.send(WorkerReport::Exiting {
        reason: reason.clone(),
    });
    reason
}

/// The `fleet:work` command: a worker process started by a master. It
/// reads commands from stdin (one word per line; end of input stops it)
/// and writes `@fleet` report lines to stdout.
pub(crate) async fn work_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let spec = WorkerSpec::from_args(&args)?;
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    // A plain thread, not the runtime's blocking pool: a read blocked on
    // stdin must never hold up the process exit.
    std::thread::Builder::new()
        .name("fleet-stdin".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                if let Some(command) = WorkerCommand::parse(&line)
                    && command_tx.send(command).is_err()
                {
                    return;
                }
            }
            // End of input: the master is gone; dropping the sender stops
            // the worker.
        })?;
    let (report_tx, mut report_rx) = mpsc::unbounded_channel::<WorkerReport>();
    let writer = tokio::spawn(async move {
        while let Some(report) = report_rx.recv().await {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{}", report.to_line());
            let _ = out.flush();
        }
    });
    let _ = report_tx.send(WorkerReport::Started {
        pid: std::process::id(),
    });
    tracing::info!(
        target: "navree::fleet",
        supervisor = %spec.supervisor,
        queues = %spec.queue_key(),
        concurrency = spec.concurrency,
        "fleet worker started"
    );
    let reason = run_worker(ctx, spec, command_rx, report_tx, true).await;
    let _ = writer.await;
    match reason {
        ExitReason::Crashed { error } => Err(error.into()),
        _ => Ok(()),
    }
}
