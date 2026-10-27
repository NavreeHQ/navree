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
}

impl Default for QueueOptions {
    fn default() -> Self {
        Self {
            driver: QueueDriver::Database,
            default_queue: "default".to_owned(),
            workers: 4,
            poll_interval_ms: 1000,
            queues: Vec::new(),
        }
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
    fn redis_is_a_driver_name() {
        let sources = ConfigSources::new().env_vars("APP_", [("APP_QUEUE__DRIVER", "redis")]);
        let options = config(sources).typed::<QueueOptions>().expect("valid");
        assert_eq!(options.driver, QueueDriver::Redis);
        assert_eq!(
            [QueueDriver::Database, QueueDriver::Memory, QueueDriver::Redis].map(QueueDriver::as_str),
            ["database", "memory", "redis"]
        );
        let sources = ConfigSources::new().env_vars("APP_", [("APP_QUEUE__DRIVER", "carrier")]);
        assert!(config(sources).typed::<QueueOptions>().is_err());
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
