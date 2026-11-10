//! How automations run: enrollment on events, the `automation` workflow
//! that walks a version's nodes, and simulations.

use std::sync::Arc;
use std::time::Duration;

use navree_kernel::BoxError;
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::blocks::{AutomationCtx, Catalog};
use super::graph::{AutomationGraph, NodeStep};
use super::model::{Automation, EnrollmentPolicy, EnrollmentQuery, EnrollmentRecord};
use crate::ctx::StepOptions;
use crate::engine::Engine;
use crate::matcher::TriggerRef;
use crate::{
    EventMatch, RunQuery, StartOptions, Workflow, WorkflowCtx, WorkflowDefinition, WorkflowsError,
};

/// The workflow that carries one enrollment: walks the nodes of one
/// automation version.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AutomationRun {
    pub(crate) automation_id: Uuid,
    pub(crate) version: u32,
    pub(crate) enrollment_id: Uuid,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) subject: Option<String>,
    pub(crate) event: Value,
}

/// The name of the workflow behind enrollments.
pub const AUTOMATION_WORKFLOW: &str = "automation";

impl Workflow for AutomationRun {
    const NAME: &'static str = AUTOMATION_WORKFLOW;
    const DESCRIPTION: &'static str = "Runs one automation enrollment.";
    type Output = Value;

    async fn run(self, ctx: WorkflowCtx) -> Result<Value, BoxError> {
        let engine = ctx.engine();
        let (automation_id, version) = (self.automation_id, self.version);
        let store = engine.automations.clone();
        // The definition is stored with the run: a replay never depends on
        // the automation still being there.
        let graph: AutomationGraph = ctx
            .run_step(
                "$definition".to_owned(),
                StepOptions::default(),
                || async move {
                    let version =
                        store
                            .version(automation_id, version)
                            .await?
                            .ok_or_else(|| {
                                format!("automation {automation_id} has no version {version}")
                            })?;
                    Ok::<_, BoxError>(version.definition)
                },
            )
            .await?;
        let mut data = initial_data(&self);
        let mut current = graph.nodes.first().map(|node| node.id.clone());
        while let Some(id) = current.take() {
            let node = graph
                .node(&id)
                .ok_or_else(|| format!("the definition has no node `{id}`"))?;
            current = match &node.step {
                NodeStep::Action {
                    block_type,
                    config,
                    next,
                } => {
                    let entry = engine
                        .catalog
                        .action(block_type)
                        .ok_or_else(|| format!("unknown action `{block_type}`"))?;
                    let run = entry.run;
                    let actx = self.ctx_for(&ctx, &id, &data, false);
                    let config = config.clone();
                    let output: Value = ctx
                        .run_step(id.clone(), StepOptions::default(), move || {
                            run(config, actx)
                        })
                        .await?;
                    set_output(&mut data, &id, output);
                    next.clone()
                }
                NodeStep::Condition {
                    block_type,
                    config,
                    then,
                    otherwise,
                } => {
                    let entry = engine
                        .catalog
                        .condition(block_type)
                        .ok_or_else(|| format!("unknown condition `{block_type}`"))?;
                    let evaluate = entry.evaluate;
                    let actx = self.ctx_for(&ctx, &id, &data, false);
                    let config = config.clone();
                    let passed: bool = ctx
                        .run_step(id.clone(), StepOptions::default(), move || {
                            evaluate(config, actx)
                        })
                        .await?;
                    set_output(&mut data, &id, json!(passed));
                    if passed {
                        then.clone()
                    } else {
                        otherwise.clone()
                    }
                }
                NodeStep::Delay { seconds, next } => {
                    ctx.sleep_keyed(id.clone(), Duration::from_secs(*seconds))
                        .await;
                    next.clone()
                }
                NodeStep::Wait {
                    trigger,
                    same_subject,
                    timeout_seconds,
                    next,
                    on_timeout,
                } => {
                    let entry = engine
                        .catalog
                        .trigger(&trigger.block_type)
                        .ok_or_else(|| format!("unknown trigger `{}`", trigger.block_type))?;
                    let filter = EventMatch {
                        fields: Default::default(),
                        trigger: Some(TriggerRef {
                            kind: trigger.block_type.clone(),
                            config: trigger.config.clone(),
                            subject: if *same_subject {
                                self.subject.clone()
                            } else {
                                None
                            },
                        }),
                    };
                    let timeout = timeout_seconds.map(Duration::from_secs);
                    match ctx
                        .wait_keyed(id.clone(), entry.event, filter, timeout)
                        .await?
                    {
                        Some(event) => {
                            set_output(&mut data, &id, event);
                            next.clone()
                        }
                        None => {
                            set_output(&mut data, &id, Value::Null);
                            on_timeout.clone()
                        }
                    }
                }
            };
        }
        Ok(data.get("nodes").cloned().unwrap_or(Value::Null))
    }
}

