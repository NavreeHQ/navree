//! What the overview adds to its figures: a series per figure for the
//! sparklines in the tiles, the median latency, the issues that began in
//! the period, and the deploys to mark on the chart. Plain functions on
//! [`LighthouseStore`]; the page only draws what they return.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_orm::Model as _;
use serde::Serialize;

use crate::aggregate::{Values, kind};
use crate::entity::{aggregate, issue};
use crate::histogram::Histogram;
use crate::options::Period;
use crate::release::ReleaseMarker;
use crate::store::{LighthouseError, LighthouseStore, TimelinePoint};

/// One value per bucket of the timeline, oldest first, for the sparklines.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OverviewSeries {
    /// Requests per minute.
    pub requests_per_minute: Vec<f64>,
    /// The share of requests answering 4xx or 5xx, from 0 to 1.
    pub error_rate: Vec<f64>,
    /// The 95th percentile latency, in milliseconds.
    pub p95_ms: Vec<f64>,
    /// Jobs moved to the failed jobs.
    pub failed_jobs: Vec<i64>,
}

/// The extras of an overview.
pub(crate) struct Extras {
    pub(crate) series: OverviewSeries,
    pub(crate) p50_ms: f64,
    pub(crate) new_issues: u64,
    pub(crate) releases: Vec<ReleaseMarker>,
}

impl LighthouseStore {
    /// The aggregate rows of `kind` in the period, merged per bucket.
    async fn buckets(
        &self,
        kind: &str,
        period: Period,
        start: DateTime<Utc>,
    ) -> Result<BTreeMap<DateTime<Utc>, Values>, LighthouseError> {
        let rows = aggregate::Model::query()
            .where_(aggregate::Model::KIND.eq(kind))
            .where_(aggregate::Model::RESOLUTION.eq(period.resolution()))
            .where_(aggregate::Model::BUCKET.gte(start))
            .get(&self.db)
            .await?;
        let mut buckets: BTreeMap<DateTime<Utc>, Values> = BTreeMap::new();
        for row in rows {
            buckets.entry(row.bucket).or_default().merge(&Values {
                count: row.count,
                errors: row.errors,
                extra: row.extra,
                total_ms: row.total_ms,
                histogram: Histogram::decode(&row.histogram, row.max_ms),
                ..Values::default()
            });
        }
        Ok(buckets)
    }

    /// The series, the median, the new issues and the deploys of a period,
    /// for the buckets of `timeline`.
    pub(crate) async fn overview_extras(
        &self,
        period: Period,
        timeline: &[TimelinePoint],
    ) -> Result<Extras, LighthouseError> {
        let Some(first) = timeline.first() else {
            return Ok(Extras {
                series: OverviewSeries::default(),
                p50_ms: 0.0,
                new_issues: 0,
                releases: Vec::new(),
            });
        };
        let start = first.at;
        let minutes = (i64::from(period.resolution()) as f64 / 60.0).max(1.0);
        let requests = self.buckets(kind::REQUEST, period, start).await?;
        let jobs = self.buckets(kind::JOB, period, start).await?;
        let mut series = OverviewSeries::default();
        let mut all = Histogram::default();
        for point in timeline {
            let values = requests.get(&point.at);
            let count = values.map_or(0, |v| v.count);
            series.requests_per_minute.push(count as f64 / minutes);
            series
                .error_rate
                .push(values.map_or(0.0, |v| ratio(v.errors, v.count)));
            series
                .p95_ms
                .push(values.map_or(0.0, |v| round1(v.histogram.quantile(0.95))));
            series
                .failed_jobs
                .push(jobs.get(&point.at).map_or(0, |v| v.extra));
            if let Some(values) = values {
                all.merge(&values.histogram);
            }
        }
        let new_issues = issue::Model::query()
            .where_(issue::Model::FIRST_SEEN.gte(start))
            .count(&self.db)
            .await?;
        Ok(Extras {
            series,
            p50_ms: round1(all.quantile(0.5)),
            new_issues,
            releases: self.release_markers(start).await?,
        })
    }
}

fn ratio(part: i64, whole: i64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}
