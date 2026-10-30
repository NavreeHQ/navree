//! The examples of `content/docs/packages/workflows.mdx` and
//! `content/docs/packages/automations.mdx`, written as an app writes them
//! (the facade, the prelude and the macros), compiled and run.
#![cfg(feature = "sqlite")]
#![allow(missing_docs)]

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree::contracts::{Backoff, Clock, FixedClock};
use navree::db::Db;
use navree::events::{Event, EventBus};
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use navree::workflows::{
    AutomationCtx, AutomationTemplate, Automations, EnrollmentPolicy, EventMatch, NewAutomation,
    RunStatus, SimulateInput, StartOptions, StepOptions, WorkflowCtx, Workflows, WorkflowsError,
    WorkflowsFake, action, condition, trigger, workflow,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

const DAY: Duration = Duration::from_secs(86_400);

// ---------------------------------------------------------------------------
// The app's own services (stand-ins for a mailer, billing and tasks)
// ---------------------------------------------------------------------------

/// What the learner mail service sent: `(user, template)`.
#[derive(Default)]
pub struct LearnerMail {
    sent: Mutex<Vec<(u64, String)>>,
}

impl LearnerMail {
    pub async fn send(&self, user_id: u64, template: &str) -> Result<(), BoxError> {
        self.lock().push((user_id, template.to_owned()));
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(u64, String)>> {
        self.sent.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn sent(&self) -> Vec<(u64, String)> {
        self.lock().clone()
    }
}

/// A card that declines the first `declines` charges.
#[derive(Default)]
pub struct Billing {
    attempts: Mutex<u32>,
    declines: u32,
}

impl Billing {
    pub async fn charge(
        &self,
        _account_id: u64,
        _idempotency_key: &str,
    ) -> Result<String, BoxError> {
        let mut attempts = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
        *attempts += 1;
        if *attempts <= self.declines {
            return Err("card declined".into());
        }
        Ok(format!("receipt-{attempts}"))
    }
}

/// The CRM's tasks: `(owner, title, idempotency key)`.
#[derive(Default)]
pub struct Tasks {
    created: Mutex<Vec<(Uuid, String, String)>>,
}

impl Tasks {
    pub async fn create(
        &self,
        owner_id: Uuid,
        title: &str,
        idempotency_key: &str,
    ) -> Result<Uuid, BoxError> {
        let mut created = self.created.lock().unwrap_or_else(PoisonError::into_inner);
        created.push((owner_id, title.to_owned(), idempotency_key.to_owned()));
        Ok(Uuid::now_v7())
    }

    fn titles(&self) -> Vec<String> {
        let created = self.created.lock().unwrap_or_else(PoisonError::into_inner);
        created.iter().map(|(_, title, _)| title.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// workflows.mdx
// ---------------------------------------------------------------------------

/// A new learner.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Onboarding {
    pub user_id: u64,
}

/// A learner finished a course.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CourseCompleted {
    pub user_id: u64,
    pub course_id: u64,
}

impl Event for CourseCompleted {
    const NAME: &'static str = "course.completed";
}

/// Welcomes a new learner, then nudges them if they have not finished a
/// course within a week.
#[workflow]
pub async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> Result<bool, BoxError> {
    let mail = ctx.app().get::<LearnerMail>().await?;
    let user_id = input.user_id;

    let welcome = mail.clone();
    ctx.step("send-welcome", || async move {
        welcome.send(user_id, "welcome").await
    })
    .await?;

    let completed = ctx
        .wait_for::<CourseCompleted>(EventMatch::field("user_id", user_id), 7 * DAY)
        .await?;
    if completed.is_none() {
        ctx.step(
            "send-nudge",
            || async move { mail.send(user_id, "nudge").await },
        )
        .await?;
    }
    Ok(completed.is_some())
}

/// A trial that ends on `ends_at`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialReminders {
    pub user_id: u64,
    pub ends_at: chrono::DateTime<chrono::Utc>,
}

/// Reminds a user three days before their trial ends, and on the day.
#[workflow(name = "trial-reminders")]
pub async fn trial_reminders(ctx: WorkflowCtx, input: TrialReminders) -> Result<(), BoxError> {
    let mail = ctx.app().get::<LearnerMail>().await?;
    let user_id = input.user_id;
    let started = ctx.now().await;
    let three_days_before = input.ends_at - chrono::TimeDelta::days(3);
    if started < three_days_before {
        ctx.sleep_until(three_days_before).await;
        let mail = mail.clone();
        ctx.step("three-days-left", || async move {
            mail.send(user_id, "trial-3-days").await
        })
        .await?;
    }
    ctx.sleep_until(input.ends_at).await;
    ctx.step("trial-ended", || async move {
        mail.send(user_id, "trial-ended").await
    })
    .await?;
    Ok(())
}

/// A renewal to charge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Renewal {
    pub account_id: u64,
}

/// Charges a renewal: five attempts a minute or more apart, then a mail
/// to the account instead of a failed run.
#[workflow]
pub async fn renewal(ctx: WorkflowCtx, input: Renewal) -> Result<String, BoxError> {
    let billing = ctx.app().get::<Billing>().await?;
    let key = format!("{}:charge", ctx.run_id());
    let retries = StepOptions::attempts(5).backoff(Backoff::Exponential {
        base_secs: 60,
        max_secs: 3_600,
    });
    let charged = ctx
        .step_with("charge", retries, || async move {
            billing.charge(input.account_id, &key).await
        })
        .await;
    match charged {
        Ok(receipt) => Ok(receipt),
        Err(WorkflowsError::StepFailed { message, .. }) => {
            let mail = ctx.app().get::<LearnerMail>().await?;
            ctx.step("payment-failed", || async move {
                mail.send(input.account_id, "payment-failed").await
            })
            .await?;
            Ok(format!("not charged: {message}"))
        }
        Err(other) => Err(other.into()),
    }
}

/// The learning module: its workflows and the event they wait for.
#[module(
    imports = [WorkflowsModule::for_feature::<Onboarding>()],
    register = Self::workflows,
)]
pub struct LearningModule;

