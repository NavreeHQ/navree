//! Per-minute and per-hour aggregates held in memory until they are
//! written: counts, errors and a latency histogram per kind and key.
//!
//! Each process owns its rows (`lighthouse_aggregates.process`) and writes
//! its whole in-memory value for a bucket every time it flushes, so writes
//! never race; readers add the rows of every process.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::histogram::Histogram;

/// Per-minute rows (read for the last hour).
pub(crate) const MINUTE: i32 = 60;
/// Per-hour rows (read for 24 hours and 7 days).
pub(crate) const HOUR: i32 = 3600;

/// The kinds of aggregate rows.
pub(crate) mod kind {
    /// Requests per endpoint (`GET /api/v1/deals`).
    pub(crate) const REQUEST: &str = "request";
    /// Queries per normalized statement.
    pub(crate) const QUERY: &str = "query";
    /// Job attempts per job type.
    pub(crate) const JOB: &str = "job";
    /// Outgoing calls per host.
    pub(crate) const OUTGOING: &str = "outgoing";
    /// Exceptions (key `all`), for the overview's timeline.
    pub(crate) const EXCEPTION: &str = "exception";
    /// Occurrences per issue fingerprint (sparklines and spikes).
    pub(crate) const ISSUE: &str = "issue";
    /// Statements that waited for a database connection (key `all`): the
    /// time they waited, and as errors the ones that never got one.
    pub(crate) const POOL_WAIT: &str = "pool_wait";
    /// Details (traces, occurrences) dropped because the write queue was
    /// full (key `all`).
    pub(crate) const SAMPLED_OUT: &str = "sampled_out";
}

/// A slow outgoing call kept in an `outgoing` row's detail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlowCall {
    /// `GET`, `POST`, ...
    pub method: String,
    /// The URL without its query string.
    pub url: String,
    /// The response status; none when no response came.
    pub status: Option<u16>,
    /// How long it took.
    pub duration_ms: f64,
}

/// Kind-specific details of a row.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Detail {
    /// Queries: the routes and jobs that ran the statement, with counts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) origins: BTreeMap<String, u64>,
    /// Outgoing calls: the slowest calls (at most five).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) slowest: Vec<SlowCall>,
}

impl Detail {
    pub(crate) fn is_empty(&self) -> bool {
        self.origins.is_empty() && self.slowest.is_empty()
    }

    pub(crate) fn merge(&mut self, other: &Detail) {
        for (origin, count) in &other.origins {
            *self.origins.entry(origin.clone()).or_default() += count;
        }
        for call in &other.slowest {
            self.add_slow_call(call.clone());
        }
    }

    pub(crate) fn add_slow_call(&mut self, call: SlowCall) {
        self.slowest.push(call);
        self.slowest
            .sort_by(|a, b| b.duration_ms.total_cmp(&a.duration_ms));
        self.slowest.truncate(5);
    }

    pub(crate) fn encode(&self) -> Option<String> {
        (!self.is_empty()).then(|| serde_json::to_string(self).unwrap_or_default())
    }

    pub(crate) fn decode(text: Option<&str>) -> Detail {
        text.and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_default()
    }
}

/// One row's values.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Values {
    pub(crate) count: i64,
    pub(crate) errors: i64,
    pub(crate) extra: i64,
    pub(crate) total_ms: f64,
    pub(crate) histogram: Histogram,
    pub(crate) detail: Detail,
}

impl Values {
    pub(crate) fn max_ms(&self) -> f64 {
        self.histogram.quantile(1.0)
    }

    pub(crate) fn merge(&mut self, other: &Values) {
        self.count += other.count;
        self.errors += other.errors;
        self.extra += other.extra;
        self.total_ms += other.total_ms;
        self.histogram.merge(&other.histogram);
        self.detail.merge(&other.detail);
    }
}

/// What a row is keyed by (besides the process).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct RowKey {
    pub(crate) resolution: i32,
    pub(crate) bucket: DateTime<Utc>,
    pub(crate) kind: &'static str,
    pub(crate) key: String,
}

/// A row to write.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) key: RowKey,
    pub(crate) values: Values,
}

/// One observation.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observation {
    pub(crate) duration_ms: Option<f64>,
    pub(crate) error: bool,
    pub(crate) extra: bool,
    pub(crate) origin: Option<String>,
    pub(crate) slow_call: Option<SlowCall>,
}

/// The aggregates of the current minutes and hours.
#[derive(Default)]
pub(crate) struct Aggregates {
    rows: HashMap<RowKey, (Values, bool)>,
}

impl Aggregates {
    /// Adds one observation of `kind`/`key` at `now` to its minute and hour.
    pub(crate) fn observe(
        &mut self,
        now: DateTime<Utc>,
        kind: &'static str,
        key: &str,
        observation: &Observation,
    ) {
        for resolution in [MINUTE, HOUR] {
            let row = RowKey {
                resolution,
                bucket: bucket_start(now, resolution),
                kind,
                key: key.to_owned(),
            };
            let (values, dirty) = self.rows.entry(row).or_default();
            *dirty = true;
            values.count += 1;
            if observation.error {
                values.errors += 1;
            }
            if observation.extra {
                values.extra += 1;
            }
            if let Some(ms) = observation.duration_ms {
                values.total_ms += ms;
                values.histogram.record(ms);
            }
            if let Some(origin) = &observation.origin {
                *values.detail.origins.entry(origin.clone()).or_default() += 1;
            }
            if let Some(call) = &observation.slow_call {
                values.detail.add_slow_call(call.clone());
            }
        }
    }

