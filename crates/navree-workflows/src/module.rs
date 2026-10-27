//! [`WorkflowsModule`] and [`ModuleBuilderWorkflowsExt`].

use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{Clock, ModuleBuilderJobsExt, SystemClock};
use navree_db::{Db, ModuleBuilderDbExt, Uow};
use navree_events::{Event, Listener, ModuleBuilderEventsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_workspace::events::WorkspaceDeleted;

use crate::automation::{
    Action, AutomationQuery, AutomationRun, AutomationStore, AutomationTemplate, Automations,
    AutomationsController, BlockRegistration, Catalog, Condition, DatabaseAutomationStore,
    FieldCondition, MemoryAutomationStore, TemplateRegistry, Trigger,
};
use crate::commands::commands;
use crate::controller::WorkflowsController;
use crate::engine::Engine;
use crate::error::map_workflows_error;
use crate::jobs::RunWorkflow;
use crate::migration::migrations;
use crate::store::{DatabaseWorkflowStore, MemoryWorkflowStore, WorkflowStore};
use crate::workflow::WorkflowRegistration;
use crate::{
    RunQuery, RunStatus, Workflow, WorkflowDefinition, WorkflowRegistry, WorkflowStoreKind,
    Workflows, WorkflowsError, WorkflowsOptions,
};

/// The name of the scheduled task that wakes due runs every minute.
pub const TICK_TASK: &str = "workflows:tick";

/// The name of the scheduled task that prunes finished runs daily.
pub const PRUNE_TASK: &str = "workflows:prune";

/// Durable workflows in code and end-user automations.
///
/// Global; provides and exports [`Workflows`], [`Automations`] and
/// [`WorkflowsOptions`], and contributes:
///
/// - the `workflows` migrations (`workflow_runs`, `workflow_steps`,
///   `automations`, `automation_versions`, `automation_enrollments`);
/// - the job `workflows.run` ([`RunWorkflow`]);
/// - the scheduled tasks `workflows:tick` (every minute: timers, timeouts,
///   executions whose worker died) and `workflows:prune` (daily at 03:40),
///   in apps with `ScheduleModule`;
/// - the routes under `{http.api_prefix}/workflows` (`workflows.routes`)
///   and `{http.api_prefix}/automations` (`workflows.automation_routes`;
///   needs `DatabaseModule`);
/// - the commands `workflows:list`, `workflows:show`, `workflows:cancel`,
///   `workflows:retry`, `workflows:tick`, `workflows:prune`,
///   `automations:list` and `automations:catalog`;
/// - the built-in condition `field`, and the `automation` workflow that
///   carries enrollments;
/// - a listener that cancels a deleted workspace's runs and deletes its
///   automations.
///
/// With `workflows.store = "database"` (the default) it needs
/// `DatabaseModule`. Event waits and triggers need `EventsModule`; a queue
/// (`QueueModule`) runs executions on workers, otherwise they run inline.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_events::EventsModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_queue::QueueModule;
/// use navree_schedule::ScheduleModule;
/// use navree_workflows::WorkflowsModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(EventsModule::for_root())
///     .module(QueueModule::from_config())
///     .module(ScheduleModule::for_root())
///     .module(WorkflowsModule::from_config()) // section `workflows`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkflowsModule;

impl WorkflowsModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: WorkflowsOptions) -> impl Module {
        ConfiguredWorkflowsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `workflows` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredWorkflowsModule { options: None }
    }

    /// Registers workflow `W`, for the module that owns it
    /// (`imports = [WorkflowsModule::for_feature::<Onboarding>()]`).
    pub fn for_feature<W: Workflow>() -> impl Module {
        WorkflowFeature::<W>(PhantomData)
    }
}

