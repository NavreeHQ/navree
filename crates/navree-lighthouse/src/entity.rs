//! Lighthouse's tables (migration set `lighthouse`): navree-orm models
//! (`issue::Model::query()`, typed columns such as
//! `occurrence::Model::ISSUE_ID`).
//!
//! - `lighthouse_issues`: one row per fingerprint, with its status and
//!   exact count;
//! - `lighthouse_occurrences`: the stored occurrences of each issue, with
//!   their context;
//! - `lighthouse_samples`: request and job samples with their spans
//!   (traces);
//! - `lighthouse_aggregates`: per-minute and per-hour counts and latency
//!   histograms, one row per bucket, kind, key and process;
//! - `lighthouse_alerts`: every alert, per channel, and whether it was
//!   delivered;
//! - `lighthouse_releases`: the releases seen, reported by a process at
//!   boot or by a deploy, with when each was deployed;
//! - `lighthouse_settings`: the settings changed from the dashboard, one
//!   row per key (the configuration holds the default).

/// `lighthouse_issues`.
pub mod issue {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// An issue row.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_issues")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The fingerprint (32 hex characters), unique.
        pub fingerprint: String,
        /// `exception`, `slow_request`, ...
        pub kind: String,
        /// `error`, `warning` or `info` (the highest seen).
        pub level: String,
        /// `open`, `resolved`, `resolved_in_next_release` or `ignored`.
        pub status: String,
        /// The first line of the message.
        pub title: String,
        /// The route, job, task or host.
        pub culprit: String,
        /// The full message of the first occurrence.
        pub message: String,
        /// The source chain (JSON array of strings).
        pub chain: String,
        /// Where it happened (`file:line:column`), when known.
        pub location: Option<String>,
        /// The error's type name, when known.
        pub error_type: Option<String>,
        /// Occurrences, exact.
        pub count: i64,
        /// The first occurrence.
        pub first_seen: DateTime<Utc>,
        /// The latest occurrence.
        pub last_seen: DateTime<Utc>,
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
        /// When it last came back after being resolved.
        pub regressed_at: Option<DateTime<Utc>>,
        /// The release it last came back in.
        pub regressed_in_release: Option<String>,
        /// The comment of the last status change.
        pub comment: Option<String>,
        /// When the row was created.
        pub created_at: DateTime<Utc>,
        /// When the row last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// `lighthouse_occurrences`.
pub mod occurrence {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// A stored occurrence.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_occurrences")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The issue (`lighthouse_issues.id`, cascading delete).
        pub issue_id: Uuid,
        /// When it happened.
        pub at: DateTime<Utc>,
        /// Its level.
        pub level: String,
        /// The message.
        pub message: String,
        /// The source chain (JSON array).
        pub chain: String,
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
        /// The signed-in user.
        pub user_id: Option<String>,
        /// The workspace.
        pub workspace_id: Option<String>,
        /// The release label.
        pub release: Option<String>,
        /// The environment label.
        pub environment: Option<String>,
        /// The server label.
        pub server: Option<String>,
        /// The process (`serve`, `queue:work`, a command).
        pub process: Option<String>,
        /// The request method.
        pub method: Option<String>,
        /// The route template.
        pub route: Option<String>,
        /// The request path.
        pub path: Option<String>,
        /// The response status.
        pub status: Option<i32>,
        /// The request or job sample (trace), when one was stored.
        pub sample_id: Option<Uuid>,
        /// Headers, body, client address, tags, report context, job details
        /// (JSON object, masked).
        pub context: String,
    }
}

/// `lighthouse_samples`.
pub mod sample {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// A request or job sample with its spans.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_samples")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// `request` or `job`.
        pub kind: String,
        /// The endpoint (`GET /api/v1/deals/board`) or the job type.
        pub name: String,
        /// When it finished.
        pub at: DateTime<Utc>,
        /// How long it took, in milliseconds.
        pub duration_ms: f64,
        /// The response status (requests).
        pub status: Option<i32>,
        /// `ok`, `error` (a 4xx or 5xx response), `failed_attempt` or
        /// `failed` (jobs).
        pub outcome: String,
        /// Whether it was slower than its threshold.
        pub slow: bool,
        /// The request id.
        pub request_id: Option<String>,
        /// The job id.
        pub job_id: Option<String>,
        /// The trace id.
        pub trace_id: Option<String>,
        /// The signed-in user.
        pub user_id: Option<String>,
        /// The workspace.
        pub workspace_id: Option<String>,
        /// The release label.
        pub release: Option<String>,
        /// The environment label.
        pub environment: Option<String>,
        /// The server label.
        pub server: Option<String>,
        /// The process.
        pub process: Option<String>,
        /// The spans (JSON array of `{kind, label, start_ms, duration_ms,
        /// failed}`).
        pub spans: String,
        /// Headers, client address, tags, job details (JSON object, masked).
        pub context: String,
        /// The error of a failed job attempt.
        pub error: Option<String>,
    }
}

