//! Metric snapshots: what finished since the previous snapshot, per queue
//! and per job type, and the dashboard's chart series built from them.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_queue::{AttemptOutcome, QueueStatus};

use crate::record::{
    CompletedJob, MetricKind, MetricPoint, MetricSeries, MetricSnapshot, MetricsPeriod,
};

/// The snapshots a master stores at `at`: one per queue it supervises
/// (with the queue's pending count and oldest wait from `statuses`) and
/// one per queue or job type that ran since the previous snapshot.
pub(crate) fn snapshots(
    at: DateTime<Utc>,
    jobs: &[CompletedJob],
    statuses: &[QueueStatus],
) -> Vec<MetricSnapshot> {
    let mut by_queue: BTreeMap<&str, Vec<&CompletedJob>> = BTreeMap::new();
    let mut by_type: BTreeMap<&str, Vec<&CompletedJob>> = BTreeMap::new();
    for status in statuses {
        by_queue.entry(status.queue.as_str()).or_default();
    }
    for job in jobs {
        by_queue.entry(job.queue.as_str()).or_default().push(job);
        by_type.entry(job.job_type.as_str()).or_default().push(job);
    }
    let mut out = Vec::new();
    for (queue, jobs) in by_queue {
        let status = statuses.iter().find(|s| s.queue == queue);
        out.push(MetricSnapshot {
            pending: status.map(QueueStatus::pending),
            wait_ms: status.map(|s| u64::try_from(s.oldest_wait().as_millis()).unwrap_or(u64::MAX)),
            ..snapshot_of(at, MetricKind::Queue, queue, &jobs)
        });
    }
    for (job_type, jobs) in by_type {
        out.push(snapshot_of(at, MetricKind::Job, job_type, &jobs));
    }
    out
}

fn snapshot_of(
    at: DateTime<Utc>,
    kind: MetricKind,
    name: &str,
    jobs: &[&CompletedJob],
) -> MetricSnapshot {
    let mut runtimes: Vec<u64> = jobs.iter().map(|job| job.runtime_ms).collect();
    MetricSnapshot {
        at,
        kind,
        name: name.to_owned(),
        processed: jobs.len() as u64,
        failed: jobs
            .iter()
            // Failed attempts (retried or not); a job skipped or stopped
            // because its batch or operation was cancelled did not fail.
            .filter(|job| {
                matches!(
                    job.outcome,
                    AttemptOutcome::Released | AttemptOutcome::Failed
                )
            })
            .count() as u64,
        runtime_avg_ms: average(&runtimes),
        runtime_p95_ms: percentile(&mut runtimes, 95),
        pending: None,
        wait_ms: None,
    }
}

/// The mean (0 for none).
pub(crate) fn average(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.iter().sum::<u64>() / values.len() as u64
}

/// The `p`th percentile by nearest rank (0 for none).
pub(crate) fn percentile(values: &mut [u64], p: u64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let rank = (p * values.len() as u64).div_ceil(100).max(1);
    values[usize::try_from(rank - 1).unwrap_or(0).min(values.len() - 1)]
}

/// Chart series over `period` ending at `now`: the snapshots of every
/// master summed per bucket.
pub(crate) fn series(
    period: MetricsPeriod,
    now: DateTime<Utc>,
    snapshots: &[MetricSnapshot],
) -> Vec<MetricSeries> {
    let start = now - period.span();
    let bucket_ms = period.bucket().num_milliseconds().max(1);
    let minutes = bucket_ms as f64 / 60_000.0;
    // (kind, name) -> bucket index -> accumulated values.
    let mut acc: BTreeMap<(MetricKind, &str), BTreeMap<i64, Bucket>> = BTreeMap::new();
    for s in snapshots.iter().filter(|s| s.at >= start && s.at <= now) {
        let index = (s.at - start).num_milliseconds() / bucket_ms;
        let bucket = acc
            .entry((s.kind, s.name.as_str()))
            .or_default()
            .entry(index)
            .or_default();
        bucket.processed += s.processed;
        bucket.failed += s.failed;
        bucket.runtime_total += s.runtime_avg_ms.saturating_mul(s.processed);
        bucket.p95 = bucket.p95.max(s.runtime_p95_ms);
        if let Some(wait) = s.wait_ms {
            bucket.wait_ms = Some(bucket.wait_ms.unwrap_or(0).max(wait));
        }
    }
    acc.into_iter()
        .map(|((kind, name), buckets)| MetricSeries {
            kind,
            name: name.to_owned(),
            points: buckets
                .into_iter()
                .map(|(index, b)| MetricPoint {
                    at: start + chrono::TimeDelta::milliseconds(index * bucket_ms),
                    processed: b.processed,
                    failed: b.failed,
                    throughput_per_minute: b.processed as f64 / minutes,
                    runtime_avg_ms: b.runtime_total.checked_div(b.processed).unwrap_or(0),
                    runtime_p95_ms: b.p95,
                    wait_secs: b.wait_ms.map(|ms| ms as f64 / 1000.0),
                })
                .collect(),
        })
        .collect()
}

