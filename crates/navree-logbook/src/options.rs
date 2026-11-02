//! [`LogbookOptions`]: the `logbook` configuration section.

use std::fmt;

use navree_config::ConfigSection;
use navree_telemetry::{EventKind, Interest, LogLevel};
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

/// The `logbook` configuration section. Every key is optional.
///
/// ```toml
/// [logbook]
/// # enabled = true        # true in debug builds, false in release builds
/// path = "/_logbook"
/// store = "file"          # or "memory"
/// file = "storage/logbook.db"
/// max_entries = 10000
/// keep_hours = 24
/// allow_remote = false
/// log_level = "debug"
/// slow_query_ms = 100
/// duplicate_queries = 5
/// replay = true
/// capture = { request_body = true, response_body = true, max_body_kb = 64 }
/// ignore = { routes = ["/health/*", "/metrics"], framework = true }
/// watchers = { cache = false }
/// ```
///
/// From the environment: `APP_LOGBOOK__STORE=memory`,
/// `APP_LOGBOOK__WATCHERS__CACHE=false`, and lists as comma-separated
/// strings (`APP_LOGBOOK__IGNORE__ROUTES=/health/*,/metrics,/up`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogbookOptions {
    /// Record and serve anything at all. `None` (unset) means on in debug
    /// builds and off in release builds; see [`LogbookOptions::activation`].
    pub enabled: Option<bool>,
    /// Where the UI is served (`/_logbook`).
    pub path: String,
    /// Where entries are kept.
    pub store: StoreKind,
    /// The SQLite file of the `file` store.
    pub file: String,
    /// Entries kept; older batches are pruned whole.
    pub max_entries: usize,
    /// Age in hours after which entries are pruned.
    pub keep_hours: u64,
    /// Serve the UI to non-loopback clients, behind the permission
    /// `logbook.view`.
    pub allow_remote: bool,
    /// The lowest level of log lines recorded (`trace`, `debug`, `info`,
    /// `warn` or `error`).
    pub log_level: String,
    /// Queries slower than this many milliseconds are marked slow.
    pub slow_query_ms: u64,
    /// Repeats of one statement in a batch that mark it as duplicated.
    pub duplicate_queries: usize,
    /// What request and response bodies are stored.
    pub capture: CaptureOptions,
    /// Allow replaying recorded requests.
    pub replay: bool,
    /// Paths never recorded.
    pub ignore: IgnoreOptions,
    /// The watchers: one per kind of entry.
    pub watchers: Watchers,
}

impl Default for LogbookOptions {
    fn default() -> Self {
        Self {
            enabled: None,
            path: "/_logbook".to_owned(),
            store: StoreKind::default(),
            file: "storage/logbook.db".to_owned(),
            max_entries: 10_000,
            keep_hours: 24,
            allow_remote: false,
            log_level: "debug".to_owned(),
            slow_query_ms: 100,
            duplicate_queries: 5,
            capture: CaptureOptions::default(),
            replay: true,
            ignore: IgnoreOptions::default(),
            watchers: Watchers::default(),
        }
    }
}

/// Whether Logbook runs, as decided by `logbook.enabled` and the build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Off: no routes, no watchers, nothing written.
    Off,
    /// On (a debug build, or `enabled = true` in one).
    On,
    /// On in a release build because `logbook.enabled = true` says so: the
    /// app logs a warning at every boot.
    Forced,
}

impl Activation {
    /// Whether Logbook records and serves anything.
    pub fn is_on(self) -> bool {
        !matches!(self, Activation::Off)
    }

    /// The activation `enabled` gives in a debug (`debug = true`) or a
    /// release build: unset means on in debug builds and off in release
    /// builds; a release build runs only when `enabled` is set to `true`.
    pub fn decide(enabled: Option<bool>, debug: bool) -> Activation {
        match (enabled, debug) {
            (Some(false), _) | (None, false) => Activation::Off,
            (Some(true) | None, true) => Activation::On,
            (Some(true), false) => Activation::Forced,
        }
    }
}

impl LogbookOptions {
    /// Whether Logbook runs in this build: see [`Activation::decide`].
    pub fn activation(&self) -> Activation {
        Activation::decide(self.enabled, cfg!(debug_assertions))
    }

    /// Whether Logbook records and serves anything in this build.
    pub fn is_enabled(&self) -> bool {
        self.activation().is_on()
    }

