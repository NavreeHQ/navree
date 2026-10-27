//! Runs and steps as stored: [`WorkflowRun`], [`WorkflowStep`] and their
//! statuses.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

/// Where a run is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Ready to run: started, woken by its timer or by an event, or put
    /// back by a retry.
    Pending,
    /// An execution holds it (until its lease runs out).
    Running,
    /// Waiting for a timer: `ctx.sleep(..)` or a step's next attempt.
    Sleeping,
    /// Waiting for an event (`ctx.wait_for::<E>(..)`), until its timeout.
    Waiting,
    /// The workflow returned `Ok`; its output is stored.
    Completed,
    /// The workflow returned an error or panicked; the error is stored.
    Failed,
    /// Cancelled before it finished.
    Cancelled,
}

impl RunStatus {
    /// Every status, in lifecycle order.
    pub const ALL: [RunStatus; 7] = [
        RunStatus::Pending,
        RunStatus::Running,
        RunStatus::Sleeping,
        RunStatus::Waiting,
        RunStatus::Completed,
        RunStatus::Failed,
        RunStatus::Cancelled,
    ];

    /// The stored name (`pending`, `running`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Pending => "pending",
            RunStatus::Running => "running",
            RunStatus::Sleeping => "sleeping",
            RunStatus::Waiting => "waiting",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }

    /// Whether the run is over (completed, failed or cancelled).
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
        )
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        RunStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
            .ok_or_else(|| {
                format!(
                    "unknown run status `{value}`: expected pending, running, sleeping, waiting, \
                     completed, failed or cancelled"
                )
            })
    }
}

/// One run of a workflow: what `workflow_runs` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WorkflowRun {
    /// The run's id (a UUID v7).
    pub id: Uuid,
    /// The workflow's name (`onboarding`).
    pub workflow: String,
    /// The workflow version the run started on; it resumes on it.
    pub version: u32,
    /// Where the run is.
    pub status: RunStatus,
    /// The workflow's input, as JSON.
    #[schema(value_type = Object)]
    pub input: Value,
    /// The workflow's output once it completed.
    #[schema(value_type = Option<Object>)]
    pub output: Option<Value>,
    /// The error once it failed.
    pub error: Option<String>,
    /// The workspace the run belongs to, if any.
    pub workspace_id: Option<Uuid>,
    /// The deduplication key it was started with, if any.
    pub key: Option<String>,
    /// When a sleeping run wakes, or a waiting run times out.
    pub wake_at: Option<DateTime<Utc>>,
    /// When the run started.
    pub created_at: DateTime<Utc>,
    /// When the run last changed.
    pub updated_at: DateTime<Utc>,
    /// When the run finished.
    pub finished_at: Option<DateTime<Utc>>,
}

/// What a step record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// `ctx.step(name, ..)`: a closure's result.
    Step,
    /// `ctx.sleep(..)`: a timer.
    Sleep,
    /// `ctx.wait_for::<E>(..)`: an event wait.
    Wait,
    /// `ctx.now()`: a value recorded once, read back on replay.
    Value,
}

impl StepKind {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            StepKind::Step => "step",
            StepKind::Sleep => "sleep",
            StepKind::Wait => "wait",
            StepKind::Value => "value",
        }
    }
}

impl FromStr for StepKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            StepKind::Step,
            StepKind::Sleep,
            StepKind::Wait,
            StepKind::Value,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
        .ok_or_else(|| format!("unknown step kind `{value}`"))
    }
}

/// Where a step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Done; its output is replayed from now on.
    Completed,
    /// Failed on its last attempt; replays return the same error.
    Failed,
    /// Failed an attempt; runs again at `wake_at`.
    Retrying,
    /// A timer or an event wait that has not ended yet.
    Waiting,
}

impl StepStatus {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            StepStatus::Completed => "completed",
            StepStatus::Failed => "failed",
            StepStatus::Retrying => "retrying",
            StepStatus::Waiting => "waiting",
        }
    }
}

