//! The public data types: [`Level`], [`IssueKind`], [`IssueStatus`],
//! [`Issue`] and fingerprints.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::normalize::normalize_message;

/// How serious a record is. The dashboard filters by level, and alerts
/// ignore [`Level::Info`].
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[cfg_attr(feature = "mcp", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Something to know about; never alerts.
    Info,
    /// Something that may need a look.
    Warning,
    /// Something broke (the default).
    #[default]
    Error,
}

impl Level {
    /// `error`, `warning` or `info`.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        }
    }

    /// Parses `error`, `warning` or `info`.
    pub fn parse(text: &str) -> Option<Level> {
        match text {
            "error" => Some(Level::Error),
            "warning" => Some(Level::Warning),
            "info" => Some(Level::Info),
            _ => None,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What an issue groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// Errors: 5xx responses, panics and reports.
    Exception,
    /// Requests slower than `lighthouse.thresholds.slow_request_ms`, per route.
    SlowRequest,
    /// Queries slower than `lighthouse.thresholds.slow_query_ms`, per
    /// normalized statement.
    SlowQuery,
    /// Jobs that used up their attempts, per job type and error.
    FailedJob,
    /// Scheduled tasks that failed, per task.
    ScheduledTask,
    /// Outgoing calls that failed or were slow, per host and failure.
    OutgoingRequest,
}

impl IssueKind {
    /// Every kind.
    pub const ALL: [IssueKind; 6] = [
        IssueKind::Exception,
        IssueKind::SlowRequest,
        IssueKind::SlowQuery,
        IssueKind::FailedJob,
        IssueKind::ScheduledTask,
        IssueKind::OutgoingRequest,
    ];

    /// The snake_case name (`slow_request`).
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::Exception => "exception",
            IssueKind::SlowRequest => "slow_request",
            IssueKind::SlowQuery => "slow_query",
            IssueKind::FailedJob => "failed_job",
            IssueKind::ScheduledTask => "scheduled_task",
            IssueKind::OutgoingRequest => "outgoing_request",
        }
    }

    /// Parses the snake_case name.
    pub fn parse(text: &str) -> Option<IssueKind> {
        IssueKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }

    /// A label for people (`Slow request`).
    pub fn label(self) -> &'static str {
        match self {
            IssueKind::Exception => "Exception",
            IssueKind::SlowRequest => "Slow request",
            IssueKind::SlowQuery => "Slow query",
            IssueKind::FailedJob => "Failed job",
            IssueKind::ScheduledTask => "Scheduled task",
            IssueKind::OutgoingRequest => "Outgoing request",
        }
    }
}

impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an issue stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// New, or seen again after it was resolved. Open issues alert.
    Open,
    /// Fixed; a new occurrence reopens it as a regression.
    Resolved,
    /// Fixed in the next release: reopens only for an occurrence from a
    /// release other than the one current when it was resolved.
    ResolvedInNextRelease,
    /// Kept and counted, never alerts; forever, until a date, or for a
    /// number of occurrences.
    Ignored,
}

impl IssueStatus {
    /// Every status.
    pub const ALL: [IssueStatus; 4] = [
        IssueStatus::Open,
        IssueStatus::Resolved,
        IssueStatus::ResolvedInNextRelease,
        IssueStatus::Ignored,
    ];

    /// The snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            IssueStatus::Open => "open",
            IssueStatus::Resolved => "resolved",
            IssueStatus::ResolvedInNextRelease => "resolved_in_next_release",
            IssueStatus::Ignored => "ignored",
        }
    }

    /// Parses the snake_case name.
    pub fn parse(text: &str) -> Option<IssueStatus> {
        IssueStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == text)
    }

    /// A label for people (`Resolved in next release`).
    pub fn label(self) -> &'static str {
        match self {
            IssueStatus::Open => "Open",
            IssueStatus::Resolved => "Resolved",
            IssueStatus::ResolvedInNextRelease => "Resolved in next release",
            IssueStatus::Ignored => "Ignored",
        }
    }
}

impl fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An issue: every occurrence of one problem. This is the shape of the
/// `issue` object in webhook alerts and in the JSON API's lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    /// The id (UUID v7).
    pub id: Uuid,
    /// The first line of the message.
    pub title: String,
    /// What it groups.
    pub kind: IssueKind,
    /// The highest level seen.
    pub level: Level,
    /// Where it stands.
    pub status: IssueStatus,
    /// Where it happens: the route (`POST /api/v1/invoices/{id}/charge`),
    /// the job type, the task or the host.
    pub culprit: String,
    /// Occurrences, exact.
    pub count: i64,
    /// Distinct users among the stored occurrences.
    pub users: i64,
    /// When it first happened.
    pub first_seen: DateTime<Utc>,
    /// When it last happened.
    pub last_seen: DateTime<Utc>,
    /// The issue's page on the dashboard.
    pub url: String,
}

/// The fingerprint of `parts`: 32 hex characters of their SHA-256.
pub(crate) fn fingerprint<S: AsRef<str>>(parts: &[S]) -> String {
    let mut hasher = Sha256::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            hasher.update([0x1f]);
        }
        hasher.update(part.as_ref().as_bytes());
    }
    hex::encode(&hasher.finalize()[..16])
}

