//! [`FleetOptions`] (configuration section `fleet`), [`SupervisorOptions`],
//! [`Balance`] and the resolved [`Supervisor`]s.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use navree_config::{ConfigSection, ConfigTable};
use navree_queue::QueueOptions;
use serde::{Deserialize, Serialize};

use crate::FleetError;

/// How a supervisor spreads its processes over its queues
/// (`fleet.supervisors.<name>.balance`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Balance {
    /// Moves processes to the queues with the most work (`"auto"`, the
    /// default): every queue keeps at least `min_processes`, the supervisor
    /// never uses more than `max_processes`.
    #[default]
    Auto,
    /// Splits `max_processes` evenly over the queues and keeps it that way
    /// (`"simple"`).
    Simple,
    /// Runs `max_processes` processes, each working every queue in the
    /// order listed (`"off"`).
    Off,
}

impl Balance {
    /// `auto`, `simple` or `off`.
    pub fn as_str(self) -> &'static str {
        match self {
            Balance::Auto => "auto",
            Balance::Simple => "simple",
            Balance::Off => "off",
        }
    }
}

impl fmt::Display for Balance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One supervisor: a group of worker processes with one configuration
/// (`fleet.supervisors.<name>`). Every key but `queues` has a default.
///
/// ```toml
/// [fleet.supervisors.default]
/// queues = ["default", "mail", "notifications"]
/// balance = "auto"
/// min_processes = 1     # per queue
/// max_processes = 8     # for the whole supervisor
/// max_jobs = 1000       # recycle a worker after this many jobs
/// max_memory_mb = 256   # ... or when it uses more memory than this
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorOptions {
    /// The queues, in priority order (required). From the environment, one
    /// comma-separated string.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub queues: Vec<String>,
    /// How the processes are spread over the queues (default `auto`).
    #[serde(default)]
    pub balance: Balance,
    /// Processes each queue keeps with `auto` (default 1).
    #[serde(default = "one")]
    pub min_processes: usize,
    /// Processes the supervisor may use (default 4).
    #[serde(default = "four")]
    pub max_processes: usize,
    /// Jobs each process runs at once (default `queue.workers`).
    #[serde(default)]
    pub concurrency: Option<usize>,
    /// Jobs before a process is recycled (default 1000; 0: no limit).
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u64,
    /// Resident memory, in MB, before a process is recycled (default 256;
    /// 0: no limit).
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u64,
    /// Lifetime, in seconds, before a process is recycled (default 3600;
    /// 0: no limit).
    #[serde(default = "default_max_time_secs")]
    pub max_time_secs: u64,
}

impl SupervisorOptions {
    /// A supervisor of `queues` with every other setting at its default.
    pub fn new<I, S>(queues: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            queues: queues.into_iter().map(Into::into).collect(),
            balance: Balance::Auto,
            min_processes: 1,
            max_processes: 4,
            concurrency: None,
            max_jobs: default_max_jobs(),
            max_memory_mb: default_max_memory_mb(),
            max_time_secs: default_max_time_secs(),
        }
    }

    /// The problems of supervisor `name`, each naming it.
    fn problems(&self, name: &str) -> Vec<String> {
        let mut problems = Vec::new();
        let at = |what: &str| format!("fleet.supervisors.{name}: {what}");
        if self.queues.is_empty() {
            problems.push(at("queues must name at least one queue"));
        }
        if self.max_processes == 0 {
            problems.push(at("max_processes must be at least 1"));
        }
        if self.concurrency == Some(0) {
            problems.push(at("concurrency must be at least 1"));
        }
        match self.balance {
            Balance::Auto => {
                let needed = self.min_processes.saturating_mul(self.queues.len());
                if needed > self.max_processes {
                    problems.push(at(&format!(
                        "min_processes ({}) times its {} queue(s) is {needed}, more than max_processes ({})",
                        self.min_processes,
                        self.queues.len(),
                        self.max_processes
                    )));
                }
            }
            Balance::Simple => {
                if self.max_processes < self.queues.len() {
                    problems.push(at(&format!(
                        "balance \"simple\" gives every queue a process: max_processes ({}) must be at least its {} queue(s)",
                        self.max_processes,
                        self.queues.len()
                    )));
                }
            }
            Balance::Off => {}
        }
        problems
    }
}

/// How long Fleet keeps what it records (`fleet.trim`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct TrimOptions {
    /// How long completed jobs are listed, in minutes (default 60).
    pub recent_minutes: u64,
    /// How long metric snapshots are kept, in days (default 7).
    pub metrics_days: u64,
}