impl LearningModule {
    fn workflows(&self, m: &mut ModuleBuilder) {
        m.workflow::<TrialReminders>()
            .workflow::<Renewal>()
            .workflow_event::<CourseCompleted>();
    }
}

// ---------------------------------------------------------------------------
// automations.mdx
// ---------------------------------------------------------------------------

/// A deal was won.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealWon {
    pub deal_id: Uuid,
    pub owner_id: Uuid,
    pub amount_cents: i64,
    pub workspace_id: Option<Uuid>,
}

impl Event for DealWon {
    const NAME: &'static str = "deal.won";
}

/// A task was completed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskCompleted {
    pub task_id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Option<Uuid>,
}

impl Event for TaskCompleted {
    const NAME: &'static str = "task.completed";
}

/// The settings of the "Deal won" trigger.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct DealWonTrigger {
    /// Only deals worth at least this much, in cents.
    #[serde(default)]
    pub min_amount_cents: Option<i64>,
}

/// A deal moved to won.
#[trigger(type = "deal.won", label = "Deal won", subject = "owner_id")]
fn deal_won(config: &DealWonTrigger, event: &DealWon) -> bool {
    config
        .min_amount_cents
        .is_none_or(|min| event.amount_cents >= min)
}

/// A task was completed.
#[trigger(type = "task.completed", label = "Task completed", event = TaskCompleted, subject = "owner_id")]
#[derive(Serialize, Deserialize, ToSchema)]
pub struct TaskCompletedTrigger {}

/// The deal is worth at least the amount.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct DealAmountAtLeast {
    /// The amount, in cents.
    pub amount_cents: i64,
}

/// The won deal is worth at least the amount.
#[condition(type = "deal.amount_at_least", label = "Deal amount")]
async fn deal_amount_at_least(
    config: &DealAmountAtLeast,
    ctx: &AutomationCtx,
) -> Result<bool, BoxError> {
    let deal: DealWon = ctx.event_as()?;
    Ok(deal.amount_cents >= config.amount_cents)
}

