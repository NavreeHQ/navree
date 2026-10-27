//! [`LighthouseStore`]: everything Lighthouse keeps in the database, read
//! for the dashboard, the JSON API and your code, plus issue status changes
//! and pruning.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_db::{Db, prune_time_series};
use sea_orm::sea_query::{Expr, OnConflict, Query};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::aggregate::{Detail, HOUR, MINUTE, Row, SlowCall, Values, bucket_start, key_hash, kind};
use crate::alert::AlertKind;
use crate::engine::{Batch, Hit, Labels, NewSample, Span};
use crate::entity::{aggregate, alert, issue, occurrence, sample};
use crate::histogram::Histogram;
use crate::migration::{AGGREGATES_TABLE, OCCURRENCES_TABLE, SAMPLES_TABLE};
use crate::model::{Issue, IssueKind, IssueStatus, Level};
use crate::options::{LighthouseOptions, Period};

/// An error reading or changing Lighthouse's data.
#[derive(Debug, thiserror::Error)]
pub enum LighthouseError {
    /// No issue, sample or alert with that id.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A database error.
    #[error(transparent)]
    Db(#[from] DbErr),
    /// Anything else.
    #[error("{0}")]
    Other(String),
}

/// What Lighthouse keeps: issues, occurrences, samples, aggregates and
/// alerts. Provided and exported by `LighthouseModule`; the dashboard and
/// its JSON API read through it.
#[derive(Clone)]
pub struct LighthouseStore {
    db: Db,
    base_url: Arc<str>,
    options: Arc<LighthouseOptions>,
    release: Arc<str>,
}

/// One page of a list: `{"data": [...], "meta": {...}}`.
#[derive(Clone, Debug, Serialize)]
pub struct ListPage<T> {
    /// The items.
    pub data: Vec<T>,
    /// The page's position.
    pub meta: PageMeta,
}

/// A page's position.
#[derive(Clone, Debug, Serialize)]
pub struct PageMeta {
    /// The page (from 1).
    pub current_page: u64,
    /// Items per page.
    pub per_page: u64,
    /// Items in all pages.
    pub total: u64,
    /// The last page.
    pub last_page: u64,
}

/// How issues are sorted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum IssueSort {
    /// Most recent occurrence first (the default).
    #[default]
    LastSeen,
    /// Newest issue first.
    FirstSeen,
    /// Most occurrences first.
    Count,
}

/// Which issues to list. Every field is optional.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IssueFilter {
    /// `open`, `resolved`, `resolved_in_next_release` or `ignored`.
    pub status: Option<String>,
    /// `exception`, `slow_request`, ...
    pub kind: Option<String>,
    /// `error`, `warning` or `info`.
    pub level: Option<String>,
    /// Issues with an occurrence in this release.
    pub release: Option<String>,
    /// The environment of the latest occurrence.
    pub environment: Option<String>,
    /// The route (part of the culprit).
    pub route: Option<String>,
    /// The job type (part of the culprit).
    pub job: Option<String>,
    /// A tag, written `key:value`.
    pub tag: Option<String>,
    /// Issues with an occurrence for this user.
    pub user: Option<String>,
    /// Issues with an occurrence in this workspace.
    pub workspace: Option<String>,
    /// Text in the title.
    pub search: Option<String>,
    /// The order.
    pub sort: IssueSort,
    /// The page (from 1).
    pub page: Option<u64>,
    /// Items per page (at most 100; 25 by default).
    pub per_page: Option<u64>,
}

/// An issue in a list: the issue, its workspaces and a 24-hour sparkline.
#[derive(Clone, Debug, Serialize)]
pub struct IssueSummary {
    /// The issue.
    #[serde(flatten)]
    pub issue: Issue,
    /// Distinct workspaces among the stored occurrences.
    pub workspaces: i64,
    /// Occurrences per hour over the last 24 hours, oldest first.
    pub sparkline: Vec<i64>,
}

/// One stored occurrence.
#[derive(Clone, Debug, Serialize)]
pub struct Occurrence {
    /// The id.
    pub id: Uuid,
    /// The issue.
    pub issue_id: Uuid,
    /// When it happened.
    pub at: DateTime<Utc>,
    /// Its level.
    pub level: String,
    /// The message.
    pub message: String,
    /// The source chain.
    pub chain: Vec<String>,
    /// A backtrace, when one was captured.
    pub backtrace: Option<String>,
    /// Where it happened, when known.
    pub location: Option<String>,
    /// The request id.
    pub request_id: Option<String>,
    /// The job id.
    pub job_id: Option<String>,
    /// The trace id.
    pub trace_id: Option<String>,
    /// The user.
    pub user_id: Option<String>,
    /// The workspace.
    pub workspace_id: Option<String>,
    /// The release.
    pub release: Option<String>,
    /// The environment.
    pub environment: Option<String>,
    /// The server.
    pub server: Option<String>,
    /// The process.
    pub process: Option<String>,
    /// The request method.
    pub method: Option<String>,
    /// The route template.
    pub route: Option<String>,
    /// The request path.
    pub path: Option<String>,
    /// The response status.
    pub status: Option<i32>,
    /// The trace of the request or job, when it was stored.
    pub sample_id: Option<Uuid>,
    /// Request (headers masked), tags, report context and job details.
    pub context: Value,
}

/// An alert as stored: per channel, with its delivery state.
#[derive(Clone, Debug, Serialize)]
pub struct AlertRecord {
    /// The id.
    pub id: Uuid,
    /// The issue.
    pub issue_id: Option<Uuid>,
    /// The kind.
    pub kind: String,
    /// The channel.
    pub channel: String,
    /// `pending`, `delivered` or `failed`.
    pub status: String,
    /// Delivery attempts.
    pub attempts: i32,
    /// The last error.
    pub error: Option<String>,
    /// When it was raised.
    pub created_at: DateTime<Utc>,
    /// When it was delivered.
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Everything about one issue.
#[derive(Clone, Debug, Serialize)]
pub struct IssueDetail {
    /// The issue.
    #[serde(flatten)]
    pub issue: Issue,
    /// The fingerprint.
    pub fingerprint: String,
    /// The full message.
    pub message: String,
    /// The source chain.
    pub chain: Vec<String>,
    /// Where it happened, when known.
    pub location: Option<String>,
    /// The error type, when known.
    pub error_type: Option<String>,
    /// The release of the first occurrence.
    pub first_release: Option<String>,
    /// The release of the latest occurrence.
    pub last_release: Option<String>,
    /// The environment of the latest occurrence.
    pub environment: Option<String>,
    /// When it was resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// The release current when it was resolved in the next release.
    pub resolved_in_release: Option<String>,
    /// Ignored until then.
    pub ignored_until: Option<DateTime<Utc>>,
    /// Ignored for this many more occurrences.
    pub ignore_remaining: Option<i64>,
    /// When it last came back.
    pub regressed_at: Option<DateTime<Utc>>,
    /// The comment of the last status change.
    pub comment: Option<String>,
    /// Distinct workspaces among the stored occurrences.
    pub workspaces: i64,
    /// The latest stored occurrence.
    pub latest: Option<Occurrence>,
    /// Occurrences per hour over the last 7 days, oldest first.
    pub timeline: Vec<TimelinePoint>,
    /// The alerts sent about it, newest first.
    pub alerts: Vec<AlertRecord>,
}

/// One bucket of a timeline.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimelinePoint {
    /// The bucket's start.
    pub at: DateTime<Utc>,
    /// Requests (or occurrences, on an issue's timeline).
    pub count: i64,
    /// Exceptions (requests answering 4xx or 5xx, on a route's timeline).
    pub errors: i64,
}

