//! [`LighthouseOptions`]: the `lighthouse` configuration section.

use std::fmt;
use std::sync::Arc;

use navree_config::{ConfigSection, ConfigTable, Environment};
use navree_contracts::Secret;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

use crate::alert::AlertKind;

/// When a request, query, job attempt or outgoing call is slow
/// (`lighthouse.thresholds`), in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct Thresholds {
    /// A request slower than this opens a slow-request issue (1000).
    pub slow_request_ms: u64,
    /// A query slower than this opens a slow-query issue (200).
    pub slow_query_ms: u64,
    /// A job attempt slower than this keeps its trace (30000).
    pub slow_job_ms: u64,
    /// An outgoing call slower than this opens an issue for its host (2000).
    pub slow_outgoing_ms: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            slow_request_ms: 1_000,
            slow_query_ms: 200,
            slow_job_ms: 30_000,
            slow_outgoing_ms: 2_000,
        }
    }
}

/// How many details (traces and occurrences) are stored
/// (`lighthouse.sample`): fractions from 0 to 1. Counts and percentiles are
/// always exact, whatever the rates. Errors (5xx responses, requests that
/// reported something), failed job attempts and slow requests and jobs
/// always keep their trace; the rates apply to the rest.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct SampleRates {
    /// Fraction of ordinary request traces stored (0.1).
    pub requests: f64,
    /// Fraction of ordinary job traces stored (0.1).
    pub jobs: f64,
    /// Fraction of occurrences stored beyond the first 100 per issue per
    /// hour (1.0).
    pub exceptions: f64,
}

impl Default for SampleRates {
    fn default() -> Self {
        Self {
            requests: 0.1,
            jobs: 0.1,
            exceptions: 1.0,
        }
    }
}

/// What Lighthouse keeps of a request (`lighthouse.capture`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct CaptureOptions {
    /// Store JSON request bodies up to 16 KiB, masked (false).
    pub request_body: bool,
    /// Store request headers, masked (true).
    pub request_headers: bool,
    /// Store the client address (true).
    pub ip: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            request_body: false,
            request_headers: true,
            ip: true,
        }
    }
}

/// What is never recorded (`lighthouse.ignore`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct IgnoreOptions {
    /// Paths never recorded; `*` at the end matches one or more segments
    /// (`["/health/*", "/metrics"]`).
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub routes: Vec<String>,
    /// Job types never recorded.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub jobs: Vec<String>,
}

impl Default for IgnoreOptions {
    fn default() -> Self {
        Self {
            routes: vec!["/health/*".to_owned(), "/metrics".to_owned()],
            jobs: Vec::new(),
        }
    }
}

/// Where Lighthouse keeps what it records (`lighthouse.store`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StoreKind {
    /// In the app's database, or `lighthouse.database_url`'s (the default).
    #[default]
    Database,
    /// Nowhere: nothing is recorded and the dashboard is not mounted.
    Off,
}

/// When occurrences make a spike (`lighthouse.alerts.spike`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
#[non_exhaustive]
pub struct SpikeOptions {
    /// Occurrences within the window that make a spike (50).
    pub occurrences: u64,
    /// The window, in minutes (5).
    pub minutes: u32,
}

impl Default for SpikeOptions {
    fn default() -> Self {
        Self {
            occurrences: 50,
            minutes: 5,
        }
    }
}

/// Email alerts (`lighthouse.alerts.email`), sent through the app's `Mailer`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
#[non_exhaustive]
pub struct EmailAlertOptions {
    /// The recipients: one address, a list, or a comma-separated string.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub to: Vec<String>,
    /// The kinds sent (default `new_issue`, `regression`, `spike`).
    #[serde(deserialize_with = "kinds")]
    pub on: Option<Vec<AlertKind>>,
}