/// The settings of the "Create a task" action.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateTask {
    /// The task's title.
    pub title: String,
}

/// What the action returns: later nodes read it as `nodes.<id>.task_id`.
#[derive(Serialize)]
pub struct TaskCreated {
    pub task_id: Uuid,
}

/// Creates a task for the deal's owner.
#[action(type = "task.create", label = "Create a task", preview = create_task_preview)]
async fn create_task(config: CreateTask, ctx: AutomationCtx) -> Result<TaskCreated, BoxError> {
    let tasks = ctx.app().get::<Tasks>().await?;
    let deal: DealWon = ctx.event_as()?;
    let task_id = tasks
        .create(deal.owner_id, &config.title, &ctx.idempotency_key())
        .await?;
    Ok(TaskCreated { task_id })
}

/// What a simulation shows instead of creating the task.
fn create_task_preview(config: &CreateTask, ctx: &AutomationCtx) -> serde_json::Value {
    json!({ "title": config.title, "owner_id": ctx.subject() })
}

/// Thanks the owner, then a call for big deals or a survey for the others,
/// a day later.
fn big_deal_follow_up() -> serde_json::Value {
    json!({
        "trigger": {"type": "deal.won", "config": {}},
        "nodes": [
            {"id": "thank", "kind": "action", "type": "task.create",
             "config": {"title": "Thank the customer"}, "next": "a-day-later"},
            {"id": "a-day-later", "kind": "delay", "seconds": 86400, "next": "big-deal"},
            {"id": "big-deal", "kind": "condition", "type": "deal.amount_at_least",
             "config": {"amount_cents": 1000000}, "then": "call", "else": "survey"},
            {"id": "call", "kind": "action", "type": "task.create",
             "config": {"title": "Call the customer"}},
            {"id": "survey", "kind": "action", "type": "task.create",
             "config": {"title": "Send the survey"}}
        ]
    })
}

/// The CRM module's automation blocks and templates.
#[module(register = Self::automations)]
pub struct CrmAutomationsModule;

impl CrmAutomationsModule {
    fn automations(&self, m: &mut ModuleBuilder) {
        m.trigger::<DealWonTrigger>()
            .trigger::<TaskCompletedTrigger>()
            .condition::<DealAmountAtLeast>()
            .action::<CreateTask>()
            .automation_template(
                AutomationTemplate::new(
                    "big-deal-follow-up",
                    "Big deal follow-up",
                    big_deal_follow_up(),
                )
                .description("Thanks the owner, then a call or a survey a day later.")
                .category("Sales")
                .enrollment(EnrollmentPolicy::EveryTime),
            );
    }
}

// ---------------------------------------------------------------------------
// The test app
// ---------------------------------------------------------------------------

fn start() -> chrono::DateTime<chrono::Utc> {
    "2026-10-26T08:00:00Z".parse().expect("a time")
}

struct Harness {
    app: TestApp,
    clock: Arc<FixedClock>,
}

impl Harness {
    async fn workflows(&self) -> Arc<Workflows> {
        self.app.get::<Workflows>().await
    }

    async fn advance(&self, by: Duration) -> usize {
        self.clock.advance(by);
        self.workflows().await.tick().await.unwrap()
    }

    async fn emit<E: Event>(&self, event: E) {
        self.app.get::<EventBus>().await.emit(event).await.unwrap();
    }
}

fn builder(clock: Arc<FixedClock>) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("workflows.routes", false)
        .config("workflows.automation_routes", false)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(WorkflowsModule::from_config())
        .module(LearningModule)
        .module(CrmAutomationsModule)
        .override_value::<dyn Clock>(clock)
        .override_value(Arc::new(LearnerMail::default()))
        .override_value(Arc::new(Tasks::default()))
}

async fn harness(billing: Billing) -> Harness {
    let clock = Arc::new(FixedClock::new(start()));
    let app = builder(clock.clone())
        .override_value(Arc::new(billing))
        .build()
        .await;
    Harness { app, clock }
}