impl AutomationRun {
    fn ctx_for(
        &self,
        ctx: &WorkflowCtx,
        node: &str,
        data: &Value,
        simulation: bool,
    ) -> AutomationCtx {
        AutomationCtx {
            app: ctx.app().clone(),
            automation_id: self.automation_id,
            version: self.version,
            enrollment_id: self.enrollment_id,
            run_id: ctx.run_id(),
            node: node.to_owned(),
            workspace_id: self.workspace_id,
            subject: self.subject.clone(),
            data: Arc::new(data.clone()),
            simulation,
        }
    }
}

fn initial_data(run: &AutomationRun) -> Value {
    json!({
        "event": run.event,
        "subject": run.subject,
        "workspace_id": run.workspace_id,
        "automation": {"id": run.automation_id, "version": run.version},
        "nodes": {},
    })
}

fn set_output(data: &mut Value, node: &str, output: Value) {
    if let Some(nodes) = data.get_mut("nodes").and_then(Value::as_object_mut) {
        nodes.insert(node.to_owned(), output);
    }
}

/// Why an event does not enroll in an automation (`None`: it does).
pub(crate) async fn refusal(
    engine: &Engine,
    automation: &Automation,
    graph: &AutomationGraph,
    event: &Value,
) -> Result<(Option<String>, Option<String>), WorkflowsError> {
    let Some(entry) = engine.catalog.trigger(&graph.trigger.block_type) else {
        return Ok((
            Some(format!("unknown trigger `{}`", graph.trigger.block_type)),
            None,
        ));
    };
    let subject = entry.subject_of(event);
    if let Some(workspace) = automation.workspace_id
        && entry.workspace_of(event) != Some(workspace)
    {
        return Ok((
            Some("the event belongs to another workspace".to_owned()),
            subject,
        ));
    }
    match (entry.matches)(&graph.trigger.config, event) {
        Ok(true) => {}
        Ok(false) => {
            return Ok((
                Some("the trigger's settings do not match the event".to_owned()),
                subject,
            ));
        }
        Err(error) => {
            return Ok((
                Some(format!("the event does not fit the trigger: {error}")),
                subject,
            ));
        }
    }
    let Some(subject_text) = &subject else {
        return Ok((None, subject));
    };
    let blocked = match automation.enrollment {
        EnrollmentPolicy::EveryTime => false,
        EnrollmentPolicy::Once => !engine
            .automations
            .enrollments(&EnrollmentQuery {
                automation_id: Some(automation.id),
                subject: Some(subject_text.clone()),
                limit: 1,
                offset: 0,
            })
            .await?
            .is_empty(),
        EnrollmentPolicy::OneAtATime => {
            active_enrollment(engine, automation.id, subject_text).await?
        }
    };
    if blocked {
        let why = match automation.enrollment {
            EnrollmentPolicy::Once => "the subject was enrolled before (enrollment: once)",
            _ => "the subject's enrollment is still under way (enrollment: one_at_a_time)",
        };
        return Ok((Some(why.to_owned()), subject));
    }
    Ok((None, subject))
}

/// Whether `subject` has an unfinished enrollment in `automation_id`.
async fn active_enrollment(
    engine: &Engine,
    automation_id: Uuid,
    subject: &str,
) -> Result<bool, WorkflowsError> {
    let enrollments = engine
        .automations
        .enrollments(&EnrollmentQuery {
            automation_id: Some(automation_id),
            subject: Some(subject.to_owned()),
            limit: 100,
            offset: 0,
        })
        .await?;
    if enrollments.is_empty() {
        return Ok(false);
    }
    let query = RunQuery {
        ids: enrollments.iter().map(|e| e.run_id).collect(),
        limit: 100,
        ..RunQuery::default()
    };
    let runs = engine.store.runs(&query).await?;
    Ok(runs.iter().any(|run| !run.status.is_finished()))
}

/// Enrolls `event` (named `name`) in every active automation it triggers;
/// returns how many enrollments started.
pub(crate) async fn enroll(
    engine: &Arc<Engine>,
    name: &str,
    event: &Value,
) -> Result<usize, WorkflowsError> {
    let mut started = 0;
    for automation in engine.automations.active_for_event(name).await? {
        let Some(version) = automation.version else {
            continue;
        };
        let Some(published) = engine.automations.version(automation.id, version).await? else {
            continue;
        };
        let (refused, subject) = refusal(engine, &automation, &published.definition, event).await?;
        if let Some(reason) = refused {
            tracing::debug!(
                target: "navree::workflows",
                automation_id = %automation.id,
                event = name,
                reason,
                "event not enrolled"
            );
            continue;
        }
        match start_enrollment(engine, &automation, version, subject, event.clone()).await {
            Ok(_) => started += 1,
            Err(error) => tracing::error!(
                target: "navree::workflows",
                automation_id = %automation.id,
                event = name,
                %error,
                "cannot enroll an event"
            ),
        }
    }
    Ok(started)
}