#[derive(Default)]
struct Bucket {
    processed: u64,
    failed: u64,
    runtime_total: u64,
    p95: u64,
    wait_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap() + chrono::TimeDelta::seconds(secs)
    }

    fn job(queue: &str, job_type: &str, outcome: AttemptOutcome, ms: u64) -> CompletedJob {
        CompletedJob {
            job_id: "j".into(),
            queue: queue.into(),
            job_type: job_type.into(),
            attempt: 1,
            outcome,
            runtime_ms: ms,
            finished_at: at(0),
        }
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let mut values: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&mut values, 95), 95);
        assert_eq!(percentile(&mut [7], 95), 7);
        assert_eq!(percentile(&mut [], 95), 0);
        assert_eq!(percentile(&mut [30, 10, 20], 95), 30);
        assert_eq!(average(&[10, 20, 60]), 30);
    }

    #[test]
    fn snapshots_cover_queues_and_job_types() {
        let jobs = [
            job("mail", "mail.send", AttemptOutcome::Succeeded, 100),
            job("mail", "mail.send", AttemptOutcome::Failed, 300),
            job("default", "report", AttemptOutcome::Released, 50),
        ];
        let status = QueueStatus {
            queue: "exports".into(),
            waiting: 2,
            delayed: 1,
            running: 0,
            oldest_available_at: Some(at(-41)),
            at: at(0),
        };
        let snaps = snapshots(at(60), &jobs, &[status]);
        let find = |kind, name: &str| {
            snaps
                .iter()
                .find(|s| s.kind == kind && s.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        let mail = find(MetricKind::Queue, "mail");
        assert_eq!((mail.processed, mail.failed), (2, 1));
        assert_eq!(mail.runtime_avg_ms, 200);
        assert_eq!(mail.runtime_p95_ms, 300);
        assert_eq!(mail.pending, None);
        let exports = find(MetricKind::Queue, "exports");
        assert_eq!(exports.processed, 0);
        assert_eq!(exports.pending, Some(3));
        assert_eq!(exports.wait_ms, Some(41_000));
        assert_eq!(find(MetricKind::Job, "report").failed, 1);
        assert_eq!(snaps.len(), 5);
    }

    #[test]
    fn series_sum_masters_per_bucket() {
        let snap = |secs, processed, avg, wait| MetricSnapshot {
            at: at(secs),
            kind: MetricKind::Queue,
            name: "mail".into(),
            processed,
            failed: 0,
            runtime_avg_ms: avg,
            runtime_p95_ms: avg * 2,
            pending: Some(1),
            wait_ms: wait,
        };
        let rows = [
            snap(-90, 10, 100, Some(1000)),
            snap(-80, 30, 200, Some(3000)),
            snap(-10, 5, 10, None),
            snap(-7200, 99, 1, None),
        ];
        let series = series(MetricsPeriod::Hour, at(0), &rows);
        assert_eq!(series.len(), 1);
        let points = &series[0].points;
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].processed, 40);
        assert_eq!(points[0].runtime_avg_ms, 175);
        assert_eq!(points[0].runtime_p95_ms, 400);
        assert_eq!(points[0].wait_secs, Some(3.0));
        assert!((points[0].throughput_per_minute - 40.0).abs() < f64::EPSILON);
        assert_eq!(points[1].processed, 5);
        assert_eq!(points[1].wait_secs, None);
    }
}
