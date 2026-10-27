//! Lighthouse's tables as SeaORM entities (migration set `lighthouse`).
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
//!   delivered.

/// `lighthouse_issues`.
pub mod issue {
    use sea_orm::entity::prelude::*;

    /// An issue row.
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "lighthouse_issues")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The fingerprint (32 hex characters), unique.
        #[sea_orm(unique)]
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
        #[sea_orm(column_type = "Text")]
        pub message: String,
        /// The source chain (JSON array of strings).
        #[sea_orm(column_type = "Text")]
        pub chain: String,
        /// Where it happened (`file:line:column`), when known.
        pub location: Option<String>,
        /// The error's type name, when known.
        pub error_type: Option<String>,
        /// Occurrences, exact.
        pub count: i64,
        /// The first occurrence.
        pub first_seen: DateTimeUtc,
        /// The latest occurrence.
        pub last_seen: DateTimeUtc,
        /// The release of the first occurrence.
        pub first_release: Option<String>,
        /// The release of the latest occurrence.
        pub last_release: Option<String>,
        /// The environment of the latest occurrence.
        pub environment: Option<String>,
        /// When it was resolved.
        pub resolved_at: Option<DateTimeUtc>,
        /// The release current when it was resolved in the next release.
        pub resolved_in_release: Option<String>,
        /// Ignored until then.
        pub ignored_until: Option<DateTimeUtc>,
        /// Ignored for this many more occurrences.
        pub ignore_remaining: Option<i64>,
        /// When it last came back after being resolved.
        pub regressed_at: Option<DateTimeUtc>,
        /// The comment of the last status change.
        #[sea_orm(column_type = "Text", nullable)]
        pub comment: Option<String>,
        /// When the row was created.
        pub created_at: DateTimeUtc,
        /// When the row last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `lighthouse_occurrences`.
pub mod occurrence {
    use sea_orm::entity::prelude::*;

    /// A stored occurrence.
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "lighthouse_occurrences")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The issue (`lighthouse_issues.id`, cascading delete).
        pub issue_id: Uuid,
        /// When it happened.
        pub at: DateTimeUtc,
        /// Its level.
        pub level: String,
        /// The message.
        #[sea_orm(column_type = "Text")]
        pub message: String,
        /// The source chain (JSON array).
        #[sea_orm(column_type = "Text")]
        pub chain: String,
        /// A backtrace, when one was captured.
        #[sea_orm(column_type = "Text", nullable)]
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
        #[sea_orm(column_type = "Text")]
        pub context: String,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `lighthouse_samples`.
pub mod sample {
    use sea_orm::entity::prelude::*;

    /// A request or job sample with its spans.
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "lighthouse_samples")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// `request` or `job`.
        pub kind: String,
        /// The endpoint (`GET /api/v1/deals/board`) or the job type.
        pub name: String,
        /// When it finished.
        pub at: DateTimeUtc,
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
        #[sea_orm(column_type = "Text")]
        pub spans: String,
        /// Headers, client address, tags, job details (JSON object, masked).
        #[sea_orm(column_type = "Text")]
        pub context: String,
        /// The error of a failed job attempt.
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `lighthouse_aggregates`.
pub mod aggregate {
    use sea_orm::entity::prelude::*;

    /// Counts and a latency histogram of one key in one bucket, written by
    /// one process.
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "lighthouse_aggregates")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The bucket size in seconds: 60 or 3600.
        pub resolution: i32,
        /// The bucket's start.
        pub bucket: DateTimeUtc,
        /// `request`, `query`, `job`, `outgoing`, `exception` or `issue`.
        pub kind: String,
        /// A hash of the key (what rows are matched by).
        pub key_hash: String,
        /// The endpoint, statement, job type, host or fingerprint.
        #[sea_orm(column_type = "Text")]
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
        #[sea_orm(column_type = "Text")]
        pub histogram: String,
        /// Kind-specific details (JSON): the routes and jobs that ran a
        /// query, the slowest outgoing calls.
        #[sea_orm(column_type = "Text", nullable)]
        pub detail: Option<String>,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `lighthouse_alerts`.
pub mod alert {
    use sea_orm::entity::prelude::*;

    /// One alert through one channel.
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "lighthouse_alerts")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
        /// The alert (JSON).
        #[sea_orm(column_type = "Text")]
        pub payload: String,
        /// When it was raised.
        pub created_at: DateTimeUtc,
        /// When it was delivered.
        pub delivered_at: Option<DateTimeUtc>,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
