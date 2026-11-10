//! The test app, a journal of what steps ran, and the workflows, events
//! and automation blocks the tests use.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::{Backoff, Clock, FixedClock};
use navree_db::{DatabaseModule, Db};
use navree_events::{Event, EventBus, EventsModule};
use navree_kernel::{BoxError, Module, ModuleBuilder};
use navree_testing::{TestApp, TestAppBuilder};
use navree_workflows::{
    Action, AutomationCtx, AutomationTemplate, Automations, EventMatch, ModuleBuilderWorkflowsExt,
    StepOptions, Workflow, WorkflowCtx, Workflows, WorkflowsError, WorkflowsModule, action,
    condition, trigger, workflow,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;
use uuid::Uuid;

pub const DAY: Duration = Duration::from_secs(86_400);
pub const HOUR: Duration = Duration::from_secs(3_600);

/// 2026-10-26T08:00:00Z.
pub fn start() -> DateTime<Utc> {
    "2026-10-26T08:00:00Z".parse().expect("valid time")
}

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

/// What steps and actions ran, in order; and a gate a step can block on.
#[derive(Default)]
pub struct Journal {
    calls: Mutex<Vec<String>>,
    blocking: AtomicBool,
    pub entered: Notify,
    pub gate: Notify,
}

impl Journal {
    /// Notes `call`; returns how many times it was noted (this one
    /// included).
    pub fn record(&self, call: impl Into<String>) -> u32 {
        let call = call.into();
        let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        calls.push(call.clone());
        u32::try_from(calls.iter().filter(|c| **c == call).count()).unwrap_or(u32::MAX)
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn count(&self, call: &str) -> usize {
        self.calls().iter().filter(|c| *c == call).count()
    }

    /// Makes the `block` step wait on [`gate`](Journal::gate).
    pub fn block(&self, on: bool) {
        self.blocking.store(on, Ordering::SeqCst);
    }

    async fn pass(&self) {
        if self.blocking.load(Ordering::SeqCst) {
            self.entered.notify_one();
            self.gate.notified().await;
        }
    }
}

async fn journal(ctx: &WorkflowCtx) -> Arc<Journal> {
    ctx.app().get::<Journal>().await.expect("the journal")
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// A learner finished a course.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CourseCompleted {
    pub user_id: u64,
    pub course_id: u64,
    pub score: u32,
}

impl Event for CourseCompleted {
    const NAME: &'static str = "course.completed";
}

/// No module registers it with `m.workflow_event`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unregistered {
    pub id: u64,
}

impl Event for Unregistered {
    const NAME: &'static str = "unregistered.happened";
}

/// A deal was won.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DealWon {
    pub deal_id: u64,
    pub owner_id: u64,
    pub amount_cents: i64,
    pub workspace_id: Option<Uuid>,
}

impl Event for DealWon {
    const NAME: &'static str = "deal.won";
}

/// A task was completed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskCompleted {
    pub owner_id: u64,
    pub title: String,
}

impl Event for TaskCompleted {
    const NAME: &'static str = "task.completed";
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

/// One step (with `#[workflow]`; the others implement `Workflow` by hand).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Greet {
    pub name: String,
}

/// Says hello.
#[workflow]
pub async fn greet(ctx: WorkflowCtx, input: Greet) -> Result<String, BoxError> {
    let journal = journal(&ctx).await;
    let name = input.name;
    let greeting = ctx
        .step("hello", || async move {
            journal.record("hello");
            Ok::<_, BoxError>(format!("Hello, {name}"))
        })
        .await?;
    Ok(greeting)
}

/// A welcome, three days of sleep, a nudge; returns the time it woke.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Onboarding {
    pub user_id: u64,
}

impl Workflow for Onboarding {
    const NAME: &'static str = "onboarding";
    type Output = DateTime<Utc>;