/// A new occurrence, for the live feed.
#[derive(Clone, Debug, Serialize)]
pub struct FeedItem {
    /// The occurrence.
    pub occurrence_id: Uuid,
    /// Its issue.
    pub issue_id: Uuid,
    /// The issue's title.
    pub title: String,
    /// The issue's kind.
    pub kind: String,
    /// The occurrence's level.
    pub level: String,
    /// Where (the route, or the issue's culprit).
    pub culprit: String,
    /// When.
    pub at: DateTime<Utc>,
}

/// The overview's figures for a period.
#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    /// `1h`, `24h` or `7d`.
    pub period: Period,
    /// Requests in the period.
    pub requests: i64,
    /// Requests per minute.
    pub requests_per_minute: f64,
    /// Share of requests answering 4xx or 5xx.
    pub error_rate: f64,
    /// The 95th percentile latency, in milliseconds.
    pub p95_ms: f64,
    /// Exceptions in the period.
    pub exceptions: i64,
    /// Jobs moved to the failed jobs in the period.
    pub failed_jobs: i64,
    /// Issues open now.
    pub open_issues: u64,
    /// Requests and exceptions per bucket (minutes for `1h`, hours
    /// otherwise), oldest first.
    pub timeline: Vec<TimelinePoint>,
    /// The latest occurrences, newest first.
    pub feed: Vec<FeedItem>,
    /// Details this process dropped because the buffer was full.
    pub dropped: u64,
}

/// A route's figures.
#[derive(Clone, Debug, Serialize)]
pub struct RouteStats {
    /// `GET /api/v1/deals/{id}`.
    pub endpoint: String,
    /// Requests.
    pub count: i64,
    /// Requests per minute.
    pub throughput_per_minute: f64,
    /// Requests answering 4xx or 5xx.
    pub errors: i64,
    /// Requests answering 5xx.
    pub server_errors: i64,
    /// `errors / count`.
    pub error_rate: f64,
    /// Average latency (ms).
    pub avg_ms: f64,
    /// Median latency (ms).
    pub p50_ms: f64,
    /// 95th percentile latency (ms).
    pub p95_ms: f64,
    /// 99th percentile latency (ms).
    pub p99_ms: f64,
    /// Slowest request (ms).
    pub max_ms: f64,
}

/// A sample without its spans, in lists.
#[derive(Clone, Debug, Serialize)]
pub struct SampleSummary {
    /// The id.
    pub id: Uuid,
    /// `request` or `job`.
    pub kind: String,
    /// The endpoint or job type.
    pub name: String,
    /// When it finished.
    pub at: DateTime<Utc>,
    /// How long it took (ms).
    pub duration_ms: f64,
    /// The response status.
    pub status: Option<i32>,
    /// `ok`, `error`, `failed_attempt` or `failed`.
    pub outcome: String,
    /// Whether it was slow.
    pub slow: bool,
    /// The trace id.
    pub trace_id: Option<String>,
    /// The request id.
    pub request_id: Option<String>,
}

/// A route's page: its figures, timeline and slowest traces.
#[derive(Clone, Debug, Serialize)]
pub struct RouteDetail {
    /// The figures.
    pub stats: RouteStats,
    /// Requests and errors per bucket, oldest first.
    pub timeline: Vec<TimelinePoint>,
    /// The slowest stored traces in the period.
    pub slowest: Vec<SampleSummary>,
}

/// A normalized statement's figures.
#[derive(Clone, Debug, Serialize)]
pub struct QueryStats {
    /// The statement, literals replaced by `?`.
    pub statement: String,
    /// Executions.
    pub count: i64,
    /// Failed executions.
    pub errors: i64,
    /// Total time (ms).
    pub total_ms: f64,
    /// Average time (ms).
    pub avg_ms: f64,
    /// 95th percentile (ms).
    pub p95_ms: f64,
    /// Slowest (ms).
    pub max_ms: f64,
    /// The routes and jobs that ran it, most frequent first.
    pub origins: Vec<Origin>,
}

/// Where a statement or call came from, and how often.
#[derive(Clone, Debug, Serialize)]
pub struct Origin {
    /// The route, job or process.
    pub name: String,
    /// How often.
    pub count: u64,
}

/// A job type's figures.
#[derive(Clone, Debug, Serialize)]
pub struct JobStats {
    /// The job type.
    pub job_type: String,
    /// Attempts.
    pub attempts: i64,
    /// Attempts per minute.
    pub throughput_per_minute: f64,
    /// Failed attempts.
    pub failed_attempts: i64,
    /// Jobs moved to the failed jobs.
    pub failed_jobs: i64,
    /// Average runtime (ms).
    pub avg_ms: f64,
    /// Median runtime (ms).
    pub p50_ms: f64,
    /// 95th percentile runtime (ms).
    pub p95_ms: f64,
    /// Longest runtime (ms).
    pub max_ms: f64,
}

/// An outgoing host's figures.
#[derive(Clone, Debug, Serialize)]
pub struct HostStats {
    /// The host (`api.stripe.com`).
    pub host: String,
    /// Calls.
    pub count: i64,
    /// Calls that failed or answered 4xx or 5xx.
    pub errors: i64,
    /// `errors / count`.
    pub error_rate: f64,
    /// Average latency (ms).
    pub avg_ms: f64,
    /// 95th percentile latency (ms).
    pub p95_ms: f64,
    /// Slowest call (ms).
    pub max_ms: f64,
    /// The slowest calls.
    pub slowest: Vec<SlowCall>,
    /// The routes and jobs that made the calls.
    pub origins: Vec<Origin>,
}

/// A stored trace.
#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    /// The summary.
    #[serde(flatten)]
    pub summary: SampleSummary,
    /// The job id.
    pub job_id: Option<String>,
    /// The user.
    pub user_id: Option<String>,
    /// The workspace.
    pub workspace_id: Option<String>,
    /// The release.
    pub release: Option<String>,
    /// The environment.
    pub environment: Option<String>,
    /// The server.
    pub server: Option<String>,
    /// The process.
    pub process: Option<String>,
    /// The spans, by start.
    pub spans: Vec<Span>,
    /// Request (headers masked), tags and job details.
    pub context: Value,
    /// The error of a failed job attempt.
    pub error: Option<String>,
}

/// How long an issue is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnoreFor {
    /// Until someone reopens it.
    Forever,
    /// Until this time; the next occurrence after it reopens the issue.
    Until(DateTime<Utc>),
    /// For this many more occurrences.
    Occurrences(i64),
}

/// What `lighthouse:prune` deleted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PruneSummary {
    /// Samples (traces).
    pub samples: u64,
    /// Occurrences.
    pub occurrences: u64,
    /// Aggregate rows.
    pub aggregates: u64,
    /// Alerts.
    pub alerts: u64,
    /// Resolved or ignored issues without occurrences.
    pub issues: u64,
}

/// An alert to consider after a batch: the issue as written and the kind.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub(crate) kind: AlertKind,
    pub(crate) issue: issue::Model,
}

const CHUNK: usize = 100;

/// Aggregate rows of one key merged over a period: the key, the summed
/// values, and (count, errors) per bucket.
type Merged = (String, Values, BTreeMap<DateTime<Utc>, (i64, i64)>);

impl LighthouseStore {
    pub(crate) fn new(
        db: Db,
        base_url: String,
        options: Arc<LighthouseOptions>,
        release: String,
    ) -> LighthouseStore {
        LighthouseStore {
            db,
            base_url: base_url.trim_end_matches('/').into(),
            options,
            release: release.into(),
        }
    }

    /// The database Lighthouse writes to (the app's, or
    /// `lighthouse.database_url`'s).
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The dashboard's base URL (`https://api.example.com/_lighthouse`).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The release label of this process.
    pub fn release(&self) -> &str {
        &self.release
    }

    /// How many issues are open.
    pub async fn open_issue_count(&self) -> Result<u64, LighthouseError> {
        Ok(issue::Entity::find()
            .filter(issue::Column::Status.eq(IssueStatus::Open.as_str()))
            .count(self.db.conn())
            .await?)
    }

