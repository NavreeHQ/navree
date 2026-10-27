//! The automations half: people build definitions from registered
//! triggers, conditions and actions; published versions enroll matching
//! events, each enrollment carried by a durable workflow run.

mod blocks;
mod controller;
mod graph;
mod model;
mod runner;
mod service;
mod store;

pub(crate) use blocks::BlockRegistration;
pub use blocks::{
    Action, AutomationCtx, BlockInfo, BlockKind, Catalog, CompareOp, Condition, FieldCondition,
    Trigger,
};
pub use controller::{
    AUTOMATIONS_MANAGE_PERMISSION, AUTOMATIONS_VIEW_PERMISSION, AutomationsController,
    CatalogResponse, CreateAutomationInput, EnrollInput, FromTemplateInput, ValidateInput,
};
pub use graph::{AutomationGraph, BlockRef, MAX_NODES, Node, NodeStep};
pub use model::{
    Automation, AutomationQuery, AutomationStatus, AutomationTemplate, AutomationVersion,
    Enrollment, EnrollmentPolicy, EnrollmentQuery, EnrollmentRecord,
};
pub use runner::{AUTOMATION_WORKFLOW, SimulateInput, SimulatedStep, Simulation, WaitOutcome};
pub(crate) use runner::{AutomationRun, enroll};
pub(crate) use service::TemplateRegistry;
pub use service::{
    AutomationChanges, AutomationDetails, Automations, MAX_AUTOMATION_NAME_LEN, NewAutomation,
    Validation,
};
pub use store::{AutomationStore, DatabaseAutomationStore, MemoryAutomationStore};