impl Module for WorkflowsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `WorkflowsModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredWorkflowsModule {
    /// `None`: read from configuration.
    options: Option<WorkflowsOptions>,
}

impl Module for ConfiguredWorkflowsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<WorkflowsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<WorkflowsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<WorkflowsOptions>::new(move |_| {
            let options = options.clone();
            async move {
                navree_config::ConfigSection::validate(&options)
                    .map_err(|message| format!("invalid workflows options: {message}"))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<WorkflowsOptions>(),
    };
    m.exports::<WorkflowsOptions>()
        .factory(
            Factory::<Engine>::new(|r: Resolver| async move { build_engine(r).await })
                .deps(vec![ProviderKey::of::<WorkflowsOptions>()]),
        )
        .factory(
            Factory::<Workflows>::new(|r: Resolver| async move {
                Ok(Arc::new(Workflows::new(r.get::<Engine>().await?)))
            })
            .deps(vec![ProviderKey::of::<Engine>()]),
        )
        .exports::<Workflows>()
        .factory(
            Factory::<Automations>::new(|r: Resolver| async move {
                Ok(Arc::new(Automations::new(r.get::<Engine>().await?)))
            })
            .deps(vec![ProviderKey::of::<Engine>()]),
        )
        .exports::<Automations>()
        .job::<RunWorkflow>()
        .controller::<WorkflowsController>()
        .controller::<AutomationsController>()
        .migrations(migrations())
        .map_error::<WorkflowsError>(map_workflows_error)
        .listen::<WorkspaceDeleted, DropWorkspace>()
        .contribute(WorkflowRegistration(
            WorkflowDefinition::of::<AutomationRun>(),
        ))
        .contribute(BlockRegistration::condition::<FieldCondition>());
    // Runs only in apps with `ScheduleModule`; inert otherwise.
    m.schedule(|s| {
        s.task(TICK_TASK, tick_task)
            .every_minute()
            .without_overlapping();
        s.task(PRUNE_TASK, prune_task).daily_at("03:40");
    });
    for command in commands() {
        m.command(command);
    }
}

/// Builds the engine from the options and every module's registrations.
async fn build_engine(r: Resolver) -> Result<Arc<Engine>, BoxError> {
    let app = r.app();
    let options = r.get::<WorkflowsOptions>().await?;
    let db = app.try_get::<Db>().await.map(|db| (*db).clone());
    let (store, automations): (Arc<dyn WorkflowStore>, Arc<dyn AutomationStore>) =
        match options.store {
            WorkflowStoreKind::Database => {
                let db = db.clone().ok_or(
                    "workflows.store = \"database\" needs DatabaseModule: add it to the app, or \
                     set workflows.store = \"memory\"",
                )?;
                (
                    Arc::new(DatabaseWorkflowStore::new(db.clone())),
                    Arc::new(DatabaseAutomationStore::new(db)),
                )
            }
            WorkflowStoreKind::Memory => (
                Arc::new(MemoryWorkflowStore::new()),
                Arc::new(MemoryAutomationStore::new()),
            ),
        };
    let clock: Arc<dyn Clock> = match app.try_get::<dyn Clock>().await {
        Some(clock) => clock,
        None => Arc::new(SystemClock),
    };
    let registry = WorkflowRegistry::new(
        app.contributions::<WorkflowRegistration>()
            .into_iter()
            .map(|c| c.value.0.clone()),
    )?;
    let catalog = Catalog::new(
        app.contributions::<BlockRegistration>()
            .into_iter()
            .map(|c| c.value),
    )?;
    let templates = TemplateRegistry::new(
        app.contributions::<AutomationTemplate>()
            .into_iter()
            .map(|c| (*c.value).clone()),
        &catalog,
    )?;
    let mut events: BTreeSet<String> = app
        .contributions::<WorkflowEventRegistration>()
        .into_iter()
        .map(|c| c.value.name.to_owned())
        .collect();
    events.extend(catalog.trigger_events().map(str::to_owned));
    Ok(Arc::new(Engine::new(
        &app,
        store,
        automations,
        clock,
        (*options).clone(),
        registry,
        catalog,
        templates,
        events,
        if options.store == WorkflowStoreKind::Database {
            db
        } else {
            None
        },
    )))
}

/// One scheduled `workflows:tick`.
async fn tick_task(app: AppContext) -> Result<(), BoxError> {
    let woken = app.get::<Workflows>().await?.tick().await?;
    if woken > 0 {
        tracing::debug!(target: "navree::workflows", woken, "workflows:tick woke due runs");
    }
    Ok(())
}

/// One scheduled `workflows:prune` (nothing when `prune_after_days` is 0).
async fn prune_task(app: AppContext) -> Result<(), BoxError> {
    let options = app.get::<WorkflowsOptions>().await?;
    if options.prune_after_days == 0 {
        return Ok(());
    }
    let workflows = app.get::<Workflows>().await?;
    let before = workflows.now() - chrono::TimeDelta::days(i64::from(options.prune_after_days));
    let pruned = workflows.prune(before).await?;
    if pruned > 0 {
        tracing::info!(target: "navree::workflows", pruned, "pruned finished workflow runs");
    }
    Ok(())
}

/// `WorkflowsModule::for_feature::<W>()`.
struct WorkflowFeature<W>(PhantomData<fn() -> W>);

impl<W: Workflow> Module for WorkflowFeature<W> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workflow::<W>();
    }
}

/// An event name workflows may wait for (from `m.workflow_event::<E>()`).
pub(crate) struct WorkflowEventRegistration {
    pub(crate) name: &'static str,
}

/// Subscribes the workflow engine to event `E`, once per app however many
/// modules ask (its module id is per event type).
struct WorkflowEventModule<E>(PhantomData<fn() -> E>);

impl<E: Event> Module for WorkflowEventModule<E> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<E, DeliverToWorkflows<E>>()
            .contribute(WorkflowEventRegistration { name: E::NAME });
    }
}

/// Hands every `E` to [`Workflows::deliver`]: open waits and automation
/// triggers.
struct DeliverToWorkflows<E>(PhantomData<fn() -> E>);

impl<E: Event> Injectable for DeliverToWorkflows<E> {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DeliverToWorkflows(PhantomData)) })
    }
}

impl<E: Event> Listener<E> for DeliverToWorkflows<E> {
    fn handle<'a>(
        &'a self,
        event: &'a E,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let Some(workflows) = ctx.try_get::<Workflows>().await else {
                tracing::debug!(
                    target: "navree::workflows",
                    event = E::NAME,
                    "WorkflowsModule is not in the app: the event reaches no workflow"
                );
                return Ok(());
            };
            let payload = serde_json::to_value(event)?;
            // Emitted inside a unit of work (a handler's): the event counts
            // once that work commits, and never if it rolls back. Waits and
            // automations then see the records the work wrote.
            if let Some(uow) = Uow::current()
                && !uow.is_finished().await
            {
                uow.after_commit(move || {
                    Box::pin(async move {
                        if let Err(error) = workflows.deliver(E::NAME, &payload).await {
                            tracing::error!(
                                target: "navree::workflows",
                                event = E::NAME,
                                %error,
                                "cannot deliver an event to workflows and automations"
                            );
                        }
                    })
                });
                return Ok(());
            }
            workflows.deliver(E::NAME, &payload).await?;
            Ok(())
        })
    }
}

/// Workflow and automation registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderWorkflowsExt {
    /// Registers workflow `W` (what [`WorkflowsModule::for_feature`] does,
    /// for a module's `register`).
    fn workflow<W: Workflow>(&mut self) -> &mut Self;

    /// Lets workflows wait for event `E` (`ctx.wait_for::<E>(..)`): every
    /// emitted `E` is checked against the open waits.
    fn workflow_event<E: Event>(&mut self) -> &mut Self;

    /// Registers trigger `T` for automations (and for their `wait` nodes);
    /// its event reaches the engine from now on.
    fn trigger<T: Trigger>(&mut self) -> &mut Self;

    /// Registers condition `C` for automations.
    fn condition<C: Condition>(&mut self) -> &mut Self;

    /// Registers action `A` for automations.
    fn action<A: Action>(&mut self) -> &mut Self;

    /// Registers a ready-made automation (checked against the catalog at
    /// boot).
    fn automation_template(&mut self, template: AutomationTemplate) -> &mut Self;
}

impl ModuleBuilderWorkflowsExt for ModuleBuilder {
    fn workflow<W: Workflow>(&mut self) -> &mut Self {
        self.contribute(WorkflowRegistration(WorkflowDefinition::of::<W>()))
    }

    fn workflow_event<E: Event>(&mut self) -> &mut Self {
        self.import(WorkflowEventModule::<E>(PhantomData))
    }

    fn trigger<T: Trigger>(&mut self) -> &mut Self {
        self.import(WorkflowEventModule::<T::Event>(PhantomData))
            .contribute(BlockRegistration::trigger::<T>())
    }

    fn condition<C: Condition>(&mut self) -> &mut Self {
        self.contribute(BlockRegistration::condition::<C>())
    }

    fn action<A: Action>(&mut self) -> &mut Self {
        self.contribute(BlockRegistration::action::<A>())
    }

    fn automation_template(&mut self, template: AutomationTemplate) -> &mut Self {
        self.contribute(template)
    }
}

/// Cancels a deleted workspace's unfinished runs and deletes its
/// automations.
struct DropWorkspace;

impl Injectable for DropWorkspace {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DropWorkspace) })
    }
}

impl Listener<WorkspaceDeleted> for DropWorkspace {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let Some(workflows) = ctx.try_get::<Workflows>().await else {
                return Ok(());
            };
            let unfinished = RunQuery {
                workspace_id: Some(event.workspace_id),
                statuses: RunStatus::ALL
                    .into_iter()
                    .filter(|s| !s.is_finished())
                    .collect(),
                limit: 500,
                ..RunQuery::default()
            };
            loop {
                let runs = workflows.runs(&unfinished).await?;
                let mut cancelled = 0;
                for run in &runs {
                    // A run that finished meanwhile is fine.
                    if workflows.cancel(run.id).await.is_ok() {
                        cancelled += 1;
                    }
                }
                if cancelled == 0 {
                    break;
                }
            }
            if let Some(automations) = ctx.try_get::<Automations>().await
                && let Some(db) = ctx.try_get::<Db>().await
            {
                let query = AutomationQuery {
                    scope: Some(Some(event.workspace_id)),
                    limit: 500,
                    ..AutomationQuery::default()
                };
                loop {
                    let list = automations.list(&query).await?;
                    if list.is_empty() {
                        break;
                    }
                    let uow = db.uow(None, None).await;
                    for automation in list {
                        automations.delete(&uow, automation.id).await?;
                    }
                    uow.commit().await?;
                }
            }
            Ok(())
        })
    }
}
