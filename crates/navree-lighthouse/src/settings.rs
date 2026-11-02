//! Settings changed at run time: the subset of the options that can change
//! while the app runs, the values they hold now, and how a change is
//! checked.
//!
//! Everything else in [`LighthouseOptions`](crate::LighthouseOptions) is
//! read when the app boots and stays as the configuration set it. A change
//! made from the dashboard is stored in `lighthouse_settings`, one row per
//! key, and every process reads the rows every [`RELOAD_SECONDS`] seconds,
//! so a change reaches the web servers, the workers and the scheduler
//! alike. Removing a row returns the setting to what the configuration
//! says.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{PoisonError, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::alert::AlertKind;
use crate::options::LighthouseOptions;

/// How often a process reads the stored settings again.
pub(crate) const RELOAD_SECONDS: u64 = 15;

/// The prefix of a channel's stored state: `alerts.channel.slack`.
pub(crate) const CHANNEL_PREFIX: &str = "alerts.channel.";

/// What kind of number a setting holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SettingKind {
    /// A whole number.
    Integer,
    /// A fraction from 0 to 1.
    Fraction,
}

/// A setting the dashboard can change.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct SettingDef {
    /// The key, as stored and as the API takes it; the option's key
    /// without `lighthouse.`.
    pub(crate) key: &'static str,
    /// The group it is shown in.
    pub(crate) group: &'static str,
    /// A label for people.
    pub(crate) label: &'static str,
    /// What it does, in a sentence.
    pub(crate) help: &'static str,
    /// The unit, if any (`ms`, `days`).
    pub(crate) unit: &'static str,
    /// Whole number or fraction.
    pub(crate) kind: SettingKind,
    /// The smallest value.
    pub(crate) min: f64,
    /// The largest value.
    pub(crate) max: f64,
}

/// The settings the dashboard can change, in the order they are shown.
pub(crate) const SETTINGS: [SettingDef; 11] = [
    SettingDef {
        key: "thresholds.slow_request_ms",
        group: "When things are slow",
        label: "Slow request",
        help: "A request slower than this opens a slow-request issue and keeps its trace.",
        unit: "ms",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 3_600_000.0,
    },
    SettingDef {
        key: "thresholds.slow_query_ms",
        group: "When things are slow",
        label: "Slow query",
        help: "A statement that runs longer than this opens a slow-query issue. The time is the statement's own, not the wait for a connection.",
        unit: "ms",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 3_600_000.0,
    },
    SettingDef {
        key: "thresholds.slow_job_ms",
        group: "When things are slow",
        label: "Slow job attempt",
        help: "A job attempt slower than this keeps its trace.",
        unit: "ms",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 86_400_000.0,
    },
    SettingDef {
        key: "thresholds.slow_outgoing_ms",
        group: "When things are slow",
        label: "Slow outgoing call",
        help: "An outgoing request slower than this opens an issue for its host.",
        unit: "ms",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 3_600_000.0,
    },
    SettingDef {
        key: "sample.requests",
        group: "How many details are kept",
        label: "Ordinary requests",
        help: "The fraction of ordinary request traces stored. Errors and slow requests are always kept; counts are always exact.",
        unit: "",
        kind: SettingKind::Fraction,
        min: 0.0,
        max: 1.0,
    },
    SettingDef {
        key: "sample.jobs",
        group: "How many details are kept",
        label: "Ordinary jobs",
        help: "The fraction of ordinary job traces stored. Failed and slow attempts are always kept.",
        unit: "",
        kind: SettingKind::Fraction,
        min: 0.0,
        max: 1.0,
    },
    SettingDef {
        key: "sample.exceptions",
        group: "How many details are kept",
        label: "Occurrences past the first 100 an hour",
        help: "The fraction of an issue's occurrences stored once it has had 100 in the hour.",
        unit: "",
        kind: SettingKind::Fraction,
        min: 0.0,
        max: 1.0,
    },
    SettingDef {
        key: "retention_days",
        group: "Retention",
        label: "Keep data for",
        help: "What the daily prune keeps; older traces, occurrences and aggregates are deleted.",
        unit: "days",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 3650.0,
    },
    SettingDef {
        key: "alerts.cooldown_minutes",
        group: "Alert throttling",
        label: "Cooldown",
        help: "At most one alert per issue and kind in this window.",
        unit: "minutes",
        kind: SettingKind::Integer,
        min: 0.0,
        max: 10_080.0,
    },
    SettingDef {
        key: "alerts.spike.occurrences",
        group: "Alert throttling",
        label: "Spike: occurrences",
        help: "Occurrences of one issue within the spike window that make a spike alert.",
        unit: "",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 1_000_000.0,
    },
    SettingDef {
        key: "alerts.spike.minutes",
        group: "Alert throttling",
        label: "Spike: window",
        help: "The spike window.",
        unit: "minutes",
        kind: SettingKind::Integer,
        min: 1.0,
        max: 60.0,
    },
];