    /// The lowest log level recorded.
    pub fn min_log_level(&self) -> LogLevel {
        parse_level(&self.log_level).unwrap_or(LogLevel::Debug)
    }

    /// The telemetry kinds the enabled watchers want.
    pub fn interest(&self) -> Interest {
        let w = &self.watchers;
        let pairs = [
            (w.requests, EventKind::RequestHandled),
            (w.queries, EventKind::QueryExecuted),
            (w.jobs, EventKind::JobProcessed),
            (w.jobs, EventKind::JobFailed),
            (w.schedule, EventKind::ScheduledTaskRan),
            (w.mail, EventKind::MailSent),
            (w.notifications, EventKind::NotificationSent),
            (w.cache, EventKind::Cache),
            (w.http_client, EventKind::HttpClientRequest),
            (w.exceptions, EventKind::ExceptionReported),
            (w.logs, EventKind::Log),
            (w.events, EventKind::EventEmitted),
        ];
        pairs
            .into_iter()
            .filter(|(on, _)| *on)
            .fold(Interest::NONE, |all, (_, kind)| all.with(kind))
    }

    /// Checks the values; the error names the key.
    pub fn check(&self) -> Result<(), String> {
        if !self.path.starts_with('/') || self.path == "/" || self.path.ends_with('/') {
            return Err(format!(
                "logbook.path must start with `/`, not end with one and not be `/` alone (got `{}`)",
                self.path
            ));
        }
        if self.path.contains(['{', '}', '*', '?', '#', ' ']) {
            return Err(format!(
                "logbook.path must be a plain path without parameters (got `{}`)",
                self.path
            ));
        }
        if self.store == StoreKind::File && self.file.trim().is_empty() {
            return Err("logbook.file must name a file when logbook.store = \"file\"".to_owned());
        }
        if self.max_entries == 0 {
            return Err("logbook.max_entries must be at least 1".to_owned());
        }
        if self.keep_hours == 0 {
            return Err("logbook.keep_hours must be at least 1".to_owned());
        }
        if self.duplicate_queries < 2 {
            return Err("logbook.duplicate_queries must be at least 2".to_owned());
        }
        if self.capture.max_body_kb == 0 {
            return Err("logbook.capture.max_body_kb must be at least 1".to_owned());
        }
        if parse_level(&self.log_level).is_none() {
            return Err(format!(
                "logbook.log_level must be one of trace, debug, info, warn or error (got `{}`)",
                self.log_level
            ));
        }
        Ok(())
    }
}

impl ConfigSection for LogbookOptions {
    const KEY: &'static str = "logbook";

    fn validate(&self) -> Result<(), String> {
        self.check()
    }
}

/// A log level name, case-insensitively.
pub(crate) fn parse_level(name: &str) -> Option<LogLevel> {
    match name.trim().to_ascii_lowercase().as_str() {
        "trace" => Some(LogLevel::Trace),
        "debug" => Some(LogLevel::Debug),
        "info" => Some(LogLevel::Info),
        "warn" | "warning" => Some(LogLevel::Warn),
        "error" => Some(LogLevel::Error),
        _ => None,
    }
}

/// Where Logbook keeps its entries.
///
/// The default is `file` with this crate's `sqlite` feature (the facade's
/// `sqlite`), and `memory` without it: the file store is SQLite, which a
/// PostgreSQL or MySQL build does not compile. `logbook.store = "file"`
/// without the feature fails at boot with that explanation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreKind {
    /// A SQLite file of its own (`logbook.file`): entries survive restarts.
    /// Needs the `sqlite` feature.
    #[cfg_attr(feature = "sqlite", default)]
    File,
    /// The process's memory: entries go when the app stops.
    #[cfg_attr(not(feature = "sqlite"), default)]
    Memory,
}

/// `logbook.capture`: what request and response bodies are stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureOptions {
    /// Store request bodies.
    pub request_body: bool,
    /// Store response bodies.
    pub response_body: bool,
    /// Bodies are cut after this many KiB (the original size is kept).
    pub max_body_kb: usize,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            request_body: true,
            response_body: true,
            max_body_kb: 64,
        }
    }
}

/// `logbook.ignore`: what is never recorded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IgnoreOptions {
    /// Request paths never recorded, with `*` wildcards (`/health/*`); a
    /// comma-separated string from the environment.
    #[serde(deserialize_with = "list_or_comma_string")]
    pub routes: Vec<String>,
    /// Leave out what the framework runs by itself, outside any request, job
    /// or scheduled task: the poll of the transactional outbox, and the
    /// transaction and actor statements around it. Off, they are recorded
    /// like any other query.
    pub framework: bool,
}