    /// The URL of an issue's page.
    pub fn issue_url(&self, id: Uuid) -> String {
        format!("{}/issues/{id}", self.base_url)
    }

    // -----------------------------------------------------------------
    // Writes
    // -----------------------------------------------------------------

    /// Writes a batch in one transaction; returns the alerts to consider.
    pub(crate) async fn write_batch(
        &self,
        batch: Batch,
        labels: &Labels,
    ) -> Result<Vec<Candidate>, DbErr> {
        let now = Utc::now();
        let txn = self.db.conn().begin().await?;
        let mut candidates = Vec::new();
        let mut touched: Vec<issue::Model> = Vec::new();

        // Group the hits by fingerprint, in order of appearance.
        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<Hit>> = HashMap::new();
        for hit in batch.hits {
            if !groups.contains_key(&hit.fingerprint) {
                order.push(hit.fingerprint.clone());
            }
            groups.entry(hit.fingerprint.clone()).or_default().push(hit);
        }
        let mut existing: HashMap<String, issue::Model> = HashMap::new();
        for chunk in order.chunks(CHUNK) {
            for row in issue::Entity::find()
                .filter(issue::Column::Fingerprint.is_in(chunk.iter().cloned()))
                .all(&txn)
                .await?
            {
                existing.insert(row.fingerprint.clone(), row);
            }
        }
        let mut occurrences = Vec::new();
        for fingerprint in &order {
            let hits = &groups[fingerprint];
            let n = hits.len() as i64;
            let first_at = hits.iter().map(|hit| hit.at).min().unwrap_or(now);
            let last_at = hits.iter().map(|hit| hit.at).max().unwrap_or(now);
            let level = hits.iter().map(|hit| hit.level).max().unwrap_or_default();
            let failed_job = hits.iter().any(|hit| hit.failed_job);
            let latest = hits.last().expect("a group has a hit");
            let mut row = match existing.get(fingerprint) {
                Some(row) => Some(row.clone()),
                None => {
                    let first = &hits[0];
                    let inserted = issue::Entity::insert(issue::ActiveModel {
                        id: Set(Uuid::now_v7()),
                        fingerprint: Set(fingerprint.clone()),
                        kind: Set(first.kind.as_str().to_owned()),
                        level: Set(level.as_str().to_owned()),
                        status: Set(IssueStatus::Open.as_str().to_owned()),
                        title: Set(first.title.clone()),
                        culprit: Set(first.culprit.clone()),
                        message: Set(first.message.clone()),
                        chain: Set(serde_json::to_string(&first.chain).unwrap_or_default()),
                        location: Set(first.location.clone()),
                        error_type: Set(first.error_type.clone()),
                        count: Set(n),
                        first_seen: Set(first_at),
                        last_seen: Set(last_at),
                        first_release: Set(Some(labels.release.clone())),
                        last_release: Set(Some(labels.release.clone())),
                        environment: Set(Some(labels.environment.clone())),
                        resolved_at: Set(None),
                        resolved_in_release: Set(None),
                        ignored_until: Set(None),
                        ignore_remaining: Set(None),
                        regressed_at: Set(None),
                        comment: Set(None),
                        created_at: Set(now),
                        updated_at: Set(now),
                    })
                    .on_conflict(
                        OnConflict::column(issue::Column::Fingerprint)
                            .do_nothing()
                            .to_owned(),
                    )
                    .exec_without_returning(&txn)
                    .await?;
                    if inserted == 0 {
                        // Another process opened it first.
                        issue::Entity::find()
                            .filter(issue::Column::Fingerprint.eq(fingerprint.clone()))
                            .one(&txn)
                            .await?
                    } else {
                        let row = issue::Entity::find()
                            .filter(issue::Column::Fingerprint.eq(fingerprint.clone()))
                            .one(&txn)
                            .await?
                            .ok_or_else(|| DbErr::RecordNotFound("lighthouse issue".into()))?;
                        if level > Level::Info {
                            candidates.push(Candidate {
                                kind: AlertKind::NewIssue,
                                issue: row.clone(),
                            });
                            if failed_job {
                                candidates.push(Candidate {
                                    kind: AlertKind::FailedJob,
                                    issue: row.clone(),
                                });
                            }
                        }
                        touched.push(row.clone());
                        for hit in hits {
                            if let Some(occurrence) = &hit.occurrence {
                                occurrences.push(occurrence_model(row.id, occurrence, labels));
                            }
                        }
                        continue;
                    }
                }
            };
            let Some(mut current) = row.take() else {
                continue;
            };
            // An existing issue: count, status transitions, alerts.
            let status = IssueStatus::parse(&current.status).unwrap_or(IssueStatus::Open);
            let mut reopened = false;
            let mut regression = false;
            let mut ignore_remaining = current.ignore_remaining;
            match status {
                IssueStatus::Open => {}
                IssueStatus::Resolved => {
                    reopened = true;
                    regression = true;
                }
                IssueStatus::ResolvedInNextRelease => {
                    if current.resolved_in_release.as_deref() != Some(labels.release.as_str()) {
                        reopened = true;
                        regression = true;
                    }
                }
                IssueStatus::Ignored => {
                    if current.ignored_until.is_some_and(|until| last_at >= until) {
                        reopened = true;
                    }
                    if let Some(remaining) = current.ignore_remaining {
                        let left = remaining - n;
                        if left <= 0 {
                            reopened = true;
                        } else {
                            ignore_remaining = Some(left);
                        }
                    }
                }
            }
            let new_level = Level::parse(&current.level).unwrap_or_default().max(level);
            let mut update = issue::Entity::update_many()
                .col_expr(
                    issue::Column::Count,
                    sea_orm::sea_query::ExprTrait::add(Expr::col(issue::Column::Count), n),
                )
                .col_expr(
                    issue::Column::LastSeen,
                    Expr::value(current.last_seen.max(last_at)),
                )
                .col_expr(issue::Column::Level, Expr::value(new_level.as_str()))
                .col_expr(
                    issue::Column::LastRelease,
                    Expr::value(labels.release.clone()),
                )
                .col_expr(
                    issue::Column::Environment,
                    Expr::value(labels.environment.clone()),
                )
                .col_expr(issue::Column::UpdatedAt, Expr::value(now));
            if reopened {
                update = update
                    .col_expr(
                        issue::Column::Status,
                        Expr::value(IssueStatus::Open.as_str()),
                    )
                    .col_expr(
                        issue::Column::ResolvedAt,
                        Expr::value(Option::<DateTime<Utc>>::None),
                    )
                    .col_expr(
                        issue::Column::ResolvedInRelease,
                        Expr::value(Option::<String>::None),
                    )
                    .col_expr(
                        issue::Column::IgnoredUntil,
                        Expr::value(Option::<DateTime<Utc>>::None),
                    )
                    .col_expr(
                        issue::Column::IgnoreRemaining,
                        Expr::value(Option::<i64>::None),
                    );
                if regression {
                    update = update.col_expr(issue::Column::RegressedAt, Expr::value(now));
                }
            } else if ignore_remaining != current.ignore_remaining {
                update = update.col_expr(
                    issue::Column::IgnoreRemaining,
                    Expr::value(ignore_remaining),
                );
            }
            update
                .filter(issue::Column::Id.eq(current.id))
                .exec(&txn)
                .await?;
            current.count += n;
            current.last_seen = current.last_seen.max(last_at);
            current.level = new_level.as_str().to_owned();
            current.last_release = Some(labels.release.clone());
            if reopened {
                current.status = IssueStatus::Open.as_str().to_owned();
                current.resolved_at = None;
                current.resolved_in_release = None;
                current.ignored_until = None;
                current.ignore_remaining = None;
            } else {
                current.ignore_remaining = ignore_remaining;
            }
            let alerting = current.status == IssueStatus::Open.as_str() && new_level > Level::Info;
            if alerting && regression {
                candidates.push(Candidate {
                    kind: AlertKind::Regression,
                    issue: current.clone(),
                });
            }
            if alerting && failed_job {
                candidates.push(Candidate {
                    kind: AlertKind::FailedJob,
                    issue: current.clone(),
                });
            }
            let _ = latest;
            touched.push(current.clone());
            for hit in hits {
                if let Some(occurrence) = &hit.occurrence {
                    occurrences.push(occurrence_model(current.id, occurrence, labels));
                }
            }
        }
        for chunk in occurrences.chunks(CHUNK) {
            occurrence::Entity::insert_many(chunk.to_vec())
                .exec_without_returning(&txn)
                .await?;
        }
        let samples: Vec<sample::ActiveModel> = batch
            .samples
            .iter()
            .map(|sample| sample_model(sample, labels))
            .collect();
        for chunk in samples.chunks(CHUNK) {
            sample::Entity::insert_many(chunk.to_vec())
                .exec_without_returning(&txn)
                .await?;
        }
        let rows: Vec<aggregate::ActiveModel> = batch
            .rows
            .iter()
            .map(|row| aggregate_model(row, labels))
            .collect();
        for chunk in rows.chunks(CHUNK) {
            aggregate::Entity::insert_many(chunk.to_vec())
                .on_conflict(
                    OnConflict::columns([
                        aggregate::Column::Resolution,
                        aggregate::Column::Bucket,
                        aggregate::Column::Kind,
                        aggregate::Column::KeyHash,
                        aggregate::Column::Process,
                    ])
                    .update_columns([
                        aggregate::Column::Count,
                        aggregate::Column::Errors,
                        aggregate::Column::Extra,
                        aggregate::Column::TotalMs,
                        aggregate::Column::MaxMs,
                        aggregate::Column::Histogram,
                        aggregate::Column::Detail,
                    ])
                    .to_owned(),
                )
                .exec_without_returning(&txn)
                .await?;
        }
        txn.commit().await?;

        // Spikes: open issues with enough occurrences in the window.
        let spike = self.options.alerts.spike;
        for row in touched {
            if row.status != IssueStatus::Open.as_str()
                || Level::parse(&row.level).unwrap_or_default() <= Level::Info
            {
                continue;
            }
            let since = bucket_start(
                now - TimeDelta::minutes(i64::from(spike.minutes) - 1),
                MINUTE,
            );
            let count = self.issue_count_since(&row.fingerprint, since).await?;
            if count >= spike.occurrences as i64 {
                candidates.push(Candidate {
                    kind: AlertKind::Spike,
                    issue: row,
                });
            }
        }
        Ok(candidates)
    }