/// The definition of `key`.
pub(crate) fn definition(key: &str) -> Option<&'static SettingDef> {
    SETTINGS.iter().find(|def| def.key == key)
}

/// Checks `value` for the setting `key` and returns it in the form that is
/// stored (a whole number as an integer, a fraction as a float).
pub(crate) fn check(key: &str, value: &Value) -> Result<Value, String> {
    let def = definition(key).ok_or_else(|| format!("`{key}` is not a setting that can change"))?;
    let number = value
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{} must be a number", def.label))?;
    if number < def.min || number > def.max {
        return Err(format!(
            "{} must be between {} and {}{}",
            def.label,
            show(def.min),
            show(def.max),
            if def.unit.is_empty() {
                String::new()
            } else {
                format!(" {}", def.unit)
            }
        ));
    }
    match def.kind {
        SettingKind::Integer => {
            if number.fract() != 0.0 {
                return Err(format!("{} must be a whole number", def.label));
            }
            Ok(json!(number as u64))
        }
        SettingKind::Fraction => Ok(json!(number)),
    }
}

/// A number without a pointless `.0`.
fn show(number: f64) -> String {
    if number.fract() == 0.0 {
        format!("{}", number as u64)
    } else {
        format!("{number}")
    }
}

/// What a channel does now, as the dashboard set it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelState {
    /// The kinds the channel gets, replacing the channel's own: `None`
    /// keeps what the configuration says.
    #[serde(default)]
    pub on: Option<Vec<AlertKind>>,
    /// Whether the channel is muted: it gets no alert until unmuted.
    #[serde(default)]
    pub muted: bool,
    /// Muted until then, when the mute has an end.
    #[serde(default)]
    pub muted_until: Option<DateTime<Utc>>,
}

impl ChannelState {
    /// Whether the channel is muted at `now`.
    pub fn muted_at(&self, now: DateTime<Utc>) -> bool {
        self.muted && self.muted_until.is_none_or(|until| now < until)
    }

    /// Whether the state changes nothing.
    pub(crate) fn is_default(&self) -> bool {
        self.on.is_none() && !self.muted
    }
}

/// The values the running process uses for the settings that can change.
/// Read on every request and job, so each is an atomic.
pub(crate) struct Tuning {
    baseline: Baseline,
    slow_request_ms: AtomicU64,
    slow_query_ms: AtomicU64,
    slow_job_ms: AtomicU64,
    slow_outgoing_ms: AtomicU64,
    sample_requests: AtomicU64,
    sample_jobs: AtomicU64,
    sample_exceptions: AtomicU64,
    retention_days: AtomicU32,
    cooldown_minutes: AtomicU32,
    spike_occurrences: AtomicU64,
    spike_minutes: AtomicU32,
    channels: RwLock<HashMap<String, ChannelState>>,
}

/// What the configuration says, to return to.
#[derive(Clone, Copy)]
struct Baseline {
    slow_request_ms: u64,
    slow_query_ms: u64,
    slow_job_ms: u64,
    slow_outgoing_ms: u64,
    sample_requests: f64,
    sample_jobs: f64,
    sample_exceptions: f64,
    retention_days: u32,
    cooldown_minutes: u32,
    spike_occurrences: u64,
    spike_minutes: u32,
}

impl Baseline {
    fn of(options: &LighthouseOptions) -> Baseline {
        Baseline {
            slow_request_ms: options.thresholds.slow_request_ms,
            slow_query_ms: options.thresholds.slow_query_ms,
            slow_job_ms: options.thresholds.slow_job_ms,
            slow_outgoing_ms: options.thresholds.slow_outgoing_ms,
            sample_requests: options.sample.requests,
            sample_jobs: options.sample.jobs,
            sample_exceptions: options.sample.exceptions,
            retention_days: options.retention_days,
            cooldown_minutes: options.alerts.cooldown_minutes,
            spike_occurrences: options.alerts.spike.occurrences,
            spike_minutes: options.alerts.spike.minutes,
        }
    }

