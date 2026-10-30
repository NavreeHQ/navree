//! Lighthouse's read tools: `lighthouse_list_issues`, `lighthouse_get_issue`,
//! `lighthouse_slow_routes`, `lighthouse_slow_queries` and
//! `lighthouse_failed_jobs`.
//!
//! Navigator lists them when the app includes Lighthouse ([`included`]: a
//! source file under `src/` adds `LighthouseModule`). They read
//! Lighthouse's tables (`lighthouse_issues`, `lighthouse_occurrences`,
//! `lighthouse_aggregates`, `lighthouse_alerts`) in the database Lighthouse
//! writes to, `lighthouse.database_url` or else `database.url`, through
//! the same read-only path as `query_database`: fixed `SELECT` statements,
//! the local-database rule, the timeout. They mirror the figures of
//! Lighthouse's JSON API: per-route and per-statement figures merge the
//! per-hour rows (per-minute rows for `1h`) of every process and their
//! latency histograms ([`Histogram`]).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::database::{Driver, QueryResult};
use crate::tools::{ToolDef, ToolError};

/// The five tools, in the order agents see them (after the others).
pub const LIGHTHOUSE_TOOLS: [&str; 5] = [
    "lighthouse_list_issues",
    "lighthouse_get_issue",
    "lighthouse_slow_routes",
    "lighthouse_slow_queries",
    "lighthouse_failed_jobs",
];

/// Whether `name` is one of [`LIGHTHOUSE_TOOLS`].
pub fn is_lighthouse_tool(name: &str) -> bool {
    LIGHTHOUSE_TOOLS.contains(&name)
}

/// What a Lighthouse tool says when the app doesn't include Lighthouse.
pub const NOT_INCLUDED: &str = "the app doesn't include Lighthouse: no file under src/ adds \
`LighthouseModule`. Add `.module(LighthouseModule::from_config())` to src/app.rs (the `lighthouse` \
feature, part of `full`) and run `navree migrate`; see https://navree.com/docs/tools/lighthouse/";

/// How many files [`included`] looks at, at most.
const SCAN_LIMIT: usize = 5_000;

/// Whether the app at `root` includes Lighthouse: a `.rs` file under its
/// `src/` names `LighthouseModule`.
pub fn included(root: &Path) -> bool {
    let mut stack = vec![root.join("src")];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                seen += 1;
                if seen > SCAN_LIMIT {
                    return false;
                }
                if std::fs::read_to_string(&path).is_ok_and(|s| s.contains("LighthouseModule")) {
                    return true;
                }
            }
        }
    }
    false
}

fn object(properties: Value, required: &[&str]) -> Value {
    let mut schema =
        json!({ "type": "object", "properties": properties, "additionalProperties": false });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn period_schema() -> Value {
    json!({ "type": "string", "enum": ["1h", "24h", "7d"],
            "description": "The period: `1h`, `24h` (the default) or `7d`." })
}

fn limit_schema(max: u64, default: u64) -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": max,
            "description": format!("How many (default {default}, at most {max}).") })
}

