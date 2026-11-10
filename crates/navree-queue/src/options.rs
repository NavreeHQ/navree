//! [`QueueOptions`] (configuration section `queue`), [`QueueDriver`] and
//! [`WorkOptions`].

use std::time::Duration;

use navree_config::{ConfigSection, ConfigTable};
use serde::{Deserialize, Serialize};

/// Where queued jobs are stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum QueueDriver {
    /// The `jobs` and `failed_jobs` tables of the app database
    /// (`"database"`, the default): jobs survive restarts and are shared by
    /// every process using the database. Needs `DatabaseModule`.
    #[default]
    Database,
    /// In-process memory (`"memory"`): jobs are lost when the process
    /// exits. For tests and single-process development.
    Memory,
    /// Redis (`"redis"`): the `dyn QueueStore` provider named `redis`,
    /// which navree-redis's `RedisModule` registers; every process using
    /// the same Redis server shares the jobs. Needs `RedisModule`.
    Redis,
}

impl QueueDriver {
    /// The configuration name: `database`, `memory` or `redis`.
    pub fn as_str(self) -> &'static str {
        match self {
            QueueDriver::Database => "database",
            QueueDriver::Memory => "memory",
            QueueDriver::Redis => "redis",
        }
    }
}

/// `QueueModule` settings: configuration section `queue`.
///
/// ```toml
/// [queue]
/// driver = "database"          # or "memory", "redis"
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "queue", docs = "/docs/digging-deeper/queues/", validate = Self::check)]
#[serde(default)]
pub struct QueueOptions {
    /// Where jobs are stored (default [`QueueDriver::Database`]).
    #[config(one_of = "database, memory, redis")]
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
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub queues: Vec<String>,
    /// Operations: their routes, heartbeat and watchdog (section
    /// `queue.operations`).
    #[config(table)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
#[non_exhaustive]
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
            problems
                .push("operations.stale_after_secs must be more than operations.heartbeat_secs");
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

    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources, FieldType};

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
    fn redis_is_a_driver_name() {
        let sources = ConfigSources::new().env_vars("APP_", [("APP_QUEUE__DRIVER", "redis")]);
        let options = config(sources).typed::<QueueOptions>().expect("valid");
        assert_eq!(options.driver, QueueDriver::Redis);
        assert_eq!(
            [
                QueueDriver::Database,
                QueueDriver::Memory,
                QueueDriver::Redis
            ]
            .map(QueueDriver::as_str),
            ["database", "memory", "redis"]
        );
        let sources = ConfigSources::new().env_vars("APP_", [("APP_QUEUE__DRIVER", "carrier")]);
        assert!(config(sources).typed::<QueueOptions>().is_err());
    }

    #[test]
    fn section_queue_operations_is_read_from_nested_keys() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_QUEUE__OPERATIONS__ROUTES", "false"),
                ("APP_QUEUE__OPERATIONS__WATCHDOG", "false"),
                ("APP_QUEUE__OPERATIONS__STALE_AFTER_SECS", "600"),
                ("APP_QUEUE__OPERATIONS__VIEW_PERMISSION", "reports.view"),
            ],
        );
        let options = config(sources).typed::<QueueOptions>().expect("valid");
        let operations = options.operations;
        assert!(!operations.routes && !operations.watchdog);
        assert_eq!(operations.stale_after_secs, 600);
        assert_eq!(operations.view_permission, "reports.view");
        // The rest keep their defaults.
        assert_eq!(operations.heartbeat_secs, 30);
        assert_eq!(operations.max_redispatches, 3);
        assert_eq!(operations.progress_interval_ms, 500);
        assert_eq!(operations.stream_poll_ms, 1000);
        assert_eq!(operations.cancel_permission, "operations.cancel");
    }

    #[test]
    fn validation_checks_the_operations_settings() {
        let invalid = |change: fn(&mut OperationsOptions)| {
            let mut options = QueueOptions::default();
            change(&mut options.operations);
            options.validate().unwrap_err()
        };
        assert!(invalid(|o| o.heartbeat_secs = 0).contains("heartbeat_secs"));
        assert!(invalid(|o| o.stale_after_secs = 30).contains("stale_after_secs"));
        assert!(invalid(|o| o.stream_poll_ms = 0).contains("stream_poll_ms"));
        assert!(invalid(|o| o.view_permission = " ".into()).contains("view_permission"));
        assert!(invalid(|o| o.cancel_permission = String::new()).contains("cancel_permission"));
        assert!(QueueOptions::default().validate().is_ok());
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

    #[test]
    fn the_section_describes_itself() {
        let schema = QueueOptions::schema().expect("schema");
        assert_eq!(schema.key, "queue");
        assert_eq!(schema.docs.as_deref(), Some("/docs/digging-deeper/queues/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect(name)
        };
        assert_eq!(
            field("driver").ty,
            FieldType::choice(&["database", "memory", "redis"])
        );
        assert_eq!(field("queues").ty, FieldType::list(FieldType::String));
        let operations = field("operations");
        assert_eq!(operations.ty, FieldType::Table);
        assert!(
            operations
                .fields
                .iter()
                .any(|field| field.name == "stale_after_secs")
        );
        let defaults = QueueOptions::defaults().expect("defaults");
        assert_eq!(defaults["driver"], "database");
        assert_eq!(defaults["workers"], 4);
        assert_eq!(defaults["operations"]["heartbeat_secs"], 30);
    }
}
