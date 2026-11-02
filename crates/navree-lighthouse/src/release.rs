//! Releases: what an app (or its deploy) reports about the release it runs,
//! and how Lighthouse lists the releases it has seen.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

/// The longest link a report can carry.
pub(crate) const MAX_URL_CHARS: usize = 255;
/// How far ahead of this machine's clock a deploy time may be: another
/// machine's clock, a little ahead. It counts as deployed now.
const DEPLOY_CLOCK_SKEW: TimeDelta = TimeDelta::days(1);

/// The earliest deploy time, a day into 1970 so that it stays in range in
/// every time zone (MySQL's `TIMESTAMP` starts with 1970).
fn earliest_deploy() -> DateTime<Utc> {
    DateTime::UNIX_EPOCH + TimeDelta::days(1)
}

/// A release an app reports: [`Lighthouse::report_release`](crate::Lighthouse::report_release)
/// from code, or `POST {path}/api/releases` from a deploy script.
///
/// ```
/// use navree_lighthouse::ReleaseReport;
///
/// let report = ReleaseReport::new("20261102-101500-3f2a9c1")
///     .commit("3f2a9c1")
///     .notes("Pipeline stages validated before saving");
/// assert_eq!(report.release, "20261102-101500-3f2a9c1");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReleaseReport {
    /// The release label: what `lighthouse.release` says on the running
    /// processes, so that issues and releases match.
    pub release: String,
    /// The environment; the app's own when not given.
    pub environment: Option<String>,
    /// The commit it was built from.
    pub commit: Option<String>,
    /// A link to the release's changes or its deploy (`http` or `https`, at
    /// most 255 characters).
    pub url: Option<String>,
    /// A few words about it.
    pub notes: Option<String>,
    /// When it was deployed; now when not given. Between 1970 and a day from
    /// now; a time ahead of now counts as now.
    pub deployed_at: Option<DateTime<Utc>>,
}

impl ReleaseReport {
    /// A report of release `release`, deployed now.
    pub fn new(release: impl Into<String>) -> ReleaseReport {
        ReleaseReport {
            release: release.into(),
            ..ReleaseReport::default()
        }
    }

    /// The environment it was deployed to.
    pub fn environment(mut self, environment: impl Into<String>) -> ReleaseReport {
        self.environment = Some(environment.into());
        self
    }

    /// The commit it was built from.
    pub fn commit(mut self, commit: impl Into<String>) -> ReleaseReport {
        self.commit = Some(commit.into());
        self
    }

    /// A link to its changes or its deploy.
    pub fn url(mut self, url: impl Into<String>) -> ReleaseReport {
        self.url = Some(url.into());
        self
    }

    /// A few words about it.
    pub fn notes(mut self, notes: impl Into<String>) -> ReleaseReport {
        self.notes = Some(notes.into());
        self
    }

    /// When it was deployed.
    pub fn deployed_at(mut self, at: DateTime<Utc>) -> ReleaseReport {
        self.deployed_at = Some(at);
        self
    }

    /// The report with its text trimmed and checked; the problem as a
    /// sentence otherwise.
    pub fn checked(mut self) -> Result<ReleaseReport, String> {
        fn clean(value: Option<String>) -> Option<String> {
            value
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        }
        self.release = self.release.trim().to_owned();
        self.environment = clean(self.environment);
        self.commit = clean(self.commit);
        self.url = clean(self.url);
        self.notes = clean(self.notes);
        if self.release.is_empty() {
            return Err("release is required".to_owned());
        }
        if self.release.chars().count() > 200 || self.release.chars().any(char::is_control) {
            return Err("release must be a label of at most 200 characters".to_owned());
        }
        if self
            .environment
            .as_ref()
            .is_some_and(|e| e.chars().count() > 100 || e.chars().any(char::is_control))
        {
            return Err("environment must be at most 100 characters".to_owned());
        }
        if self.commit.as_ref().is_some_and(|c| {
            c.chars().count() > 80 || c.chars().any(|ch| ch.is_whitespace() || ch.is_control())
        }) {
            return Err("commit must be at most 80 characters, without spaces".to_owned());
        }
        if let Some(url) = &self.url
            && (url.chars().count() > MAX_URL_CHARS
                || !(url.starts_with("https://") || url.starts_with("http://"))
                || url.chars().any(|c| c.is_control() || c.is_whitespace()))
        {
            return Err(format!(
                "url must be an http(s) link of at most {MAX_URL_CHARS} characters"
            ));
        }
        if self.notes.as_ref().is_some_and(|n| {
            n.chars().count() > 2000
                || n.chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        }) {
            return Err(
                "notes must be at most 2000 characters, without control characters".to_owned(),
            );
        }
        if let Some(at) = self.deployed_at
            && (at < earliest_deploy() || at > Utc::now() + DEPLOY_CLOCK_SKEW)
        {
            return Err("deployed_at must be between 1970 and a day from now".to_owned());
        }
        Ok(self)
    }
}

/// A release as Lighthouse lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReleaseSummary {
    /// The release label.
    pub name: String,
    /// The environment.
    pub environment: String,
    /// The commit, when reported.
    pub commit: Option<String>,
    /// A link, when reported.
    pub url: Option<String>,
    /// Notes, when reported.
    pub notes: Option<String>,
    /// `process` (a process booted with this label) or `api` (reported).
    pub source: String,
    /// When it was deployed: as reported, else when first seen.
    pub deployed_at: DateTime<Utc>,
    /// When a process first reported it.
    pub first_seen: DateTime<Utc>,
    /// When a process last reported it.
    pub last_seen: DateTime<Utc>,
    /// Issues whose first occurrence was in this release.
    pub introduced: i64,
    /// Issues that came back (after being resolved) in this release.
    pub regressed: i64,
    /// Whether it is the release its environment is at: the newest deploy. A
    /// process that boots on an older label deploys it again, so a rollback
    /// moves this.
    pub current: bool,
}

/// A deploy, for the mark on a chart.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReleaseMarker {
    /// The release label.
    pub name: String,
    /// When it was deployed.
    pub at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_trimmed_and_checked() {
        let ok = ReleaseReport::new("  r2 ")
            .commit(" abc123 ")
            .url("https://git.example/r2")
            .checked()
            .unwrap();
        assert_eq!(ok.release, "r2");
        assert_eq!(ok.commit.as_deref(), Some("abc123"));
        assert!(ReleaseReport::new("   ").checked().is_err());
        assert!(ReleaseReport::new("r\n2").checked().is_err());
        assert!(
            ReleaseReport::new("r2")
                .url("javascript:alert(1)")
                .checked()
                .is_err(),
            "links are http(s) only"
        );
        assert!(ReleaseReport::new("r2").commit("a b").checked().is_err());
        assert!(
            ReleaseReport::new("r2")
                .notes("x".repeat(2001))
                .checked()
                .is_err()
        );
        let blank = ReleaseReport::new("r2").notes("  ").checked().unwrap();
        assert_eq!(blank.notes, None);
    }
}