impl Default for TrimOptions {
    fn default() -> Self {
        Self {
            recent_minutes: 60,
            metrics_days: 7,
        }
    }
}

/// `FleetModule` settings: configuration section `fleet`. Every key is
/// optional except each supervisor's `queues`.
///
/// ```toml
/// [fleet]
/// path = "/_fleet"
/// balance_cooldown_secs = 3
///
/// [fleet.supervisors.default]
/// queues = ["default", "mail"]
///
/// [fleet.long_wait]
/// mail = 30
/// ```
///
/// From the environment, nested keys use `__` and lists are comma-separated
/// strings: `APP_FLEET__SUPERVISORS__DEFAULT__QUEUES=default,mail`,
/// `APP_FLEET__ONLY=exports`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "fleet", docs = "/docs/tools/fleet/", validate = Self::check)]
#[serde(default)]
pub struct FleetOptions {
    /// Mount the dashboard and its API (default true).
    pub dashboard: bool,
    /// Where the dashboard is served (default `/_fleet`), outside the API
    /// prefix.
    pub path: String,
    /// How often `auto` supervisors rebalance, in seconds (default 3).
    pub balance_cooldown_secs: u64,
    /// Processes added or removed per queue per step (default 1).
    pub balance_max_shift: usize,
    /// How often a master stores metrics, in seconds (default 60).
    pub snapshot_secs: u64,
    /// How long completed jobs and metric snapshots are kept.
    #[config(table)]
    pub trim: TrimOptions,
    /// Seconds a queue's oldest job may wait before a long-wait alert, per
    /// queue (none by default: such queues never raise one).
    pub long_wait: BTreeMap<String, u64>,
    /// Minimum time between two long-wait events for one queue, in minutes
    /// (default 15).
    pub long_wait_cooldown_minutes: u64,
    /// The supervisors this server runs (default: all). From the
    /// environment, one comma-separated string.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub only: Vec<String>,
    /// The supervisors, by name. Without any, one supervisor `default`
    /// works `queue.queues` (or `queue.default_queue`).
    pub supervisors: BTreeMap<String, SupervisorOptions>,
}

impl Default for FleetOptions {
    fn default() -> Self {
        Self {
            dashboard: true,
            path: "/_fleet".to_owned(),
            balance_cooldown_secs: 3,
            balance_max_shift: 1,
            snapshot_secs: 60,
            trim: TrimOptions::default(),
            long_wait: BTreeMap::new(),
            long_wait_cooldown_minutes: 15,
            only: Vec::new(),
            supervisors: BTreeMap::new(),
        }
    }
}

/// The name of the supervisor an app without `fleet.supervisors` gets.
pub const DEFAULT_SUPERVISOR: &str = "default";

impl FleetOptions {
    /// Every configured supervisor, resolved against the queue settings:
    /// the `fleet.supervisors` table, or the one `default` supervisor
    /// working `queue.queues` (or `queue.default_queue`). Sorted by name.
    pub fn all_supervisors(&self, queue: &QueueOptions) -> Vec<Supervisor> {
        if self.supervisors.is_empty() {
            let options = SupervisorOptions::new(queue.worker_queues());
            return vec![Supervisor::resolve(DEFAULT_SUPERVISOR, &options, queue)];
        }
        self.supervisors
            .iter()
            .map(|(name, options)| Supervisor::resolve(name, options, queue))
            .collect()
    }

    /// The supervisors this server runs: [`all_supervisors`](FleetOptions::all_supervisors)
    /// limited to [`only`](FleetOptions::only). A name in `only` that is no
    /// supervisor is an error. The implicit `default` supervisor (an app
    /// without `fleet.supervisors`) gets enough processes for one per queue
    /// when `queue.queues` lists more than its 4 (see
    /// [`Supervisor::resolve`]).
    pub fn supervisors(&self, queue: &QueueOptions) -> Result<Vec<Supervisor>, FleetError> {
        let all = self.all_supervisors(queue);
        if let Some(unknown) = self
            .only
            .iter()
            .find(|name| !all.iter().any(|s| &s.name == *name))
        {
            return Err(FleetError::UnknownSupervisor(unknown.clone()));
        }
        Ok(all
            .into_iter()
            .filter(|s| self.only.is_empty() || self.only.contains(&s.name))
            .collect())
    }

    /// [`balance_cooldown_secs`](FleetOptions::balance_cooldown_secs) as a duration.
    pub fn balance_cooldown(&self) -> Duration {
        Duration::from_secs(self.balance_cooldown_secs)
    }