    /// Occurrences of `fingerprint` counted since `since` (minute rows of
    /// every process).
    async fn issue_count_since(
        &self,
        fingerprint: &str,
        since: DateTime<Utc>,
    ) -> Result<i64, DbErr> {
        let rows = aggregate::Entity::find()
            .filter(aggregate::Column::Kind.eq(kind::ISSUE))
            .filter(aggregate::Column::Resolution.eq(MINUTE))
            .filter(aggregate::Column::KeyHash.eq(key_hash(fingerprint)))
            .filter(aggregate::Column::Bucket.gte(since))
            .all(self.db.conn())
            .await?;
        Ok(rows.iter().map(|row| row.count).sum())
    }

    /// Distinct users and workspaces among the stored occurrences of each
    /// issue.
    async fn reach(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, (i64, i64)>, DbErr> {
        let mut reach: HashMap<Uuid, (HashSet<String>, HashSet<String>)> = HashMap::new();
        for chunk in ids.chunks(CHUNK) {
            let rows: Vec<(Uuid, Option<String>, Option<String>)> = occurrence::Entity::find()
                .select_only()
                .column(occurrence::Column::IssueId)
                .column(occurrence::Column::UserId)
                .column(occurrence::Column::WorkspaceId)
                .distinct()
                .filter(occurrence::Column::IssueId.is_in(chunk.iter().copied()))
                .into_tuple()
                .all(self.db.conn())
                .await?;
            for (id, user, workspace) in rows {
                let entry = reach.entry(id).or_default();
                if let Some(user) = user {
                    entry.0.insert(user);
                }
                if let Some(workspace) = workspace {
                    entry.1.insert(workspace);
                }
            }
        }
        Ok(reach
            .into_iter()
            .map(|(id, (users, workspaces))| (id, (users.len() as i64, workspaces.len() as i64)))
            .collect())
    }

    /// The public form of an issue row.
    pub(crate) fn issue_of(&self, row: &issue::Model, users: i64) -> Issue {
        Issue {
            id: row.id,
            title: row.title.clone(),
            kind: IssueKind::parse(&row.kind).unwrap_or(IssueKind::Exception),
            level: Level::parse(&row.level).unwrap_or_default(),
            status: IssueStatus::parse(&row.status).unwrap_or(IssueStatus::Open),
            culprit: row.culprit.clone(),
            count: row.count,
            users,
            first_seen: row.first_seen,
            last_seen: row.last_seen,
            url: self.issue_url(row.id),
        }
    }

    /// The issue as alerts carry it.
    pub(crate) async fn issue_for_alert(&self, row: &issue::Model) -> Result<Issue, DbErr> {
        let users = self.reach(&[row.id]).await?.get(&row.id).map_or(0, |r| r.0);
        Ok(self.issue_of(row, users))
    }

    // -----------------------------------------------------------------
    // Issues
    // -----------------------------------------------------------------

    /// A page of issues.
    pub async fn issues(
        &self,
        filter: &IssueFilter,
    ) -> Result<ListPage<IssueSummary>, LighthouseError> {
        let per_page = filter.per_page.unwrap_or(25).clamp(1, 100);
        let page = filter.page.unwrap_or(1).max(1);
        let mut query = issue::Entity::find();
        if let Some(status) = non_empty(&filter.status) {
            query = query.filter(issue::Column::Status.eq(status));
        }
        if let Some(kind) = non_empty(&filter.kind) {
            query = query.filter(issue::Column::Kind.eq(kind));
        }
        if let Some(level) = non_empty(&filter.level) {
            query = query.filter(issue::Column::Level.eq(level));
        }
        if let Some(environment) = non_empty(&filter.environment) {
            query = query.filter(issue::Column::Environment.eq(environment));
        }
        for culprit in [non_empty(&filter.route), non_empty(&filter.job)]
            .into_iter()
            .flatten()
        {
            query = query.filter(issue::Column::Culprit.contains(culprit));
        }
        if let Some(search) = non_empty(&filter.search) {
            query = query.filter(issue::Column::Title.contains(search));
        }
        let occurrence_filters: Vec<(occurrence::Column, String, bool)> = [
            (
                occurrence::Column::Release,
                non_empty(&filter.release),
                false,
            ),
            (occurrence::Column::UserId, non_empty(&filter.user), false),
            (
                occurrence::Column::WorkspaceId,
                non_empty(&filter.workspace),
                false,
            ),
        ]
        .into_iter()
        .filter_map(|(column, value, like)| value.map(|value| (column, value.to_owned(), like)))
        .chain(non_empty(&filter.tag).and_then(|tag| {
            let (key, value) = tag.split_once(':')?;
            Some((
                occurrence::Column::Context,
                format!(
                    "%{}%",
                    serde_json::to_string(&serde_json::json!({ key.trim(): value.trim() }))
                        .ok()?
                        .trim_matches(['{', '}'])
                ),
                true,
            ))
        }))
        .collect();
        for (column, value, like) in occurrence_filters {
            let condition = if like {
                sea_orm::sea_query::ExprTrait::like(Expr::col(column), value)
            } else {
                sea_orm::sea_query::ExprTrait::eq(Expr::col(column), value)
            };
            query = query.filter(
                issue::Column::Id.in_subquery(
                    Query::select()
                        .column(occurrence::Column::IssueId)
                        .from(occurrence::Entity)
                        .and_where(condition)
                        .to_owned(),
                ),
            );
        }
        query = match filter.sort {
            IssueSort::LastSeen => query.order_by_desc(issue::Column::LastSeen),
            IssueSort::FirstSeen => query.order_by_desc(issue::Column::FirstSeen),
            IssueSort::Count => query.order_by_desc(issue::Column::Count),
        };
        let paginator = query.paginate(self.db.conn(), per_page);
        let total = paginator.num_items().await?;
        let rows = paginator.fetch_page(page - 1).await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        let reach = self.reach(&ids).await?;
        let sparklines = self
            .sparklines(
                &rows
                    .iter()
                    .map(|row| row.fingerprint.clone())
                    .collect::<Vec<_>>(),
            )
            .await?;
        let data = rows
            .iter()
            .map(|row| {
                let (users, workspaces) = reach.get(&row.id).copied().unwrap_or((0, 0));
                IssueSummary {
                    issue: self.issue_of(row, users),
                    workspaces,
                    sparkline: sparklines
                        .get(&key_hash(&row.fingerprint))
                        .cloned()
                        .unwrap_or_else(|| vec![0; 24]),
                }
            })
            .collect();
        Ok(ListPage {
            data,
            meta: PageMeta {
                current_page: page,
                per_page,
                total,
                last_page: total.div_ceil(per_page).max(1),
            },
        })
    }

    /// Hourly occurrence counts of the last 24 hours, per fingerprint hash.
    async fn sparklines(
        &self,
        fingerprints: &[String],
    ) -> Result<HashMap<String, Vec<i64>>, DbErr> {
        let now = Utc::now();
        let start = bucket_start(now, HOUR) - TimeDelta::hours(23);
        let mut out: HashMap<String, Vec<i64>> = HashMap::new();
        let hashes: Vec<String> = fingerprints.iter().map(|f| key_hash(f)).collect();
        for chunk in hashes.chunks(CHUNK) {
            let rows = aggregate::Entity::find()
                .filter(aggregate::Column::Kind.eq(kind::ISSUE))
                .filter(aggregate::Column::Resolution.eq(HOUR))
                .filter(aggregate::Column::Bucket.gte(start))
                .filter(aggregate::Column::KeyHash.is_in(chunk.iter().cloned()))
                .all(self.db.conn())
                .await?;
            for row in rows {
                let index = ((row.bucket - start).num_hours()).clamp(0, 23) as usize;
                out.entry(row.key_hash.clone())
                    .or_insert_with(|| vec![0; 24])[index] += row.count;
            }
        }
        Ok(out)
    }

    /// One issue's row.
    async fn issue_row(&self, id: Uuid) -> Result<issue::Model, LighthouseError> {
        issue::Entity::find_by_id(id)
            .one(self.db.conn())
            .await?
            .ok_or(LighthouseError::NotFound("issue"))
    }

    /// One issue.
    pub async fn issue(&self, id: Uuid) -> Result<Issue, LighthouseError> {
        let row = self.issue_row(id).await?;
        Ok(self.issue_for_alert(&row).await?)
    }

    /// Everything about one issue.
    pub async fn issue_detail(&self, id: Uuid) -> Result<IssueDetail, LighthouseError> {
        let row = self.issue_row(id).await?;
        let (users, workspaces) = self.reach(&[id]).await?.get(&id).copied().unwrap_or((0, 0));
        let latest = occurrence::Entity::find()
            .filter(occurrence::Column::IssueId.eq(id))
            .order_by_desc(occurrence::Column::At)
            .one(self.db.conn())
            .await?
            .map(occurrence_of);
        let now = Utc::now();
        let start = bucket_start(now, HOUR) - TimeDelta::hours(7 * 24 - 1);
        let rows = aggregate::Entity::find()
            .filter(aggregate::Column::Kind.eq(kind::ISSUE))
            .filter(aggregate::Column::Resolution.eq(HOUR))
            .filter(aggregate::Column::KeyHash.eq(key_hash(&row.fingerprint)))
            .filter(aggregate::Column::Bucket.gte(start))
            .all(self.db.conn())
            .await?;
        let mut timeline: Vec<TimelinePoint> = (0..7 * 24)
            .map(|i| TimelinePoint {
                at: start + TimeDelta::hours(i),
                count: 0,
                errors: 0,
            })
            .collect();
        for aggregate in rows {
            let index = (aggregate.bucket - start).num_hours().clamp(0, 7 * 24 - 1) as usize;
            timeline[index].count += aggregate.count;
        }
        let alerts = self.alerts_for(Some(id)).await?;
        Ok(IssueDetail {
            issue: self.issue_of(&row, users),
            fingerprint: row.fingerprint.clone(),
            message: row.message.clone(),
            chain: serde_json::from_str(&row.chain).unwrap_or_default(),
            location: row.location.clone(),
            error_type: row.error_type.clone(),
            first_release: row.first_release.clone(),
            last_release: row.last_release.clone(),
            environment: row.environment.clone(),
            resolved_at: row.resolved_at,
            resolved_in_release: row.resolved_in_release.clone(),
            ignored_until: row.ignored_until,
            ignore_remaining: row.ignore_remaining,
            regressed_at: row.regressed_at,
            comment: row.comment.clone(),
            workspaces,
            latest,
            timeline,
            alerts,
        })
    }

    /// A page of an issue's stored occurrences, newest first.
    pub async fn occurrences(
        &self,
        issue_id: Uuid,
        page: u64,
        per_page: u64,
    ) -> Result<ListPage<Occurrence>, LighthouseError> {
        let per_page = per_page.clamp(1, 100);
        let page = page.max(1);
        let paginator = occurrence::Entity::find()
            .filter(occurrence::Column::IssueId.eq(issue_id))
            .order_by_desc(occurrence::Column::At)
            .paginate(self.db.conn(), per_page);
        let total = paginator.num_items().await?;
        let data = paginator
            .fetch_page(page - 1)
            .await?
            .into_iter()
            .map(occurrence_of)
            .collect();
        Ok(ListPage {
            data,
            meta: PageMeta {
                current_page: page,
                per_page,
                total,
                last_page: total.div_ceil(per_page).max(1),
            },
        })
    }

    /// The alerts about an issue (or every alert), newest first, at most 50.
    pub async fn alerts_for(&self, issue_id: Option<Uuid>) -> Result<Vec<AlertRecord>, DbErr> {
        let mut query = alert::Entity::find().order_by_desc(alert::Column::CreatedAt);
        if let Some(id) = issue_id {
            query = query.filter(alert::Column::IssueId.eq(id));
        }
        Ok(query
            .limit(50)
            .all(self.db.conn())
            .await?
            .into_iter()
            .map(|row| AlertRecord {
                id: row.id,
                issue_id: row.issue_id,
                kind: row.kind,
                channel: row.channel,
                status: row.status,
                attempts: row.attempts,
                error: row.error,
                created_at: row.created_at,
                delivered_at: row.delivered_at,
            })
            .collect())
    }

    /// Resolves an issue: now (a new occurrence reopens it as a
    /// regression), or in the next release (only an occurrence from a
    /// release other than the current one reopens it).
    pub async fn resolve(
        &self,
        id: Uuid,
        in_next_release: bool,
        comment: Option<String>,
    ) -> Result<Issue, LighthouseError> {
        self.issue_row(id).await?;
        let now = Utc::now();
        let (status, release) = if in_next_release {
            (
                IssueStatus::ResolvedInNextRelease,
                Some(self.release.to_string()),
            )
        } else {
            (IssueStatus::Resolved, None)
        };
        issue::Entity::update_many()
            .col_expr(issue::Column::Status, Expr::value(status.as_str()))
            .col_expr(issue::Column::ResolvedAt, Expr::value(now))
            .col_expr(issue::Column::ResolvedInRelease, Expr::value(release))
            .col_expr(
                issue::Column::IgnoredUntil,
                Expr::value(Option::<DateTime<Utc>>::None),
            )
            .col_expr(
                issue::Column::IgnoreRemaining,
                Expr::value(Option::<i64>::None),
            )
            .col_expr(issue::Column::Comment, Expr::value(comment))
            .col_expr(issue::Column::UpdatedAt, Expr::value(now))
            .filter(issue::Column::Id.eq(id))
            .exec(self.db.conn())
            .await?;
        self.issue(id).await
    }

    /// Ignores an issue: kept and counted, never alerting.
    pub async fn ignore(
        &self,
        id: Uuid,
        ignore: IgnoreFor,
        comment: Option<String>,
    ) -> Result<Issue, LighthouseError> {
        self.issue_row(id).await?;
        let (until, remaining) = match ignore {
            IgnoreFor::Forever => (None, None),
            IgnoreFor::Until(until) => (Some(until), None),
            IgnoreFor::Occurrences(n) if n > 0 => (None, Some(n)),
            IgnoreFor::Occurrences(_) => {
                return Err(LighthouseError::Other(
                    "ignore for at least one occurrence".to_owned(),
                ));
            }
        };
        issue::Entity::update_many()
            .col_expr(
                issue::Column::Status,
                Expr::value(IssueStatus::Ignored.as_str()),
            )
            .col_expr(issue::Column::IgnoredUntil, Expr::value(until))
            .col_expr(issue::Column::IgnoreRemaining, Expr::value(remaining))
            .col_expr(issue::Column::Comment, Expr::value(comment))
            .col_expr(issue::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(issue::Column::Id.eq(id))
            .exec(self.db.conn())
            .await?;
        self.issue(id).await
    }

    /// Reopens an issue.
    pub async fn reopen(&self, id: Uuid) -> Result<Issue, LighthouseError> {
        self.issue_row(id).await?;
        issue::Entity::update_many()
            .col_expr(
                issue::Column::Status,
                Expr::value(IssueStatus::Open.as_str()),
            )
            .col_expr(
                issue::Column::ResolvedAt,
                Expr::value(Option::<DateTime<Utc>>::None),
            )
            .col_expr(
                issue::Column::ResolvedInRelease,
                Expr::value(Option::<String>::None),
            )
            .col_expr(
                issue::Column::IgnoredUntil,
                Expr::value(Option::<DateTime<Utc>>::None),
            )
            .col_expr(
                issue::Column::IgnoreRemaining,
                Expr::value(Option::<i64>::None),
            )
            .col_expr(issue::Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(issue::Column::Id.eq(id))
            .exec(self.db.conn())
            .await?;
        self.issue(id).await
    }

    // -----------------------------------------------------------------
    // Samples and aggregates
    // -----------------------------------------------------------------

    /// One stored trace.
    pub async fn sample(&self, id: Uuid) -> Result<Sample, LighthouseError> {
        let row = sample::Entity::find_by_id(id)
            .one(self.db.conn())
            .await?
            .ok_or(LighthouseError::NotFound("trace"))?;
        Ok(Sample {
            summary: sample_summary(&row),
            job_id: row.job_id.clone(),
            user_id: row.user_id.clone(),
            workspace_id: row.workspace_id.clone(),
            release: row.release.clone(),
            environment: row.environment.clone(),
            server: row.server.clone(),
            process: row.process.clone(),
            spans: serde_json::from_str(&row.spans).unwrap_or_default(),
            context: serde_json::from_str(&row.context).unwrap_or(Value::Null),
            error: row.error,
        })
    }

    /// The aggregate rows of `kind` in `period`, merged by key.
    async fn merged(
        &self,
        kind: &str,
        period: Period,
        key: Option<&str>,
    ) -> Result<Vec<Merged>, DbErr> {
        let now = Utc::now();
        let resolution = period.resolution();
        let start = period_start(now, period);
        let mut query = aggregate::Entity::find()
            .filter(aggregate::Column::Kind.eq(kind))
            .filter(aggregate::Column::Resolution.eq(resolution))
            .filter(aggregate::Column::Bucket.gte(start));
        if let Some(key) = key {
            query = query.filter(aggregate::Column::KeyHash.eq(key_hash(key)));
        }
        let rows = query.all(self.db.conn()).await?;
        let mut merged: BTreeMap<String, Merged> = BTreeMap::new();
        for row in rows {
            let values = Values {
                count: row.count,
                errors: row.errors,
                extra: row.extra,
                total_ms: row.total_ms,
                histogram: Histogram::decode(&row.histogram, row.max_ms),
                detail: Detail::decode(row.detail.as_deref()),
            };
            let entry = merged
                .entry(row.key_hash.clone())
                .or_insert_with(|| (row.key.clone(), Values::default(), BTreeMap::new()));
            entry.1.merge(&values);
            let point = entry.2.entry(row.bucket).or_default();
            point.0 += row.count;
            point.1 += row.errors;
        }
        Ok(merged.into_values().collect())
    }

    /// The overview's figures.
    pub async fn overview(&self, period: Period) -> Result<Overview, LighthouseError> {
        let requests = self.merged(kind::REQUEST, period, None).await?;
        let exceptions = self.merged(kind::EXCEPTION, period, None).await?;
        let jobs = self.merged(kind::JOB, period, None).await?;
        let mut all = Values::default();
        for (_, values, _) in &requests {
            all.merge(values);
        }
        let exception_count: i64 = exceptions.iter().map(|(_, values, _)| values.count).sum();
        let failed_jobs: i64 = jobs.iter().map(|(_, values, _)| values.extra).sum();
        let open_issues = issue::Entity::find()
            .filter(issue::Column::Status.eq(IssueStatus::Open.as_str()))
            .count(self.db.conn())
            .await?;
        let now = Utc::now();
        let mut timeline = empty_timeline(now, period);
        let start = period_start(now, period);
        let step = i64::from(period.resolution());
        for (_, _, points) in &requests {
            for (at, (count, _)) in points {
                let index = ((*at - start).num_seconds() / step).clamp(0, timeline.len() as i64 - 1)
                    as usize;
                timeline[index].count += count;
            }
        }
        for (_, _, points) in &exceptions {
            for (at, (count, _)) in points {
                let index = ((*at - start).num_seconds() / step).clamp(0, timeline.len() as i64 - 1)
                    as usize;
                timeline[index].errors += count;
            }
        }
        let feed = self.feed(20).await?;
        Ok(Overview {
            period,
            requests: all.count,
            requests_per_minute: all.count as f64 / period.minutes() as f64,
            error_rate: ratio(all.errors, all.count),
            p95_ms: round2(all.histogram.quantile(0.95)),
            exceptions: exception_count,
            failed_jobs,
            open_issues,
            timeline,
            feed,
            dropped: 0,
        })
    }

    /// The latest occurrences, newest first.
    pub async fn feed(&self, limit: u64) -> Result<Vec<FeedItem>, DbErr> {
        let rows = occurrence::Entity::find()
            .order_by_desc(occurrence::Column::At)
            .limit(limit)
            .all(self.db.conn())
            .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.issue_id).collect();
        let issues: HashMap<Uuid, issue::Model> = issue::Entity::find()
            .filter(issue::Column::Id.is_in(ids))
            .all(self.db.conn())
            .await?
            .into_iter()
            .map(|row| (row.id, row))
            .collect();
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let issue = issues.get(&row.issue_id)?;
                Some(FeedItem {
                    occurrence_id: row.id,
                    issue_id: row.issue_id,
                    title: issue.title.clone(),
                    kind: issue.kind.clone(),
                    level: row.level,
                    culprit: match (&row.method, &row.route) {
                        (Some(method), Some(route)) => format!("{method} {route}"),
                        _ => issue.culprit.clone(),
                    },
                    at: row.at,
                })
            })
            .collect())
    }

    /// Every route with its figures, slowest (p95) first.
    pub async fn routes(&self, period: Period) -> Result<Vec<RouteStats>, LighthouseError> {
        let mut routes: Vec<RouteStats> = self
            .merged(kind::REQUEST, period, None)
            .await?
            .into_iter()
            .map(|(endpoint, values, _)| route_stats(endpoint, &values, period))
            .collect();
        routes.sort_by(|a, b| b.p95_ms.total_cmp(&a.p95_ms));
        Ok(routes)
    }

    /// One route: its figures, timeline and slowest stored traces.
    pub async fn route(
        &self,
        endpoint: &str,
        period: Period,
    ) -> Result<RouteDetail, LighthouseError> {
        let merged = self.merged(kind::REQUEST, period, Some(endpoint)).await?;
        let (values, points) = merged
            .into_iter()
            .next()
            .map(|(_, values, points)| (values, points))
            .unwrap_or_default();
        let now = Utc::now();
        let start = period_start(now, period);
        let step = i64::from(period.resolution());
        let mut timeline = empty_timeline(now, period);
        for (at, (count, errors)) in points {
            let index =
                ((at - start).num_seconds() / step).clamp(0, timeline.len() as i64 - 1) as usize;
            timeline[index].count += count;
            timeline[index].errors += errors;
        }
        let slowest = sample::Entity::find()
            .filter(sample::Column::Kind.eq("request"))
            .filter(sample::Column::Name.eq(endpoint))
            .filter(sample::Column::At.gte(now - TimeDelta::minutes(period.minutes())))
            .order_by_desc(sample::Column::DurationMs)
            .limit(20)
            .all(self.db.conn())
            .await?
            .iter()
            .map(sample_summary)
            .collect();
        Ok(RouteDetail {
            stats: route_stats(endpoint.to_owned(), &values, period),
            timeline,
            slowest,
        })
    }

    /// Normalized statements by total time.
    pub async fn queries(&self, period: Period) -> Result<Vec<QueryStats>, LighthouseError> {
        let mut queries: Vec<QueryStats> = self
            .merged(kind::QUERY, period, None)
            .await?
            .into_iter()
            .map(|(statement, values, _)| QueryStats {
                statement,
                count: values.count,
                errors: values.errors,
                total_ms: round2(values.total_ms),
                avg_ms: round2(values.total_ms / values.count.max(1) as f64),
                p95_ms: round2(values.histogram.quantile(0.95)),
                max_ms: round2(values.max_ms()),
                origins: origins(&values.detail),
            })
            .collect();
        queries.sort_by(|a, b| b.total_ms.total_cmp(&a.total_ms));
        Ok(queries)
    }

    /// Job types by attempts.
    pub async fn jobs(&self, period: Period) -> Result<Vec<JobStats>, LighthouseError> {
        let mut jobs: Vec<JobStats> = self
            .merged(kind::JOB, period, None)
            .await?
            .into_iter()
            .map(|(job_type, values, _)| JobStats {
                job_type,
                attempts: values.count,
                throughput_per_minute: round2(values.count as f64 / period.minutes() as f64),
                failed_attempts: values.errors,
                failed_jobs: values.extra,
                avg_ms: round2(values.total_ms / values.count.max(1) as f64),
                p50_ms: round2(values.histogram.quantile(0.5)),
                p95_ms: round2(values.histogram.quantile(0.95)),
                max_ms: round2(values.max_ms()),
            })
            .collect();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.attempts));
        Ok(jobs)
    }

    /// Hosts called through the HTTP client, slowest (p95) first.
    pub async fn outgoing(&self, period: Period) -> Result<Vec<HostStats>, LighthouseError> {
        let mut hosts: Vec<HostStats> = self
            .merged(kind::OUTGOING, period, None)
            .await?
            .into_iter()
            .map(|(host, values, _)| HostStats {
                host,
                count: values.count,
                errors: values.errors,
                error_rate: ratio(values.errors, values.count),
                avg_ms: round2(values.total_ms / values.count.max(1) as f64),
                p95_ms: round2(values.histogram.quantile(0.95)),
                max_ms: round2(values.max_ms()),
                slowest: values.detail.slowest.clone(),
                origins: origins(&values.detail),
            })
            .collect();
        hosts.sort_by(|a, b| b.p95_ms.total_cmp(&a.p95_ms));
        Ok(hosts)
    }

    // -----------------------------------------------------------------
    // Pruning
    // -----------------------------------------------------------------

    /// Deletes samples, occurrences, aggregates and alerts older than
    /// `days`, per-minute aggregates older than two days, and resolved or
    /// ignored issues with no occurrence left. Open issues are kept. On
    /// TimescaleDB, where samples, occurrences and aggregates are
    /// hypertables, the days entirely before the cutoff go as whole chunks
    /// (see [`prune_time_series`]).
    pub async fn prune(&self, days: u32) -> Result<PruneSummary, LighthouseError> {
        let now = Utc::now();
        let cutoff = now - TimeDelta::days(i64::from(days.max(1)));
        let conn = self.db.conn();
        let samples = prune_time_series(conn, SAMPLES_TABLE, "at", cutoff).await?;
        let occurrences = prune_time_series(conn, OCCURRENCES_TABLE, "at", cutoff).await?;
        let minute_cutoff = cutoff.max(now - TimeDelta::days(2));
        let aggregates = prune_time_series(conn, AGGREGATES_TABLE, "bucket", cutoff).await?
            + aggregate::Entity::delete_many()
                .filter(aggregate::Column::Resolution.eq(MINUTE))
                .filter(aggregate::Column::Bucket.lt(minute_cutoff))
                .exec(conn)
                .await?
                .rows_affected;
        let alerts = alert::Entity::delete_many()
            .filter(alert::Column::CreatedAt.lt(cutoff))
            .exec(conn)
            .await?
            .rows_affected;
        let issues = issue::Entity::delete_many()
            .filter(issue::Column::Status.ne(IssueStatus::Open.as_str()))
            .filter(issue::Column::LastSeen.lt(cutoff))
            .filter(
                issue::Column::Id.not_in_subquery(
                    Query::select()
                        .column(occurrence::Column::IssueId)
                        .from(occurrence::Entity)
                        .to_owned(),
                ),
            )
            .exec(conn)
            .await?
            .rows_affected;
        Ok(PruneSummary {
            samples,
            occurrences,
            aggregates,
            alerts,
            issues,
        })
    }

    // -----------------------------------------------------------------
    // Alerts
    // -----------------------------------------------------------------

    /// Whether an alert of `kind` about `subject` was raised since `since`.
    pub(crate) async fn alerted_since(
        &self,
        subject: &str,
        kind: AlertKind,
        since: DateTime<Utc>,
    ) -> Result<bool, DbErr> {
        Ok(alert::Entity::find()
            .filter(alert::Column::SubjectKey.eq(subject))
            .filter(alert::Column::Kind.eq(kind.as_str()))
            .filter(alert::Column::CreatedAt.gt(since))
            .one(self.db.conn())
            .await?
            .is_some())
    }

    /// Stores an alert for one channel.
    pub(crate) async fn insert_alert(
        &self,
        id: Uuid,
        alert: &crate::alert::Alert,
        channel: &str,
    ) -> Result<(), DbErr> {
        alert::Entity::insert(alert::ActiveModel {
            id: Set(id),
            issue_id: Set(Some(alert.issue.id)),
            kind: Set(alert.kind.as_str().to_owned()),
            channel: Set(channel.to_owned()),
            subject_key: Set(alert.issue.id.to_string()),
            status: Set("pending".to_owned()),
            attempts: Set(0),
            error: Set(None),
            payload: Set(serde_json::to_string(alert).unwrap_or_default()),
            created_at: Set(alert.at),
            delivered_at: Set(None),
        })
        .exec_without_returning(self.db.conn())
        .await?;
        Ok(())
    }

    /// A stored alert row.
    pub(crate) async fn alert_row(&self, id: Uuid) -> Result<Option<alert::Model>, DbErr> {
        alert::Entity::find_by_id(id).one(self.db.conn()).await
    }

    /// Records a delivery attempt.
    pub(crate) async fn record_delivery(
        &self,
        id: Uuid,
        result: Result<(), String>,
        attempts: i32,
        last_attempt: bool,
    ) -> Result<(), DbErr> {
        let now = Utc::now();
        let update = alert::Entity::update_many()
            .col_expr(alert::Column::Attempts, Expr::value(attempts))
            .filter(alert::Column::Id.eq(id));
        let update = match result {
            Ok(()) => update
                .col_expr(alert::Column::Status, Expr::value("delivered"))
                .col_expr(alert::Column::DeliveredAt, Expr::value(now))
                .col_expr(alert::Column::Error, Expr::value(Option::<String>::None)),
            Err(error) => update
                .col_expr(
                    alert::Column::Status,
                    Expr::value(if last_attempt { "failed" } else { "pending" }),
                )
                .col_expr(alert::Column::Error, Expr::value(error)),
        };
        update.exec(self.db.conn()).await?;
        Ok(())
    }
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn ratio(part: i64, whole: i64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        round4(part as f64 / whole as f64)
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn origins(detail: &Detail) -> Vec<Origin> {
    let mut origins: Vec<Origin> = detail
        .origins
        .iter()
        .map(|(name, count)| Origin {
            name: name.clone(),
            count: *count,
        })
        .collect();
    origins.sort_by_key(|origin| std::cmp::Reverse(origin.count));
    origins.truncate(10);
    origins
}

fn route_stats(endpoint: String, values: &Values, period: Period) -> RouteStats {
    RouteStats {
        endpoint,
        count: values.count,
        throughput_per_minute: round2(values.count as f64 / period.minutes() as f64),
        errors: values.errors,
        server_errors: values.extra,
        error_rate: ratio(values.errors, values.count),
        avg_ms: round2(values.total_ms / values.count.max(1) as f64),
        p50_ms: round2(values.histogram.quantile(0.5)),
        p95_ms: round2(values.histogram.quantile(0.95)),
        p99_ms: round2(values.histogram.quantile(0.99)),
        max_ms: round2(values.max_ms()),
    }
}

/// The first bucket of `period`.
fn period_start(now: DateTime<Utc>, period: Period) -> DateTime<Utc> {
    let resolution = period.resolution();
    let buckets = period.minutes() * 60 / i64::from(resolution);
    bucket_start(now, resolution) - TimeDelta::seconds(i64::from(resolution) * (buckets - 1))
}

fn empty_timeline(now: DateTime<Utc>, period: Period) -> Vec<TimelinePoint> {
    let resolution = i64::from(period.resolution());
    let buckets = period.minutes() * 60 / resolution;
    let start = period_start(now, period);
    (0..buckets)
        .map(|i| TimelinePoint {
            at: start + TimeDelta::seconds(resolution * i),
            count: 0,
            errors: 0,
        })
        .collect()
}

fn occurrence_model(
    issue_id: Uuid,
    occurrence: &crate::engine::NewOccurrence,
    labels: &Labels,
) -> occurrence::ActiveModel {
    occurrence::ActiveModel {
        id: Set(occurrence.id),
        issue_id: Set(issue_id),
        at: Set(occurrence.at),
        level: Set(occurrence.level.as_str().to_owned()),
        message: Set(occurrence.message.clone()),
        chain: Set(serde_json::to_string(&occurrence.chain).unwrap_or_default()),
        backtrace: Set(occurrence.backtrace.clone()),
        location: Set(occurrence.location.clone()),
        request_id: Set(occurrence.request_id.clone()),
        job_id: Set(occurrence.job_id.clone()),
        trace_id: Set(occurrence.trace_id.clone()),
        user_id: Set(occurrence.user_id.clone()),
        workspace_id: Set(occurrence.workspace_id.clone()),
        release: Set(Some(labels.release.clone())),
        environment: Set(Some(labels.environment.clone())),
        server: Set(Some(labels.server.clone())),
        process: Set(Some(labels.process.clone())),
        method: Set(occurrence.method.clone()),
        route: Set(occurrence.route.clone()),
        path: Set(occurrence.path.clone()),
        status: Set(occurrence.status.map(i32::from)),
        sample_id: Set(occurrence.sample_id),
        context: Set(serde_json::to_string(&occurrence.context).unwrap_or_else(|_| "{}".into())),
    }
}

fn sample_model(sample: &NewSample, labels: &Labels) -> sample::ActiveModel {
    sample::ActiveModel {
        id: Set(sample.id),
        kind: Set(sample.kind.to_owned()),
        name: Set(sample.name.clone()),
        at: Set(sample.at),
        duration_ms: Set(sample.duration_ms),
        status: Set(sample.status.map(i32::from)),
        outcome: Set(sample.outcome.to_owned()),
        slow: Set(sample.slow),
        request_id: Set(sample.context.request_id.clone()),
        job_id: Set(sample.context.job_id.clone()),
        trace_id: Set(sample
            .context
            .trace
            .as_ref()
            .map(|trace| trace.trace_id.to_string())),
        user_id: Set(sample.context.user_id.clone()),
        workspace_id: Set(sample.context.workspace_id.clone()),
        release: Set(Some(labels.release.clone())),
        environment: Set(Some(labels.environment.clone())),
        server: Set(Some(labels.server.clone())),
        process: Set(Some(labels.process.clone())),
        spans: Set(serde_json::to_string(&sample.spans).unwrap_or_else(|_| "[]".into())),
        context: Set(serde_json::to_string(&sample.details).unwrap_or_else(|_| "{}".into())),
        error: Set(sample.error.clone()),
    }
}

fn aggregate_model(row: &Row, labels: &Labels) -> aggregate::ActiveModel {
    aggregate::ActiveModel {
        id: Set(Uuid::now_v7()),
        resolution: Set(row.key.resolution),
        bucket: Set(row.key.bucket),
        kind: Set(row.key.kind.to_owned()),
        key_hash: Set(key_hash(&row.key.key)),
        key: Set(row.key.key.clone()),
        process: Set(labels.process_key.clone()),
        count: Set(row.values.count),
        errors: Set(row.values.errors),
        extra: Set(row.values.extra),
        total_ms: Set(row.values.total_ms),
        max_ms: Set(row.values.max_ms()),
        histogram: Set(row.values.histogram.encode()),
        detail: Set(row.values.detail.encode()),
    }
}

fn occurrence_of(row: occurrence::Model) -> Occurrence {
    Occurrence {
        id: row.id,
        issue_id: row.issue_id,
        at: row.at,
        level: row.level,
        message: row.message,
        chain: serde_json::from_str(&row.chain).unwrap_or_default(),
        backtrace: row.backtrace,
        location: row.location,
        request_id: row.request_id,
        job_id: row.job_id,
        trace_id: row.trace_id,
        user_id: row.user_id,
        workspace_id: row.workspace_id,
        release: row.release,
        environment: row.environment,
        server: row.server,
        process: row.process,
        method: row.method,
        route: row.route,
        path: row.path,
        status: row.status,
        sample_id: row.sample_id,
        context: serde_json::from_str(&row.context).unwrap_or(Value::Null),
    }
}

fn sample_summary(row: &sample::Model) -> SampleSummary {
    SampleSummary {
        id: row.id,
        kind: row.kind.clone(),
        name: row.name.clone(),
        at: row.at,
        duration_ms: round2(row.duration_ms),
        status: row.status,
        outcome: row.outcome.clone(),
        slow: row.slow,
        trace_id: row.trace_id.clone(),
        request_id: row.request_id.clone(),
    }
}

/// The current UTC time on `ConnectionTrait` (keeps the import used).
#[allow(dead_code)]
fn _assert_connection<C: ConnectionTrait>(_c: &C) {}
