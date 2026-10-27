//! [`QueueOptions`] (configuration section `queue`), [`QueueDriver`] and
//! [`WorkOptions`].

use std::time::Duration;

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer, Serialize};

/// Where queued jobs are stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueDriver {
    /// The `jobs` and `failed_jobs` tables of the app database
    /// (`"database"`, the default): jobs survive restarts and are shared by
    /// every process using the database. Needs `DatabaseModule`.
    #[default]
    Database,
    /// In-process memory (`"memory"`): jobs are lost when the process
    /// exits. For tests and single-process development.
    Memory,
}

/// `QueueModule` settings: configuration section `queue`.
///
/// ```toml
/// [queue]
/// driver = "database"          # or "memory"
/// default_queue = "default"    # where jobs with an empty queue name go
/// workers = 4                  # concurrent jobs per `queue:work` process
/// poll_interval_ms = 1000      # how long an idle worker waits before polling again
/// queues = ["high", "default"] # what `queue:work` processes, in priority order
/// ```
///
/// Every field has a default, so a missing section is valid.
/// `queues` may also be one comma-separated string
/// (`APP_QUEUE__QUEUES=high,default`); when empty, workers process
/// `default_queue`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueOptions {
    /// Where jobs are stored (default [`QueueDriver::Database`]).
    pub driver: QueueDriver,
    /// The queue of jobs dispatched with an empty queue name, and the queue
    /// workers process when [`queues`](QueueOptions::queues) is empty
    /// (default `default`, the queue [`JobOptions`](navree_contracts::JobOptions)
    /// names by default).
    pub default_queue: String,
    /// How many jobs one [`Queue::work`](crate::Queue::work) call runs at
    /// the same time (default 4).
    pub workers: usize,
    /// How long an idle worker waits before polling again, in milliseconds
    /// (default 1000).
    pub poll_interval_ms: u64,
    /// The queues workers process, highest priority first (default: just
    /// `default_queue`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub queues: Vec<String>,
    /// Operations: their routes, heartbeat and watchdog (section
    /// `queue.operations`).
    pub operations: OperationsOptions,
}

impl Default for QueueOptions {
    fn default() -> Self {
        Self {
            driver: QueueDriver::Database,
            default_queue: "default".to_owned(),
            workers: 4,
            poll_interval_ms: 1000,
            queues: Vec::new(),
            operations: OperationsOptions::default(),
        }
    }
}

/// Operations settings: configuration section `queue.operations`.
///
/// ```toml
/// [queue.operations]
/// routes = true                        # GET/DELETE /api/operations/{id}, the SSE stream
/// watchdog = true                      # sweep stale operations every minute (with ScheduleModule)
/// heartbeat_secs = 30                  # how often a running job's operation shows it is alive
/// stale_after_secs = 300               # no heartbeat for this long: the watchdog steps in
/// max_redispatches = 3                 # restarts of an idempotent operation's job
/// progress_interval_ms = 500           # ctx.progress writes at most this often per job
/// stream_poll_ms = 1000                # how often an SSE stream re-reads the operation
/// view_permission = "operations.view"  # see other users' operations
/// cancel_permission = "operations.cancel"
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OperationsOptions {
    /// Mounts the `/api/operations` routes (default true; they also need
    /// `DatabaseModule` and an authenticator).
    pub routes: bool,
    /// Schedules the watchdog ([`Operations::sweep`](crate::Operations::sweep))
    /// every minute, as task `queue.watchdog`, in apps with
    /// `ScheduleModule` (default true). Without `ScheduleModule`, run
    /// `queue:watchdog` every minute from system cron instead.
    pub watchdog: bool,
    /// How often the worker renews the heartbeat of a running job's
    /// operation, in seconds (default 30).
    pub heartbeat_secs: u64,
    /// A pending or running operation without a heartbeat for this many
    /// seconds is stale (default 300): see `queue:watchdog`.
    pub stale_after_secs: u64,
    /// How many times the watchdog dispatches the job of a stale
    /// idempotent operation again before failing it (default 3).
    pub max_redispatches: u32,
    /// `ctx.progress` writes to the database at most this often per job, in
    /// milliseconds (default 500); a report with `done == total` is always
    /// written.
    pub progress_interval_ms: u64,
    /// How often an SSE stream re-reads its operation, in milliseconds
    /// (default 1000). Changes made in the same process arrive at once.
    pub stream_poll_ms: u64,
    /// The permission that shows other users' operations (default
    /// `operations.view`), app-wide or in the operation's workspace.
    pub view_permission: String,
    /// The permission that cancels other users' operations (default
    /// `operations.cancel`).
    pub cancel_permission: String,
}

