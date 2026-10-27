//! [`WorkflowsError`] and [`DefinitionIssue`].

use std::collections::BTreeMap;

use navree_http::AppError;
use sea_orm::DbErr;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// What can go wrong starting, running or managing workflows and
/// automations.
///
/// Returned from a handler (`?`): [`UnknownRun`](WorkflowsError::UnknownRun),
/// [`UnknownAutomation`](WorkflowsError::UnknownAutomation) and
/// [`UnknownTemplate`](WorkflowsError::UnknownTemplate) are 404s,
/// [`Conflict`](WorkflowsError::Conflict) is a 409,
/// [`InvalidDefinition`](WorkflowsError::InvalidDefinition) is a 400
/// `validation_failed` whose `errors` map each issue's JSON pointer to its
/// messages, [`Invalid`](WorkflowsError::Invalid) is a 400; the rest are
/// 500s (logged, never shown).
#[derive(Debug, thiserror::Error)]
pub enum WorkflowsError {
    /// The workflow type was started, but no module registered it with
    /// `WorkflowsModule::for_feature::<W>()`.
    #[error(
        "the workflow `{name}` ({type_name}) is not registered: import \
         WorkflowsModule::for_feature::<{short}>() in the module that owns it"
    )]
    Unregistered {
        /// The workflow's name.
        name: String,
        /// The workflow's Rust type.
        type_name: String,
        /// The type's last path segment, for the hint.
        short: String,
    },
    /// `ctx.wait_for::<E>()` for an event type no module registered with
    /// `m.workflow_event::<E>()`: such an event would never reach the run.
    #[error(
        "workflows cannot wait for `{name}` ({type_name}): register it with \
         m.workflow_event::<{short}>() (ModuleBuilderWorkflowsExt)"
    )]
    UnregisteredEvent {
        /// The event's name.
        name: String,
        /// The event's Rust type.
        type_name: String,
        /// The type's last path segment, for the hint.
        short: String,
    },
    /// A step failed on its last attempt. The workflow may catch it (to
    /// compensate) or return it, which fails the run.
    #[error("step `{step}` failed after {attempts} attempt(s): {message}")]
    StepFailed {
        /// The step's key (`send-welcome`, `send-welcome#2`).
        step: String,
        /// How many times it ran.
        attempts: u32,
        /// The last attempt's error.
        message: String,
    },
    /// A step name that is empty, starts with `$` or holds `#`.
    #[error(
        "invalid step name `{0}`: use 1 to 100 characters without `#`, not starting with `$`"
    )]
    InvalidStepName(String),
    /// No workflow run has this id.
    #[error("unknown workflow run `{0}`")]
    UnknownRun(Uuid),
    /// No automation has this id (in the request's workspace).
    #[error("unknown automation `{0}`")]
    UnknownAutomation(Uuid),
    /// No registered automation template has this key.
    #[error("unknown automation template `{0}`")]
    UnknownTemplate(String),
    /// The run or automation is not in a state that allows the change
    /// (cancelling a finished run, retrying one that did not fail).
    #[error("{0}")]
    Conflict(String),
    /// A request or command argument is invalid.
    #[error("{0}")]
    Invalid(String),
    /// An automation definition that does not match the registry.
    #[error("the automation definition is invalid: {}", summary(.0))]
    InvalidDefinition(Vec<DefinitionIssue>),
    /// An input, output, step result or event payload could not be
    /// serialized or read back.
    #[error("{0}")]
    Payload(String),
    /// The database failed.
    #[error("workflow storage failed: {0}")]
    Database(#[from] DbErr),
    /// Something else failed (the app shutting down, a missing module).
    #[error("{0}")]
    Other(String),
}

/// One problem found validating an automation definition: where it is
/// (a JSON pointer such as `/nodes/2/config`) and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DefinitionIssue {
    /// A JSON pointer into the definition (`""` for the whole document).
    pub path: String,
    /// What is wrong, for people.
    pub message: String,
}