/// Slack alerts (`lighthouse.alerts.slack`) through an incoming webhook.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
#[non_exhaustive]
pub struct SlackAlertOptions {
    /// The incoming webhook's URL; a secret
    /// (`APP_LIGHTHOUSE__ALERTS__SLACK__WEBHOOK_URL`).
    pub webhook_url: Option<Secret<String>>,
    /// The kinds sent (default `new_issue`, `regression`, `spike`).
    #[serde(deserialize_with = "kinds")]
    pub on: Option<Vec<AlertKind>>,
}

/// Signed webhook alerts (`lighthouse.alerts.webhook`), in the Standard
/// Webhooks format.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
#[non_exhaustive]
pub struct WebhookAlertOptions {
    /// Where alerts are posted.
    pub url: Option<String>,
    /// The signing secret, at least 32 bytes; required with `url`
    /// (`APP_LIGHTHOUSE__ALERTS__WEBHOOK__SECRET`). A `whsec_` secret is
    /// base64-decoded, as Standard Webhooks secrets are.
    pub secret: Option<Secret<String>>,
    /// The kinds sent (default `new_issue`, `regression`, `spike`).
    #[serde(deserialize_with = "kinds")]
    pub on: Option<Vec<AlertKind>>,
}

/// The alert channels and their throttling (`lighthouse.alerts`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
#[non_exhaustive]
pub struct AlertOptions {
    /// At most one alert per issue and kind in this window (30).
    pub cooldown_minutes: u32,
    /// When occurrences make a spike.
    #[config(table)]
    pub spike: SpikeOptions,
    /// Email alerts.
    #[config(table)]
    pub email: EmailAlertOptions,
    /// Slack alerts.
    #[config(table)]
    pub slack: SlackAlertOptions,
    /// Webhook alerts.
    #[config(table)]
    pub webhook: WebhookAlertOptions,
}

impl Default for AlertOptions {
    fn default() -> Self {
        Self {
            cooldown_minutes: 30,
            spike: SpikeOptions::default(),
            email: EmailAlertOptions::default(),
            slack: SlackAlertOptions::default(),
            webhook: WebhookAlertOptions::default(),
        }
    }
}

/// The kinds a channel gets when its `on` is not set.
pub const DEFAULT_ALERT_KINDS: [AlertKind; 3] =
    [AlertKind::NewIssue, AlertKind::Regression, AlertKind::Spike];

