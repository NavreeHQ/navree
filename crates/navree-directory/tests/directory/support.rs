//! Helpers shared by the navree-directory tests: app builders with the fake
//! directory, a recording sink, an event inbox and a fixed clock.

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, MANAGE, Rules};
use navree_contracts::{AuthVia, Clock, FixedClock, Principal};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::DatabaseModule;
use navree_directory::{
    ChangeAction, CreatedConnection, Directory, DirectoryFake, DirectoryModule, DirectorySink,
    DirectorySyncFinished, DirectoryUserCreated, DirectoryUserDeactivated, DirectoryUserUpdated,
    GroupChange, ModuleBuilderDirectoryExt, NewConnection, Owner, SinkContext, UserChange,
};
use navree_events::{Event, EventsModule, Listener, ModuleBuilderEventsExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::Value;
use uuid::Uuid;

/// An admin who manages directory connections app-wide.
pub const ADMIN: Uuid = Uuid::from_u128(1);
/// A user without the permission.
pub const OTHER: Uuid = Uuid::from_u128(2);

/// 2026-10-28T08:00:00Z.
pub fn start() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-28T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

pub fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

// -- the recording sink ----------------------------------------------------------

/// What the sink was told, in order, and the users it refuses.
#[derive(Default)]
pub struct SinkLog {
    calls: Mutex<Vec<String>>,
    refuse: Mutex<HashSet<String>>,
}

impl SinkLog {
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn clear(&self) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// The sink fails for this user's external id.
    pub fn refuse(&self, external_id: &str) {
        self.refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(external_id.to_owned());
    }

    pub fn accept_all(&self) {
        self.refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn push(&self, call: String) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
    }

    fn refuses(&self, external_id: &str) -> bool {
        self.refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(external_id)
    }
}

/// Records every change; returns `local-<external id>` as the local id.
pub struct RecordingSink(Arc<SinkLog>);

impl Injectable for RecordingSink {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(RecordingSink(r.get::<SinkLog>().await?)) })
    }
}

impl DirectorySink for RecordingSink {
    fn upsert_user<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a UserChange,
    ) -> BoxFuture<'a, Result<Option<String>, BoxError>> {
        Box::pin(async move {
            if self.0.refuses(&change.user.external_id) {
                return Err(format!("refusing {}", change.user.external_id).into());
            }
            // The unit of work is the change's own.
            assert!(!ctx.uow().is_finished().await);
            self.0.push(format!(
                "user:{}:{}{}",
                change.action.as_str(),
                change.user.external_id,
                if change.user.active {
                    ""
                } else {
                    " (inactive)"
                }
            ));
            Ok(Some(format!("local-{}", change.user.external_id)))
        })
    }

    fn deactivate_user<'a>(
        &'a self,
        _ctx: &'a SinkContext,
        change: &'a UserChange,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if self.0.refuses(&change.user.external_id) {
                return Err(format!("refusing {}", change.user.external_id).into());
            }
            assert_eq!(change.action, ChangeAction::Deactivate);
            self.0.push(format!(
                "user:deactivate:{}{}",
                change.user.external_id,
                if change.deleted { " (deleted)" } else { "" }
            ));
            Ok(())
        })
    }

    fn upsert_group<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a GroupChange,
    ) -> BoxFuture<'a, Result<Option<String>, BoxError>> {
        Box::pin(async move {
            let locals = ctx.local_ids(&change.added).await?;
            let mut added: Vec<String> = change
                .added
                .iter()
                .map(|id| locals.get(id).cloned().unwrap_or_else(|| format!("?{id}")))
                .collect();
            added.sort();
            self.0.push(format!(
                "group:{}:{}:+{}-{}",
                change.action.as_str(),
                change.group.external_id,
                added.join(","),
                change.removed.join(",")
            ));
            Ok(Some(format!("team-{}", change.group.external_id)))
        })
    }

    fn delete_group<'a>(
        &'a self,
        _ctx: &'a SinkContext,
        change: &'a GroupChange,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.0
                .push(format!("group:delete:{}", change.group.external_id));
            Ok(())
        })
    }
}

/// Registers the recording sink.
pub struct SinkModule;

impl Module for SinkModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.directory_sink::<RecordingSink>();
    }
}

// -- the event inbox -----------------------------------------------------------