/// The definitions of the five tools.
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "lighthouse_list_issues",
            title: "Lighthouse issues",
            description: "Lighthouse's issues (exceptions, slow requests, slow queries, failed jobs, failed \
                scheduled tasks and failing outgoing calls, grouped by fingerprint), most recently seen first: \
                id, kind, level, status, title, culprit, count, first_seen, last_seen. Open issues unless \
                `status` says otherwise. Pass an id to lighthouse_get_issue for the details.",
            input_schema: object(
                json!({
                    "status": { "type": "string",
                                "enum": ["open", "resolved", "resolved_in_next_release", "ignored", "all"],
                                "description": "Issues with this status (default `open`; `all` for every one)." },
                    "kind": { "type": "string",
                              "enum": ["exception", "slow_request", "slow_query", "failed_job", "scheduled_task",
                                       "outgoing_request"],
                              "description": "Only issues of this kind." },
                    "level": { "type": "string", "enum": ["error", "warning", "info"],
                               "description": "Only issues of this level." },
                    "search": { "type": "string", "description": "Text in the title or culprit (any case)." },
                    "sort": { "type": "string", "enum": ["last_seen", "first_seen", "count"],
                              "description": "The order, newest or largest first (default `last_seen`)." },
                    "limit": limit_schema(100, 20),
                }),
                &[],
            ),
            open_world: false,
        },
        ToolDef {
            name: "lighthouse_get_issue",
            title: "Lighthouse issue",
            description: "One Lighthouse issue: its message, source chain, location, error type, releases and \
                resolution, the five latest occurrences (time, request id, method, route, path, status, user, \
                workspace, release, backtrace and context), and the alerts sent for it.",
            input_schema: object(
                json!({ "id": { "type": "string", "description": "The issue's id (a UUID)." } }),
                &["id"],
            ),
            open_world: false,
        },
        ToolDef {
            name: "lighthouse_slow_routes",
            title: "Lighthouse slow routes",
            description: "Every route Lighthouse measured in the period, slowest (p95) first: endpoint, \
                requests, errors, error_rate, avg_ms, p50_ms, p95_ms, p99_ms and max_ms, merged across \
                processes.",
            input_schema: object(
                json!({ "period": period_schema(), "limit": limit_schema(50, 10) }),
                &[],
            ),
            open_world: false,
        },
        ToolDef {
            name: "lighthouse_slow_queries",
            title: "Lighthouse slow queries",
            description: "The normalized SQL statements Lighthouse measured in the period, by total time: \
                statement, count, errors, total_ms, avg_ms, p95_ms and max_ms.",
            input_schema: object(
                json!({ "period": period_schema(), "limit": limit_schema(50, 10) }),
                &[],
            ),
            open_world: false,
        },
        ToolDef {
            name: "lighthouse_failed_jobs",
            title: "Lighthouse failed jobs",
            description: "Job types with failed attempts in the period (job_type, attempts, failed_attempts, \
                failed_jobs that used up their attempts, avg_ms, p95_ms), most failures first, and the open \
                `failed_job` issues with their ids for lighthouse_get_issue.",
            input_schema: object(
                json!({ "period": period_schema(), "limit": limit_schema(50, 10) }),
                &[],
            ),
            open_world: false,
        },
    ]
}

/// `lighthouse_list_issues`'s arguments.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IssuesArgs {
    status: Option<IssueStatus>,
    kind: Option<IssueKind>,
    level: Option<Level>,
    search: Option<String>,
    sort: Option<Sort>,
    limit: Option<usize>,
}

/// `lighthouse_get_issue`'s argument.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IssueArgs {
    id: String,
}

/// The arguments of the per-period tools.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PeriodArgs {
    period: Option<Period>,
    limit: Option<usize>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IssueStatus {
    Open,
    Resolved,
    ResolvedInNextRelease,
    Ignored,
    All,
}