    async fn run(self, ctx: WorkflowCtx) -> Result<DateTime<Utc>, BoxError> {
        let j = journal(&ctx).await;
        let welcome = j.clone();
        ctx.step("welcome", || async move {
            welcome.record("welcome");
            Ok::<_, BoxError>(())
        })
        .await?;
        ctx.sleep(3 * DAY).await;
        let woke = ctx.now().await;
        ctx.step("nudge", || async move {
            j.record("nudge");
            Ok::<_, BoxError>(())
        })
        .await?;
        Ok(woke)
    }
}

/// Waits up to 14 days for the learner's course; returns the score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AwaitCourse {
    pub user_id: u64,
}

impl Workflow for AwaitCourse {
    const NAME: &'static str = "await-course";
    type Output = Option<u32>;

    async fn run(self, ctx: WorkflowCtx) -> Result<Option<u32>, BoxError> {
        let j = journal(&ctx).await;
        let done = ctx
            .wait_for::<CourseCompleted>(EventMatch::field("user_id", self.user_id), 14 * DAY)
            .await?;
        let call = if done.is_some() {
            "congratulate"
        } else {
            "remind"
        };
        ctx.step(call, || async move {
            j.record(call);
            Ok::<_, BoxError>(())
        })
        .await?;
        Ok(done.map(|event| event.score))
    }
}

/// Waits for an event no module registered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AwaitUnregistered;

impl Workflow for AwaitUnregistered {
    const NAME: &'static str = "await-unregistered";
    type Output = ();

    async fn run(self, ctx: WorkflowCtx) -> Result<(), BoxError> {
        ctx.wait_for::<Unregistered>(EventMatch::any(), None)
            .await?;
        Ok(())
    }
}

/// A charge that fails `failures` times before it goes through (3
/// attempts, 10 s apart); returns the attempt that worked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flaky {
    pub failures: u32,
}

impl Workflow for Flaky {
    const NAME: &'static str = "flaky";
    type Output = u32;

    async fn run(self, ctx: WorkflowCtx) -> Result<u32, BoxError> {
        let j = journal(&ctx).await;
        let failures = self.failures;
        let options = StepOptions::attempts(3).backoff(Backoff::Fixed { secs: 10 });
        let attempt = ctx
            .step_with("charge", options, || async move {
                let n = j.record("charge");
                if n <= failures {
                    Err(format!("card declined ({n})"))
                } else {
                    Ok(n)
                }
            })
            .await?;
        Ok(attempt)
    }
}

/// A charge that fails once (one attempt), and a refund instead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Compensating;

impl Workflow for Compensating {
    const NAME: &'static str = "compensating";
    type Output = String;

    async fn run(self, ctx: WorkflowCtx) -> Result<String, BoxError> {
        let j = journal(&ctx).await;
        let charged = ctx
            .step_with("charge", StepOptions::attempts(1), || async {
                Err::<(), _>("card declined")
            })
            .await;
        match charged {
            Ok(()) => Ok("charged".to_owned()),
            Err(WorkflowsError::StepFailed { message, .. }) => {
                ctx.step("refund", || async move {
                    j.record("refund");
                    Ok::<_, BoxError>(())
                })
                .await?;
                Ok(format!("refunded after: {message}"))
            }
            Err(other) => Err(other.into()),
        }
    }
}

/// `before`, then `block` (which may wait on the journal's gate), then
/// `after`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crashy;

impl Workflow for Crashy {
    const NAME: &'static str = "crashy";
    type Output = ();

    async fn run(self, ctx: WorkflowCtx) -> Result<(), BoxError> {
        let j = journal(&ctx).await;
        for name in ["before", "block", "after"] {
            let j = j.clone();
            ctx.step(name, || async move {
                j.record(name);
                if name == "block" {
                    j.pass().await;
                }
                Ok::<_, BoxError>(())
            })
            .await?;
        }
        Ok(())
    }
}

/// Panics outside any step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Panicky;

impl Workflow for Panicky {
    const NAME: &'static str = "panicky";
    type Output = ();