impl Default for IgnoreOptions {
    fn default() -> Self {
        Self {
            routes: vec!["/health/*".to_owned(), "/metrics".to_owned()],
            framework: true,
        }
    }
}

/// `logbook.watchers`: each kind of entry, on by default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Watchers {
    /// HTTP requests, with headers and bodies.
    pub requests: bool,
    /// SQL statements.
    pub queries: bool,
    /// Job attempts.
    pub jobs: bool,
    /// Mail handed to the transport.
    pub mail: bool,
    /// App events on the event bus.
    pub events: bool,
    /// Cache hits, misses, writes and forgets.
    pub cache: bool,
    /// Log lines at `logbook.log_level` or above.
    pub logs: bool,
    /// 5xx errors, panics and errors reported with
    /// `Telemetry::report_exception` (a job's error is part of its entry).
    pub exceptions: bool,
    /// Outgoing HTTP calls through `HttpClient`.
    pub http_client: bool,
    /// Notifications, one entry per channel.
    pub notifications: bool,
    /// Inserts, updates and deletes a model makes in a unit of work.
    pub models: bool,
    /// Scheduled task runs.
    pub schedule: bool,
}

impl Default for Watchers {
    fn default() -> Self {
        Self {
            requests: true,
            queries: true,
            jobs: true,
            mail: true,
            events: true,
            cache: true,
            logs: true,
            exceptions: true,
            http_client: true,
            notifications: true,
            models: true,
            schedule: true,
        }
    }
}

/// A list, or a comma-separated string (environment variables).
fn list_or_comma_string<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    struct ListVisitor;

    impl<'de> Visitor<'de> for ListVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list of strings or a comma-separated string")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect())
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                items.push(item);
            }
            Ok(items)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
    }

    d.deserialize_any(ListVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_builds_run_only_when_forced() {
        assert_eq!(Activation::decide(None, true), Activation::On);
        assert_eq!(Activation::decide(Some(true), true), Activation::On);
        assert_eq!(Activation::decide(Some(false), true), Activation::Off);
        assert_eq!(Activation::decide(None, false), Activation::Off);
        assert_eq!(Activation::decide(Some(false), false), Activation::Off);
        assert_eq!(Activation::decide(Some(true), false), Activation::Forced);
        assert!(Activation::Forced.is_on());
        assert!(!Activation::Off.is_on());
    }

    #[test]
    fn the_default_follows_the_build() {
        assert_eq!(
            LogbookOptions::default().is_enabled(),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn checks_name_the_key() {
        let bad = |f: fn(&mut LogbookOptions)| {
            let mut options = LogbookOptions::default();
            f(&mut options);
            options.check().unwrap_err()
        };
        assert!(bad(|o| o.path = "_logbook".into()).contains("logbook.path"));
        assert!(bad(|o| o.path = "/_logbook/".into()).contains("logbook.path"));
        assert!(bad(|o| o.path = "/{x}".into()).contains("logbook.path"));
        assert!(bad(|o| o.max_entries = 0).contains("logbook.max_entries"));
        assert!(bad(|o| o.keep_hours = 0).contains("logbook.keep_hours"));
        assert!(bad(|o| o.duplicate_queries = 1).contains("logbook.duplicate_queries"));
        assert!(bad(|o| o.log_level = "loud".into()).contains("logbook.log_level"));
        assert!(bad(|o| o.capture.max_body_kb = 0).contains("max_body_kb"));
        assert!(bad(|o| o.file = " ".into()).contains("logbook.file"));
        assert!(LogbookOptions::default().check().is_ok());
    }

    #[test]
    fn watchers_decide_the_interest() {
        let mut options = LogbookOptions::default();
        assert!(options.interest().contains(EventKind::EventEmitted));
        options.watchers.cache = false;
        options.watchers.jobs = false;
        let interest = options.interest();
        assert!(!interest.contains(EventKind::Cache));
        assert!(!interest.contains(EventKind::JobFailed));
        assert!(interest.contains(EventKind::QueryExecuted));
    }

    #[test]
    fn levels_parse_case_insensitively() {
        assert_eq!(parse_level("WARN"), Some(LogLevel::Warn));
        assert_eq!(parse_level(" debug "), Some(LogLevel::Debug));
        assert_eq!(parse_level("verbose"), None);
    }
}
