//! Automations as stored: [`Automation`], [`AutomationVersion`],
//! [`Enrollment`], their statuses and queries, and [`AutomationTemplate`].

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::graph::AutomationGraph;
use crate::RunStatus;

/// Where an automation is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStatus {
    /// Never published: it enrolls nobody.
    #[default]
    Draft,
    /// Its published version enrolls on every matching event.
    Active,
    /// Published, but enrolls nobody new; enrollments under way go on.
    Paused,
}

impl AutomationStatus {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            AutomationStatus::Draft => "draft",
            AutomationStatus::Active => "active",
            AutomationStatus::Paused => "paused",
        }
    }
}

impl fmt::Display for AutomationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AutomationStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            AutomationStatus::Draft,
            AutomationStatus::Active,
            AutomationStatus::Paused,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
        .ok_or_else(|| format!("unknown automation status `{value}`: expected draft, active or paused"))
    }
}

/// Who may be enrolled again, by the trigger's subject (events without a
/// subject always enroll).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentPolicy {
    /// Once the subject's enrollment finished (the default): one at a
    /// time per subject.
    #[default]
    OneAtATime,
    /// Never again: once per subject, ever.
    Once,
    /// On every matching event, even while one is under way.
    EveryTime,
}

impl EnrollmentPolicy {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            EnrollmentPolicy::OneAtATime => "one_at_a_time",
            EnrollmentPolicy::Once => "once",
            EnrollmentPolicy::EveryTime => "every_time",
        }
    }
}

impl FromStr for EnrollmentPolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            EnrollmentPolicy::OneAtATime,
            EnrollmentPolicy::Once,
            EnrollmentPolicy::EveryTime,
        ]
        .into_iter()
        .find(|policy| policy.as_str() == value)
        .ok_or_else(|| {
            format!("unknown enrollment policy `{value}`: expected one_at_a_time, once or every_time")
        })
    }
}

/// An automation: a draft definition people edit, and the published
/// version that enrolls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Automation {
    /// Its id (a UUID v7).
    pub id: Uuid,
    /// The workspace it belongs to (`None`: app-wide).
    pub workspace_id: Option<Uuid>,
    /// Its name.
    pub name: String,
    /// A note for people.
    pub description: Option<String>,
    /// Draft, active or paused.
    pub status: AutomationStatus,
    /// The latest saved definition (what `publish` turns into a version).
    pub definition: AutomationGraph,
    /// Who may be enrolled again.
    pub enrollment: EnrollmentPolicy,
    /// The published version that enrolls (`None` until published).
    pub version: Option<u32>,
    /// The event of the published version's trigger.
    pub trigger_event: Option<String>,
    /// The template it was made from.
    pub template: Option<String>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A published, immutable definition. Enrollments run the version they
/// started on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AutomationVersion {
    /// The automation.
    pub automation_id: Uuid,
    /// 1, 2, 3, ... per automation.
    pub version: u32,
    /// The definition.
    pub definition: AutomationGraph,
    /// Who published it.
    pub published_by: Option<Uuid>,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// One enrollment as stored (`automation_enrollments`): which event put
/// which subject into which version, and the run that carries it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnrollmentRecord {
    /// Its id.
    pub id: Uuid,
    /// The automation.
    pub automation_id: Uuid,
    /// The version it runs.
    pub version: u32,
    /// The automation's workspace.
    pub workspace_id: Option<Uuid>,
    /// Who or what is enrolled (the trigger's subject).
    pub subject: Option<String>,
    /// The workflow run carrying it.
    pub run_id: Uuid,
    /// When it was enrolled.
    pub created_at: DateTime<Utc>,
}

/// An enrollment with the status of its run (what the routes return).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Enrollment {
    /// Its id.
    pub id: Uuid,
    /// The automation.
    pub automation_id: Uuid,
    /// The version it runs.
    pub version: u32,
    /// The automation's workspace.
    pub workspace_id: Option<Uuid>,
    /// Who or what is enrolled.
    pub subject: Option<String>,
    /// The workflow run carrying it (`GET /api/workflows/runs/{id}` shows
    /// its steps).
    pub run_id: Uuid,
    /// The run's status: running, sleeping, waiting, completed, failed,
    /// cancelled.
    pub status: RunStatus,
    /// The run's error, when it failed.
    pub error: Option<String>,
    /// When it was enrolled.
    pub created_at: DateTime<Utc>,
    /// When its run finished.
    pub finished_at: Option<DateTime<Utc>>,
}

