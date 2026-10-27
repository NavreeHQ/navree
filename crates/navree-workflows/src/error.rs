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
/// [`UnknownAutomation`](WorkflowsError::UnknownAutomation),
/// [`UnknownEnrollment`](WorkflowsError::UnknownEnrollment) and
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
    #[error("invalid step name `{0}`: use 1 to 100 characters without `#`, not starting with `$`")]
    InvalidStepName(String),
    /// No workflow run has this id.
    #[error("unknown workflow run `{0}`")]
    UnknownRun(Uuid),
    /// No automation has this id (in the request's workspace).
    #[error("unknown automation `{0}`")]
    UnknownAutomation(Uuid),
    /// No enrollment has this id.
    #[error("unknown enrollment `{0}`")]
    UnknownEnrollment(Uuid),
    /// No registered automation template has this key.
    #[error("unknown automation template `{0}`")]
    UnknownTemplate(String),
    /// The run or automation is not in a state that allows the change
    /// (cancelling a finished run, retrying one that did not fail).
    #[error("{message}")]
    Conflict {
        /// The machine-readable code (`workflow_run_finished`).
        code: &'static str,
        /// What is wrong, for people.
        message: String,
    },
    /// A request or command argument is invalid.
    #[error("{message}")]
    Invalid {
        /// The machine-readable code (`invalid_automation_event`).
        code: &'static str,
        /// What is wrong, for people.
        message: String,
    },
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

    /// A 409 with `code`.
    pub(crate) fn conflict(code: &'static str, message: impl Into<String>) -> WorkflowsError {
        WorkflowsError::Conflict {
            code,
            message: message.into(),
        }
    }

    /// A 400 with `code`.
    pub(crate) fn invalid(code: &'static str, message: impl Into<String>) -> WorkflowsError {
        WorkflowsError::Invalid {
            code,
            message: message.into(),
        }
    }

    /// The machine-readable code of a client error (`None` for a 500).
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            WorkflowsError::UnknownRun(_) => codes::WORKFLOW_RUN_NOT_FOUND,
            WorkflowsError::UnknownAutomation(_) => codes::AUTOMATION_NOT_FOUND,
            WorkflowsError::UnknownEnrollment(_) => codes::ENROLLMENT_NOT_FOUND,
            WorkflowsError::UnknownTemplate(_) => codes::AUTOMATION_TEMPLATE_NOT_FOUND,
            WorkflowsError::Conflict { code, .. } | WorkflowsError::Invalid { code, .. } => code,
            WorkflowsError::InvalidDefinition(_) => "validation_failed",
            _ => return None,
        })
    }

    /// The error response of this error, `None` for a 500.
    pub fn to_app_error(&self) -> Option<AppError> {
        let code = self.code()?;
        Some(match self {
            WorkflowsError::UnknownRun(_)
            | WorkflowsError::UnknownAutomation(_)
            | WorkflowsError::UnknownEnrollment(_)
            | WorkflowsError::UnknownTemplate(_) => AppError::not_found()
                .with_detail(capitalized(&self.to_string()))
                .with_code(code),
            WorkflowsError::Conflict { message, .. } => {
                AppError::conflict(message.clone()).with_code(code)
            }
            WorkflowsError::Invalid { message, .. } => {
                AppError::bad_request(message.clone()).with_code(code)
            }
            WorkflowsError::InvalidDefinition(issues) => {
                let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for issue in issues {
                    let field = field_of(&issue.path);
                    let message = format!("{field}: {}", issue.message);
                    errors.entry(field).or_default().push(message);
                }
                AppError::validation(errors)
            }
            _ => return None,
        })
    }
}