impl Default for OperationsOptions {
    fn default() -> Self {
        Self {
            routes: true,
            watchdog: true,
            heartbeat_secs: 30,
            stale_after_secs: 300,
            max_redispatches: 3,
            progress_interval_ms: 500,
            stream_poll_ms: 1000,
            view_permission: "operations.view".to_owned(),
            cancel_permission: "operations.cancel".to_owned(),
        }
    }
}

impl OperationsOptions {
    /// [`heartbeat_secs`](OperationsOptions::heartbeat_secs) as a `Duration`.
    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_secs.max(1))
    }

    /// [`progress_interval_ms`](OperationsOptions::progress_interval_ms) as
    /// a `Duration`.
    pub fn progress_interval(&self) -> Duration {
        Duration::from_millis(self.progress_interval_ms)
    }

    /// [`stream_poll_ms`](OperationsOptions::stream_poll_ms) as a
    /// `Duration`.
    pub fn stream_poll_interval(&self) -> Duration {
        Duration::from_millis(self.stream_poll_ms.max(1))
    }

    fn problems(&self) -> Vec<&'static str> {
        let mut problems = Vec::new();
        if self.heartbeat_secs == 0 {
            problems.push("operations.heartbeat_secs must be at least 1");
        }
        if self.stale_after_secs <= self.heartbeat_secs {
            problems.push("operations.stale_after_secs must be more than operations.heartbeat_secs");
        }
        if self.stream_poll_ms == 0 {
            problems.push("operations.stream_poll_ms must be at least 1");
        }
        if self.view_permission.trim().is_empty() {
            problems.push("operations.view_permission must not be empty");
        }
        if self.cancel_permission.trim().is_empty() {
            problems.push("operations.cancel_permission must not be empty");
        }
        problems
    }
}

impl QueueOptions {
    /// The queues workers process when none are given: [`queues`](QueueOptions::queues),
    /// or just [`default_queue`](QueueOptions::default_queue).
    pub fn worker_queues(&self) -> Vec<String> {
        if self.queues.is_empty() {
            vec![self.default_queue.clone()]
        } else {
            self.queues.clone()
        }
    }

    /// [`poll_interval_ms`](QueueOptions::poll_interval_ms) as a `Duration`.
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }
}

impl ConfigSection for QueueOptions {
    const KEY: &'static str = "queue";

    fn validate(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if self.default_queue.trim().is_empty() {
            problems.push("default_queue must not be empty");
        }
        if self.workers == 0 {
            problems.push("workers must be at least 1");
        }
        if self.poll_interval_ms == 0 {
            problems.push("poll_interval_ms must be at least 1");
        }
        problems.extend(self.operations.problems());
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

/// A list, or one comma-separated string (what an environment variable
/// holds); entries are trimmed and empty ones dropped.
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Names {
        List(Vec<String>),
        Joined(String),
    }
    let names = match Names::deserialize(deserializer)? {
        Names::List(names) => names,
        Names::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};

    use super::*;

    fn config(sources: ConfigSources) -> Config {
        Config::load(&sources).expect("configuration loads")
    }

    #[test]
    fn defaults_match_the_plan() {
        let options = QueueOptions::default();
        assert_eq!(options.driver, QueueDriver::Database);
        assert_eq!(options.default_queue, "default");
        assert_eq!(options.workers, 4);
        assert_eq!(options.poll_interval_ms, 1000);
        assert!(options.queues.is_empty());
        assert_eq!(options.worker_queues(), vec!["default".to_owned()]);
    }

    #[test]
    fn missing_section_uses_defaults() {
        let options = config(ConfigSources::new())
            .typed::<QueueOptions>()
            .expect("valid");
        assert_eq!(options, QueueOptions::default());
    }

    #[test]
    fn section_queue_is_read_and_queues_may_be_comma_separated() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_QUEUE__DRIVER", "memory"),
                ("APP_QUEUE__WORKERS", "2"),
                ("APP_QUEUE__QUEUES", "high, default"),
            ],
        );
        let options = config(sources).typed::<QueueOptions>().expect("valid");
        assert_eq!(options.driver, QueueDriver::Memory);
        assert_eq!(options.workers, 2);
        assert_eq!(
            options.queues,
            vec!["high".to_owned(), "default".to_owned()]
        );
        assert_eq!(options.worker_queues(), options.queues);
    }

    #[test]
    fn validation_rejects_zero_workers_and_empty_default_queue() {
        let options = QueueOptions {
            workers: 0,
            ..QueueOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("workers"));
        let options = QueueOptions {
            default_queue: " ".into(),
            ..QueueOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("default_queue"));
        let options = QueueOptions {
            poll_interval_ms: 0,
            ..QueueOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("poll_interval_ms"));
    }
}