    async fn run(self, _ctx: WorkflowCtx) -> Result<(), BoxError> {
        panic!("boom");
    }
}

/// Two steps at once, and the same step name twice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fanout;

impl Workflow for Fanout {
    const NAME: &'static str = "fanout";
    type Output = Vec<u32>;

    async fn run(self, ctx: WorkflowCtx) -> Result<Vec<u32>, BoxError> {
        let (a, b) = futures_util::join!(
            ctx.step("left", || async { Ok::<_, BoxError>(1) }),
            ctx.step("right", || async { Ok::<_, BoxError>(2) }),
        );
        let mut out = vec![a?, b?];
        for i in 0..3 {
            out.push(
                ctx.step("repeat", || async move { Ok::<_, BoxError>(10 + i) })
                    .await?,
            );
        }
        Ok(out)
    }
}

/// Version 1 of `upgrade`: `a`, an hour of sleep, `b-v1`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpgradeV1;

impl Workflow for UpgradeV1 {
    const NAME: &'static str = "upgrade";
    type Output = String;

    async fn run(self, ctx: WorkflowCtx) -> Result<String, BoxError> {
        upgrade(ctx, "v1").await
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpgradeV2;

/// Version 2 of `upgrade`; resumes version 1 runs too.
#[workflow(name = "upgrade", version = 2, min_version = 1)]
pub async fn upgrade_v2(ctx: WorkflowCtx, _input: UpgradeV2) -> Result<String, BoxError> {
    upgrade(ctx, "v2").await
}

/// Version 3 of `upgrade`, which no longer resumes version 1 runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpgradeV3;

impl Workflow for UpgradeV3 {
    const NAME: &'static str = "upgrade";
    const VERSION: u32 = 3;
    const MIN_VERSION: u32 = 2;
    type Output = String;

    async fn run(self, ctx: WorkflowCtx) -> Result<String, BoxError> {
        upgrade(ctx, "v3").await
    }
}

async fn upgrade(ctx: WorkflowCtx, code: &'static str) -> Result<String, BoxError> {
    let j = journal(&ctx).await;
    let first = j.clone();
    ctx.step("a", || async move {
        first.record("a");
        Ok::<_, BoxError>(())
    })
    .await?;
    ctx.sleep(HOUR).await;
    // A version 2 implementation branches on the run's version.
    let b = if ctx.version() == 1 { "b-v1" } else { "b-new" };
    ctx.step(b, || async move {
        j.record(b);
        Ok::<_, BoxError>(())
    })
    .await?;
    Ok(format!("{code} ran a version {} run", ctx.version()))
}

// ---------------------------------------------------------------------------
// Automation blocks
// ---------------------------------------------------------------------------

// `#[trigger]`, `#[condition]` and `#[action]` implement most blocks;
// `Explode` implements `Action` by hand.

/// A deal was won, optionally only above an amount.
#[derive(Clone, Debug, Default, Serialize, Deserialize, navree_openapi::ToSchema)]
pub struct DealWonTrigger {
    /// Only deals worth at least this much.
    #[serde(default)]
    pub min_amount_cents: Option<i64>,
}

/// A deal moved to won.
#[trigger(type = "deal.won", label = "Deal won", subject = "owner_id")]
pub fn deal_won(config: &DealWonTrigger, event: &DealWon) -> bool {
    config
        .min_amount_cents
        .is_none_or(|min| event.amount_cents >= min)
}

/// A task was completed (no workspace field in the event).
#[trigger(type = "task.completed", event = TaskCompleted, subject = "owner_id", workspace = false)]
#[derive(Clone, Debug, Default, Serialize, Deserialize, navree_openapi::ToSchema)]
pub struct TaskCompletedTrigger {}

/// Sends a note (to the journal).
#[derive(Clone, Debug, Serialize, Deserialize, navree_openapi::ToSchema)]
pub struct SendNote {
    /// The template.
    pub template: String,
}

/// Notes `template` for the subject in the journal.
#[action(type = "note.send", label = "Send a note", preview = note_preview)]
pub async fn send_note(config: SendNote, ctx: AutomationCtx) -> Result<Value, BoxError> {
    let journal = ctx.app().get::<Journal>().await?;
    let subject = ctx.subject().unwrap_or("-").to_owned();
    journal.record(format!("note:{}:{subject}", config.template));
    Ok(json!({"sent": config.template, "to": subject}))
}

/// What a simulation shows for a note.
pub fn note_preview(config: &SendNote, ctx: &AutomationCtx) -> Value {
    json!({"would_send": config.template, "to": ctx.subject()})
}

/// Fails every time.
#[derive(Clone, Debug, Serialize, Deserialize, navree_openapi::ToSchema)]
pub struct Explode {}

impl Action for Explode {
    const TYPE: &'static str = "explode";
    type Output = ();

    async fn run(self, _ctx: AutomationCtx) -> Result<(), BoxError> {
        Err("the action failed".into())
    }
}

/// The won deal is worth at least `cents`.
#[derive(Clone, Debug, Serialize, Deserialize, navree_openapi::ToSchema)]
pub struct AmountAtLeast {
    /// The threshold.
    pub cents: i64,
}

#[condition(type = "deal.amount_at_least")]
pub async fn amount_at_least(
    config: &AmountAtLeast,
    ctx: &AutomationCtx,
) -> Result<bool, BoxError> {
    let deal: DealWon = ctx.event_as()?;
    Ok(deal.amount_cents >= config.cents)
}

/// The event names a workspace (a plain `fn` returning `bool`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, navree_openapi::ToSchema)]
pub struct InWorkspace {}

/// The deal belongs to a workspace.
#[condition(type = "deal.in_workspace", label = "In a workspace")]
pub fn in_workspace(_config: &InWorkspace, ctx: &AutomationCtx) -> bool {
    ctx.field("event.workspace_id")
        .is_some_and(|id| !id.is_null())
}

/// A follow-up for big deals.
pub fn follow_up() -> Value {
    json!({
        "trigger": {"type": "deal.won", "config": {}},
        "nodes": [
            {"id": "thanks", "kind": "action", "type": "note.send",
             "config": {"template": "thanks"}, "next": "wait-a-day"},
            {"id": "wait-a-day", "kind": "delay", "seconds": 86400, "next": "big"},
            {"id": "big", "kind": "condition", "type": "deal.amount_at_least",
             "config": {"cents": 100000}, "then": "call", "else": "survey"},
            {"id": "call", "kind": "action", "type": "note.send", "config": {"template": "call"}},
            {"id": "survey", "kind": "action", "type": "note.send",
             "config": {"template": "survey"}}
        ]
    })
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

/// The tests' feature module: every workflow, the events workflows wait
/// for, the automation blocks and a template.
pub struct FlowsModule;

impl Module for FlowsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(WorkflowsModule::for_feature::<Greet>())
            .workflow::<Onboarding>()
            .workflow::<AwaitCourse>()
            .workflow::<AwaitUnregistered>()
            .workflow::<Flaky>()
            .workflow::<Compensating>()
            .workflow::<Crashy>()
            .workflow::<Panicky>()
            .workflow::<Fanout>()
            .workflow_event::<CourseCompleted>()
            // Twice: one subscription.
            .workflow_event::<CourseCompleted>()
            .trigger::<DealWonTrigger>()
            .trigger::<TaskCompletedTrigger>()
            .action::<SendNote>()
            .action::<Explode>()
            .condition::<AmountAtLeast>()
            .condition::<InWorkspace>()
            .automation_template(
                AutomationTemplate::new("big-deal-follow-up", "Big deal follow-up", follow_up())
                    .description("Thanks the owner, then a call or a survey a day later.")
                    .category("Sales"),
            );
    }
}

/// The `upgrade` versions an app registers.
pub struct UpgradeModule(pub &'static [u32]);

impl Module for UpgradeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        for version in self.0 {
            match version {
                1 => m.workflow::<UpgradeV1>(),
                2 => m.workflow::<UpgradeV2>(),
                _ => m.workflow::<UpgradeV3>(),
            };
        }
    }
}

