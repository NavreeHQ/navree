//! What a master and its workers say to each other: [`WorkerSpec`] (how a
//! worker is started), [`WorkerCommand`] (master to worker, one word per
//! line on the worker's stdin) and [`WorkerReport`] (worker to master,
//! `@fleet <json>` lines on its stdout).

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::record::CompletedJob;

/// The prefix of a worker's report lines on its stdout; every other line
/// is the worker's own output, which the master prints as it is.
pub const REPORT_PREFIX: &str = "@fleet ";

/// How often a worker reports its job count and memory by default.
pub const WORKER_HEARTBEAT: Duration = Duration::from_secs(2);

/// How one worker process runs: the arguments of `fleet:work`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    /// Its supervisor.
    pub supervisor: String,
    /// The queues it works, in priority order.
    pub queues: Vec<String>,
    /// Jobs it runs at once.
    pub concurrency: usize,
    /// Jobs before it exits to be recycled (0: no limit).
    pub max_jobs: u64,
    /// Resident memory in MB above which it exits to be recycled (0: no
    /// limit).
    pub max_memory_mb: u64,
    /// Seconds after which it exits to be recycled (0: no limit).
    pub max_time_secs: u64,
    /// Starts paused: takes no job until told to continue.
    pub paused: bool,
    /// How often it reports, in milliseconds.
    pub heartbeat_ms: u64,
}

impl WorkerSpec {
    /// The queues as one comma-separated key (`--queue`).
    pub fn queue_key(&self) -> String {
        self.queues.join(",")
    }

    /// The `fleet:work` arguments for this spec.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "--supervisor".to_owned(),
            self.supervisor.clone(),
            "--queue".to_owned(),
            self.queue_key(),
            "--concurrency".to_owned(),
            self.concurrency.to_string(),
            "--max-jobs".to_owned(),
            self.max_jobs.to_string(),
            "--max-memory-mb".to_owned(),
            self.max_memory_mb.to_string(),
            "--max-time-secs".to_owned(),
            self.max_time_secs.to_string(),
            "--heartbeat-ms".to_owned(),
            self.heartbeat_ms.to_string(),
        ];
        if self.paused {
            args.push("--paused".to_owned());
        }
        args
    }

    /// Parses `fleet:work` arguments (`--flag value` or `--flag=value`).
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let mut spec = WorkerSpec {
            supervisor: String::new(),
            queues: Vec::new(),
            concurrency: 1,
            max_jobs: 0,
            max_memory_mb: 0,
            max_time_secs: 0,
            paused: false,
            heartbeat_ms: u64::try_from(WORKER_HEARTBEAT.as_millis()).unwrap_or(2000),
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_owned())),
                None => (arg.as_str(), None),
            };
            let mut value = || {
                inline
                    .clone()
                    .or_else(|| args.next().cloned())
                    .ok_or_else(|| format!("`{flag}` needs a value"))
            };
            let number = |text: String| {
                text.parse::<u64>()
                    .map_err(|_| format!("`{flag}` must be a whole number, not `{text}`"))
            };
            match flag {
                "--supervisor" => spec.supervisor = value()?,
                "--queue" => {
                    spec.queues = value()?
                        .split(',')
                        .map(|q| q.trim().to_owned())
                        .filter(|q| !q.is_empty())
                        .collect();
                }
                "--concurrency" => {
                    spec.concurrency = usize::try_from(number(value()?)?)
                        .unwrap_or(usize::MAX)
                        .max(1);
                }
                "--max-jobs" => spec.max_jobs = number(value()?)?,
                "--max-memory-mb" => spec.max_memory_mb = number(value()?)?,
                "--max-time-secs" => spec.max_time_secs = number(value()?)?,
                "--heartbeat-ms" => spec.heartbeat_ms = number(value()?)?.max(10),
                "--paused" => spec.paused = true,
                other => {
                    return Err(format!(
                        "unknown option `{other}` for fleet:work (expected --supervisor, --queue, --concurrency, --max-jobs, --max-memory-mb, --max-time-secs, --heartbeat-ms or --paused)"
                    ));
                }
            }
        }
        if spec.queues.is_empty() {
            return Err("fleet:work needs `--queue <a,b>`".to_owned());
        }
        Ok(spec)
    }

    /// [`heartbeat_ms`](WorkerSpec::heartbeat_ms) as a duration.
    pub fn heartbeat(&self) -> Duration {
        Duration::from_millis(self.heartbeat_ms.max(10))
    }
}

/// What a master tells a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerCommand {
    /// Take no new job (the supervisor is paused).
    Pause,
    /// Take jobs again.
    Continue,
    /// Finish the running jobs and exit.
    Stop,
}

impl WorkerCommand {
    /// The stdin line: `pause`, `continue` or `stop`.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkerCommand::Pause => "pause",
            WorkerCommand::Continue => "continue",
            WorkerCommand::Stop => "stop",
        }
    }

    /// Parses a stdin line.
    pub fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "pause" => Some(WorkerCommand::Pause),
            "continue" => Some(WorkerCommand::Continue),
            "stop" => Some(WorkerCommand::Stop),
            _ => None,
        }
    }
}