/// Stores an enrollment and its run (in one transaction with the database
/// stores), then wakes the run.
pub(crate) async fn start_enrollment(
    engine: &Arc<Engine>,
    automation: &Automation,
    version: u32,
    subject: Option<String>,
    event: Value,
) -> Result<EnrollmentRecord, WorkflowsError> {
    let definition = WorkflowDefinition::of::<AutomationRun>();
    let enrollment_id = Uuid::now_v7();
    let input = AutomationRun {
        automation_id: automation.id,
        version,
        enrollment_id,
        workspace_id: automation.workspace_id,
        subject: subject.clone(),
        event,
    };
    let input = serde_json::to_value(&input)
        .map_err(|error| WorkflowsError::payload("the enrollment cannot be stored", error))?;
    let options = StartOptions {
        workspace_id: automation.workspace_id,
        ..StartOptions::default()
    };
    let uow = match &engine.db {
        Some(db) => Some(db.uow(None, None).await),
        None => None,
    };
    let result = async {
        let (run, _) = engine
            .insert_run(uow.as_ref(), &definition, input, &options)
            .await?;
        let record = EnrollmentRecord {
            id: enrollment_id,
            automation_id: automation.id,
            version,
            workspace_id: automation.workspace_id,
            subject,
            run_id: run.id,
            created_at: run.created_at,
        };
        engine
            .automations
            .insert_enrollment(uow.as_ref(), &record)
            .await?;
        Ok::<_, WorkflowsError>(record)
    }
    .await;
    let record = match (result, &uow) {
        (Ok(record), Some(uow)) => {
            uow.commit().await?;
            record
        }
        (Ok(record), None) => record,
        (Err(error), Some(uow)) => {
            if let Err(rollback) = uow.rollback().await {
                tracing::warn!(target: "navree::workflows", error = %rollback, "rollback failed");
            }
            return Err(error);
        }
        (Err(error), None) => return Err(error),
    };
    tracing::debug!(
        target: "navree::workflows",
        automation_id = %automation.id,
        enrollment_id = %record.id,
        run_id = %record.run_id,
        "enrolled"
    );
    engine.wake(record.run_id, None).await?;
    Ok(record)
}

/// What a wait does in a simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitOutcome {
    /// The awaited event comes (the default).
    #[default]
    Received,
    /// The wait times out.
    TimedOut,
}

/// The body of `POST /api/automations/{id}/simulate`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SimulateInput {
    /// A sample of the trigger's event, as JSON.
    #[schema(value_type = Object)]
    pub event: Value,
    /// Simulate this published version (default: the draft).
    #[serde(default)]
    pub version: Option<u32>,
    /// What waits do (default: the event comes).
    #[serde(default)]
    pub waits: WaitOutcome,
}

/// One node a simulated enrollment reached.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SimulatedStep {
    /// The node's id.
    pub node: String,
    /// `action`, `condition`, `delay` or `wait`.
    pub kind: String,
    /// The block type, for actions, conditions and waits.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub block_type: Option<String>,
    /// Seconds after enrollment the node is reached (delays and timeouts
    /// add up).
    pub at_seconds: u64,
    /// What happens: `would_run` (an action), `true` or `false` (a
    /// condition), `waits` (a delay), `received` or `timed_out` (a wait),
    /// `error` (a condition that failed).
    pub outcome: String,
    /// An action's preview, a delay's length, a condition's error.
    #[schema(value_type = Object)]
    pub detail: Value,
}

/// What `POST /api/automations/{id}/simulate` answers: whether the event
/// would enroll, and the path its enrollment would take. Nothing is
/// stored and no action runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Simulation {
    /// Whether the event would enroll now.
    pub enrolls: bool,
    /// Why not, when it would not.
    pub reason: Option<String>,
    /// The subject the event names.
    pub subject: Option<String>,
    /// The version simulated (`None`: the draft).
    pub version: Option<u32>,
    /// The nodes reached, in order.
    pub steps: Vec<SimulatedStep>,
}