impl FromStr for StepStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            StepStatus::Completed,
            StepStatus::Failed,
            StepStatus::Retrying,
            StepStatus::Waiting,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
        .ok_or_else(|| format!("unknown step status `{value}`"))
    }
}

/// One step of a run: what `workflow_steps` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WorkflowStep {
    /// The run.
    pub run_id: Uuid,
    /// The step's key within the run: its name, `#2`, `#3`, ... for later
    /// calls with the same name; `$sleep`, `$wait:<event>` and `$now` for
    /// the built-in steps.
    pub key: String,
    /// The order the step was first reached in (from 1).
    pub position: u32,
    /// What the step is.
    pub kind: StepKind,
    /// Where it is.
    pub status: StepStatus,
    /// Its result (a step's output, a received event, a recorded value).
    #[schema(value_type = Option<Object>)]
    pub output: Option<Value>,
    /// The last attempt's error.
    pub error: Option<String>,
    /// How many times the step ran.
    pub attempts: u32,
    /// When a timer ends, a wait times out or a retry runs.
    pub wake_at: Option<DateTime<Utc>>,
    /// The event a wait is for.
    pub event: Option<String>,
    /// The filter of a wait (an [`EventMatch`](crate::EventMatch) as JSON).
    #[schema(value_type = Option<Object>)]
    pub filter: Option<Value>,
    /// When the step was first reached.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A run with its steps (what `GET /api/workflows/runs/{id}` returns).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RunDetails {
    /// The run.
    #[serde(flatten)]
    pub run: WorkflowRun,
    /// Its steps, in the order they were reached.
    pub steps: Vec<WorkflowStep>,
}

/// A filter over runs (`Workflows::runs`, `workflows:list`, the routes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunQuery {
    /// Only this workflow.
    pub workflow: Option<String>,
    /// Only these statuses (any when empty).
    pub statuses: Vec<RunStatus>,
    /// Only this workspace.
    pub workspace_id: Option<Uuid>,
    /// Only these runs (any when empty).
    pub ids: Vec<Uuid>,
    /// At most this many (newest first; 50 when 0).
    pub limit: u64,
    /// Skip this many.
    pub offset: u64,
}

impl RunQuery {
    /// Every run, newest first, 50 at a time.
    pub fn new() -> Self {
        Self::default()
    }

    /// Only runs of `workflow`.
    pub fn workflow(mut self, workflow: impl Into<String>) -> Self {
        self.workflow = Some(workflow.into());
        self
    }

    /// Only runs with `status` (call again to add more).
    pub fn status(mut self, status: RunStatus) -> Self {
        self.statuses.push(status);
        self
    }

    /// Only runs of `workspace`.
    pub fn workspace(mut self, workspace: Uuid) -> Self {
        self.workspace_id = Some(workspace);
        self
    }

    /// At most `limit` runs.
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    /// The effective limit.
    pub(crate) fn effective_limit(&self) -> u64 {
        if self.limit == 0 { 50 } else { self.limit }
    }

    /// Whether `run` passes the filter (the memory store's query).
    pub(crate) fn matches(&self, run: &WorkflowRun) -> bool {
        self.workflow.as_ref().is_none_or(|w| *w == run.workflow)
            && (self.statuses.is_empty() || self.statuses.contains(&run.status))
            && self
                .workspace_id
                .is_none_or(|w| run.workspace_id == Some(w))
            && (self.ids.is_empty() || self.ids.contains(&run.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for status in RunStatus::ALL {
            assert_eq!(status.as_str().parse::<RunStatus>(), Ok(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::json!(status.as_str())
            );
        }
        assert!("done".parse::<RunStatus>().unwrap_err().contains("`done`"));
        assert!(RunStatus::Cancelled.is_finished());
        assert!(!RunStatus::Waiting.is_finished());
        assert_eq!("wait".parse::<StepKind>(), Ok(StepKind::Wait));
        assert_eq!("retrying".parse::<StepStatus>(), Ok(StepStatus::Retrying));
    }
}
