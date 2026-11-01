---
name: navree-workflows
description: "Durable workflows and end-user automations with navree-workflows: the `workflows` facade feature (in `full`), `WorkflowsModule` and the `workflows` config section (`store`, `lease_secs`, `queue`, `routes`, `automations`, `automation_routes`, `prune_after_days`), `#[workflow]` async functions on an input type (`name`, `version`, `min_version`), `WorkflowCtx` (`step`, `step_with` and `StepOptions` retries, `sleep`, `sleep_until`, `wait_for::<E>` with `EventMatch` and timeouts, `now`, `version`), `m.workflow::<W>()`, `WorkflowsModule::for_feature::<W>()`, `m.workflow_event::<E>()`, `Workflows::start`/`start_with`/`spawn` with `StartOptions` (key, workspace, delay), run statuses, replay rules, leases and takeovers, the `workflows.run` job and `workflows:tick`, versions, cancel and retry, `/api/workflows`, `workflows:*` commands, `WorkflowsFake`; automations: `#[trigger]` (`type`, `label`, `subject`, `workspace`, `event`), `#[condition]`, `#[action]` (`preview`), the `field` condition, `AutomationCtx`, `m.trigger/condition/action::<T>()`, `AutomationTemplate`, definitions (action, condition, delay and wait nodes), validation with JSON pointers, drafts, publish, versions, pause, enrollment policies, simulation, `/api/automations`. Use when a feature waits days or for an event (onboarding sequences, reminders, trials, dunning, follow-ups, approvals), when a multi-step process must survive restarts or retry steps, or when users build their own automations, rules or sequences in an editor; also to debug a run stuck in `waiting` or `running`, a step that ran twice, `UnregisteredEvent`, or an automation that does not enroll."
---

# Workflows and automations

`navree::workflows` (crate `navree-workflows`, facade feature `workflows`,
in `full`) runs durable workflows: `async` functions whose steps are stored
and replayed on resume, with durable sleeps and event waits. The same
engine runs end-user automations built from blocks you register. Everything
below is in the code: when in doubt read `crates/navree-workflows/src`, its
tests in `crates/navree-workflows/tests/workflows/` and
`crates/navree/tests/workflows_docs.rs` (the docs examples through the
facade). The docs pages are
`apps/website/content/docs/packages/workflows.mdx` and `automations.mdx`.

## Steps: add a workflow

1. The app has `WorkflowsModule::from_config()` (with `DatabaseModule`,
   `EventsModule`; `QueueModule` and `ScheduleModule` in production).
2. Write the input type and the function:

```rust
use std::time::Duration;

use navree::prelude::*;
use navree::workflows::{EventMatch, WorkflowCtx, workflow};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Onboarding {
    pub user_id: u64,
}

/// Welcomes a learner, nudges them after a week without a finished course.
#[workflow]                                   // name "onboarding", version 1
pub async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> Result<bool, BoxError> {
    let mail = ctx.app().get::<LearnerMail>().await?;
    let user_id = input.user_id;
    let welcome = mail.clone();
    ctx.step("send-welcome", || async move { welcome.send(user_id, "welcome").await })
        .await?;
    let done = ctx
        .wait_for::<CourseCompleted>(EventMatch::field("user_id", user_id), Duration::from_secs(7 * 86_400))
        .await?;
    if done.is_none() {
        ctx.step("send-nudge", || async move { mail.send(user_id, "nudge").await }).await?;
    }
    Ok(done.is_some())
}
```

3. Register it and the events it waits for, in the owning module:

```rust
#[module(imports = [WorkflowsModule::for_feature::<Onboarding>()], register = Self::workflows)]
pub struct LearningModule;

impl LearningModule {
    fn workflows(&self, m: &mut ModuleBuilder) {
        m.workflow_event::<CourseCompleted>();   // every event a run waits for
    }
}
```

4. Start runs in the unit of work that creates their reason:
   `workflows.start(&uow, Onboarding { user_id }).await?` (starts after the
   commit; a rollback leaves nothing). `start_with(&uow, input,
   StartOptions::new().key("onboarding:7").workspace(id).delay(d))`;
   `spawn(input)` uses the request's unit of work or starts at once.

## The replay rule

A run executes from the top on every wake-up. `step`, `sleep`,
`sleep_until`, `wait_for` and `now` calls are keyed by name and call count
(`send-reminder`, `send-reminder#2`); a stored result is returned instead of
running again. So:

- Anything with side effects or varying answers (DB reads, HTTP, random
  numbers, the time) goes inside a `step`. Read the time with
  `ctx.now().await`, never `Utc::now()`.
- Code between steps must decide the same way every time: branch on step
  results, the input, `ctx.now()`, `ctx.version()`.
- Steps run at least once: a step whose process dies runs again. Pass an
  idempotency key (`format!("{}:charge", ctx.run_id())`).
- Step names: 1 to 100 chars, no `#`, not starting with `$`.
- Retries: `step` gives 3 attempts (10 s, 20 s); `step_with(name,
  StepOptions::attempts(5).backoff(Backoff::Exponential { base_secs: 60,
  max_secs: 3_600 }), f)`. The run sleeps between attempts. After the last,
  `WorkflowsError::StepFailed { step, attempts, message }`: `?` fails the
  run; `match` it to compensate.

## Changing a workflow with runs under way

Runs resume on the version they started on. Either register the new code
on a new input type with `#[workflow(name = "onboarding", version = 2)]`
and keep the old type registered until its runs finish, or use
`#[workflow(name = "onboarding", version = 2, min_version = 1)]` and branch
on `ctx.version()`. Never rename steps or reorder calls of a version that
has runs waiting.

