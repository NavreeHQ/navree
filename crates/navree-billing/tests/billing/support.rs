//! Shared fixtures: a test app with auth, access, workspaces, a memory queue
//! and billing on the fake provider, with a fixed clock.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_auth::{AuthModule, AuthService, User};
use navree_billing::{
    Billing, BillingFake, BillingModule, InvoicePaid, PaymentFailed, Subscription,
    SubscriptionCanceled, SubscriptionCreated, SubscriptionUpdated, TrialEnding,
};
use navree_contracts::{Clock, FixedClock};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db};
use navree_events::{Event, EventBus, EventsModule, Listener, ModuleBuilderEventsExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_queue::{Queue, QueueModule};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_workspace::{Members, NewWorkspace, Workspace, WorkspaceModule, Workspaces};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

/// When every test app's clock starts.
pub fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap()
}

/// Registers workspaces as the billable.
pub struct WorkspaceBilling;

impl Module for WorkspaceBilling {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(BillingModule::for_feature::<Workspace>());
    }
}

/// Registers users as the billable.
pub struct UserBilling;

impl Module for UserBilling {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(BillingModule::for_feature::<User>());
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// The plans: `starter` (monthly only), `pro` (14-day trial, seats, a
/// meter) and `business`.
pub fn plans() -> Value {
    json!({
        "starter": { "name": "Starter", "prices": { "monthly": "price_starter_m" } },
        "pro": {
            "name": "Pro",
            "prices": { "monthly": "price_pro_m", "yearly": "price_pro_y" },
            "trial_days": 14,
            "seats": true,
            "meters": { "ai_summaries": "price_ai" }
        },
        "business": {
            "name": "Business",
            "prices": { "monthly": "price_business_m", "yearly": "price_business_y" }
        }
    })
}

/// The platform without a billable registration.
pub fn platform() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .fake_auth()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("queue.driver", "memory")
        .config("billing.provider", "fake")
        .config(
            "billing.return_url",
            "https://app.acme.test/settings/billing",
        )
        .config("billing.plans", plans())
        .config("workspace.roles.member.permissions", json!(["deals.*"]))
        .config(
            "workspace.roles.admin.permissions",
            json!(["billing.manage"]),
        )
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(WorkspaceModule::from_config())
        .module(BillingModule::from_config())
}

/// The platform billing workspaces, with the event log.
pub fn builder() -> TestAppBuilder {
    platform().module(WorkspaceBilling).module(EventLogModule)
}

/// Every billing event emitted, in order: (name, payload).
#[derive(Default)]
pub struct EventLog(Mutex<Vec<(String, Value)>>);

impl EventLog {
    /// The events so far, drained.
    pub fn take(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// The names of the events so far, drained.
    pub fn names(&self) -> Vec<String> {
        self.take().into_iter().map(|(name, _)| name).collect()
    }
}

/// A sync listener that appends to the [`EventLog`].
pub struct LogListener {
    log: Arc<EventLog>,
}

impl Injectable for LogListener {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(LogListener {
                log: r.get::<EventLog>().await?,
            })
        })
    }
}

impl<E: Event> Listener<E> for LogListener {
    fn handle<'a>(
        &'a self,
        event: &'a E,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let payload = serde_json::to_value(event)?;
            self.log
                .0
                .lock()
                .unwrap()
                .push((E::NAME.to_owned(), payload));
            Ok(())
        })
    }
}

/// Records every billing event in the [`EventLog`].
pub struct EventLogModule;

impl Module for EventLogModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(EventLog::default())
            .exports::<EventLog>()
            .listen::<SubscriptionCreated, LogListener>()
            .on::<SubscriptionUpdated, LogListener>()
            .on::<SubscriptionCanceled, LogListener>()
            .on::<TrialEnding, LogListener>()
            .on::<InvoicePaid, LogListener>()
            .on::<PaymentFailed, LogListener>();
    }
}