#[tokio::test]
async fn onboarding_waits_for_the_course_or_nudges() {
    let h = harness(Billing::default()).await;
    let workflows = h.workflows().await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let finishes = workflows
        .start(&uow, Onboarding { user_id: 7 })
        .await
        .unwrap();
    let drops_out = workflows
        .start_with(
            &uow,
            Onboarding { user_id: 8 },
            StartOptions::new().key("onboarding:8"),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(
        workflows.run(finishes).await.unwrap().status,
        RunStatus::Waiting
    );

    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 1,
    })
    .await;
    let run = workflows.run(finishes).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(true)));

    assert_eq!(h.advance(7 * DAY).await, 1);
    let run = workflows.run(drops_out).await.unwrap();
    assert_eq!(run.output, Some(json!(false)));
    let mail = h.app.get::<LearnerMail>().await;
    assert_eq!(
        mail.sent(),
        [
            (7, "welcome".to_owned()),
            (8, "welcome".to_owned()),
            (8, "nudge".to_owned())
        ]
    );
}

#[tokio::test]
async fn trial_reminders_sleep_until_their_days() {
    let h = harness(Billing::default()).await;
    let workflows = h.workflows().await;
    let id = workflows
        .spawn(TrialReminders {
            user_id: 3,
            ends_at: start() + chrono::TimeDelta::days(14),
        })
        .await
        .unwrap();
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    assert_eq!(run.wake_at, Some(start() + chrono::TimeDelta::days(11)));
    assert_eq!(h.advance(11 * DAY).await, 1);
    assert_eq!(h.advance(3 * DAY).await, 1);
    assert_eq!(
        workflows.run(id).await.unwrap().status,
        RunStatus::Completed
    );
    let sent: Vec<String> = h
        .app
        .get::<LearnerMail>()
        .await
        .sent()
        .into_iter()
        .map(|(_, template)| template)
        .collect();
    assert_eq!(sent, ["trial-3-days", "trial-ended"]);
}

#[tokio::test]
async fn a_renewal_retries_then_mails_instead_of_failing() {
    let h = harness(Billing {
        declines: 10,
        ..Billing::default()
    })
    .await;
    let workflows = h.workflows().await;
    let id = workflows.spawn(Renewal { account_id: 4 }).await.unwrap();
    // Attempts 2 to 5: 60, 120, 240 and 480 seconds apart.
    for wait in [60, 120, 240, 480] {
        assert_eq!(h.advance(Duration::from_secs(wait)).await, 1);
    }
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!("not charged: card declined")));

    let paid = harness(Billing {
        declines: 1,
        ..Billing::default()
    })
    .await;
    let workflows = paid.workflows().await;
    let id = workflows.spawn(Renewal { account_id: 4 }).await.unwrap();
    paid.advance(Duration::from_secs(60)).await;
    assert_eq!(
        workflows.run(id).await.unwrap().output,
        Some(json!("receipt-2"))
    );
}

/// What app code under test starts: the fake records it.
#[tokio::test]
async fn the_fake_records_starts() {
    let fake = WorkflowsFake::new();
    let app = TestApp::builder()
        .config("workflows.store", "memory")
        .config("workflows.routes", false)
        .config("workflows.automation_routes", false)
        .module(WorkflowsModule::from_config())
        .module(LearningModule)
        .override_value(Arc::new(fake.workflows()))
        .build()
        .await;
    let workflows = app.get::<Workflows>().await;
    workflows.spawn(Onboarding { user_id: 7 }).await.unwrap();
    fake.assert_started::<Onboarding>(|input| input.user_id == 7);
    fake.assert_started_times::<Onboarding>(1);
    fake.assert_not_started::<Renewal>();
}