impl DefinitionIssue {
    /// An issue at `path`.
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// The first issue, plus how many more.
fn summary(issues: &[DefinitionIssue]) -> String {
    match issues {
        [] => "no details".to_owned(),
        [one] => format!("{}: {}", path_or_root(&one.path), one.message),
        [first, rest @ ..] => format!(
            "{}: {} (and {} more)",
            path_or_root(&first.path),
            first.message,
            rest.len()
        ),
    }
}

fn path_or_root(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

impl WorkflowsError {
    /// `Unregistered` for a workflow type.
    pub(crate) fn unregistered(name: &str, type_name: &str) -> WorkflowsError {
        WorkflowsError::Unregistered {
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            short: short(type_name),
        }
    }

    /// `UnregisteredEvent` for an event type.
    pub(crate) fn unregistered_event(name: &str, type_name: &str) -> WorkflowsError {
        WorkflowsError::UnregisteredEvent {
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            short: short(type_name),
        }
    }

    /// A serialization error.
    pub(crate) fn payload(what: &str, error: impl std::fmt::Display) -> WorkflowsError {
        WorkflowsError::Payload(format!("{what}: {error}"))
    }

    /// The error response of this error, `None` for a 500.
    pub fn to_app_error(&self) -> Option<AppError> {
        match self {
            WorkflowsError::UnknownRun(_)
            | WorkflowsError::UnknownAutomation(_)
            | WorkflowsError::UnknownTemplate(_) => {
                Some(AppError::not_found().with_detail(self.to_string()))
            }
            WorkflowsError::Conflict(message) => Some(AppError::conflict(message.clone())),
            WorkflowsError::Invalid(message) => Some(AppError::bad_request(message.clone())),
            WorkflowsError::InvalidDefinition(issues) => {
                let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for issue in issues {
                    errors
                        .entry(path_or_root(&issue.path).to_owned())
                        .or_default()
                        .push(issue.message.clone());
                }
                Some(AppError::validation(errors))
            }
            _ => None,
        }
    }
}

/// The type name's last path segment, generics removed.
fn short(type_name: &str) -> String {
    let base = type_name.split('<').next().unwrap_or(type_name);
    base.rsplit("::").next().unwrap_or(base).to_owned()
}

/// The `map_error` hook of the module.
pub(crate) fn map_workflows_error(error: &WorkflowsError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_errors_have_statuses() {
        let unknown = WorkflowsError::UnknownRun(Uuid::nil())
            .to_app_error()
            .unwrap();
        assert_eq!(unknown.status.as_u16(), 404);
        let conflict = WorkflowsError::Conflict("finished".into())
            .to_app_error()
            .unwrap();
        assert_eq!(conflict.status.as_u16(), 409);
        assert_eq!(conflict.body()["message"], "finished");
        let invalid = WorkflowsError::InvalidDefinition(vec![DefinitionIssue::new(
            "/nodes/0/type",
            "unknown action `nope`",
        )])
        .to_app_error()
        .unwrap();
        assert_eq!(invalid.status.as_u16(), 400);
        assert_eq!(
            invalid.body()["errors"]["/nodes/0/type"][0],
            "unknown action `nope`"
        );
        assert!(WorkflowsError::Other("x".into()).to_app_error().is_none());
    }

    #[test]
    fn messages_name_the_fix() {
        let error = WorkflowsError::unregistered("onboarding", "app::flows::Onboarding");
        assert_eq!(
            error.to_string(),
            "the workflow `onboarding` (app::flows::Onboarding) is not registered: import \
             WorkflowsModule::for_feature::<Onboarding>() in the module that owns it"
        );
        let event = WorkflowsError::unregistered_event("course.completed", "app::CourseCompleted");
        assert!(
            event
                .to_string()
                .contains("m.workflow_event::<CourseCompleted>()")
        );
        let many = WorkflowsError::InvalidDefinition(vec![
            DefinitionIssue::new("", "no nodes"),
            DefinitionIssue::new("/trigger", "unknown"),
        ]);
        assert_eq!(
            many.to_string(),
            "the automation definition is invalid: /: no nodes (and 1 more)"
        );
    }
}