    /// The configured value of the setting `key`.
    fn value(&self, key: &str) -> Value {
        match key {
            "thresholds.slow_request_ms" => json!(self.slow_request_ms),
            "thresholds.slow_query_ms" => json!(self.slow_query_ms),
            "thresholds.slow_job_ms" => json!(self.slow_job_ms),
            "thresholds.slow_outgoing_ms" => json!(self.slow_outgoing_ms),
            "sample.requests" => json!(self.sample_requests),
            "sample.jobs" => json!(self.sample_jobs),
            "sample.exceptions" => json!(self.sample_exceptions),
            "retention_days" => json!(self.retention_days),
            "alerts.cooldown_minutes" => json!(self.cooldown_minutes),
            "alerts.spike.occurrences" => json!(self.spike_occurrences),
            "alerts.spike.minutes" => json!(self.spike_minutes),
            _ => Value::Null,
        }
    }
}

impl Tuning {
    /// The configured values.
    pub(crate) fn new(options: &LighthouseOptions) -> Tuning {
        let baseline = Baseline::of(options);
        let tuning = Tuning {
            baseline,
            slow_request_ms: AtomicU64::new(0),
            slow_query_ms: AtomicU64::new(0),
            slow_job_ms: AtomicU64::new(0),
            slow_outgoing_ms: AtomicU64::new(0),
            sample_requests: AtomicU64::new(0),
            sample_jobs: AtomicU64::new(0),
            sample_exceptions: AtomicU64::new(0),
            retention_days: AtomicU32::new(0),
            cooldown_minutes: AtomicU32::new(0),
            spike_occurrences: AtomicU64::new(0),
            spike_minutes: AtomicU32::new(0),
            channels: RwLock::new(HashMap::new()),
        };
        tuning.apply(&BTreeMap::new());
        tuning
    }

    /// The value the configuration set for the setting `key`.
    pub(crate) fn configured(&self, key: &str) -> Value {
        self.baseline.value(key)
    }

    /// The value in use for the setting `key`.
    pub(crate) fn current(&self, key: &str) -> Value {
        match key {
            "thresholds.slow_request_ms" => json!(self.slow_request_ms()),
            "thresholds.slow_query_ms" => json!(self.slow_query_ms()),
            "thresholds.slow_job_ms" => json!(self.slow_job_ms()),
            "thresholds.slow_outgoing_ms" => json!(self.slow_outgoing_ms()),
            "sample.requests" => json!(self.sample_requests()),
            "sample.jobs" => json!(self.sample_jobs()),
            "sample.exceptions" => json!(self.sample_exceptions()),
            "retention_days" => json!(self.retention_days()),
            "alerts.cooldown_minutes" => json!(self.cooldown_minutes()),
            "alerts.spike.occurrences" => json!(self.spike_occurrences()),
            "alerts.spike.minutes" => json!(self.spike_minutes()),
            _ => Value::Null,
        }
    }

    /// Makes `overrides` (setting key to JSON value, as stored) the values
    /// in use, and every setting without one the configured value. A
    /// stored value that no longer passes [`check`] is ignored. Channel
    /// states are the entries under [`CHANNEL_PREFIX`].
    pub(crate) fn apply(&self, overrides: &BTreeMap<String, Value>) {
        let b = &self.baseline;
        let number = |key: &str| {
            overrides
                .get(key)
                .and_then(|value| check(key, value).ok())
                .and_then(|value| value.as_f64())
        };
        let int = |key: &str, default: u64| number(key).map_or(default, |n| n as u64);
        let fraction = |key: &str, default: f64| number(key).unwrap_or(default);
        let store = |cell: &AtomicU64, value: u64| cell.store(value, Ordering::Relaxed);
        store(
            &self.slow_request_ms,
            int("thresholds.slow_request_ms", b.slow_request_ms),
        );
        store(
            &self.slow_query_ms,
            int("thresholds.slow_query_ms", b.slow_query_ms),
        );
        store(
            &self.slow_job_ms,
            int("thresholds.slow_job_ms", b.slow_job_ms),
        );
        store(
            &self.slow_outgoing_ms,
            int("thresholds.slow_outgoing_ms", b.slow_outgoing_ms),
        );
        store(
            &self.sample_requests,
            fraction("sample.requests", b.sample_requests).to_bits(),
        );
        store(
            &self.sample_jobs,
            fraction("sample.jobs", b.sample_jobs).to_bits(),
        );
        store(
            &self.sample_exceptions,
            fraction("sample.exceptions", b.sample_exceptions).to_bits(),
        );
        self.retention_days.store(
            int("retention_days", u64::from(b.retention_days)) as u32,
            Ordering::Relaxed,
        );
        self.cooldown_minutes.store(
            int("alerts.cooldown_minutes", u64::from(b.cooldown_minutes)) as u32,
            Ordering::Relaxed,
        );
        store(
            &self.spike_occurrences,
            int("alerts.spike.occurrences", b.spike_occurrences),
        );
        self.spike_minutes.store(
            int("alerts.spike.minutes", u64::from(b.spike_minutes)) as u32,
            Ordering::Relaxed,
        );
        let channels: HashMap<String, ChannelState> = overrides
            .iter()
            .filter_map(|(key, value)| {
                let name = key.strip_prefix(CHANNEL_PREFIX)?;
                let state: ChannelState = serde_json::from_value(value.clone()).ok()?;
                Some((name.to_owned(), state))
            })
            .collect();
        *self
            .channels
            .write()
            .unwrap_or_else(PoisonError::into_inner) = channels;
    }