/// Why a worker exited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ExitReason {
    /// It was told to stop (or its app shut down).
    Stopped,
    /// It ran `max_jobs` jobs (recycled).
    MaxJobs,
    /// It used more than `max_memory_mb` (recycled).
    MaxMemory,
    /// It ran for `max_time_secs` (recycled).
    MaxTime,
    /// It failed or exited unexpectedly.
    Crashed {
        /// What happened.
        error: String,
    },
}

impl ExitReason {
    /// Whether the worker reached one of its limits and should be replaced
    /// at once.
    pub fn is_recycle(&self) -> bool {
        matches!(
            self,
            ExitReason::MaxJobs | ExitReason::MaxMemory | ExitReason::MaxTime
        )
    }
}

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitReason::Stopped => f.write_str("stopped"),
            ExitReason::MaxJobs => f.write_str("reached max_jobs"),
            ExitReason::MaxMemory => f.write_str("reached max_memory_mb"),
            ExitReason::MaxTime => f.write_str("reached max_time_secs"),
            ExitReason::Crashed { error } => write!(f, "crashed: {error}"),
        }
    }
}

/// What a worker tells its master.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerReport {
    /// Its process id, once, when it starts.
    Started {
        /// The worker's process id.
        pid: u32,
    },
    /// Every heartbeat: jobs run so far and resident memory.
    Heartbeat {
        /// Jobs it has run.
        jobs: u64,
        /// Resident memory in MB (none when it cannot be measured).
        memory_mb: Option<u64>,
    },
    /// Attempts that finished since the previous report.
    Jobs {
        /// The attempts.
        jobs: Vec<CompletedJob>,
    },
    /// It is about to exit, and why.
    Exiting {
        /// Why.
        #[serde(flatten)]
        reason: ExitReason,
    },
    /// It exited (the last report of every worker, sent by its spawner).
    Exited {
        /// Why.
        #[serde(flatten)]
        reason: ExitReason,
    },
}

impl WorkerReport {
    /// The stdout line of this report.
    pub fn to_line(&self) -> String {
        format!(
            "{REPORT_PREFIX}{}",
            serde_json::to_string(self).unwrap_or_default()
        )
    }

    /// The report on a stdout line, if it is one.
    pub fn from_line(line: &str) -> Option<Self> {
        line.trim_end()
            .strip_prefix(REPORT_PREFIX)
            .and_then(|json| serde_json::from_str(json).ok())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use navree_queue::AttemptOutcome;

    use super::*;

    fn spec() -> WorkerSpec {
        WorkerSpec {
            supervisor: "default".into(),
            queues: vec!["mail".into(), "default".into()],
            concurrency: 3,
            max_jobs: 1000,
            max_memory_mb: 256,
            max_time_secs: 3600,
            paused: true,
            heartbeat_ms: 500,
        }
    }

    #[test]
    fn specs_round_trip_through_arguments() {
        let spec = spec();
        assert_eq!(WorkerSpec::from_args(&spec.to_args()).unwrap(), spec);
        let args: Vec<String> = ["--queue=a", "--max-jobs=5"].iter().map(|s| (*s).to_owned()).collect();
        let parsed = WorkerSpec::from_args(&args).unwrap();
        assert_eq!(parsed.queues, ["a"]);
        assert_eq!(parsed.max_jobs, 5);
        assert!(!parsed.paused);
    }

    #[test]
    fn bad_worker_arguments_are_rejected() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(WorkerSpec::from_args(&args(&[])).unwrap_err().contains("--queue"));
        assert!(WorkerSpec::from_args(&args(&["--queue", "a", "--bogus"])).unwrap_err().contains("--bogus"));
        assert!(WorkerSpec::from_args(&args(&["--queue", "a", "--max-jobs", "x"])).unwrap_err().contains("--max-jobs"));
    }

    #[test]
    fn reports_round_trip_through_lines() {
        let reports = [
            WorkerReport::Started { pid: 7 },
            WorkerReport::Heartbeat { jobs: 3, memory_mb: Some(40) },
            WorkerReport::Jobs {
                jobs: vec![CompletedJob {
                    job_id: "j".into(),
                    queue: "mail".into(),
                    job_type: "mail.send".into(),
                    attempt: 1,
                    outcome: AttemptOutcome::Succeeded,
                    runtime_ms: 12,
                    finished_at: Utc::now(),
                }],
            },
            WorkerReport::Exiting { reason: ExitReason::MaxJobs },
            WorkerReport::Exited { reason: ExitReason::Crashed { error: "boom".into() } },
        ];
        for report in reports {
            let line = report.to_line();
            assert!(line.starts_with("@fleet {"), "{line}");
            assert_eq!(WorkerReport::from_line(&line), Some(report));
        }
        assert_eq!(WorkerReport::from_line("INFO job succeeded"), None);
    }

    #[test]
    fn commands_are_single_words() {
        for command in [WorkerCommand::Pause, WorkerCommand::Continue, WorkerCommand::Stop] {
            assert_eq!(WorkerCommand::parse(command.as_str()), Some(command));
        }
        assert_eq!(WorkerCommand::parse(" stop\r\n"), Some(WorkerCommand::Stop));
        assert_eq!(WorkerCommand::parse("halt"), None);
        assert!(ExitReason::MaxTime.is_recycle());
        assert!(!ExitReason::Stopped.is_recycle());
    }
}