    /// [`snapshot_secs`](FleetOptions::snapshot_secs) as a duration.
    pub fn snapshot_interval(&self) -> Duration {
        Duration::from_secs(self.snapshot_secs)
    }

    /// The long-wait threshold of `queue`, if it has one.
    pub fn long_wait_threshold(&self, queue: &str) -> Option<Duration> {
        self.long_wait
            .get(queue)
            .map(|secs| Duration::from_secs(*secs))
    }

    /// The dashboard path without a trailing slash.
    pub(crate) fn base_path(&self) -> &str {
        self.path.trim_end_matches('/')
    }

    /// Checks the values (the boot calls this): every problem names its key.
    fn check(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if !self.path.starts_with('/') || self.base_path().is_empty() {
            problems.push("fleet.path must start with `/` and not be `/` alone".to_owned());
        }
        for (key, value) in [
            ("balance_cooldown_secs", self.balance_cooldown_secs),
            ("snapshot_secs", self.snapshot_secs),
            ("trim.recent_minutes", self.trim.recent_minutes),
            ("trim.metrics_days", self.trim.metrics_days),
        ] {
            if value == 0 {
                problems.push(format!("fleet.{key} must be at least 1"));
            }
        }
        if self.balance_max_shift == 0 {
            problems.push("fleet.balance_max_shift must be at least 1".to_owned());
        }
        for (name, supervisor) in &self.supervisors {
            if name.trim().is_empty() || name.contains(',') {
                problems.push(format!(
                    "fleet.supervisors: `{name}` is not a valid supervisor name"
                ));
            }
            problems.extend(supervisor.problems(name));
        }
        if !self.supervisors.is_empty()
            && let Some(unknown) = self
                .only
                .iter()
                .find(|name| !self.supervisors.contains_key(*name))
        {
            problems.push(format!(
                "fleet.only names `{unknown}`, which is not a supervisor"
            ));
        }
        if self.supervisors.is_empty()
            && let Some(unknown) = self.only.iter().find(|name| *name != DEFAULT_SUPERVISOR)
        {
            problems.push(format!(
                "fleet.only names `{unknown}`, but the only supervisor is `{DEFAULT_SUPERVISOR}`"
            ));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

/// A supervisor as a master runs it: its name and settings, with
/// `concurrency` resolved from `queue.workers`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Supervisor {
    /// The supervisor name (the key under `fleet.supervisors`).
    pub name: String,
    /// The queues, in priority order.
    pub queues: Vec<String>,
    /// How processes are spread over the queues.
    pub balance: Balance,
    /// Processes each queue keeps (`auto`).
    pub min_processes: usize,
    /// Processes the supervisor may use.
    pub max_processes: usize,
    /// Jobs each process runs at once.
    pub concurrency: usize,
    /// Jobs before a process is recycled (0: no limit).
    pub max_jobs: u64,
    /// Memory in MB before a process is recycled (0: no limit).
    pub max_memory_mb: u64,
    /// Seconds before a process is recycled (0: no limit).
    pub max_time_secs: u64,
}

impl Supervisor {
    /// Supervisor `name` with `options`, its concurrency defaulting to
    /// `queue.workers`.
    pub fn resolve(name: &str, options: &SupervisorOptions, queue: &QueueOptions) -> Self {
        let mut supervisor = Self {
            name: name.to_owned(),
            queues: options.queues.clone(),
            balance: options.balance,
            min_processes: options.min_processes,
            max_processes: options.max_processes,
            concurrency: options.concurrency.unwrap_or(queue.workers).max(1),
            max_jobs: options.max_jobs,
            max_memory_mb: options.max_memory_mb,
            max_time_secs: options.max_time_secs,
        };
        // The implicit default supervisor must be valid whatever
        // `queue.queues` lists: `auto` needs `min × queues ≤ max`.
        if supervisor.balance == Balance::Auto {
            let needed = supervisor.min_processes * supervisor.queues.len();
            if needed > supervisor.max_processes {
                supervisor.max_processes = needed;
            }
        }
        supervisor
    }

    /// The process count `fleet` prints: `1-8 processes` (`auto`: the
    /// minimum per queue and the maximum), `4 processes` (`simple`, `off`).
    pub fn processes_label(&self) -> String {
        match self.balance {
            Balance::Auto => format!("{}-{} processes", self.min_processes, self.max_processes),
            Balance::Simple | Balance::Off => format!("{} processes", self.max_processes),
        }
    }
}

const fn one() -> usize {
    1
}

const fn four() -> usize {
    4
}

const fn default_max_jobs() -> u64 {
    1000
}

const fn default_max_memory_mb() -> u64 {
    256
}

const fn default_max_time_secs() -> u64 {
    3600
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources, FieldType};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<FleetOptions, String> {
        let config = Config::load(&sources).expect("configuration loads");
        config.typed::<FleetOptions>().map_err(|e| e.to_string())
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = FleetOptions::schema().expect("schema");
        assert_eq!(schema.key, "fleet");
        assert_eq!(schema.docs.as_deref(), Some("/docs/tools/fleet/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        assert!(schema.secret_paths().is_empty());
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        for name in [
            "dashboard",
            "path",
            "balance_cooldown_secs",
            "balance_max_shift",
            "snapshot_secs",
            "long_wait",
            "long_wait_cooldown_minutes",
            "supervisors",
        ] {
            assert!(!field(name).doc.is_empty(), "{name} has no description");
        }
        assert_eq!(field("only").ty, FieldType::list(FieldType::String));
        assert_eq!(field("long_wait").ty, FieldType::map(FieldType::Integer));
        assert_eq!(field("trim").ty, FieldType::Table);
        assert!(
            field("trim")
                .fields
                .iter()
                .any(|field| field.name == "recent_minutes")
        );
        let defaults = FleetOptions::defaults().expect("defaults");
        assert_eq!(defaults["dashboard"], json!(true));
        assert_eq!(defaults["path"], json!("/_fleet"));
        assert_eq!(defaults["trim"]["recent_minutes"], json!(60));
        assert_eq!(defaults["only"], json!([]));
    }

    #[test]
    fn defaults_match_the_docs() {
        let options = load(ConfigSources::new()).unwrap();
        assert_eq!(options, FleetOptions::default());
        assert!(options.dashboard);
        assert_eq!(options.path, "/_fleet");
        assert_eq!(options.balance_cooldown_secs, 3);
        assert_eq!(options.balance_max_shift, 1);
        assert_eq!(options.snapshot_secs, 60);
        assert_eq!(options.trim.recent_minutes, 60);
        assert_eq!(options.trim.metrics_days, 7);
        assert_eq!(options.long_wait_cooldown_minutes, 15);
        assert!(options.long_wait.is_empty());
        assert!(options.only.is_empty());

        let supervisor: SupervisorOptions =
            serde_json::from_value(json!({ "queues": ["default"] })).unwrap();
        assert_eq!(supervisor, SupervisorOptions::new(["default"]));
        assert_eq!(supervisor.balance, Balance::Auto);
        assert_eq!(supervisor.min_processes, 1);
        assert_eq!(supervisor.max_processes, 4);
        assert_eq!(supervisor.concurrency, None);
        assert_eq!(supervisor.max_jobs, 1000);
        assert_eq!(supervisor.max_memory_mb, 256);
        assert_eq!(supervisor.max_time_secs, 3600);
    }

    #[test]
    fn a_supervisor_needs_its_queues() {
        let error = serde_json::from_value::<SupervisorOptions>(json!({ "balance": "off" }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("queues"), "{error}");
    }

    #[test]
    fn the_docs_example_is_read_from_toml_values() {
        let sources = ConfigSources::new().set(
            "fleet",
            json!({
                "supervisors": {
                    "default": {
                        "queues": ["default", "mail", "notifications"],
                        "balance": "auto",
                        "min_processes": 1,
                        "max_processes": 8,
                        "max_jobs": 1000,
                        "max_memory_mb": 256
                    },
                    "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2 }
                },
                "long_wait": { "default": 60, "mail": 30, "exports": 600 }
            }),
        );
        let options = load(sources).unwrap();
        let exports = &options.supervisors["exports"];
        assert_eq!(exports.balance, Balance::Off);
        assert_eq!(exports.max_processes, 2);
        assert_eq!(options.supervisors["default"].max_processes, 8);
        assert_eq!(
            options.long_wait_threshold("mail"),
            Some(Duration::from_secs(30))
        );
        assert_eq!(options.long_wait_threshold("other"), None);
    }

    #[test]
    fn environment_variables_override_nested_keys_and_lists() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_FLEET__SUPERVISORS__DEFAULT__QUEUES", "default, mail"),
                ("APP_FLEET__SUPERVISORS__DEFAULT__MAX_PROCESSES", "16"),
                ("APP_FLEET__ONLY", "default"),
                ("APP_FLEET__LONG_WAIT__MAIL", "30"),
                ("APP_FLEET__DASHBOARD", "false"),
            ],
        );
        let options = load(sources).unwrap();
        let default = &options.supervisors["default"];
        assert_eq!(default.queues, ["default", "mail"]);
        assert_eq!(default.max_processes, 16);
        assert_eq!(options.only, ["default"]);
        assert_eq!(options.long_wait["mail"], 30);
        assert!(!options.dashboard);
    }

    #[test]
    fn min_processes_times_queues_must_fit_max_processes() {
        let sources = ConfigSources::new().set(
            "fleet.supervisors.busy",
            json!({ "queues": ["a", "b", "c"], "min_processes": 2, "max_processes": 4 }),
        );
        let error = load(sources).unwrap_err();
        assert!(error.contains("fleet.supervisors.busy"), "{error}");
        assert!(error.contains("min_processes"), "{error}");
    }

    #[test]
    fn invalid_settings_are_reported_together() {
        let options = FleetOptions {
            path: "_fleet".into(),
            balance_cooldown_secs: 0,
            balance_max_shift: 0,
            snapshot_secs: 0,
            only: vec!["nope".into()],
            supervisors: BTreeMap::from([
                (
                    "empty".to_owned(),
                    SupervisorOptions::new(Vec::<String>::new()),
                ),
                (
                    "simple".to_owned(),
                    SupervisorOptions {
                        balance: Balance::Simple,
                        max_processes: 1,
                        concurrency: Some(0),
                        ..SupervisorOptions::new(["a", "b"])
                    },
                ),
            ]),
            ..FleetOptions::default()
        };
        let error = options.validate().unwrap_err();
        for fragment in [
            "fleet.path",
            "balance_cooldown_secs",
            "balance_max_shift",
            "snapshot_secs",
            "fleet.supervisors.empty: queues",
            "fleet.supervisors.simple: balance \"simple\"",
            "concurrency",
            "fleet.only names `nope`",
        ] {
            assert!(error.contains(fragment), "{fragment} in {error}");
        }
        assert!(
            FleetOptions {
                path: "/".into(),
                ..FleetOptions::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn without_supervisors_one_default_supervisor_works_the_queue_settings() {
        let queue = QueueOptions {
            queues: vec!["high".into(), "default".into()],
            workers: 3,
            ..QueueOptions::default()
        };
        let options = FleetOptions::default();
        let supervisors = options.supervisors(&queue).unwrap();
        assert_eq!(supervisors.len(), 1);
        let default = &supervisors[0];
        assert_eq!(default.name, "default");
        assert_eq!(default.queues, ["high", "default"]);
        assert_eq!(default.concurrency, 3);
        assert_eq!(default.balance, Balance::Auto);
        assert_eq!(default.max_processes, 4);

        let only_default = QueueOptions::default();
        assert_eq!(
            options.supervisors(&only_default).unwrap()[0].queues,
            ["default"]
        );
    }

    #[test]
    fn only_limits_the_supervisors_a_server_runs() {
        let mut options = FleetOptions::default();
        options
            .supervisors
            .insert("default".into(), SupervisorOptions::new(["default"]));
        options.supervisors.insert(
            "exports".into(),
            SupervisorOptions {
                concurrency: Some(1),
                ..SupervisorOptions::new(["exports"])
            },
        );
        let queue = QueueOptions::default();
        assert_eq!(options.supervisors(&queue).unwrap().len(), 2);
        options.only = vec!["exports".into()];
        let only = options.supervisors(&queue).unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].name, "exports");
        assert_eq!(only[0].concurrency, 1);
        options.only = vec!["missing".into()];
        assert!(matches!(
            options.supervisors(&queue),
            Err(FleetError::UnknownSupervisor(name)) if name == "missing"
        ));
    }

    #[test]
    fn process_labels_match_the_fleet_banner() {
        let queue = QueueOptions::default();
        let auto = Supervisor::resolve(
            "default",
            &SupervisorOptions {
                max_processes: 8,
                ..SupervisorOptions::new(["default", "mail", "notifications"])
            },
            &queue,
        );
        assert_eq!(auto.processes_label(), "1-8 processes");
        let off = Supervisor::resolve(
            "exports",
            &SupervisorOptions {
                balance: Balance::Off,
                max_processes: 2,
                ..SupervisorOptions::new(["exports"])
            },
            &queue,
        );
        assert_eq!(off.processes_label(), "2 processes");
    }

    #[test]
    fn the_default_supervisor_gets_a_process_per_queue() {
        let queue = QueueOptions {
            queues: (0..6).map(|i| format!("q{i}")).collect(),
            ..QueueOptions::default()
        };
        let supervisors = FleetOptions::default().supervisors(&queue).unwrap();
        assert_eq!(supervisors[0].queues.len(), 6);
        assert_eq!(supervisors[0].max_processes, 6);
    }
}