/// Options of the Lighthouse module (configuration section `lighthouse`).
/// Every key is optional.
///
/// ```toml
/// [lighthouse]
/// retention_days = 30
///
/// [lighthouse.thresholds]
/// slow_request_ms = 500
///
/// [lighthouse.sample]
/// requests = 0.05
///
/// [lighthouse.alerts.email]
/// to = ["oncall@example.com"]
/// ```
///
/// Environment variables: `APP_LIGHTHOUSE__RETENTION_DAYS=30`,
/// `APP_LIGHTHOUSE__SAMPLE__REQUESTS=0.05`, lists as comma-separated
/// strings (`APP_LIGHTHOUSE__IGNORE__ROUTES=/health/*,/metrics`).
/// [`check`](Self::check) names the key of every problem.
#[derive(Clone, Debug, PartialEq, Deserialize, ConfigSection)]
#[config(key = "lighthouse", docs = "/docs/tools/lighthouse/", validate = Self::check_values)]
#[serde(default)]
pub struct LighthouseOptions {
    /// Record anything at all. When off, the dashboard still shows what was
    /// recorded.
    pub enabled: bool,
    /// Where records are kept: `database` (the default) or `off`.
    #[config(one_of = "database, off")]
    pub store: StoreKind,
    /// A separate database for Lighthouse's tables (same forms as
    /// `database.url`); the app's database when unset.
    pub database_url: Option<Secret<String>>,
    /// Mount the dashboard and its JSON API.
    pub dashboard: bool,
    /// Offer Lighthouse's MCP tools, resource and prompt on the app's MCP
    /// server (with the `mcp` feature and `McpModule` in the app).
    pub mcp: bool,
    /// Where the dashboard is served (`/_lighthouse`).
    pub path: String,
    /// The dashboard's public URL, for links in alerts; `app.url` followed
    /// by `path` when unset.
    pub url: Option<String>,
    /// The environment label; the app's environment (`app.env`, `APP_ENV`)
    /// when unset, and when the app names none, `production` in release
    /// builds and `development` in debug builds.
    pub environment: Option<String>,
    /// The release label; `app.version`, else `unknown`, when unset
    /// (`APP_LIGHTHOUSE__RELEASE`, which deploys set).
    pub release: Option<String>,
    /// The server label; the host name when unset (`HOSTNAME`, or
    /// `COMPUTERNAME` on Windows).
    #[config(env_fallback = "HOSTNAME")]
    pub server: Option<String>,
    /// When things are slow.
    #[config(table)]
    pub thresholds: Thresholds,
    /// How many details are stored.
    #[config(table)]
    pub sample: SampleRates,
    /// What `lighthouse:prune` keeps, in days (14).
    pub retention_days: u32,
    /// When the scheduler prunes, `"HH:MM"` UTC (`"03:10"`); empty to not
    /// schedule it.
    pub prune_at: String,
    /// Details (traces and occurrences) waiting to be written before new
    /// ones are sampled out (10000). Counts are never dropped.
    pub buffer: usize,
    /// What is kept of a request.
    #[config(table)]
    pub capture: CaptureOptions,
    /// Name fragments masked in addition to `log.redact`.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub redact: Vec<String>,
    /// What is never recorded.
    #[config(table)]
    pub ignore: IgnoreOptions,
    /// The alert channels.
    #[config(table)]
    pub alerts: AlertOptions,
}

impl Default for LighthouseOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            store: StoreKind::Database,
            database_url: None,
            dashboard: true,
            mcp: true,
            path: "/_lighthouse".to_owned(),
            url: None,
            environment: None,
            release: None,
            server: None,
            thresholds: Thresholds::default(),
            sample: SampleRates::default(),
            retention_days: 14,
            prune_at: "03:10".to_owned(),
            buffer: 10_000,
            capture: CaptureOptions::default(),
            redact: Vec::new(),
            ignore: IgnoreOptions::default(),
            alerts: AlertOptions::default(),
        }
    }
}