/// `lighthouse_aggregates`.
pub mod aggregate {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// Counts and a latency histogram of one key in one bucket, written by
    /// one process.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_aggregates")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The bucket size in seconds: 60 or 3600.
        pub resolution: i32,
        /// The bucket's start.
        pub bucket: DateTime<Utc>,
        /// `request`, `query`, `job`, `outgoing`, `exception` or `issue`.
        pub kind: String,
        /// A hash of the key (what rows are matched by).
        pub key_hash: String,
        /// The endpoint, statement, job type, host or fingerprint.
        pub key: String,
        /// The process that wrote the row.
        pub process: String,
        /// How many.
        pub count: i64,
        /// How many failed (requests answering 4xx or 5xx, failed attempts,
        /// failed outgoing calls).
        pub errors: i64,
        /// Requests: 5xx responses. Jobs: jobs moved to the failed jobs.
        pub extra: i64,
        /// The summed duration in milliseconds.
        pub total_ms: f64,
        /// The longest duration in milliseconds.
        pub max_ms: f64,
        /// The latency histogram (`bucket:count,...`).
        pub histogram: String,
        /// Kind-specific details (JSON): the routes and jobs that ran a
        /// query, the slowest outgoing calls.
        pub detail: Option<String>,
    }
}

/// `lighthouse_alerts`.
pub mod alert {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// One alert through one channel.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_alerts")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The issue it is about.
        pub issue_id: Option<Uuid>,
        /// `new_issue`, `regression`, `spike`, `failed_job` or `long_wait`.
        pub kind: String,
        /// The channel's name (`email`, `slack`, `webhook`, or yours).
        pub channel: String,
        /// What the cooldown is counted for (the issue id).
        pub subject_key: String,
        /// `pending`, `delivered` or `failed`.
        pub status: String,
        /// Delivery attempts.
        pub attempts: i32,
        /// The last delivery error.
        pub error: Option<String>,
        /// The alert (JSON).
        pub payload: String,
        /// When it was raised.
        pub created_at: DateTime<Utc>,
        /// When it was delivered.
        pub delivered_at: Option<DateTime<Utc>>,
    }
}

/// `lighthouse_releases`.
pub mod release {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// A release Lighthouse has seen.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_releases")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The release label (`3f2a9c1`, `20261026-091512-3f2a9c1`).
        pub name: String,
        /// The environment it runs in; the pair is unique.
        pub environment: String,
        /// The commit it was built from, when reported.
        pub commit_sha: Option<String>,
        /// A link to the release (the changes, the deploy), when reported.
        pub url: Option<String>,
        /// Notes reported with it.
        pub notes: Option<String>,
        /// `process` (a process booted with this label) or `api` (reported
        /// by a deploy, or by the app's code).
        pub source: String,
        /// When it was deployed: as reported, else when it was first seen.
        pub deployed_at: DateTime<Utc>,
        /// When a process first reported it.
        pub first_seen: DateTime<Utc>,
        /// When a process last reported it.
        pub last_seen: DateTime<Utc>,
        /// When the row was created.
        pub created_at: DateTime<Utc>,
        /// When the row last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// `lighthouse_settings`.
pub mod setting {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// One setting changed from the dashboard.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, serde::Serialize)]
    #[model(table = "lighthouse_settings")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The setting (`thresholds.slow_query_ms`), unique.
        pub setting_key: String,
        /// Its value, as JSON.
        pub value: String,
        /// The user who changed it.
        pub updated_by: Option<String>,
        /// When it changed.
        pub updated_at: DateTime<Utc>,
    }
}