/// Walks `graph` for `event` without storing or running anything.
pub(crate) async fn simulate(
    engine: &Arc<Engine>,
    automation: &Automation,
    graph: &AutomationGraph,
    version: Option<u32>,
    event: Value,
    waits: WaitOutcome,
) -> Result<Simulation, WorkflowsError> {
    let app = engine.app()?;
    let catalog: &Catalog = &engine.catalog;
    if let Some(entry) = catalog.trigger(&graph.trigger.block_type) {
        // The sample must be the trigger's event.
        (entry.check_event)(&event).map_err(|error| {
            WorkflowsError::invalid(
                crate::error::codes::INVALID_AUTOMATION_EVENT,
                format!("The sample is not a `{}` event: {error}", entry.event),
            )
        })?;
    }
    let (reason, subject) = match automation.status {
        super::model::AutomationStatus::Active => {
            refusal(engine, automation, graph, &event).await?
        }
        status => {
            let (refused, subject) = refusal(engine, automation, graph, &event).await?;
            (
                refused.or_else(|| Some(format!("the automation is {status}"))),
                subject,
            )
        }
    };
    let run = AutomationRun {
        automation_id: automation.id,
        version: version.unwrap_or(0),
        enrollment_id: Uuid::nil(),
        workspace_id: automation.workspace_id,
        subject: subject.clone(),
        event,
    };
    let mut data = initial_data(&run);
    let mut steps = Vec::new();
    let mut at_seconds: u64 = 0;
    let mut current = graph.nodes.first().map(|node| node.id.clone());
    while let Some(id) = current.take() {
        let Some(node) = graph.node(&id) else {
            break;
        };
        // Each node at most once, even in a draft with a loop.
        if steps.iter().any(|s: &SimulatedStep| s.node == id) {
            break;
        }
        let ctx = AutomationCtx {
            app: app.clone(),
            automation_id: automation.id,
            version: run.version,
            enrollment_id: Uuid::nil(),
            run_id: Uuid::nil(),
            node: id.clone(),
            workspace_id: automation.workspace_id,
            subject: subject.clone(),
            data: Arc::new(data.clone()),
            simulation: true,
        };
        let block_type = node.step.block_type().map(str::to_owned);
        let (outcome, detail, next) = match &node.step {
            NodeStep::Action {
                block_type,
                config,
                next,
            } => {
                let detail = match catalog.action(block_type) {
                    Some(entry) => {
                        (entry.preview)(config, &ctx).unwrap_or_else(|e| json!({"error": e}))
                    }
                    None => json!({"error": format!("unknown action `{block_type}`")}),
                };
                set_output(&mut data, &id, Value::Null);
                ("would_run".to_owned(), detail, next.clone())
            }
            NodeStep::Condition {
                block_type,
                config,
                then,
                otherwise,
            } => {
                let result = match catalog.condition(block_type) {
                    Some(entry) => (entry.evaluate)(config.clone(), ctx)
                        .await
                        .map_err(|e| e.to_string()),
                    None => Err(format!("unknown condition `{block_type}`")),
                };
                match result {
                    Ok(passed) => {
                        set_output(&mut data, &id, json!(passed));
                        (
                            passed.to_string(),
                            Value::Null,
                            if passed {
                                then.clone()
                            } else {
                                otherwise.clone()
                            },
                        )
                    }
                    Err(error) => ("error".to_owned(), json!({"error": error}), None),
                }
            }
            NodeStep::Delay { seconds, next } => {
                let reached = at_seconds;
                at_seconds = at_seconds.saturating_add(*seconds);
                steps.push(SimulatedStep {
                    node: id.clone(),
                    kind: node.step.kind().to_owned(),
                    block_type,
                    at_seconds: reached,
                    outcome: "waits".to_owned(),
                    detail: json!({"seconds": seconds}),
                });
                current = next.clone();
                continue;
            }
            NodeStep::Wait {
                timeout_seconds,
                next,
                on_timeout,
                ..
            } => match waits {
                WaitOutcome::Received => {
                    set_output(&mut data, &id, Value::Null);
                    (
                        "received".to_owned(),
                        json!({"timeout_seconds": timeout_seconds}),
                        next.clone(),
                    )
                }
                WaitOutcome::TimedOut => {
                    let reached = at_seconds;
                    at_seconds = at_seconds.saturating_add(timeout_seconds.unwrap_or(0));
                    set_output(&mut data, &id, Value::Null);
                    steps.push(SimulatedStep {
                        node: id.clone(),
                        kind: node.step.kind().to_owned(),
                        block_type,
                        at_seconds: reached,
                        outcome: if timeout_seconds.is_some() {
                            "timed_out"
                        } else {
                            "waits_forever"
                        }
                        .to_owned(),
                        detail: json!({"timeout_seconds": timeout_seconds}),
                    });
                    current = if timeout_seconds.is_some() {
                        on_timeout.clone()
                    } else {
                        None
                    };
                    continue;
                }
            },
        };
        steps.push(SimulatedStep {
            node: id.clone(),
            kind: node.step.kind().to_owned(),
            block_type,
            at_seconds,
            outcome,
            detail,
        });
        current = next;
    }
    Ok(Simulation {
        enrolls: reason.is_none(),
        reason,
        subject,
        version,
        steps,
    })
}