/// A database for one test: a fresh SQLite file, or a fresh database on the
/// server `NAVREE_TEST_DATABASE` names (see [`servers`](crate::servers));
/// dropped with this.
pub struct TestDatabase {
    pub url: String,
    _guard: Box<dyn std::any::Any + Send + Sync>,
}

impl TestDatabase {
    pub async fn new() -> TestDatabase {
        let (url, guard) = crate::servers::scratch();
        TestDatabase { url, _guard: guard }
    }
}

/// A booted app with a fixed clock and the journal.
pub struct Harness {
    pub app: TestApp,
    pub clock: Arc<FixedClock>,
    pub journal: Arc<Journal>,
    pub database: Arc<TestDatabase>,
}

/// The base app on `database`: events, the workflows module (routes off),
/// the flows module, a fixed clock and the journal.
pub fn builder(
    database: &TestDatabase,
    clock: Arc<FixedClock>,
    journal: Arc<Journal>,
) -> TestAppBuilder {
    TestApp::builder()
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .config("workflows.routes", false)
        .config("workflows.automation_routes", false)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(WorkflowsModule::from_config())
        .module(FlowsModule)
        .override_value::<dyn Clock>(clock)
        .override_value(journal)
}

/// A harness on a fresh database.
pub async fn harness(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> Harness {
    let database = Arc::new(TestDatabase::new().await);
    harness_on(
        database,
        Arc::new(FixedClock::new(start())),
        Arc::new(Journal::default()),
        configure,
    )
    .await
}

/// A harness on `database`, with `clock` and `journal` (a second boot of
/// the same app).
pub async fn harness_on(
    database: Arc<TestDatabase>,
    clock: Arc<FixedClock>,
    journal: Arc<Journal>,
    configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder,
) -> Harness {
    let app = configure(builder(&database, clock.clone(), journal.clone()))
        .build()
        .await;
    Harness {
        app,
        clock,
        journal,
        database,
    }
}

impl Harness {
    pub async fn workflows(&self) -> Arc<Workflows> {
        self.app.get::<Workflows>().await
    }

    pub async fn automations(&self) -> Arc<Automations> {
        self.app.get::<Automations>().await
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    /// Starts `input` in a committed unit of work.
    pub async fn start<W: Workflow>(&self, input: W) -> Uuid {
        let uow = self.db().await.uow(None, None).await;
        let id = self.workflows().await.start(&uow, input).await.unwrap();
        uow.commit().await.unwrap();
        id
    }

    pub async fn emit<E: Event>(&self, event: E) {
        self.app.get::<EventBus>().await.emit(event).await.unwrap();
    }

    /// Moves the clock forward, then runs `workflows:tick`; returns how
    /// many runs it woke.
    pub async fn advance(&self, by: Duration) -> usize {
        self.clock.advance(by);
        self.workflows().await.tick().await.unwrap()
    }

    pub async fn status(&self, id: Uuid) -> navree_workflows::RunStatus {
        self.workflows().await.run(id).await.unwrap().status
    }

    /// The run's steps as `key: status` lines.
    pub async fn steps(&self, id: Uuid) -> BTreeMap<String, String> {
        self.workflows()
            .await
            .details(id)
            .await
            .unwrap()
            .steps
            .into_iter()
            .map(|step| (step.key, step.status.as_str().to_owned()))
            .collect()
    }

    /// Runs `f` in a committed unit of work.
    pub async fn in_uow<T, F, Fut>(&self, f: F) -> T
    where
        F: FnOnce(navree_db::Uow) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let uow = self.db().await.uow(None, None).await;
        let value = f(uow.clone()).await;
        uow.commit().await.unwrap();
        value
    }
}