#[tokio::test]
async fn the_follow_up_enrolls_won_deals() {
    let h = harness(Billing::default()).await;
    let automations = h.app.get::<Automations>().await;
    let catalog = automations.catalog();
    // The trigger as the catalog in automations.mdx shows it.
    assert_eq!(
        serde_json::to_value(&catalog.triggers()[0]).unwrap(),
        json!({
            "kind": "trigger",
            "type": "deal.won",
            "label": "Deal won",
            "description": "A deal moved to won.",
            "event": "deal.won",
            "subject": "owner_id",
            "schema": {
                "type": "object",
                "description": "The settings of the \"Deal won\" trigger.",
                "properties": {
                    "min_amount_cents": {
                        "type": ["integer", "null"],
                        "format": "int64",
                        "description": "Only deals worth at least this much, in cents."
                    }
                }
            }
        })
    );
    // Validation answers with JSON pointers.
    let mut broken = big_deal_follow_up();
    broken["nodes"][0]["type"] = json!("task.make");
    broken["nodes"][1]["seconds"] = json!(0);
    broken["nodes"][2]["config"] = json!({});
    broken["nodes"][4]["next"] = json!("thank");
    let issues: Vec<(String, String)> = automations
        .validate(&broken)
        .issues
        .into_iter()
        .map(|issue| (issue.path, issue.message))
        .collect();
    assert_eq!(
        issues,
        [
            (
                "/nodes/0/type".to_owned(),
                "unknown action `task.make`".to_owned()
            ),
            (
                "/nodes/1/seconds".to_owned(),
                "a delay lasts at least 1 second".to_owned()
            ),
            (
                "/nodes/2/config".to_owned(),
                "missing field `amount_cents`".to_owned()
            ),
            (
                "/nodes/4/next".to_owned(),
                "going to `thank` makes a loop: each node runs at most once".to_owned()
            ),
        ]
    );

    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let draft = automations
        .create_from_template(&uow, "big-deal-follow-up", None, None)
        .await
        .unwrap();
    let published = automations.publish(&uow, draft.id, None).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(published.version, Some(1));

    let owner = Uuid::now_v7();
    let deal = DealWon {
        deal_id: Uuid::now_v7(),
        owner_id: owner,
        amount_cents: 2_500_000,
        workspace_id: None,
    };
    let simulation = automations
        .simulate(
            draft.id,
            SimulateInput {
                event: serde_json::to_value(&deal).unwrap(),
                ..SimulateInput::default()
            },
        )
        .await
        .unwrap();
    let path: Vec<&str> = simulation.steps.iter().map(|s| s.node.as_str()).collect();
    assert_eq!(path, ["thank", "a-day-later", "big-deal", "call"]);
    assert_eq!(
        simulation.steps[0].detail,
        json!({"title": "Thank the customer", "owner_id": owner.to_string()})
    );

    h.emit(deal).await;
    h.advance(DAY).await;
    let tasks = h.app.get::<Tasks>().await;
    assert_eq!(tasks.titles(), ["Thank the customer", "Call the customer"]);

    // A definition with a wait for the owner's next task.
    let uow = db.uow(None, None).await;
    let chase = automations
        .create(
            &uow,
            NewAutomation::new(
                "Chase the next task",
                json!({
                    "trigger": {"type": "deal.won"},
                    "nodes": [
                        {"id": "next-task", "kind": "wait", "trigger": {"type": "task.completed"},
                         "timeout_seconds": 259200, "next": "thanks", "on_timeout": "chase"},
                        {"id": "thanks", "kind": "action", "type": "task.create",
                         "config": {"title": "Say thanks"}},
                        {"id": "chase", "kind": "action", "type": "task.create",
                         "config": {"title": "Chase the owner"}}
                    ]
                }),
            ),
        )
        .await
        .unwrap();
    automations.publish(&uow, chase.id, None).await.unwrap();
    uow.commit().await.unwrap();
    let second = Uuid::now_v7();
    h.emit(DealWon {
        deal_id: Uuid::now_v7(),
        owner_id: second,
        amount_cents: 1,
        workspace_id: None,
    })
    .await;
    h.emit(TaskCompleted {
        task_id: Uuid::now_v7(),
        owner_id: second,
        workspace_id: None,
    })
    .await;
    assert!(tasks.titles().contains(&"Say thanks".to_owned()));
    let enrollments = automations
        .enrollments(chase.id, PageQuery::default())
        .await
        .unwrap();
    assert_eq!(enrollments.data[0].status, RunStatus::Completed);
}
