//! [`LighthouseStore`]: everything Lighthouse keeps in the database, read
//! for the dashboard, the JSON API and your code, plus issue status changes
//! and pruning.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_db::{Db, DbError, Uow, prune_time_series_on};
use navree_orm::{Assignment, ConflictUpdate, Filter, Model as _, OrmError, Record, col, table};
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
    Db(#[from] DbError),
    /// A model operation failed (a query, a write). It maps the way the
    /// error in its source does.
    #[error(transparent)]
    Orm(#[from] OrmError),
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
    /// Details (traces and occurrences) every process dropped in the period
    /// because the write queue was full.
    pub sampled_out: i64,
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
/// Times a status change is tried again after another process changed
/// the issue first.
const STATUS_ATTEMPTS: usize = 5;
/// Times a batch is written again after the database ended its transaction
/// as a deadlock or a serialization failure.
const WRITE_ATTEMPTS: usize = 5;

/// Whether the database ended the write in a way that writing again fixes.
fn is_retryable(error: &LighthouseError) -> bool {
    match error {
        LighthouseError::Db(error) | LighthouseError::Orm(OrmError::Db(error)) => {
            error.is_retryable()
        }
        _ => false,
    }
}

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
        Ok(issue::Model::query()
            .where_(issue::Model::STATUS.eq(IssueStatus::Open.as_str()))
            .count(&self.db)
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
    ///
    /// The transaction is a unit of work. It reads before it writes (the
    /// issues of the batch, then their counts and statuses), and on SQLite a
    /// unit of work begins `IMMEDIATE` (`BEGIN IMMEDIATE`, like
    /// [`Db::begin_for_write`]): the batch queues for the write lock, up to
    /// the busy timeout, instead of failing with `database is locked` when
    /// another connection holds it between the batch's read and its write.
    pub(crate) async fn write_batch(
        &self,
        batch: Batch,
        labels: &Labels,
    ) -> Result<Vec<Candidate>, LighthouseError> {
        let now = Utc::now();
        let mut attempt = 0;
        let (mut candidates, touched) = loop {
            attempt += 1;
            let uow = self.db.uow(None, None).await;
            let written = match write_rows(&uow, &batch, labels, now).await {
                Ok(written) => uow
                    .commit()
                    .await
                    .map(|()| written)
                    .map_err(|error| LighthouseError::from(DbError::from(error))),
                Err(error) => {
                    let _ = uow.rollback().await;
                    Err(error)
                }
            };
            match written {
                Ok(written) => break written,
                // Two processes writing at once can deadlock (TimescaleDB
                // adds a chunk's foreign key to the issues, which a writer
                // of the issues holds up): the database ended this one and
                // the other went on, so write the batch again.
                Err(error) if attempt < WRITE_ATTEMPTS && is_retryable(&error) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10 * attempt as u64)).await;
                }
                Err(error) => return Err(error),
            }
        };

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
            // Written already: a failed read here must not make the
            // caller write the batch again.
            let count = match self.issue_count_since(&row.fingerprint, since).await {
                Ok(count) => count,
                Err(error) => {
                    tracing::warn!(target: "navree::lighthouse", %error, "could not check for a spike");
                    continue;
                }
            };
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
    ) -> Result<i64, LighthouseError> {
        let rows = aggregate::Model::query()
            .where_(aggregate::Model::KIND.eq(kind::ISSUE))
            .where_(aggregate::Model::RESOLUTION.eq(MINUTE))
            .where_(aggregate::Model::KEY_HASH.eq(key_hash(fingerprint)))
            .where_(aggregate::Model::BUCKET.gte(since))
            .get(&self.db)
            .await?;
        Ok(rows.iter().map(|row| row.count).sum())
    }

    /// Distinct users and workspaces among the stored occurrences of each
    /// issue.
    async fn reach(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, (i64, i64)>, LighthouseError> {
        let mut reach: HashMap<Uuid, (HashSet<String>, HashSet<String>)> = HashMap::new();
        for chunk in ids.chunks(CHUNK) {
            let rows = table(OCCURRENCES_TABLE)
                .select(["issue_id", "user_id", "workspace_id"])
                .distinct()
                .where_(col("issue_id").is_in(chunk.iter().copied()))
                .get(&self.db)
                .await?;
            for row in rows {
                let id: Uuid = row.get("issue_id")?;
                let user: Option<String> = row.get("user_id")?;
                let workspace: Option<String> = row.get("workspace_id")?;
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
    pub(crate) async fn issue_for_alert(
        &self,
        row: &issue::Model,
    ) -> Result<Issue, LighthouseError> {
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
        let mut query = issue::Model::query();
        if let Some(status) = non_empty(&filter.status) {
            query = query.where_(issue::Model::STATUS.eq(status));
        }
        if let Some(kind) = non_empty(&filter.kind) {
            query = query.where_(issue::Model::KIND.eq(kind));
        }
        if let Some(level) = non_empty(&filter.level) {
            query = query.where_(issue::Model::LEVEL.eq(level));
        }
        if let Some(environment) = non_empty(&filter.environment) {
            query = query.where_(issue::Model::ENVIRONMENT.eq(environment));
        }
        for culprit in [non_empty(&filter.route), non_empty(&filter.job)]
            .into_iter()
            .flatten()
        {
            query = query.where_(issue::Model::CULPRIT.contains_ignore_case(culprit));
        }
        if let Some(search) = non_empty(&filter.search) {
            query = query.where_(issue::Model::TITLE.contains_ignore_case(search));
        }
        // Issues with a stored occurrence that matches.
        let occurrence_filters: Vec<Filter> = [
            non_empty(&filter.release).map(|release| occurrence::Model::RELEASE.eq(release)),
            non_empty(&filter.user).map(|user| occurrence::Model::USER_ID.eq(user)),
            non_empty(&filter.workspace)
                .map(|workspace| occurrence::Model::WORKSPACE_ID.eq(workspace)),
            non_empty(&filter.tag).and_then(|tag| {
                let (key, value) = tag.split_once(':')?;
                let json =
                    serde_json::to_string(&serde_json::json!({ key.trim(): value.trim() })).ok()?;
                Some(occurrence::Model::CONTEXT.contains_ignore_case(json.trim_matches(['{', '}'])))
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        for condition in occurrence_filters {
            query = query.where_(
                issue::Model::ID.in_subquery(
                    occurrence::Model::query()
                        .select([occurrence::Model::ISSUE_ID])
                        .where_(condition),
                ),
            );
        }
        query = match filter.sort {
            IssueSort::LastSeen => query.order_by_desc(issue::Model::LAST_SEEN),
            IssueSort::FirstSeen => query.order_by_desc(issue::Model::FIRST_SEEN),
            IssueSort::Count => query.order_by_desc(issue::Model::COUNT),
        };
        let paged = query.paginate(&self.db, (page, per_page)).await?;
        let total = paged.meta.total;
        let rows: Vec<issue::Model> = paged.data.into_iter().map(Record::into_inner).collect();
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
    ) -> Result<HashMap<String, Vec<i64>>, LighthouseError> {
        let now = Utc::now();
        let start = bucket_start(now, HOUR) - TimeDelta::hours(23);
        let mut out: HashMap<String, Vec<i64>> = HashMap::new();
        let hashes: Vec<String> = fingerprints.iter().map(|f| key_hash(f)).collect();
        for chunk in hashes.chunks(CHUNK) {
            let rows = aggregate::Model::query()
                .where_(aggregate::Model::KIND.eq(kind::ISSUE))
                .where_(aggregate::Model::RESOLUTION.eq(HOUR))
                .where_(aggregate::Model::BUCKET.gte(start))
                .where_(aggregate::Model::KEY_HASH.is_in(chunk.iter().cloned()))
                .get(&self.db)
                .await?;
            for row in rows {
                let index = ((row.bucket - start).num_hours()).clamp(0, 23) as usize;
                out.entry(row.key_hash.clone())
                    .or_insert_with(|| vec![0; 24])[index] += row.count;
            }
        }
        Ok(out)
    }

    /// One issue's row, read from the writer: a status change reads it
    /// back right after writing it.
    async fn issue_row(&self, id: Uuid) -> Result<issue::Model, LighthouseError> {
        issue::Model::query()
            .where_key(id)
            .use_write()
            .first(&self.db)
            .await?
            .map(Record::into_inner)
            .ok_or(LighthouseError::NotFound("issue"))
    }

    /// One issue.
    pub async fn issue(&self, id: Uuid) -> Result<Issue, LighthouseError> {
        let row = self.issue_row(id).await?;
        self.issue_for_alert(&row).await
    }

    /// Everything about one issue.
    pub async fn issue_detail(&self, id: Uuid) -> Result<IssueDetail, LighthouseError> {
        let row = self.issue_row(id).await?;
        let (users, workspaces) = self.reach(&[id]).await?.get(&id).copied().unwrap_or((0, 0));
        let latest = occurrence::Model::query()
            .where_(occurrence::Model::ISSUE_ID.eq(id))
            .order_by_desc(occurrence::Model::AT)
            .first(&self.db)
            .await?
            .map(|record| occurrence_of(record.into_inner()));
        let now = Utc::now();
        let start = bucket_start(now, HOUR) - TimeDelta::hours(7 * 24 - 1);
        let rows = aggregate::Model::query()
            .where_(aggregate::Model::KIND.eq(kind::ISSUE))
            .where_(aggregate::Model::RESOLUTION.eq(HOUR))
            .where_(aggregate::Model::KEY_HASH.eq(key_hash(&row.fingerprint)))
            .where_(aggregate::Model::BUCKET.gte(start))
            .get(&self.db)
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
        let paged = occurrence::Model::query()
            .where_(occurrence::Model::ISSUE_ID.eq(issue_id))
            .order_by_desc(occurrence::Model::AT)
            .paginate(&self.db, (page, per_page))
            .await?;
        let total = paged.meta.total;
        let data = paged
            .data
            .into_iter()
            .map(|record| occurrence_of(record.into_inner()))
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
    pub async fn alerts_for(
        &self,
        issue_id: Option<Uuid>,
    ) -> Result<Vec<AlertRecord>, LighthouseError> {
        let mut query = alert::Model::query().order_by_desc(alert::Model::CREATED_AT);
        if let Some(id) = issue_id {
            query = query.where_(alert::Model::ISSUE_ID.eq(id));
        }
        Ok(query
            .limit(50)
            .get(&self.db)
            .await?
            .into_models()
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
        issue::Model::query()
            .where_key(id)
            .update(
                &self.db,
                [
                    issue::Model::STATUS.set(status.as_str()),
                    issue::Model::RESOLVED_AT.set(Some(now)),
                    issue::Model::RESOLVED_IN_RELEASE.set(release),
                    issue::Model::IGNORED_UNTIL.set_null(),
                    issue::Model::IGNORE_REMAINING.set_null(),
                    issue::Model::COMMENT.set(comment),
                    issue::Model::UPDATED_AT.set(now),
                ],
            )
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
        issue::Model::query()
            .where_key(id)
            .update(
                &self.db,
                [
                    issue::Model::STATUS.set(IssueStatus::Ignored.as_str()),
                    issue::Model::IGNORED_UNTIL.set(until),
                    issue::Model::IGNORE_REMAINING.set(remaining),
                    issue::Model::COMMENT.set(comment),
                    issue::Model::UPDATED_AT.set(Utc::now()),
                ],
            )
            .await?;
        self.issue(id).await
    }

    /// Reopens an issue.
    pub async fn reopen(&self, id: Uuid) -> Result<Issue, LighthouseError> {
        self.issue_row(id).await?;
        let mut columns = reopen_columns();
        columns.push(issue::Model::UPDATED_AT.set(Utc::now()));
        issue::Model::query()
            .where_key(id)
            .update(&self.db, columns)
            .await?;
        self.issue(id).await
    }

    // -----------------------------------------------------------------
    // Samples and aggregates
    // -----------------------------------------------------------------

    /// One stored trace.
    pub async fn sample(&self, id: Uuid) -> Result<Sample, LighthouseError> {
        let row = sample::Model::find(&self.db, id)
            .await?
            .map(Record::into_inner)
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
    ) -> Result<Vec<Merged>, LighthouseError> {
        let now = Utc::now();
        let resolution = period.resolution();
        let start = period_start(now, period);
        let mut query = aggregate::Model::query()
            .where_(aggregate::Model::KIND.eq(kind))
            .where_(aggregate::Model::RESOLUTION.eq(resolution))
            .where_(aggregate::Model::BUCKET.gte(start));
        if let Some(key) = key {
            query = query.where_(aggregate::Model::KEY_HASH.eq(key_hash(key)));
        }
        let rows = query.get(&self.db).await?;
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
        let sampled_out: i64 = self
            .merged(kind::SAMPLED_OUT, period, None)
            .await?
            .iter()
            .map(|(_, values, _)| values.count)
            .sum();
        let mut all = Values::default();
        for (_, values, _) in &requests {
            all.merge(values);
        }
        let exception_count: i64 = exceptions.iter().map(|(_, values, _)| values.count).sum();
        let failed_jobs: i64 = jobs.iter().map(|(_, values, _)| values.extra).sum();
        let open_issues = self.open_issue_count().await?;
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
            sampled_out,
        })
    }

    /// The latest occurrences, newest first.
    pub async fn feed(&self, limit: u64) -> Result<Vec<FeedItem>, LighthouseError> {
        let rows = occurrence::Model::query()
            .order_by_desc(occurrence::Model::AT)
            .limit(limit)
            .get(&self.db)
            .await?
            .into_models();
        let ids: Vec<Uuid> = rows.iter().map(|row| row.issue_id).collect();
        let issues: HashMap<Uuid, issue::Model> = issue::Model::query()
            .where_(issue::Model::ID.is_in(ids))
            .get(&self.db)
            .await?
            .into_models()
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
        let slowest = sample::Model::query()
            .where_(sample::Model::KIND.eq("request"))
            .where_(sample::Model::NAME.eq(endpoint))
            .where_(sample::Model::AT.gte(now - TimeDelta::minutes(period.minutes())))
            .order_by_desc(sample::Model::DURATION_MS)
            .limit(20)
            .get(&self.db)
            .await?
            .into_models()
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
    /// (see [`prune_time_series_on`]).
    pub async fn prune(&self, days: u32) -> Result<PruneSummary, LighthouseError> {
        let now = Utc::now();
        let cutoff = now - TimeDelta::days(i64::from(days.max(1)));
        let db = &self.db;
        let samples = prune_time_series_on(db, SAMPLES_TABLE, "at", cutoff).await?;
        let occurrences = prune_time_series_on(db, OCCURRENCES_TABLE, "at", cutoff).await?;
        let minute_cutoff = cutoff.max(now - TimeDelta::days(2));
        let aggregates = prune_time_series_on(db, AGGREGATES_TABLE, "bucket", cutoff).await?
            + aggregate::Model::query()
                .where_(aggregate::Model::RESOLUTION.eq(MINUTE))
                .where_(aggregate::Model::BUCKET.lt(minute_cutoff))
                .delete(db)
                .await?;
        let alerts = alert::Model::query()
            .where_(alert::Model::CREATED_AT.lt(cutoff))
            .delete(db)
            .await?;
        let issues =
            issue::Model::query()
                .where_(issue::Model::STATUS.ne(IssueStatus::Open.as_str()))
                .where_(issue::Model::LAST_SEEN.lt(cutoff))
                .where_(issue::Model::ID.not_in_subquery(
                    occurrence::Model::query().select([occurrence::Model::ISSUE_ID]),
                ))
                .delete(db)
                .await?;
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
    ) -> Result<bool, LighthouseError> {
        Ok(alert::Model::query()
            .where_(alert::Model::SUBJECT_KEY.eq(subject))
            .where_(alert::Model::KIND.eq(kind.as_str()))
            .where_(alert::Model::CREATED_AT.gt(since))
            .use_write()
            .exists(&self.db)
            .await?)
    }

    /// Stores an alert for one channel.
    pub(crate) async fn insert_alert(
        &self,
        id: Uuid,
        alert: &crate::alert::Alert,
        channel: &str,
    ) -> Result<(), LighthouseError> {
        alert::Model::query()
            .insert(
                &self.db,
                [alert::Model {
                    id,
                    issue_id: Some(alert.issue.id),
                    kind: alert.kind.as_str().to_owned(),
                    channel: channel.to_owned(),
                    subject_key: alert.issue.id.to_string(),
                    status: "pending".to_owned(),
                    attempts: 0,
                    error: None,
                    payload: serde_json::to_string(alert).unwrap_or_default(),
                    created_at: alert.at,
                    delivered_at: None,
                }],
            )
            .await?;
        Ok(())
    }

    /// A stored alert row, read from the writer (a delivery job reads the
    /// alert its dispatch has just stored).
    pub(crate) async fn alert_row(
        &self,
        id: Uuid,
    ) -> Result<Option<alert::Model>, LighthouseError> {
        Ok(alert::Model::query()
            .where_key(id)
            .use_write()
            .first(&self.db)
            .await?
            .map(Record::into_inner))
    }

    /// Records a delivery attempt.
    pub(crate) async fn record_delivery(
        &self,
        id: Uuid,
        result: Result<(), String>,
        attempts: i32,
        last_attempt: bool,
    ) -> Result<(), LighthouseError> {
        let now = Utc::now();
        let mut columns = vec![alert::Model::ATTEMPTS.set(attempts)];
        match result {
            Ok(()) => columns.extend([
                alert::Model::STATUS.set("delivered"),
                alert::Model::DELIVERED_AT.set(Some(now)),
                alert::Model::ERROR.set_null(),
            ]),
            Err(error) => columns.extend([
                alert::Model::STATUS.set(if last_attempt { "failed" } else { "pending" }),
                alert::Model::ERROR.set(Some(error)),
            ]),
        }
        alert::Model::query()
            .where_key(id)
            .update(&self.db, columns)
            .await?;
        Ok(())
    }
}

/// The rows of a batch, written through `uow`: the issues (opened or
/// counted), their occurrences, the samples and the aggregates. Returns the
/// alerts to consider and the issues the batch touched.
async fn write_rows(
    uow: &Uow,
    batch: &Batch,
    labels: &Labels,
    now: DateTime<Utc>,
) -> Result<(Vec<Candidate>, Vec<issue::Model>), LighthouseError> {
    let mut candidates = Vec::new();
    let mut touched: Vec<issue::Model> = Vec::new();

    // Group the hits by fingerprint, in order of appearance.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&Hit>> = HashMap::new();
    for hit in &batch.hits {
        if !groups.contains_key(&hit.fingerprint) {
            order.push(hit.fingerprint.clone());
        }
        groups.entry(hit.fingerprint.clone()).or_default().push(hit);
    }
    let mut existing: HashMap<String, issue::Model> = HashMap::new();
    for chunk in order.chunks(CHUNK) {
        for row in issue::Model::query()
            .where_(issue::Model::FINGERPRINT.is_in(chunk.iter().cloned()))
            .get(uow)
            .await?
            .into_models()
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
        let mut row = match existing.get(fingerprint) {
            Some(row) => Some(row.clone()),
            None => {
                let first = &hits[0];
                let inserted = issue::Model::query()
                    .upsert(
                        uow,
                        [issue::Model {
                            id: Uuid::now_v7(),
                            fingerprint: fingerprint.clone(),
                            kind: first.kind.as_str().to_owned(),
                            level: level.as_str().to_owned(),
                            status: IssueStatus::Open.as_str().to_owned(),
                            title: first.title.clone(),
                            culprit: first.culprit.clone(),
                            message: first.message.clone(),
                            chain: serde_json::to_string(&first.chain).unwrap_or_default(),
                            location: first.location.clone(),
                            error_type: first.error_type.clone(),
                            count: n,
                            first_seen: first_at,
                            last_seen: last_at,
                            first_release: Some(labels.release.clone()),
                            last_release: Some(labels.release.clone()),
                            environment: Some(labels.environment.clone()),
                            resolved_at: None,
                            resolved_in_release: None,
                            ignored_until: None,
                            ignore_remaining: None,
                            regressed_at: None,
                            comment: None,
                            created_at: now,
                            updated_at: now,
                        }],
                        [issue::Model::FINGERPRINT],
                        Vec::<ConflictUpdate>::new(),
                    )
                    .await?;
                let stored = issue::Model::query()
                    .where_(issue::Model::FINGERPRINT.eq(fingerprint.clone()))
                    .first(uow)
                    .await?
                    .map(Record::into_inner);
                if inserted == 0 {
                    // Another process opened it first.
                    stored
                } else {
                    let row = stored.ok_or(LighthouseError::NotFound("issue"))?;
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
        let Some(current) = row.take() else {
            continue;
        };
        let (current, regression) =
            count_existing(uow, current, n, last_at, level, labels, now).await?;
        let new_level = Level::parse(&current.level).unwrap_or_default();
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
        touched.push(current.clone());
        for hit in hits {
            if let Some(occurrence) = &hit.occurrence {
                occurrences.push(occurrence_model(current.id, occurrence, labels));
            }
        }
    }
    for chunk in occurrences.chunks(CHUNK) {
        occurrence::Model::query()
            .insert(uow, chunk.to_vec())
            .await?;
    }
    let samples: Vec<sample::Model> = batch
        .samples
        .iter()
        .map(|sample| sample_model(sample, labels))
        .collect();
    for chunk in samples.chunks(CHUNK) {
        sample::Model::query().insert(uow, chunk.to_vec()).await?;
    }
    let rows: Vec<aggregate::Model> = batch
        .rows
        .iter()
        .map(|row| aggregate_model(row, labels))
        .collect();
    for chunk in rows.chunks(CHUNK) {
        aggregate::Model::query()
            .upsert(
                uow,
                chunk.to_vec(),
                [
                    aggregate::Model::RESOLUTION.name(),
                    aggregate::Model::BUCKET.name(),
                    aggregate::Model::KIND.name(),
                    aggregate::Model::KEY_HASH.name(),
                    aggregate::Model::PROCESS.name(),
                ],
                [
                    ConflictUpdate::from(aggregate::Model::COUNT),
                    aggregate::Model::ERRORS.into(),
                    aggregate::Model::EXTRA.into(),
                    aggregate::Model::TOTAL_MS.into(),
                    aggregate::Model::MAX_MS.into(),
                    aggregate::Model::HISTOGRAM.into(),
                    aggregate::Model::DETAIL.into(),
                ],
            )
            .await?;
    }
    Ok((candidates, touched))
}

/// The columns that make an issue open again: no resolution, no ignore.
fn reopen_columns() -> Vec<Assignment> {
    vec![
        issue::Model::STATUS.set(IssueStatus::Open.as_str()),
        issue::Model::RESOLVED_AT.set_null(),
        issue::Model::RESOLVED_IN_RELEASE.set_null(),
        issue::Model::IGNORED_UNTIL.set_null(),
        issue::Model::IGNORE_REMAINING.set_null(),
    ]
}

/// Adds `n` hits (the latest at `last_at`, the highest level `level`) to
/// the existing issue `current` (as read at the start of the write) and
/// makes the status change they cause. Returns the issue as it now is and
/// whether this write reopened it as a regression.
async fn count_existing(
    uow: &Uow,
    mut current: issue::Model,
    n: i64,
    last_at: DateTime<Utc>,
    level: Level,
    labels: &Labels,
    now: DateTime<Utc>,
) -> Result<(issue::Model, bool), LighthouseError> {
    // An existing issue. The counts are one relative statement, so
    // processes writing at once never lose each other's hits.
    let read_level = Level::parse(&current.level).unwrap_or_default();
    let mut also = vec![
        issue::Model::LAST_RELEASE.set(Some(labels.release.clone())),
        issue::Model::ENVIRONMENT.set(Some(labels.environment.clone())),
        issue::Model::UPDATED_AT.set(now),
    ];
    if level > read_level {
        also.push(issue::Model::LEVEL.set(level.as_str()));
    }
    issue::Model::query()
        .where_key(current.id)
        .increment_with(uow, issue::Model::COUNT, n, also)
        .await?;
    issue::Model::query()
        .where_key(current.id)
        .where_(issue::Model::LAST_SEEN.lt(last_at))
        .update(uow, [issue::Model::LAST_SEEN.set(last_at)])
        .await?;
    current.count += n;
    current.last_seen = current.last_seen.max(last_at);
    current.last_release = Some(labels.release.clone());
    if level > read_level {
        current.level = level.as_str().to_owned();
    }
    // The status: changed only if it is still what the decision was
    // made from (compare and set). When another process changed it
    // first, read it again and decide again: only one of them
    // reopens a resolved issue, sets `regressed_at` and alerts.
    let mut regression = false;
    for _ in 0..STATUS_ATTEMPTS {
        let change = status_change(&current, n, last_at, &labels.release);
        let sets = match change {
            StatusChange::None => break,
            StatusChange::Reopen { regression } => {
                let mut sets = reopen_columns();
                if regression {
                    sets.push(issue::Model::REGRESSED_AT.set(Some(now)));
                }
                sets
            }
            StatusChange::IgnoreRemaining(left) => {
                vec![issue::Model::IGNORE_REMAINING.set(Some(left))]
            }
        };
        let changed = issue::Model::query()
            .where_key(current.id)
            .where_(issue::Model::STATUS.eq(current.status.as_str()))
            .where_(issue::Model::IGNORE_REMAINING.eq(current.ignore_remaining))
            .where_(issue::Model::RESOLVED_IN_RELEASE.eq(current.resolved_in_release.clone()))
            .update(uow, sets)
            .await?;
        if changed > 0 {
            match change {
                StatusChange::None => {}
                StatusChange::Reopen {
                    regression: regressed,
                } => {
                    regression = regressed;
                    current.status = IssueStatus::Open.as_str().to_owned();
                    current.resolved_at = None;
                    current.resolved_in_release = None;
                    current.ignored_until = None;
                    current.ignore_remaining = None;
                    if regressed {
                        current.regressed_at = Some(now);
                    }
                }
                StatusChange::IgnoreRemaining(left) => {
                    current.ignore_remaining = Some(left);
                }
            }
            break;
        }
        match issue::Model::find(uow, current.id).await? {
            Some(fresh) => current = fresh.into_inner(),
            None => break,
        }
    }
    Ok((current, regression))
}

/// What `n` new hits at `last_at` do to an issue's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusChange {
    /// Nothing (open, or still ignored with no count to lower).
    None,
    /// Back to open; a regression when it was resolved.
    Reopen { regression: bool },
    /// Still ignored, with this many occurrences left.
    IgnoreRemaining(i64),
}

/// The status change `n` hits at `last_at` in `release` make to `issue`.
fn status_change(
    issue: &issue::Model,
    n: i64,
    last_at: DateTime<Utc>,
    release: &str,
) -> StatusChange {
    match IssueStatus::parse(&issue.status).unwrap_or(IssueStatus::Open) {
        IssueStatus::Open => StatusChange::None,
        IssueStatus::Resolved => StatusChange::Reopen { regression: true },
        IssueStatus::ResolvedInNextRelease => {
            if issue.resolved_in_release.as_deref() == Some(release) {
                StatusChange::None
            } else {
                StatusChange::Reopen { regression: true }
            }
        }
        IssueStatus::Ignored => {
            if issue.ignored_until.is_some_and(|until| last_at >= until) {
                return StatusChange::Reopen { regression: false };
            }
            match issue.ignore_remaining {
                Some(remaining) if remaining - n <= 0 => StatusChange::Reopen { regression: false },
                Some(remaining) => StatusChange::IgnoreRemaining(remaining - n),
                None => StatusChange::None,
            }
        }
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
) -> occurrence::Model {
    occurrence::Model {
        id: occurrence.id,
        issue_id,
        at: occurrence.at,
        level: occurrence.level.as_str().to_owned(),
        message: occurrence.message.clone(),
        chain: serde_json::to_string(&occurrence.chain).unwrap_or_default(),
        backtrace: occurrence.backtrace.clone(),
        location: occurrence.location.clone(),
        request_id: occurrence.request_id.clone(),
        job_id: occurrence.job_id.clone(),
        trace_id: occurrence.trace_id.clone(),
        user_id: occurrence.user_id.clone(),
        workspace_id: occurrence.workspace_id.clone(),
        release: Some(labels.release.clone()),
        environment: Some(labels.environment.clone()),
        server: Some(labels.server.clone()),
        process: Some(labels.process.clone()),
        method: occurrence.method.clone(),
        route: occurrence.route.clone(),
        path: occurrence.path.clone(),
        status: occurrence.status.map(i32::from),
        sample_id: occurrence.sample_id,
        context: serde_json::to_string(&occurrence.context).unwrap_or_else(|_| "{}".into()),
    }
}

fn sample_model(sample: &NewSample, labels: &Labels) -> sample::Model {
    sample::Model {
        id: sample.id,
        kind: sample.kind.to_owned(),
        name: sample.name.clone(),
        at: sample.at,
        duration_ms: sample.duration_ms,
        status: sample.status.map(i32::from),
        outcome: sample.outcome.to_owned(),
        slow: sample.slow,
        request_id: sample.context.request_id.clone(),
        job_id: sample.context.job_id.clone(),
        trace_id: sample
            .context
            .trace
            .as_ref()
            .map(|trace| trace.trace_id.to_string()),
        user_id: sample.context.user_id.clone(),
        workspace_id: sample.context.workspace_id.clone(),
        release: Some(labels.release.clone()),
        environment: Some(labels.environment.clone()),
        server: Some(labels.server.clone()),
        process: Some(labels.process.clone()),
        spans: serde_json::to_string(&sample.spans).unwrap_or_else(|_| "[]".into()),
        context: serde_json::to_string(&sample.details).unwrap_or_else(|_| "{}".into()),
        error: sample.error.clone(),
    }
}

fn aggregate_model(row: &Row, labels: &Labels) -> aggregate::Model {
    aggregate::Model {
        id: Uuid::now_v7(),
        resolution: row.key.resolution,
        bucket: row.key.bucket,
        kind: row.key.kind.to_owned(),
        key_hash: key_hash(&row.key.key),
        key: row.key.key.clone(),
        process: labels.process_key.clone(),
        count: row.values.count,
        errors: row.values.errors,
        extra: row.values.extra,
        total_ms: row.values.total_ms,
        max_ms: row.values.max_ms(),
        histogram: row.values.histogram.encode(),
        detail: row.values.detail.encode(),
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

#[cfg(test)]
mod tests {
    use navree_db::navree_schema::Schema;

    use super::*;

    fn labels() -> Labels {
        Labels {
            environment: "test".into(),
            release: "r1".into(),
            server: "host".into(),
            process: "serve".into(),
            process_key: "host/serve/x".into(),
        }
    }

    async fn database() -> (Db, navree_testing::TempDirGuard) {
        let (url, guard) = navree_testing::temp_sqlite_url();
        let db = Db::connect(&navree_db::DbOptions {
            url,
            ..navree_db::DbOptions::default()
        })
        .await
        .unwrap();
        crate::migration::create_tables(&Schema::new(&db))
            .await
            .unwrap();
        (db, guard)
    }

    async fn issue_with(db: &Db, status: IssueStatus, remaining: Option<i64>) -> issue::Model {
        let at = Utc::now() - TimeDelta::hours(1);
        let model = issue::Model {
            id: Uuid::now_v7(),
            fingerprint: format!("fp-{}", Uuid::now_v7()),
            kind: IssueKind::Exception.as_str().to_owned(),
            level: Level::Error.as_str().to_owned(),
            status: status.as_str().to_owned(),
            title: "boom".into(),
            culprit: "GET /x".into(),
            message: "boom".into(),
            chain: "[]".into(),
            location: None,
            error_type: None,
            count: 10,
            first_seen: at,
            last_seen: at,
            first_release: Some("r1".into()),
            last_release: Some("r1".into()),
            environment: Some("test".into()),
            resolved_at: (status == IssueStatus::Resolved).then_some(at),
            resolved_in_release: None,
            ignored_until: None,
            ignore_remaining: remaining,
            regressed_at: None,
            comment: None,
            created_at: at,
            updated_at: at,
        };
        issue::Model::query()
            .insert(db, [model.clone()])
            .await
            .unwrap();
        stored(db, model.id).await
    }

    /// Adds `n` hits at `level` to `read` (a possibly stale read) in one
    /// transaction.
    async fn hits(db: &Db, read: &issue::Model, n: i64, level: Level) -> (issue::Model, bool) {
        let uow = db.uow(None, None).await;
        let result = count_existing(
            &uow,
            read.clone(),
            n,
            Utc::now(),
            level,
            &labels(),
            Utc::now(),
        )
        .await
        .unwrap();
        uow.commit().await.unwrap();
        result
    }

    async fn stored(db: &Db, id: Uuid) -> issue::Model {
        issue::Model::find(db, id)
            .await
            .unwrap()
            .unwrap()
            .into_inner()
    }

    #[tokio::test]
    async fn a_regression_is_raised_once_when_another_process_reopened_first() {
        let (db, _guard) = database().await;
        let read = issue_with(&db, IssueStatus::Resolved, None).await;
        // Another process saw the same resolved issue, reopened it as a
        // regression and committed, after this one read it.
        let earlier = Utc::now() - TimeDelta::minutes(1);
        issue::Model::query()
            .where_key(read.id)
            .update(
                &db,
                [
                    issue::Model::STATUS.set(IssueStatus::Open.as_str()),
                    issue::Model::RESOLVED_AT.set_null(),
                    issue::Model::REGRESSED_AT.set(Some(earlier)),
                    issue::Model::COUNT.set(11),
                ],
            )
            .await
            .unwrap();

        let (now_row, regression) = hits(&db, &read, 2, Level::Error).await;
        assert!(!regression, "the other process raised it");
        assert_eq!(now_row.status, "open");
        let row = stored(&db, read.id).await;
        assert_eq!(row.count, 13, "both processes' hits are counted");
        assert_eq!(
            row.regressed_at.map(|at| at.timestamp()),
            Some(earlier.timestamp()),
            "regressed_at is not moved"
        );
    }

    #[tokio::test]
    async fn a_resolved_issue_reopens_as_a_regression_once() {
        let (db, _guard) = database().await;
        let read = issue_with(&db, IssueStatus::Resolved, None).await;
        let (row, regression) = hits(&db, &read, 1, Level::Warning).await;
        assert!(regression);
        assert_eq!(row.status, "open");
        assert!(row.regressed_at.is_some());
        assert_eq!(row.level, "error", "a lower level does not lower it");
        let (_, again) = hits(&db, &read, 1, Level::Error).await;
        assert!(!again, "the same stale read raises no second regression");
        assert_eq!(stored(&db, read.id).await.count, 12);
    }

    #[tokio::test]
    async fn ignore_counters_are_not_lost_between_processes() {
        let (db, _guard) = database().await;
        let read = issue_with(&db, IssueStatus::Ignored, Some(10)).await;
        // Two processes read `ignore_remaining = 10`; each adds 3 hits.
        hits(&db, &read, 3, Level::Error).await;
        hits(&db, &read, 3, Level::Error).await;
        let row = stored(&db, read.id).await;
        assert_eq!(row.ignore_remaining, Some(4));
        assert_eq!(row.status, "ignored");
        assert_eq!(row.count, 16);
        // The next 4 reopen it, without a regression.
        let (row, regression) = hits(&db, &read, 4, Level::Error).await;
        assert!(!regression);
        assert_eq!(row.status, "open");
        assert_eq!(row.ignore_remaining, None);
    }
}
