//! # navree-workflows
//!
//! `WorkflowsModule`: durable workflows in code, and end-user automations
//! built on them.
//!
//! **Workflows in code.** A [`Workflow`] is an input type and an `async`
//! function (`#[workflow]` writes the impl). It runs with a
//! [`WorkflowCtx`]: every [`step`](WorkflowCtx::step) stores its result in
//! `workflow_steps`, [`sleep`](WorkflowCtx::sleep) waits durably for
//! minutes or months, [`wait_for`](WorkflowCtx::wait_for) waits for an
//! event (with a timeout), and [`now`](WorkflowCtx::now) records the time.
//! When a run must wait, its execution stops and nothing holds a worker; a
//! later execution replays it (completed steps return their stored
//! results) and goes on. A run survives restarts and crashes, keeps the
//! version it started on, and can be cancelled or retried.
//!
//! ```ignore
//! /// Welcomes a new user, then nudges them if they have not finished the
//! /// course after a week.
//! #[workflow]
//! async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> Result<(), BoxError> {
//!     ctx.step("send-welcome", || send_welcome(input.user_id)).await?;
//!     ctx.sleep(Duration::from_secs(3 * 86_400)).await;
//!     let done = ctx
//!         .wait_for::<CourseCompleted>(EventMatch::field("user_id", input.user_id), WEEK)
//!         .await?;
//!     if done.is_none() {
//!         ctx.step("send-nudge", || send_nudge(input.user_id)).await?;
//!     }
//!     Ok(())
//! }
//!
//! let run_id = workflows.start(&uow, Onboarding { user_id }).await?;
//! ```
//!
//! **Automations.** Modules register [`Trigger`]s (events), [`Condition`]s
//! and [`Action`]s whose configuration types derive `ToSchema`, so an
//! editor gets a JSON Schema per block (`GET /api/automations/catalog`).
//! People build [`AutomationGraph`]s from them per workspace, publish them
//! as versions, and every matching event enrolls: one durable run per
//! enrollment walks the version's nodes (actions, conditions, delays,
//! waits). Definitions are validated against the registry, enrollments
//! follow an [`EnrollmentPolicy`], `POST /api/automations/{id}/simulate`
//! shows the path an event would take, and [`AutomationTemplate`]s give
//! people a start.
//!
//! **Tests.** [`WorkflowsFake`] records the runs app code starts; with
//! `workflows.store = "memory"` and a fixed clock, runs execute inline and
//! [`Workflows::tick`] wakes their timers.
#![forbid(unsafe_code)]

// `#[workflow]`, `#[trigger]`, `#[condition]` and `#[action]` name this
// crate as `::navree_workflows` (or through the facade), also from its own
// tests.
extern crate self as navree_workflows;

mod automation;
mod commands;
mod controller;
mod ctx;
mod engine;
mod error;
mod fake;
mod jobs;
mod matcher;
mod migration;
mod module;
mod options;
mod run;
mod schema;
mod service;
mod store;
mod workflow;

pub use automation::{
    AUTOMATION_WORKFLOW, AUTOMATIONS_MANAGE_PERMISSION, AUTOMATIONS_VIEW_PERMISSION, Action,
    Automation, AutomationChanges, AutomationCtx, AutomationDetails, AutomationGraph,
    AutomationQuery, AutomationStatus, AutomationStore, AutomationTemplate, AutomationVersion,
    Automations, AutomationsController, BlockInfo, BlockKind, BlockRef, Catalog, CatalogResponse,
    CompareOp, Condition, CreateAutomationInput, DatabaseAutomationStore, EnrollInput, Enrollment,
    EnrollmentPolicy, EnrollmentQuery, EnrollmentRecord, FieldCondition, FromTemplateInput,
    MAX_AUTOMATION_NAME_LEN, MAX_NODES, MemoryAutomationStore, NewAutomation, Node, NodeStep,
    SimulateInput, SimulatedStep, Simulation, Trigger, ValidateInput, Validation, WaitOutcome,
};
pub use controller::{MANAGE_PERMISSION, RunsFilter, VIEW_PERMISSION, WorkflowInfo, WorkflowsController};
pub use ctx::{StepOptions, WorkflowCtx};
pub use engine::Execution;
pub use error::{DefinitionIssue, WorkflowsError, codes};
pub use fake::{StartedRun, WorkflowsFake};
pub use jobs::RunWorkflow;
pub use matcher::EventMatch;
pub use migration::{
    AUTOMATIONS_TABLE, ENROLLMENTS_TABLE, RUNS_TABLE, STEPS_TABLE, VERSIONS_TABLE,
    WORKFLOWS_MIGRATIONS,
};
pub use module::{ModuleBuilderWorkflowsExt, PRUNE_TASK, TICK_TASK, WorkflowsModule};
pub use options::{WorkflowStoreKind, WorkflowsOptions};
pub use run::{RunDetails, RunQuery, RunStatus, StepKind, StepStatus, WorkflowRun, WorkflowStep};
pub use schema::json_schema;
pub use service::Workflows;
pub use store::{
    DatabaseWorkflowStore, DueQuery, Lease, MemoryWorkflowStore, Release, StoreResult,
    WorkflowStore,
};
pub use workflow::{
    MAX_NAME_LEN, StartOptions, Workflow, WorkflowDefinition, WorkflowRegistry, check_name,
};