## Steps: add automation blocks

```rust
use navree::workflows::{AutomationCtx, action, condition, trigger};
use navree::openapi::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct DealWonTrigger {
    /// Only deals worth at least this much, in cents.
    #[serde(default)]
    pub min_amount_cents: Option<i64>,
}

/// A deal moved to won.
#[trigger(type = "deal.won", label = "Deal won", subject = "owner_id")]
fn deal_won(config: &DealWonTrigger, event: &DealWon) -> bool {
    config.min_amount_cents.is_none_or(|min| event.amount_cents >= min)
}

/// A task was completed (fires on every event: attribute on the struct).
#[trigger(type = "task.completed", event = TaskCompleted, subject = "owner_id")]
#[derive(Serialize, Deserialize, ToSchema)]
pub struct TaskCompletedTrigger {}

#[condition(type = "deal.amount_at_least", label = "Deal amount")]
async fn amount_at_least(config: &DealAmountAtLeast, ctx: &AutomationCtx) -> Result<bool, BoxError> {
    Ok(ctx.event_as::<DealWon>()?.amount_cents >= config.amount_cents)
}

#[action(type = "task.create", label = "Create a task", preview = task_preview)]
async fn create_task(config: CreateTask, ctx: AutomationCtx) -> Result<TaskCreated, BoxError> {
    let tasks = ctx.app().get::<Tasks>().await?;
    let deal: DealWon = ctx.event_as()?;
    let task_id = tasks.create(deal.owner_id, &config.title, &ctx.idempotency_key()).await?;
    Ok(TaskCreated { task_id })
}
```

Register: `m.trigger::<DealWonTrigger>().condition::<DealAmountAtLeast>()
.action::<CreateTask>().automation_template(AutomationTemplate::new(key,
name, definition).category("Sales"))`. Settings types derive `Serialize`,
`Deserialize` and `ToSchema` (the editor's form); doc comments become
descriptions.

Definitions are JSON: `{"trigger": {"type", "config"}, "nodes": [...]}`;
node kinds `action` (`type`, `config`, `next`), `condition` (`type`,
`config`, `then`, `else`; built-in `field` with `path`, `op`, `value`),
`delay` (`seconds`, `next`), `wait` (`trigger`, `same_subject`,
`timeout_seconds`, `next`, `on_timeout`). The first node runs first; no
loops; at most 200 nodes.

Service flow: `automations.create(&uow, NewAutomation::new(name,
definition).workspace(id).enrollment(EnrollmentPolicy::EveryTime))`,
`publish(&uow, id, Some(user))` (400 `validation_failed` with JSON
pointers), `pause`, `resume`, `update`, `delete`, `simulate(id,
SimulateInput { event, version, waits })`, `enroll(id, event_json)`. Reads
inside the unit of work see its writes.

## Testing

- Code that starts workflows: install `WorkflowsFake`
  (`.override_value(Arc::new(fake.workflows()))`), then
  `fake.assert_started::<Onboarding>(|i| i.user_id == 7)`.
- Real runs: `TestApp` with `temp_database()` (a SQLite file: the test build
  needs the `sqlite` feature in `[dev-dependencies]`), `DatabaseModule`,
  `EventsModule`, `WorkflowsModule`, your module, `workflows.routes = false`
  and `workflows.automation_routes = false` (unless you test them with an
  authorizer), and `.override_value::<dyn Clock>(clock.clone())` with a
  `FixedClock`. Without a queue, runs execute inline; then
  `clock.advance(d)` and `workflows.tick().await` wake timers and timeouts.
  Emit events with `EventBus` to resolve waits and enroll automations.
- Read `workflows.details(id).await?.steps` to see what ran.

## Checks before you finish

- [ ] Every workflow and every awaited event is registered
      (`for_feature`/`m.workflow`, `m.workflow_event`).
- [ ] Side effects live in steps with idempotency keys; no `Utc::now()` in
      workflow code.
- [ ] Steps are shorter than `workflows.lease_secs` (300 s).
- [ ] Runs are started in the request's unit of work, with a `key` where a
      second start must not create a second run.
- [ ] Production has `QueueModule` workers on `workflows.queue` and
      `ScheduleModule` running (`workflows:tick` every minute).
- [ ] The routes have an authorizer (`workflows.view`, `workflows.manage`,
      `automations.view`, `automations.manage`) or are turned off.
- [ ] Tests cover the happy path, a timeout and a failing step.

## Gotchas

- `UnregisteredEvent` from `wait_for`: add `m.workflow_event::<E>()`.
- `Unregistered` when starting: import `WorkflowsModule::for_feature::<W>()`.
- A run stays `sleeping` past its time without a queue: `workflows:tick`
  (ScheduleModule) wakes timers; with a queue they are delayed jobs.
- A run stays `running`: its worker died; `workflows:tick` takes it over
  after `lease_secs`.
- A run that never resumes after a deploy: its version is newer than the
  code (it waits) or older than every `min_version` (it fails).
- An event emitted before the wait began does not resolve it.
- `#[workflow]` errors must convert into `BoxError`: `navree::Result<T>`
  (whose error is `AppError`) does not; use `Result<T, BoxError>`.
- An automation that does not enroll: it is `draft` or `paused`, the event
  belongs to another workspace, the trigger's settings reject it, or the
  policy (`one_at_a_time`, `once`) blocks the subject. `simulate` gives the
  reason.