/// The events the directory module emitted, by name, with payloads.
#[derive(Default)]
pub struct Inbox(Mutex<Vec<(String, Value)>>);

impl Inbox {
    pub fn names(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn of<E: Event>(&self) -> Vec<E> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(name, _)| name == E::NAME)
            .map(|(_, payload)| serde_json::from_value(payload.clone()).unwrap())
            .collect()
    }

    pub fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

struct Collector(Arc<Inbox>);

impl Injectable for Collector {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Collector(r.get::<Inbox>().await?)) })
    }
}

macro_rules! collect {
    ($($event:ty),*) => {$(
        impl Listener<$event> for Collector {
            fn handle<'a>(
                &'a self,
                event: &'a $event,
                _ctx: &'a AppContext,
            ) -> BoxFuture<'a, Result<(), BoxError>> {
                self.0
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((<$event as Event>::NAME.to_owned(), serde_json::to_value(event).unwrap()));
                Box::pin(async { Ok(()) })
            }
        }
    )*};
}

collect!(
    DirectoryUserCreated,
    DirectoryUserUpdated,
    DirectoryUserDeactivated,
    DirectorySyncFinished
);

/// Subscribes the collector to the module's events.
pub struct InboxModule;

impl Module for InboxModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<DirectoryUserCreated, Collector>()
            .on::<DirectoryUserUpdated, Collector>()
            .on::<DirectoryUserDeactivated, Collector>()
            .on::<DirectorySyncFinished, Collector>();
    }
}

// -- apps ----------------------------------------------------------------------

/// The admin manages directory connections app-wide.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.as_ref().is_some_and(|user| user.id == ADMIN) {
        rules.can(MANAGE, "directory");
    }
}

/// A test app: database, crypto, events (recorded in the inbox), a fixed
/// clock, the directory module from configuration (admin routes off), the
/// recording sink.
pub fn builder(clock: Arc<FixedClock>, inbox: Arc<Inbox>, log: Arc<SinkLog>) -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("directory.routes", false)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(DirectoryModule::from_config())
        .module(SinkModule)
        .module(InboxModule)
        .override_value(inbox)
        .override_value(log)
        .override_value::<dyn Clock>(clock)
}

/// A booted app.
pub struct Harness {
    pub app: TestApp,
    pub fake: Option<DirectoryFake>,
    pub clock: Arc<FixedClock>,
    pub inbox: Arc<Inbox>,
    pub log: Arc<SinkLog>,
}

impl Harness {
    pub async fn directory(&self) -> Arc<Directory> {
        self.app.get::<Directory>().await
    }

    pub fn fake(&self) -> &DirectoryFake {
        self.fake.as_ref().expect("a harness with the fake")
    }

    /// A pull connection of the app reading the fake.
    pub async fn connection(&self, source: &str) -> Uuid {
        self.directory()
            .await
            .create_connection(Owner::App, NewConnection::new("Acme directory", source))
            .await
            .unwrap()
            .connection
            .id
    }

    /// A SCIM connection of `owner`.
    pub async fn scim(&self, owner: Owner) -> CreatedConnection {
        self.directory()
            .await
            .create_connection(owner, NewConnection::new("Acme Entra ID", "scim"))
            .await
            .unwrap()
    }
}

/// A harness with the fake directory; `configure` adds configuration and
/// modules.
pub async fn harness(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> Harness {
    let (directory, fake) = Directory::fake();
    let clock = Arc::new(FixedClock::new(start()));
    let inbox = Arc::new(Inbox::default());
    let log = Arc::new(SinkLog::default());
    let app = configure(builder(clock.clone(), inbox.clone(), log.clone()))
        .override_value(Arc::new(directory))
        .build()
        .await;
    Harness {
        app,
        fake: Some(fake),
        clock,
        inbox,
        log,
    }
}

/// A harness with the real sources.
pub async fn real(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> Harness {
    let clock = Arc::new(FixedClock::new(start()));
    let inbox = Arc::new(Inbox::default());
    let log = Arc::new(SinkLog::default());
    let app = configure(builder(clock.clone(), inbox.clone(), log.clone()))
        .build()
        .await;
    Harness {
        app,
        fake: None,
        clock,
        inbox,
        log,
    }
}

/// Admin routes on, with AccessModule (ADMIN manages the directory).
pub fn with_routes(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("directory.routes", true)
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
}