/// The machine-readable codes of the errors the workflows module returns
/// (besides `validation_failed` for invalid definitions).
pub mod codes {
    /// 404: no workflow run has the id (or it belongs to another
    /// workspace).
    pub const WORKFLOW_RUN_NOT_FOUND: &str = "workflow_run_not_found";
    /// 404: no automation has the id in this workspace.
    pub const AUTOMATION_NOT_FOUND: &str = "automation_not_found";
    /// 404: no enrollment has the id in this automation.
    pub const ENROLLMENT_NOT_FOUND: &str = "enrollment_not_found";
    /// 404: no registered template has the key.
    pub const AUTOMATION_TEMPLATE_NOT_FOUND: &str = "automation_template_not_found";
    /// 409: cancelling a run that already finished.
    pub const WORKFLOW_RUN_FINISHED: &str = "workflow_run_finished";
    /// 409: retrying a run that did not fail.
    pub const WORKFLOW_RUN_NOT_FAILED: &str = "workflow_run_not_failed";
    /// 409: pausing an automation that is not active.
    pub const AUTOMATION_NOT_ACTIVE: &str = "automation_not_active";
    /// 409: resuming an automation that is not paused.
    pub const AUTOMATION_NOT_PAUSED: &str = "automation_not_paused";
    /// 409: enrolling in an automation that was never published.
    pub const AUTOMATION_NOT_PUBLISHED: &str = "automation_not_published";
    /// 409: cancelling an enrollment that already finished.
    pub const ENROLLMENT_FINISHED: &str = "enrollment_finished";
    /// 400: a sample or manual event that is not the trigger's event.
    pub const INVALID_AUTOMATION_EVENT: &str = "invalid_automation_event";
    /// 400: simulating a version the automation does not have.
    pub const UNKNOWN_AUTOMATION_VERSION: &str = "unknown_automation_version";

    /// Every code above.
    pub const ALL: [&str; 12] = [
        WORKFLOW_RUN_NOT_FOUND,
        AUTOMATION_NOT_FOUND,
        ENROLLMENT_NOT_FOUND,
        AUTOMATION_TEMPLATE_NOT_FOUND,
        WORKFLOW_RUN_FINISHED,
        WORKFLOW_RUN_NOT_FAILED,
        AUTOMATION_NOT_ACTIVE,
        AUTOMATION_NOT_PAUSED,
        AUTOMATION_NOT_PUBLISHED,
        ENROLLMENT_FINISHED,
        INVALID_AUTOMATION_EVENT,
        UNKNOWN_AUTOMATION_VERSION,
    ];
}

/// A JSON pointer as a dotted field path (`/definition/nodes/0/type` →
/// `definition.nodes.0.type`); the root is `definition`.
fn field_of(path: &str) -> String {
    let dotted = path.trim_start_matches('/').replace('/', ".");
    if dotted.is_empty() {
        "definition".to_owned()
    } else {
        dotted
    }
}

/// `text` with its first letter in upper case.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
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
        assert_eq!(unknown.body()["code"], "workflow_run_not_found");
        assert_eq!(
            unknown.body()["message"],
            format!("Unknown workflow run `{}`", Uuid::nil())
        );
        let conflict = WorkflowsError::conflict(codes::WORKFLOW_RUN_FINISHED, "finished")
            .to_app_error()
            .unwrap();
        assert_eq!(conflict.status.as_u16(), 409);
        assert_eq!(conflict.body()["message"], "finished");
        assert_eq!(conflict.body()["code"], "workflow_run_finished");
        let invalid = WorkflowsError::InvalidDefinition(vec![DefinitionIssue::new(
            "/definition/nodes/0/type",
            "unknown action `nope`",
        )])
        .to_app_error()
        .unwrap();
        assert_eq!(invalid.status.as_u16(), 400);
        assert_eq!(
            invalid.body()["errors"]["definition.nodes.0.type"][0],
            "definition.nodes.0.type: unknown action `nope`"
        );
        assert!(WorkflowsError::Other("x".into()).to_app_error().is_none());
        for code in codes::ALL {
            assert!(navree_http::codes::is_valid(code), "{code}");
        }
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