/// The normalized message chain: each message normalized, joined.
pub(crate) fn normalized_chain(chain: &[String]) -> String {
    chain
        .iter()
        .map(|message| normalize_message(message))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Fingerprint of an exception: error type + route or job + normalized
/// message chain.
pub(crate) fn exception_fingerprint(
    error_type: Option<&str>,
    culprit: &str,
    chain: &[String],
) -> String {
    fingerprint(&[
        "exception",
        error_type.unwrap_or(""),
        culprit,
        &normalized_chain(chain),
    ])
}

/// Fingerprint of a panic: file and line + normalized message.
pub(crate) fn panic_fingerprint(location: &str, message: &str) -> String {
    // `file:line:column` → `file:line`
    let file_line = match location.rsplit_once(':') {
        Some((head, column)) if column.chars().all(|c| c.is_ascii_digit()) => head,
        _ => location,
    };
    fingerprint(&["panic", file_line, &normalize_message(message)])
}

/// Fingerprint of a slow request: method + route template.
pub(crate) fn slow_request_fingerprint(endpoint: &str) -> String {
    fingerprint(&["slow_request", endpoint])
}

/// Fingerprint of a slow query: the normalized statement.
pub(crate) fn slow_query_fingerprint(normalized_sql: &str) -> String {
    fingerprint(&["slow_query", normalized_sql])
}

/// Fingerprint of a failed job: job type + normalized error.
pub(crate) fn failed_job_fingerprint(job_type: &str, error: &str) -> String {
    fingerprint(&["failed_job", job_type, &normalize_message(error)])
}

/// Fingerprint of a failed scheduled task: task name + normalized message.
pub(crate) fn scheduled_task_fingerprint(task: &str, error: &str) -> String {
    fingerprint(&["scheduled_task", task, &normalize_message(error)])
}

/// Fingerprint of an outgoing failure: host + status class or error kind.
pub(crate) fn outgoing_fingerprint(host: &str, failure: &str) -> String {
    fingerprint(&["outgoing_request", host, failure])
}

/// Fingerprint chosen by a report (`.fingerprint([..])`).
pub(crate) fn custom_fingerprint(parts: &[String]) -> String {
    let mut all = vec!["custom".to_owned()];
    all.extend(parts.iter().cloned());
    fingerprint(&all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(message: &str) -> Vec<String> {
        vec![message.to_owned()]
    }

    #[test]
    fn exceptions_group_by_type_culprit_and_normalized_message() {
        let a = exception_fingerprint(
            Some("app::DealError"),
            "GET /api/v1/deals/{id}",
            &chain("deal 0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23 not found"),
        );
        let b = exception_fingerprint(
            Some("app::DealError"),
            "GET /api/v1/deals/{id}",
            &chain("deal 0199c1e7-6f7e-7a31-b5d4-2f0c8e9a1b99 not found"),
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        let other_route = exception_fingerprint(
            Some("app::DealError"),
            "PATCH /api/v1/deals/{id}",
            &chain("deal 0199c1e7-6f7e-7a31-b5d4-2f0c8e9a1b99 not found"),
        );
        assert_ne!(a, other_route);
        let other_type = exception_fingerprint(
            Some("app::OtherError"),
            "GET /api/v1/deals/{id}",
            &chain("deal 0199c1e7-6f7e-7a31-b5d4-2f0c8e9a1b99 not found"),
        );
        assert_ne!(a, other_type);
    }

    #[test]
    fn panics_group_by_file_and_line() {
        assert_eq!(
            panic_fingerprint("src/deals.rs:42:9", "index 3 out of range"),
            panic_fingerprint("src/deals.rs:42:17", "index 7 out of range"),
        );
        assert_ne!(
            panic_fingerprint("src/deals.rs:42:9", "boom"),
            panic_fingerprint("src/deals.rs:43:9", "boom"),
        );
    }

    #[test]
    fn every_kind_has_its_own_space() {
        let all = [
            slow_request_fingerprint("GET /x"),
            slow_query_fingerprint("GET /x"),
            failed_job_fingerprint("GET /x", ""),
            scheduled_task_fingerprint("GET /x", ""),
            outgoing_fingerprint("GET /x", ""),
            custom_fingerprint(&["GET /x".to_owned()]),
        ];
        let unique: std::collections::BTreeSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        assert_eq!(
            failed_job_fingerprint("mail.send", "smtp 421 at 10.0.0.1"),
            failed_job_fingerprint("mail.send", "smtp 450 at 10.0.0.2"),
        );
        assert_eq!(
            custom_fingerprint(&["acme-pay".into(), "card-declined".into()]),
            custom_fingerprint(&["acme-pay".into(), "card-declined".into()]),
        );
    }

    #[test]
    fn names_round_trip() {
        for kind in IssueKind::ALL {
            assert_eq!(IssueKind::parse(kind.as_str()), Some(kind));
        }
        for status in IssueStatus::ALL {
            assert_eq!(IssueStatus::parse(status.as_str()), Some(status));
        }
        for level in [Level::Error, Level::Warning, Level::Info] {
            assert_eq!(Level::parse(level.as_str()), Some(level));
        }
        assert!(Level::Error > Level::Warning && Level::Warning > Level::Info);
    }
}