/// A booted test app with its clock.
pub struct World {
    pub app: TestApp,
    pub clock: Arc<FixedClock>,
}

impl World {
    pub async fn new() -> World {
        World::with(builder()).await
    }

    pub async fn with(builder: TestAppBuilder) -> World {
        let clock = Arc::new(FixedClock::new(start()));
        let app = builder
            .override_value::<dyn Clock>(clock.clone())
            .build()
            .await;
        World { app, clock }
    }

    pub async fn billing(&self) -> Arc<Billing> {
        self.app.get::<Billing>().await
    }

    pub async fn log(&self) -> Arc<EventLog> {
        self.app.get::<EventLog>().await
    }

    pub async fn fake(&self) -> Arc<BillingFake> {
        self.billing().await.fake().expect("the fake provider")
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    /// Creates a user.
    pub async fn user(&self, name: &str, email: &str) -> User {
        self.app
            .get::<AuthService>()
            .await
            .create_user(name, email, "password123")
            .await
            .unwrap()
    }

    /// Creates workspace `name` owned by `owner`.
    pub async fn workspace(&self, owner: Uuid, name: &str) -> Workspace {
        let workspaces = self.app.get::<Workspaces>().await;
        let uow = self.db().await.uow(None, None).await;
        let workspace = workspaces
            .create(&uow, owner, NewWorkspace::named(name))
            .await
            .unwrap();
        uow.commit().await.unwrap();
        workspace
    }

    /// Adds `user` to `workspace` with `role`.
    pub async fn join(&self, workspace: Uuid, user: Uuid, role: &str) {
        let members = self.app.get::<Members>().await;
        let uow = self.db().await.uow(None, None).await;
        members
            .add(&uow, workspace, user, role.into())
            .await
            .unwrap();
        uow.commit().await.unwrap();
    }

    /// Removes `user` from `workspace`.
    pub async fn leave(&self, workspace: Uuid, user: Uuid) {
        let members = self.app.get::<Members>().await;
        let uow = self.db().await.uow(None, None).await;
        members.remove(&uow, workspace, user).await.unwrap();
        uow.commit().await.unwrap();
    }

    /// Runs every queued job.
    pub async fn run_queue(&self) {
        self.app.get::<Queue>().await.run_pending().await.unwrap();
    }

    /// Checks out `plan` for `workspace` on the fake, completes it and runs
    /// the queue.
    pub async fn subscribe(&self, workspace: &Workspace, plan: &str) -> Subscription {
        let billing = self.billing().await;
        let checkout = billing.checkout(workspace, plan).create().await.unwrap();
        self.fake()
            .await
            .complete_checkout(&checkout.id)
            .await
            .unwrap();
        self.run_queue().await;
        billing.subscription(workspace).await.unwrap().unwrap()
    }

    /// Records the JSON payloads of events `names`.
    pub async fn events(&self, names: &[&str]) -> Events {
        let bus = self.app.get::<EventBus>().await;
        Events {
            receivers: names
                .iter()
                .map(|name| ((*name).to_owned(), bus.subscribe_json(name)))
                .collect(),
        }
    }

    pub fn as_user(&self, user: Uuid) -> TestClient {
        self.app.acting_as(principal(user))
    }

    /// Moves the clock forward by `days`.
    pub fn advance_days(&self, days: u64) {
        self.clock
            .advance(std::time::Duration::from_secs(days * 24 * 60 * 60));
    }
}

/// JSON event receivers.
pub struct Events {
    receivers: Vec<(String, broadcast::Receiver<Value>)>,
}

impl Events {
    /// Every event received so far, as (name, payload), drained, in the
    /// order of the names given.
    pub fn drain(&mut self) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        for (name, receiver) in &mut self.receivers {
            while let Ok(payload) = receiver.try_recv() {
                out.push((name.clone(), payload));
            }
        }
        out
    }

    /// The names of the events received so far, drained.
    pub fn names(&mut self) -> Vec<String> {
        self.drain().into_iter().map(|(name, _)| name).collect()
    }
}