impl IssueStatus {
    fn as_str(self) -> Option<&'static str> {
        match self {
            IssueStatus::Open => Some("open"),
            IssueStatus::Resolved => Some("resolved"),
            IssueStatus::ResolvedInNextRelease => Some("resolved_in_next_release"),
            IssueStatus::Ignored => Some("ignored"),
            IssueStatus::All => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IssueKind {
    Exception,
    SlowRequest,
    SlowQuery,
    FailedJob,
    ScheduledTask,
    OutgoingRequest,
}

impl IssueKind {
    fn as_str(self) -> &'static str {
        match self {
            IssueKind::Exception => "exception",
            IssueKind::SlowRequest => "slow_request",
            IssueKind::SlowQuery => "slow_query",
            IssueKind::FailedJob => "failed_job",
            IssueKind::ScheduledTask => "scheduled_task",
            IssueKind::OutgoingRequest => "outgoing_request",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Level {
    Error,
    Warning,
    Info,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Sort {
    LastSeen,
    FirstSeen,
    Count,
}

/// A period of Lighthouse's figures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum Period {
    /// The last hour, from per-minute rows.
    #[serde(rename = "1h")]
    Hour,
    /// The last 24 hours, from per-hour rows (the default).
    #[default]
    #[serde(rename = "24h")]
    Day,
    /// The last 7 days, from per-hour rows.
    #[serde(rename = "7d")]
    Week,
}

impl Period {
    /// `1h`, `24h` or `7d`.
    pub fn as_str(self) -> &'static str {
        match self {
            Period::Hour => "1h",
            Period::Day => "24h",
            Period::Week => "7d",
        }
    }

    /// The seconds per row it reads: 60 for `1h`, 3600 otherwise.
    pub fn resolution(self) -> i64 {
        match self {
            Period::Hour => 60,
            Period::Day | Period::Week => 3600,
        }
    }

    fn minutes(self) -> i64 {
        match self {
            Period::Hour => 60,
            Period::Day => 24 * 60,
            Period::Week => 7 * 24 * 60,
        }
    }

    /// The first bucket of the period ending at `now`, as Lighthouse counts
    /// it: the current bucket and the ones before it.
    pub fn start(self, now: DateTime<Utc>) -> DateTime<Utc> {
        let resolution = self.resolution();
        let buckets = self.minutes() * 60 / resolution;
        let current = now
            .duration_trunc(TimeDelta::seconds(resolution))
            .unwrap_or(now);
        current - TimeDelta::seconds(resolution * (buckets - 1))
    }
}

/// A latency histogram in Lighthouse's stored form (`bucket:count` pairs;
/// buckets grow by 25% from 0.5 ms), for percentiles over merged rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Histogram {
    counts: BTreeMap<u16, u64>,
    count: u64,
    max_ms: f64,
}

const FIRST: f64 = 0.5;
const GROWTH: f64 = 1.25;
const BUCKETS: usize = 66;

impl Histogram {
    /// An empty histogram.
    pub fn new() -> Histogram {
        Histogram::default()
    }

    /// Records one duration in milliseconds.
    pub fn record(&mut self, ms: f64) {
        let ms = if ms.is_finite() { ms.max(0.0) } else { 0.0 };
        *self.counts.entry(bucket_of(ms)).or_default() += 1;
        self.count += 1;
        self.max_ms = self.max_ms.max(ms);
    }

    /// How many durations it holds.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The stored form: `bucket:count` pairs separated by commas.
    pub fn encode(&self) -> String {
        self.counts
            .iter()
            .map(|(bucket, count)| format!("{bucket}:{count}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Reads the stored form (several rows' forms joined by commas merge),
    /// with the largest duration. Malformed pairs are skipped.
    pub fn decode(text: &str, max_ms: f64) -> Histogram {
        let mut histogram = Histogram {
            max_ms,
            ..Histogram::default()
        };
        for pair in text.split(',') {
            let Some((bucket, count)) = pair.split_once(':') else {
                continue;
            };
            let (Ok(bucket), Ok(count)) =
                (bucket.trim().parse::<u16>(), count.trim().parse::<u64>())
            else {
                continue;
            };
            *histogram.counts.entry(bucket).or_default() += count;
            histogram.count += count;
        }
        histogram
    }

    /// The `q` quantile in milliseconds (the upper bound of its bucket,
    /// never above the maximum); 0 when empty.
    pub fn quantile(&self, q: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let rank = ((q.clamp(0.0, 1.0) * self.count as f64).ceil() as u64).max(1);
        let mut seen = 0;
        for (bucket, count) in &self.counts {
            seen += count;
            if seen >= rank {
                return upper_bound(*bucket).min(self.max_ms);
            }
        }
        self.max_ms
    }
}

fn bucket_of(ms: f64) -> u16 {
    if ms <= FIRST {
        return 0;
    }
    let index = ((ms / FIRST).ln() / GROWTH.ln()).ceil() as usize;
    index.min(BUCKETS - 1) as u16
}

fn upper_bound(bucket: u16) -> f64 {
    if usize::from(bucket) >= BUCKETS - 1 {
        return f64::INFINITY;
    }
    FIRST * GROWTH.powi(i32::from(bucket))
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

/// A SQL string literal.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// A time literal in the form Lighthouse stores (RFC 3339, UTC offset).
fn time_literal(at: DateTime<Utc>) -> String {
    literal(&at.format("%Y-%m-%dT%H:%M:%S+00:00").to_string())
}

/// A UUID column as text: SQLite holds Lighthouse's ids as 16-byte blobs.
fn uuid_column(driver: Driver, column: &str) -> String {
    match driver {
        Driver::Sqlite => format!(
            "CASE WHEN typeof({column}) = 'blob' THEN lower(hex({column})) ELSE {column} END AS {column}"
        ),
        Driver::Postgres => format!("{column}::text AS {column}"),
    }
}

/// `column = <the id>` for an id given as 32 hex digits.
fn uuid_equals(driver: Driver, column: &str, hex: &str) -> String {
    let hyphenated = hyphenate(hex);
    match driver {
        Driver::Sqlite => format!("({column} = X'{hex}' OR lower({column}) = '{hyphenated}')"),
        Driver::Postgres => format!("{column} = '{hyphenated}'"),
    }
}

/// An id as 32 lowercase hex digits, from its hyphenated or simple form.
fn parse_id(id: &str) -> Option<String> {
    let hex: String = id.trim().chars().filter(|c| *c != '-').collect();
    (hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

fn hyphenate(hex: &str) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

const ISSUE_COLUMNS: &str = "kind, level, status, title, culprit, count, first_seen, last_seen";

/// `lighthouse_list_issues`'s statement.
pub(crate) fn issues_sql(driver: Driver, args: &IssuesArgs) -> String {
    let mut conditions = Vec::new();
    if let Some(status) = args.status.unwrap_or(IssueStatus::Open).as_str() {
        conditions.push(format!("status = {}", literal(status)));
    }
    if let Some(kind) = args.kind {
        conditions.push(format!("kind = {}", literal(kind.as_str())));
    }
    if let Some(level) = args.level {
        conditions.push(format!("level = {}", literal(level.as_str())));
    }
    if let Some(search) = args
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let escaped = search
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = literal(&format!("%{escaped}%"));
        conditions.push(format!(
            "(lower(title) LIKE {pattern} ESCAPE '\\' OR lower(culprit) LIKE {pattern} ESCAPE '\\')"
        ));
    }
    let order = match args.sort.unwrap_or(Sort::LastSeen) {
        Sort::LastSeen => "last_seen DESC",
        Sort::FirstSeen => "first_seen DESC",
        Sort::Count => "count DESC, last_seen DESC",
    };
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    format!(
        "SELECT {}, {ISSUE_COLUMNS}, last_release, environment FROM lighthouse_issues{filter} ORDER BY {order} LIMIT {}",
        uuid_column(driver, "id"),
        args.limit.unwrap_or(20).clamp(1, 100)
    )
}

/// The issue's own statement.
fn issue_sql(driver: Driver, hex: &str) -> String {
    format!(
        "SELECT {}, {ISSUE_COLUMNS}, message, chain, location, error_type, first_release, last_release, \
         environment, resolved_at, resolved_in_release, ignored_until, ignore_remaining, regressed_at, comment \
         FROM lighthouse_issues WHERE {}",
        uuid_column(driver, "id"),
        uuid_equals(driver, "id", hex)
    )
}

/// The issue's five latest occurrences.
fn occurrences_sql(driver: Driver, hex: &str) -> String {
    format!(
        "SELECT at, level, message, chain, location, request_id, job_id, trace_id, user_id, workspace_id, release, \
         environment, server, process, method, route, path, status, backtrace, context FROM lighthouse_occurrences \
         WHERE {} ORDER BY at DESC LIMIT 5",
        uuid_equals(driver, "issue_id", hex)
    )
}

/// The alerts sent for the issue.
fn alerts_sql(driver: Driver, hex: &str) -> String {
    format!(
        "SELECT kind, channel, status, attempts, error, created_at, delivered_at FROM lighthouse_alerts \
         WHERE {} ORDER BY created_at DESC LIMIT 10",
        uuid_equals(driver, "issue_id", hex)
    )
}

/// The rows of one aggregate kind in the period, merged per key.
fn aggregates_sql(
    driver: Driver,
    kind: &str,
    period: Period,
    now: DateTime<Utc>,
    order: &str,
) -> String {
    let concat = match driver {
        Driver::Sqlite => "group_concat(histogram, ',')",
        Driver::Postgres => "string_agg(histogram, ',')",
    };
    format!(
        "SELECT key, sum(count) AS count, sum(errors) AS errors, sum(extra) AS extra, sum(total_ms) AS total_ms, \
         max(max_ms) AS max_ms, {concat} AS histogram FROM lighthouse_aggregates WHERE kind = {} AND resolution = {} \
         AND bucket >= {} GROUP BY key_hash, key ORDER BY {order} LIMIT 100",
        literal(kind),
        period.resolution(),
        time_literal(period.start(now)),
    )
}

// ---------------------------------------------------------------------------
// Answers
// ---------------------------------------------------------------------------

/// Rows as objects, ids hyphenated and JSON text columns parsed.
fn objects(result: QueryResult) -> Vec<Map<String, Value>> {
    result
        .rows
        .into_iter()
        .map(|row| {
            result
                .columns
                .iter()
                .cloned()
                .zip(row)
                .map(|(column, value)| {
                    let value = match (column.as_str(), &value) {
                        ("id", Value::String(text)) => match parse_id(text) {
                            Some(hex) => Value::from(hyphenate(&hex)),
                            None => value,
                        },
                        ("chain" | "context", Value::String(text)) => {
                            serde_json::from_str(text).unwrap_or(value)
                        }
                        _ => value,
                    };
                    (column, value)
                })
                .collect()
        })
        .collect()
}

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn integer(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        Some(Value::String(s)) => s
            .parse()
            .unwrap_or_else(|_| s.parse::<f64>().unwrap_or(0.0) as i64),
        _ => 0,
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// One merged key of an aggregate kind.
struct Merged {
    key: String,
    count: i64,
    errors: i64,
    extra: i64,
    total_ms: f64,
    max_ms: f64,
    histogram: Histogram,
}

impl Merged {
    fn avg_ms(&self) -> f64 {
        round2(self.total_ms / self.count.max(1) as f64)
    }

    fn quantile(&self, q: f64) -> f64 {
        round2(self.histogram.quantile(q))
    }
}

fn merged(result: QueryResult) -> Vec<Merged> {
    objects(result)
        .into_iter()
        .map(|row| {
            let max_ms = number(row.get("max_ms"));
            Merged {
                key: row
                    .get("key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                count: integer(row.get("count")),
                errors: integer(row.get("errors")),
                extra: integer(row.get("extra")),
                total_ms: number(row.get("total_ms")),
                max_ms,
                histogram: Histogram::decode(
                    row.get("histogram")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    max_ms,
                ),
            }
        })
        .collect()
}

/// Runs Lighthouse statements for one tool call.
pub(crate) trait Reader {
    /// The driver of Lighthouse's database.
    fn driver(&self) -> Driver;
    /// Runs one statement.
    fn read(
        &self,
        sql: String,
    ) -> impl std::future::Future<Output = Result<QueryResult, ToolError>> + Send;
}

/// `lighthouse_list_issues`.
pub(crate) async fn list_issues(db: &impl Reader, args: IssuesArgs) -> Result<Value, ToolError> {
    let issues = objects(db.read(issues_sql(db.driver(), &args)).await?);
    Ok(json!({ "issues": issues }))
}

/// `lighthouse_get_issue`.
pub(crate) async fn get_issue(db: &impl Reader, args: IssueArgs) -> Result<Value, ToolError> {
    let hex = parse_id(&args.id).ok_or_else(|| {
        ToolError(format!(
            "`{}` is not an issue id; lighthouse_list_issues gives the ids (UUIDs)",
            args.id
        ))
    })?;
    let driver = db.driver();
    let Some(issue) = objects(db.read(issue_sql(driver, &hex)).await?)
        .into_iter()
        .next()
    else {
        return Err(ToolError(format!(
            "no Lighthouse issue {}; lighthouse_list_issues lists them",
            hyphenate(&hex)
        )));
    };
    let occurrences = objects(db.read(occurrences_sql(driver, &hex)).await?);
    let alerts = objects(db.read(alerts_sql(driver, &hex)).await?);
    Ok(json!({ "issue": issue, "occurrences": occurrences, "alerts": alerts }))
}

/// `lighthouse_slow_routes`.
pub(crate) async fn slow_routes(
    db: &impl Reader,
    args: PeriodArgs,
    now: DateTime<Utc>,
) -> Result<Value, ToolError> {
    let period = args.period.unwrap_or_default();
    let sql = aggregates_sql(db.driver(), "request", period, now, "max(max_ms) DESC");
    let mut routes = merged(db.read(sql).await?);
    routes.sort_by(|a, b| {
        b.histogram
            .quantile(0.95)
            .total_cmp(&a.histogram.quantile(0.95))
    });
    let routes: Vec<Value> = routes
        .iter()
        .take(args.limit.unwrap_or(10).clamp(1, 50))
        .map(|r| {
            json!({
                "endpoint": r.key,
                "requests": r.count,
                "errors": r.errors,
                "error_rate": round2(r.errors as f64 / r.count.max(1) as f64),
                "avg_ms": r.avg_ms(),
                "p50_ms": r.quantile(0.5),
                "p95_ms": r.quantile(0.95),
                "p99_ms": r.quantile(0.99),
                "max_ms": round2(r.max_ms),
            })
        })
        .collect();
    Ok(json!({ "period": period.as_str(), "routes": routes }))
}

/// `lighthouse_slow_queries`.
pub(crate) async fn slow_queries(
    db: &impl Reader,
    args: PeriodArgs,
    now: DateTime<Utc>,
) -> Result<Value, ToolError> {
    let period = args.period.unwrap_or_default();
    let sql = aggregates_sql(db.driver(), "query", period, now, "sum(total_ms) DESC");
    let queries: Vec<Value> = merged(db.read(sql).await?)
        .iter()
        .take(args.limit.unwrap_or(10).clamp(1, 50))
        .map(|q| {
            json!({
                "statement": q.key,
                "count": q.count,
                "errors": q.errors,
                "total_ms": round2(q.total_ms),
                "avg_ms": q.avg_ms(),
                "p95_ms": q.quantile(0.95),
                "max_ms": round2(q.max_ms),
            })
        })
        .collect();
    Ok(json!({ "period": period.as_str(), "queries": queries }))
}

/// `lighthouse_failed_jobs`.
pub(crate) async fn failed_jobs(
    db: &impl Reader,
    args: PeriodArgs,
    now: DateTime<Utc>,
) -> Result<Value, ToolError> {
    let period = args.period.unwrap_or_default();
    let limit = args.limit.unwrap_or(10).clamp(1, 50);
    let driver = db.driver();
    let sql = aggregates_sql(
        driver,
        "job",
        period,
        now,
        "sum(extra) DESC, sum(errors) DESC",
    );
    let jobs: Vec<Value> = merged(db.read(sql).await?)
        .iter()
        .filter(|j| j.errors > 0 || j.extra > 0)
        .take(limit)
        .map(|j| {
            json!({
                "job_type": j.key,
                "attempts": j.count,
                "failed_attempts": j.errors,
                "failed_jobs": j.extra,
                "avg_ms": j.avg_ms(),
                "p95_ms": j.quantile(0.95),
            })
        })
        .collect();
    let issues = IssuesArgs {
        kind: Some(IssueKind::FailedJob),
        limit: Some(limit),
        ..IssuesArgs::default()
    };
    let issues = objects(db.read(issues_sql(driver, &issues)).await?);
    Ok(json!({ "period": period.as_str(), "jobs": jobs, "issues": issues }))
}

/// A database error from Lighthouse's tables, said in Lighthouse's terms.
pub(crate) fn explain(message: String, url: &str) -> ToolError {
    let lower = message.to_lowercase();
    if lower.contains("lighthouse_")
        && (lower.contains("no such table") || lower.contains("does not exist"))
    {
        return ToolError(format!(
            "Lighthouse's tables aren't in the database ({url}): run `navree migrate` (LighthouseModule's \
             `lighthouse` migration creates them), then use the app so Lighthouse has something to show."
        ));
    }
    ToolError(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_start_at_their_first_bucket() {
        let now = DateTime::parse_from_rfc3339("2026-10-27T10:37:12Z")
            .unwrap()
            .with_timezone(&Utc);
        let at = |p: Period| p.start(now).to_rfc3339();
        assert_eq!(at(Period::Hour), "2026-10-27T09:38:00+00:00");
        assert_eq!(at(Period::Day), "2026-10-26T11:00:00+00:00");
        assert_eq!(at(Period::Week), "2026-10-20T11:00:00+00:00");
        assert_eq!(
            time_literal(Period::Day.start(now)),
            "'2026-10-26T11:00:00+00:00'"
        );
    }

    #[test]
    fn histograms_merge_from_joined_rows() {
        let mut a = Histogram::new();
        let mut b = Histogram::new();
        for ms in 1..=100 {
            a.record(f64::from(ms));
            b.record(f64::from(ms));
        }
        let joined = format!("{},,{}", a.encode(), b.encode());
        let merged = Histogram::decode(&joined, 100.0);
        assert_eq!(merged.count(), 200);
        assert!((95.0..=100.0).contains(&merged.quantile(0.95)));
        assert_eq!(merged.quantile(1.0), 100.0);
        assert_eq!(Histogram::decode("x,1:2,bad:3", 4.0).count(), 2);
        assert_eq!(Histogram::new().quantile(0.5), 0.0);
    }

    #[test]
    fn ids_are_checked_before_they_reach_a_statement() {
        assert_eq!(
            parse_id("0192F3A4-B5C6-7D8E-9F00-112233445566").as_deref(),
            Some("0192f3a4b5c67d8e9f00112233445566")
        );
        assert_eq!(parse_id("1' OR '1'='1"), None);
        assert_eq!(parse_id("0192f3a4"), None);
        let sql = issue_sql(Driver::Sqlite, "0192f3a4b5c67d8e9f00112233445566");
        assert!(
            sql.contains("id = X'0192f3a4b5c67d8e9f00112233445566'"),
            "{sql}"
        );
        let sql = issue_sql(Driver::Postgres, "0192f3a4b5c67d8e9f00112233445566");
        assert!(
            sql.contains("id = '0192f3a4-b5c6-7d8e-9f00-112233445566'"),
            "{sql}"
        );
    }

    #[test]
    fn search_text_is_a_literal() {
        let args = IssuesArgs {
            search: Some("it's 50%_off\\".to_owned()),
            ..IssuesArgs::default()
        };
        let sql = issues_sql(Driver::Postgres, &args);
        assert!(
            sql.contains(r"LIKE '%it''s 50\%\_off\\%' ESCAPE '\'"),
            "{sql}"
        );
        assert!(sql.contains("status = 'open'"), "{sql}");
        let all = IssuesArgs {
            status: Some(IssueStatus::All),
            ..IssuesArgs::default()
        };
        assert!(!issues_sql(Driver::Sqlite, &all).contains("WHERE"));
    }

    #[test]
    fn every_statement_passes_the_read_only_guard() {
        let now = Utc::now();
        for driver in [Driver::Sqlite, Driver::Postgres] {
            let hex = "0192f3a4b5c67d8e9f00112233445566";
            let args = IssuesArgs {
                search: Some("x".to_owned()),
                kind: Some(IssueKind::Exception),
                level: Some(Level::Error),
                sort: Some(Sort::Count),
                ..IssuesArgs::default()
            };
            for sql in [
                issues_sql(driver, &args),
                issue_sql(driver, hex),
                occurrences_sql(driver, hex),
                alerts_sql(driver, hex),
                aggregates_sql(driver, "request", Period::Week, now, "max(max_ms) DESC"),
            ] {
                crate::database::check(&sql, driver.dialect())
                    .unwrap_or_else(|e| panic!("{sql}: {e}"));
            }
        }
    }

    #[test]
    fn only_a_source_naming_the_module_includes_lighthouse() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        assert!(!included(dir.path()));
        std::fs::create_dir_all(dir.path().join("src/modules")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        assert!(!included(dir.path()));
        std::fs::write(
            dir.path().join("src/modules/app.rs"),
            ".module(LighthouseModule::from_config())",
        )
        .unwrap();
        assert!(included(dir.path()));
    }
}