    /// The state the dashboard set for the channel `name`.
    pub(crate) fn channel(&self, name: &str) -> ChannelState {
        self.channels
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn slow_request_ms(&self) -> u64 {
        self.slow_request_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn slow_query_ms(&self) -> u64 {
        self.slow_query_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn slow_job_ms(&self) -> u64 {
        self.slow_job_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn slow_outgoing_ms(&self) -> u64 {
        self.slow_outgoing_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn sample_requests(&self) -> f64 {
        f64::from_bits(self.sample_requests.load(Ordering::Relaxed))
    }

    pub(crate) fn sample_jobs(&self) -> f64 {
        f64::from_bits(self.sample_jobs.load(Ordering::Relaxed))
    }

    pub(crate) fn sample_exceptions(&self) -> f64 {
        f64::from_bits(self.sample_exceptions.load(Ordering::Relaxed))
    }

    pub(crate) fn retention_days(&self) -> u32 {
        self.retention_days.load(Ordering::Relaxed)
    }

    pub(crate) fn cooldown_minutes(&self) -> u32 {
        self.cooldown_minutes.load(Ordering::Relaxed)
    }

    pub(crate) fn spike_occurrences(&self) -> u64 {
        self.spike_occurrences.load(Ordering::Relaxed)
    }

    pub(crate) fn spike_minutes(&self) -> u32 {
        self.spike_minutes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_checked_against_their_range_and_kind() {
        assert_eq!(check("retention_days", &json!(30)).unwrap(), json!(30));
        assert!(check("retention_days", &json!(0)).is_err());
        assert!(check("retention_days", &json!(1.5)).is_err());
        assert!(check("retention_days", &json!("30")).is_err());
        assert!(check("sample.requests", &json!(1.5)).is_err());
        assert_eq!(check("sample.requests", &json!(0.25)).unwrap(), json!(0.25));
        assert!(check("alerts.spike.minutes", &json!(61)).is_err());
        assert!(
            check("path", &json!(1)).is_err(),
            "only listed settings change"
        );
        assert!(check("sample.jobs", &Value::Null).is_err());
    }

    #[test]
    fn applying_overrides_changes_values_and_removing_them_restores_the_configuration() {
        let tuning = Tuning::new(&LighthouseOptions::default());
        assert_eq!(tuning.slow_query_ms(), 200);
        let mut overrides = BTreeMap::new();
        overrides.insert("thresholds.slow_query_ms".to_owned(), json!(750));
        overrides.insert("sample.requests".to_owned(), json!(0.5));
        overrides.insert("retention_days".to_owned(), json!(0));
        overrides.insert(
            "alerts.channel.slack".to_owned(),
            json!({ "muted": true, "on": ["spike"] }),
        );
        tuning.apply(&overrides);
        assert_eq!(tuning.slow_query_ms(), 750);
        assert_eq!(tuning.sample_requests(), 0.5);
        assert_eq!(
            tuning.retention_days(),
            14,
            "an invalid stored value is ignored"
        );
        let slack = tuning.channel("slack");
        assert!(slack.muted_at(Utc::now()));
        assert_eq!(slack.on, Some(vec![AlertKind::Spike]));
        assert_eq!(tuning.current("thresholds.slow_query_ms"), json!(750));
        assert_eq!(tuning.configured("thresholds.slow_query_ms"), json!(200));
        tuning.apply(&BTreeMap::new());
        assert_eq!(tuning.slow_query_ms(), 200);
        assert_eq!(tuning.sample_requests(), 0.1);
        assert!(tuning.channel("slack").is_default());
    }

    #[test]
    fn a_mute_can_end() {
        let now = Utc::now();
        let state = ChannelState {
            muted: true,
            muted_until: Some(now + chrono::TimeDelta::hours(1)),
            ..ChannelState::default()
        };
        assert!(state.muted_at(now));
        assert!(!state.muted_at(now + chrono::TimeDelta::hours(2)));
        assert!(!ChannelState::default().muted_at(now));
    }
}