impl LighthouseOptions {
    /// Checks the values at boot: [`check`](Self::check) with the problems
    /// joined into one message.
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }

    /// Checks the options; every problem names its key and none echoes a
    /// secret.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for (key, rate) in [
            ("lighthouse.sample.requests", self.sample.requests),
            ("lighthouse.sample.jobs", self.sample.jobs),
            ("lighthouse.sample.exceptions", self.sample.exceptions),
        ] {
            if !(0.0..=1.0).contains(&rate) || rate.is_nan() {
                errors.push(format!("{key} must be between 0 and 1 (got {rate})"));
            }
        }
        let path = self.path.trim_end_matches('/');
        if !path.starts_with('/') || path.len() < 2 || path.contains(['?', '#', ' ', '{', '}']) {
            errors.push(format!(
                "lighthouse.path must be an absolute path such as /_lighthouse (got {:?})",
                self.path
            ));
        }
        if let Some(url) = &self.url
            && !is_http_url(url)
        {
            errors.push(format!(
                "lighthouse.url must be an absolute http(s) URL (got {url:?})"
            ));
        }
        if !self.prune_at.trim().is_empty() && parse_hh_mm(self.prune_at.trim()).is_none() {
            errors.push(format!(
                "lighthouse.prune_at must be \"HH:MM\" (UTC) or empty (got {:?})",
                self.prune_at
            ));
        }
        if self.retention_days == 0 {
            errors.push("lighthouse.retention_days must be at least 1".to_owned());
        }
        if self.buffer < 100 {
            errors.push(format!(
                "lighthouse.buffer must be at least 100 (got {})",
                self.buffer
            ));
        }
        if let Some(url) = &self.database_url
            && url.expose().trim().is_empty()
        {
            errors.push("lighthouse.database_url must not be empty when set".to_owned());
        }
        if self.alerts.spike.occurrences < 2 {
            errors.push("lighthouse.alerts.spike.occurrences must be at least 2".to_owned());
        }
        if self.alerts.cooldown_minutes == 0 {
            errors.push("lighthouse.alerts.cooldown_minutes must be at least 1".to_owned());
        }
        if self.alerts.spike.minutes == 0 || self.alerts.spike.minutes > 60 {
            errors.push("lighthouse.alerts.spike.minutes must be between 1 and 60".to_owned());
        }
        for address in &self.alerts.email.to {
            if !address.contains('@') {
                errors.push(format!(
                    "lighthouse.alerts.email.to has an invalid address {address:?}"
                ));
            }
        }
        if let Some(url) = &self.alerts.slack.webhook_url
            && !is_http_url(url.expose())
        {
            errors.push(
                "lighthouse.alerts.slack.webhook_url must be an absolute http(s) URL".to_owned(),
            );
        }
        if let Some(url) = &self.alerts.webhook.url {
            if !is_http_url(url) {
                errors.push(format!(
                    "lighthouse.alerts.webhook.url must be an absolute http(s) URL (got {url:?})"
                ));
            }
            match &self.alerts.webhook.secret {
                None => errors.push(
                    "lighthouse.alerts.webhook.secret is required with lighthouse.alerts.webhook.url"
                        .to_owned(),
                ),
                Some(secret) if secret.expose().len() < 32 => errors.push(
                    "lighthouse.alerts.webhook.secret must be at least 32 bytes".to_owned(),
                ),
                Some(_) => {}
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The dashboard's mount path without a trailing slash (`/_lighthouse`).
    pub fn mount_path(&self) -> &str {
        let path = self.path.trim_end_matches('/');
        if path.is_empty() { "/" } else { path }
    }

    /// The options with the environment label taken from the app's
    /// environment, unless `environment` is set.
    pub(crate) fn with_app_environment(
        options: Arc<LighthouseOptions>,
        app_environment: Option<Environment>,
    ) -> Arc<LighthouseOptions> {
        let set = options
            .environment
            .as_deref()
            .is_some_and(|environment| !environment.trim().is_empty());
        match app_environment {
            Some(environment) if !set => {
                let mut labelled = (*options).clone();
                labelled.environment = Some(environment.to_string());
                Arc::new(labelled)
            }
            _ => options,
        }
    }

    /// The environment label.
    pub fn environment_label(&self) -> String {
        match &self.environment {
            Some(environment) if !environment.trim().is_empty() => environment.clone(),
            _ if cfg!(debug_assertions) => "development".to_owned(),
            _ => "production".to_owned(),
        }
    }

    /// The server label: `server`, else the host name.
    pub fn server_label(&self) -> String {
        match &self.server {
            Some(server) if !server.trim().is_empty() => server.clone(),
            _ => host_name(),
        }
    }
}

/// The host name: `HOSTNAME` or `COMPUTERNAME`, else `/etc/hostname`, else
/// `localhost`.
pub(crate) fn host_name() -> String {
    // The variable `lighthouse.server` declares, then Windows' own.
    let declared = navree_config::env_fallback::<LighthouseOptions>("server");
    for var in declared.iter().map(String::as_str).chain(["COMPUTERNAME"]) {
        if let Ok(name) = std::env::var(var)
            && !name.trim().is_empty()
        {
            return name.trim().to_owned();
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

/// Whether `url` is an absolute `http://` or `https://` URL with a host.
pub(crate) fn is_http_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

/// Parses `"HH:MM"` into (hour, minute).
pub(crate) fn parse_hh_mm(text: &str) -> Option<(u32, u32)> {
    let (hour, minute) = text.split_once(':')?;
    if hour.is_empty() || hour.len() > 2 || minute.len() != 2 {
        return None;
    }
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// Alert kinds: a list or a comma-separated string; an unknown kind is an
/// error naming the accepted ones.
fn kinds<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<AlertKind>>, D::Error> {
    let names = navree_config::comma_list(d)?;
    names
        .iter()
        .map(|name| {
            AlertKind::parse(name).ok_or_else(|| {
                de::Error::custom(format!(
                    "unknown alert kind {name:?} (expected one of {})",
                    AlertKind::ALL
                        .iter()
                        .map(|kind| kind.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// A period of the dashboard: the last hour, 24 hours or 7 days.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(navree_openapi::ToSchema))]
pub enum Period {
    /// The last hour (per-minute buckets).
    #[serde(rename = "1h")]
    Hour,
    /// The last 24 hours (per-hour buckets).
    #[default]
    #[serde(rename = "24h")]
    Day,
    /// The last 7 days (per-hour buckets).
    #[serde(rename = "7d")]
    Week,
}

impl Period {
    /// Every period, shortest first.
    pub const ALL: [Period; 3] = [Period::Hour, Period::Day, Period::Week];

    /// `1h`, `24h` or `7d`.
    pub fn as_str(self) -> &'static str {
        match self {
            Period::Hour => "1h",
            Period::Day => "24h",
            Period::Week => "7d",
        }
    }

    /// Parses `1h`, `24h` or `7d`.
    pub fn parse(text: &str) -> Option<Period> {
        Period::ALL
            .into_iter()
            .find(|period| period.as_str() == text)
    }

    /// The period's length in minutes.
    pub fn minutes(self) -> i64 {
        match self {
            Period::Hour => 60,
            Period::Day => 24 * 60,
            Period::Week => 7 * 24 * 60,
        }
    }

    /// The bucket resolution read for this period, in seconds.
    pub(crate) fn resolution(self) -> i32 {
        match self {
            Period::Hour => 60,
            Period::Day | Period::Week => 3600,
        }
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_config::{Config, ConfigSources, FieldType};
    use serde_json::json;

    fn load(sources: ConfigSources) -> Result<LighthouseOptions, String> {
        let config = Config::load(&sources).map_err(|e| e.to_string())?;
        config
            .typed::<LighthouseOptions>()
            .map_err(|e| e.to_string())
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = LighthouseOptions::schema().expect("schema");
        assert_eq!(schema.key, "lighthouse");
        assert_eq!(schema.docs.as_deref(), Some("/docs/tools/lighthouse/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        for name in [
            "enabled",
            "store",
            "database_url",
            "dashboard",
            "path",
            "environment",
            "release",
            "server",
            "thresholds",
            "sample",
            "retention_days",
            "capture",
            "redact",
            "ignore",
            "alerts",
        ] {
            assert!(
                schema.fields.iter().any(|field| field.name == name),
                "missing {name}"
            );
        }
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect("field")
        };
        assert_eq!(field("store").ty, FieldType::choice(&["database", "off"]));
        assert_eq!(field("thresholds").ty, FieldType::Table);
        assert!(
            field("thresholds")
                .fields
                .iter()
                .any(|field| field.name == "slow_request_ms")
        );
        let secrets = schema.secret_paths();
        for path in [
            "lighthouse.database_url",
            "lighthouse.alerts.slack.webhook_url",
            "lighthouse.alerts.webhook.secret",
        ] {
            assert!(secrets.iter().any(|secret| secret == path), "{secrets:?}");
        }
        let fallbacks = schema.env_fallbacks();
        assert!(
            fallbacks
                .iter()
                .any(|fallback| fallback.key == "lighthouse.server"
                    && fallback.variable == "HOSTNAME"),
            "{fallbacks:?}"
        );
        let defaults = LighthouseOptions::defaults().expect("defaults");
        assert_eq!(defaults["store"], json!("database"));
        assert_eq!(defaults["retention_days"], json!(14));
        assert_eq!(defaults["thresholds"]["slow_request_ms"], json!(1000));
        assert_eq!(defaults["alerts"]["spike"]["occurrences"], json!(50));
        assert_eq!(defaults["database_url"], json!(null));
    }

    #[test]
    fn credentials_never_print() {
        let options = LighthouseOptions {
            database_url: Some(Secret::new(
                "postgres://user:hunter2@db/lighthouse".to_owned(),
            )),
            alerts: AlertOptions {
                slack: SlackAlertOptions {
                    webhook_url: Some(Secret::new(
                        "https://hooks.slack.com/services/T0/B0/hunter2".to_owned(),
                    )),
                    ..SlackAlertOptions::default()
                },
                webhook: WebhookAlertOptions {
                    url: Some("https://ops.example.com/hooks".to_owned()),
                    secret: Some(Secret::new("hunter2-hunter2-hunter2-hunter2".to_owned())),
                    ..WebhookAlertOptions::default()
                },
                ..AlertOptions::default()
            },
            ..LighthouseOptions::default()
        };
        assert!(!format!("{options:?}").contains("hunter2"));
        assert!(
            !ConfigTable::to_json(&options)
                .to_string()
                .contains("hunter2")
        );
    }

    #[test]
    fn defaults_match_the_documented_table() {
        let options = LighthouseOptions::default();
        assert!(options.enabled);
        assert!(options.dashboard);
        assert!(options.mcp);
        assert_eq!(options.path, "/_lighthouse");
        assert_eq!(options.thresholds.slow_request_ms, 1000);
        assert_eq!(options.thresholds.slow_query_ms, 200);
        assert_eq!(options.thresholds.slow_job_ms, 30_000);
        assert_eq!(options.thresholds.slow_outgoing_ms, 2000);
        assert_eq!(options.sample, SampleRates::default());
        assert_eq!(options.sample.requests, 0.1);
        assert_eq!(options.sample.jobs, 0.1);
        assert_eq!(options.sample.exceptions, 1.0);
        assert_eq!(options.retention_days, 14);
        assert_eq!(options.prune_at, "03:10");
        assert_eq!(options.buffer, 10_000);
        assert!(!options.capture.request_body);
        assert!(options.capture.request_headers);
        assert!(options.capture.ip);
        assert_eq!(options.ignore.routes, ["/health/*", "/metrics"]);
        assert_eq!(options.alerts.cooldown_minutes, 30);
        assert_eq!(options.alerts.spike.occurrences, 50);
        assert_eq!(options.alerts.spike.minutes, 5);
        assert!(options.check().is_ok());
        assert_eq!(
            options.environment_label(),
            if cfg!(debug_assertions) {
                "development"
            } else {
                "production"
            }
        );
    }

    #[test]
    fn the_label_follows_the_app_environment_unless_it_is_set() {
        let options = Arc::new(LighthouseOptions::default());
        let labelled =
            LighthouseOptions::with_app_environment(options.clone(), Some(Environment::Staging));
        assert_eq!(labelled.environment_label(), "staging");
        let own = Arc::new(LighthouseOptions {
            environment: Some("eu-west".to_owned()),
            ..LighthouseOptions::default()
        });
        let labelled = LighthouseOptions::with_app_environment(own, Some(Environment::Production));
        assert_eq!(labelled.environment_label(), "eu-west");
        let unchanged = LighthouseOptions::with_app_environment(options.clone(), None);
        assert!(Arc::ptr_eq(&unchanged, &options));
    }

    #[test]
    fn environment_variables_configure_every_kind_of_key() {
        let options = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_LIGHTHOUSE__RETENTION_DAYS", "30"),
                ("APP_LIGHTHOUSE__SAMPLE__REQUESTS", "0.25"),
                (
                    "APP_LIGHTHOUSE__IGNORE__ROUTES",
                    "/health/*,/metrics,/internal/*",
                ),
                (
                    "APP_LIGHTHOUSE__ALERTS__EMAIL__TO",
                    "oncall@example.com,cto@example.com",
                ),
                ("APP_LIGHTHOUSE__ALERTS__SLACK__ON", "new_issue,failed_job"),
                ("APP_LIGHTHOUSE__RELEASE", "20261026-091512-3f2a9c1"),
                ("APP_LIGHTHOUSE__MCP", "false"),
            ],
        ))
        .unwrap();
        assert_eq!(options.retention_days, 30);
        assert!(!options.mcp);
        assert!((options.sample.requests - 0.25).abs() < f64::EPSILON);
        assert_eq!(
            options.ignore.routes,
            ["/health/*", "/metrics", "/internal/*"]
        );
        assert_eq!(
            options.alerts.email.to,
            ["oncall@example.com", "cto@example.com"]
        );
        assert_eq!(
            options.alerts.slack.on,
            Some(vec![AlertKind::NewIssue, AlertKind::FailedJob])
        );
        assert_eq!(options.release.as_deref(), Some("20261026-091512-3f2a9c1"));
    }

    #[test]
    fn invalid_values_name_their_key() {
        let error =
            load(ConfigSources::new().set("lighthouse.sample.requests", json!(1.5))).unwrap_err();
        assert!(error.contains("lighthouse.sample.requests"), "{error}");
        let error = load(
            ConfigSources::new().set("lighthouse.alerts.email.on", json!(["new_issue", "nope"])),
        )
        .unwrap_err();
        assert!(error.contains("unknown alert kind \"nope\""), "{error}");
        let error = load(ConfigSources::new().set(
            "lighthouse.alerts.webhook",
            json!({ "url": "https://ops.example.com/hooks" }),
        ))
        .unwrap_err();
        assert!(
            error.contains("lighthouse.alerts.webhook.secret is required"),
            "{error}"
        );
        let error = load(ConfigSources::new().set(
            "lighthouse.alerts.webhook",
            json!({ "url": "https://ops.example.com/hooks", "secret": "short" }),
        ))
        .unwrap_err();
        assert!(error.contains("at least 32 bytes"), "{error}");
        assert!(!error.contains("short"), "never echoes the secret: {error}");
        let error =
            load(ConfigSources::new().set("lighthouse.prune_at", json!("3am"))).unwrap_err();
        assert!(error.contains("lighthouse.prune_at"), "{error}");
        let error = load(ConfigSources::new().set("lighthouse.path", json!("dash"))).unwrap_err();
        assert!(error.contains("lighthouse.path"), "{error}");
    }

    #[test]
    fn prune_at_parses_hours_and_minutes() {
        assert_eq!(parse_hh_mm("03:10"), Some((3, 10)));
        assert_eq!(parse_hh_mm("3:10"), Some((3, 10)));
        assert_eq!(parse_hh_mm("23:59"), Some((23, 59)));
        assert_eq!(parse_hh_mm("24:00"), None);
        assert_eq!(parse_hh_mm("03:1"), None);
        assert_eq!(parse_hh_mm(""), None);
    }

    #[test]
    fn periods_round_trip() {
        for period in Period::ALL {
            assert_eq!(Period::parse(period.as_str()), Some(period));
        }
        assert_eq!(Period::Hour.resolution(), 60);
        assert_eq!(Period::Week.minutes(), 10_080);
        assert_eq!(Period::parse("2h"), None);
    }

    #[test]
    fn the_mount_path_has_no_trailing_slash() {
        let options = LighthouseOptions {
            path: "/ops/lighthouse/".into(),
            ..LighthouseOptions::default()
        };
        assert_eq!(options.mount_path(), "/ops/lighthouse");
    }
}