/// A filter over automations (the stores' `list` and `count`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationQuery {
    /// `None`: every workspace; `Some(None)`: app-wide automations only;
    /// `Some(Some(id))`: that workspace's.
    pub scope: Option<Option<Uuid>>,
    /// Only this status.
    pub status: Option<AutomationStatus>,
    /// At most this many, newest first (50 when 0).
    pub limit: u64,
    /// Skip this many.
    pub offset: u64,
}

impl AutomationQuery {
    /// The effective limit.
    pub(crate) fn effective_limit(&self) -> u64 {
        if self.limit == 0 { 50 } else { self.limit }
    }

    /// Whether `automation` passes (the memory store's query).
    pub(crate) fn matches(&self, automation: &Automation) -> bool {
        self.scope.is_none_or(|scope| automation.workspace_id == scope)
            && self.status.is_none_or(|status| automation.status == status)
    }
}

/// A filter over enrollments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnrollmentQuery {
    /// Only this automation's.
    pub automation_id: Option<Uuid>,
    /// Only this subject's.
    pub subject: Option<String>,
    /// At most this many, newest first (50 when 0).
    pub limit: u64,
    /// Skip this many.
    pub offset: u64,
}

impl EnrollmentQuery {
    /// The effective limit.
    pub(crate) fn effective_limit(&self) -> u64 {
        if self.limit == 0 { 50 } else { self.limit }
    }

    pub(crate) fn matches(&self, enrollment: &EnrollmentRecord) -> bool {
        self.automation_id
            .is_none_or(|id| enrollment.automation_id == id)
            && self
                .subject
                .as_ref()
                .is_none_or(|subject| enrollment.subject.as_ref() == Some(subject))
    }
}

/// A ready-made automation people start from
/// (`POST /api/automations/templates/{key}`), registered with
/// `m.automation_template(..)`. Its definition is validated at boot.
///
/// ```
/// use navree_workflows::AutomationTemplate;
/// use serde_json::json;
///
/// let template = AutomationTemplate::new(
///     "welcome-series",
///     "Welcome series",
///     json!({
///         "trigger": {"type": "user.registered"},
///         "nodes": [
///             {"id": "welcome", "kind": "action", "type": "mail.send",
///              "config": {"template": "welcome"}}
///         ]
///     }),
/// )
/// .description("Greets new users, then checks in after a week.")
/// .category("Onboarding");
/// assert_eq!(template.key, "welcome-series");
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AutomationTemplate {
    /// Its stable key (`welcome-series`).
    pub key: String,
    /// The name an automation made from it gets.
    pub name: String,
    /// A note for people.
    pub description: String,
    /// A group for the template picker.
    pub category: Option<String>,
    /// The definition (an [`AutomationGraph`] as JSON).
    #[schema(value_type = Object)]
    pub definition: Value,
    /// The enrollment policy of automations made from it.
    pub enrollment: EnrollmentPolicy,
}

impl AutomationTemplate {
    /// A template `key` named `name` with `definition`.
    pub fn new(key: impl Into<String>, name: impl Into<String>, definition: Value) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            description: String::new(),
            category: None,
            definition,
            enrollment: EnrollmentPolicy::default(),
        }
    }

    /// With a description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// In `category`.
    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// With enrollment policy `policy`.
    pub fn enrollment(mut self, policy: EnrollmentPolicy) -> Self {
        self.enrollment = policy;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_and_policies_round_trip() {
        for status in [
            AutomationStatus::Draft,
            AutomationStatus::Active,
            AutomationStatus::Paused,
        ] {
            assert_eq!(status.as_str().parse::<AutomationStatus>(), Ok(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::json!(status.as_str())
            );
        }
        for policy in [
            EnrollmentPolicy::OneAtATime,
            EnrollmentPolicy::Once,
            EnrollmentPolicy::EveryTime,
        ] {
            assert_eq!(policy.as_str().parse::<EnrollmentPolicy>(), Ok(policy));
            assert_eq!(
                serde_json::to_value(policy).unwrap(),
                serde_json::json!(policy.as_str())
            );
        }
        assert!("live".parse::<AutomationStatus>().is_err());
        assert_eq!(EnrollmentPolicy::default(), EnrollmentPolicy::OneAtATime);
    }
}