    /// Adds `n` to the count of `kind`/`key` at `now` (minute and hour),
    /// without a duration.
    pub(crate) fn add(&mut self, now: DateTime<Utc>, kind: &'static str, key: &str, n: u64) {
        for resolution in [MINUTE, HOUR] {
            let row = RowKey {
                resolution,
                bucket: bucket_start(now, resolution),
                kind,
                key: key.to_owned(),
            };
            let (values, dirty) = self.rows.entry(row).or_default();
            *dirty = true;
            values.count += i64::try_from(n).unwrap_or(i64::MAX);
        }
    }

    /// The rows changed since the last call, marked clean.
    pub(crate) fn take_dirty(&mut self) -> Vec<Row> {
        self.rows
            .iter_mut()
            .filter(|(_, (_, dirty))| *dirty)
            .map(|(key, (values, dirty))| {
                *dirty = false;
                Row {
                    key: key.clone(),
                    values: values.clone(),
                }
            })
            .collect()
    }

    /// Marks rows dirty again (their write failed).
    pub(crate) fn mark_dirty(&mut self, keys: &[RowKey]) {
        for key in keys {
            if let Some((_, dirty)) = self.rows.get_mut(key) {
                *dirty = true;
            }
        }
    }

    /// Forgets clean rows whose bucket ended more than two minutes before
    /// `now` (they are written; no new observation can land in them).
    pub(crate) fn evict(&mut self, now: DateTime<Utc>) {
        self.rows.retain(|key, (_, dirty)| {
            *dirty
                || key.bucket
                    + TimeDelta::seconds(i64::from(key.resolution))
                    + TimeDelta::minutes(2)
                    > now
        });
    }

    /// How many rows are held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
}

/// The start of the bucket holding `at`.
pub(crate) fn bucket_start(at: DateTime<Utc>, resolution: i32) -> DateTime<Utc> {
    at.duration_trunc(TimeDelta::seconds(i64::from(resolution)))
        .unwrap_or(at)
}

/// The hash rows are matched by: 16 hex characters of the key's SHA-256.
pub(crate) fn key_hash(key: &str) -> String {
    hex::encode(&Sha256::digest(key.as_bytes())[..8])
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 27, h, m, s).unwrap()
    }

    #[test]
    fn observations_land_in_their_minute_and_hour() {
        let mut aggregates = Aggregates::default();
        let request = |ms| Observation {
            duration_ms: Some(ms),
            ..Observation::default()
        };
        aggregates.observe(at(9, 14, 2), kind::REQUEST, "GET /x", &request(10.0));
        aggregates.observe(at(9, 14, 59), kind::REQUEST, "GET /x", &request(30.0));
        aggregates.observe(
            at(9, 15, 0),
            kind::REQUEST,
            "GET /x",
            &Observation {
                error: true,
                ..request(50.0)
            },
        );
        let mut rows = aggregates.take_dirty();
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(rows.len(), 3);
        let minute = rows
            .iter()
            .find(|row| row.key.resolution == MINUTE && row.key.bucket == at(9, 14, 0))
            .unwrap();
        assert_eq!(minute.values.count, 2);
        assert_eq!(minute.values.total_ms, 40.0);
        assert_eq!(minute.values.max_ms(), 30.0);
        let hour = rows.iter().find(|row| row.key.resolution == HOUR).unwrap();
        assert_eq!(hour.key.bucket, at(9, 0, 0));
        assert_eq!(hour.values.count, 3);
        assert_eq!(hour.values.errors, 1);
        assert!(
            aggregates.take_dirty().is_empty(),
            "rows are clean once taken"
        );

        aggregates.evict(at(9, 17, 30));
        assert_eq!(aggregates.len(), 2, "the 09:14 minute is gone");
        aggregates.evict(at(10, 3, 0));
        assert_eq!(aggregates.len(), 0);
    }

    #[test]
    fn details_keep_origins_and_the_five_slowest_calls() {
        let mut detail = Detail::default();
        for i in 0..8 {
            detail.add_slow_call(SlowCall {
                method: "GET".into(),
                url: format!("https://api.example.com/{i}"),
                status: Some(200),
                duration_ms: f64::from(i),
            });
        }
        detail.origins.insert("GET /x".into(), 2);
        assert_eq!(detail.slowest.len(), 5);
        assert_eq!(detail.slowest[0].duration_ms, 7.0);
        let decoded = Detail::decode(detail.encode().as_deref());
        assert_eq!(decoded, detail);
        assert!(Detail::default().encode().is_none());
        assert_eq!(key_hash("GET /x").len(), 16);
    }
}
