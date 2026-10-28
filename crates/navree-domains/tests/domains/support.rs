//! Helpers shared by the navree-domains tests: an app with the fakes, a
//! fixed clock, workspaces, an event inbox and a test controller.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use http::Method;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_contracts::{Clock, FixedClock, Principal};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::{DatabaseModule, Db};
use navree_domains::{
    AcmeClient, AcmeFake, CertificateProbe, CurrentDomain, CustomDomain, DnsFake, DnsLookup,
    DnsProvider, DnsProviderFake, Domains, DomainsModule, NewDomain, ProbeFake,
};
use navree_events::{EventBus, EventsModule};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_workspace::{CurrentWorkspace, NewWorkspace, WorkspaceModule, Workspaces};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

/// The target customers point their domains at.
pub const TARGET: &str = "domains.example.com";
/// The address of the app (and of the target).
pub const ADDRESS: &str = "203.0.113.10";
/// The verification record prefix.
pub const PREFIX: &str = "_domain-verification";

/// 2026-10-28T09:00:00Z.
pub fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 28, 9, 0, 0).unwrap()
}

/// Everyone may do what their permissions say (the workspace owner has
/// `all.*` inside the workspace).
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// A test app: database, crypto (an app key), events, access, workspaces
/// (routes off, no users table) and domains with a target and an address,
/// the test routes, a fixed clock and the fakes.
pub fn builder(world: &Fakes) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("app.url", "https://app.example.com")
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.routes", false)
        .config("domains.target", TARGET)
        .config("domains.addresses", json!([ADDRESS]))
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(WorkspaceModule::from_config())
        .module(DomainsModule::from_config())
        .module(TestRoutesModule)
        .override_value::<dyn DnsLookup>(Arc::new(world.dns.clone()))
        .override_value::<dyn AcmeClient>(Arc::new(world.ca.clone()))
        .override_value::<dyn CertificateProbe>(Arc::new(world.probe.clone()))
        .override_value::<dyn DnsProvider>(Arc::new(world.zone.clone()))
        .override_value::<dyn Clock>(world.clock.clone())
}

/// The fakes and the clock of a test app.
#[derive(Clone)]
pub struct Fakes {
    pub dns: DnsFake,
    pub ca: AcmeFake,
    pub probe: ProbeFake,
    pub zone: DnsProviderFake,
    pub clock: Arc<FixedClock>,
}

impl Fakes {
    pub fn new() -> Fakes {
        Fakes {
            dns: DnsFake::new().with_address(TARGET, ADDRESS),
            ca: AcmeFake::new(),
            probe: ProbeFake::new(),
            zone: DnsProviderFake::new(),
            clock: Arc::new(FixedClock::new(start())),
        }
    }
}

/// A booted app with its fakes.
pub struct World {
    pub app: TestApp,
    pub fakes: Fakes,
}

impl World {
    /// The default app, with `configure` on top.
    pub async fn with(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> World {
        let fakes = Fakes::new();
        let app = configure(builder(&fakes)).build().await;
        World { app, fakes }
    }

    /// The default app.
    pub async fn new() -> World {
        World::with(|builder| builder).await
    }

    pub async fn domains(&self) -> Arc<Domains> {
        self.app.get::<Domains>().await
    }

    pub async fn db(&self) -> Arc<Db> {
        self.app.get::<Db>().await
    }

    pub fn dns(&self) -> &DnsFake {
        &self.fakes.dns
    }

    pub fn advance(&self, by: Duration) {
        self.fakes.clock.advance(by);
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.fakes.clock.now()
    }

    /// A workspace owned by `owner`.
    pub async fn workspace(&self, owner: Uuid, name: &str) -> Uuid {
        let workspaces = self.app.get::<Workspaces>().await;
        let db = self.db().await;
        let uow = db.uow(None, None).await;
        let workspace = workspaces
            .create(&uow, owner, NewWorkspace::named(name))
            .await
            .unwrap();
        uow.commit().await.unwrap();
        workspace.id
    }

    /// Adds `host` to `workspace`.
    pub async fn add(&self, workspace: Uuid, host: &str) -> CustomDomain {
        self.domains()
            .await
            .add(workspace, NewDomain::new(host))
            .await
            .unwrap()
    }

    /// Creates the TXT record and the CNAME of `domain`.
    pub fn publish(&self, domain: &CustomDomain) {
        for record in &domain.dns_records {
            match record.kind.as_str() {
                "TXT" => self.fakes.dns.set_txt(&record.name, &record.value),
                "CNAME" => self.fakes.dns.set_cname(&record.name, &record.value),
                _ => self.fakes.dns.set_address(&record.name, &record.value),
            }
        }
    }

    /// Adds `host` to `workspace`, publishes its records and verifies it.
    pub async fn verified(&self, workspace: Uuid, host: &str) -> CustomDomain {
        let domain = self.add(workspace, host).await;
        self.publish(&domain);
        let domain = self.domains().await.verify(domain.id).await.unwrap();
        assert!(domain.is_verified(), "{domain:?}");
        domain
    }

    pub fn as_user(&self, user: Uuid) -> TestClient {
        self.app.acting_as(principal(user))
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
}

/// A signed-in user.
pub fn signed_in(user: Uuid) -> Principal {
    principal(user)
}

/// JSON event receivers.
pub struct Events {
    receivers: Vec<(String, broadcast::Receiver<Value>)>,
}

impl Events {
    /// Every event received so far, as (name, payload), drained.
    pub fn drain(&mut self) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        for (name, receiver) in &mut self.receivers {
            while let Ok(payload) = receiver.try_recv() {
                out.push((name.clone(), payload));
            }
        }
        out
    }

    /// The names received so far, drained.
    pub fn names(&mut self) -> Vec<String> {
        self.drain().into_iter().map(|(name, _)| name).collect()
    }
}

// -- test routes -----------------------------------------------------------------

/// `GET /landing` (public, `CurrentDomain`), `GET /maybe` (public,
/// `Option<CurrentDomain>`) and `GET /api/whoami` (signed in,
/// `CurrentWorkspace`).
pub struct TestRoutes;

impl Injectable for TestRoutes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(TestRoutes) })
    }
}

impl Controller for TestRoutes {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: std::sync::Arc<Self>) -> Vec<RouteDef> {
        let landing = |domain: CurrentDomain| async move { Json(domain) };
        let maybe =
            |domain: Option<CurrentDomain>| async move { Json(json!({ "domain": domain })) };
        let whoami = |workspace: CurrentWorkspace| async move {
            Ok::<_, AppError>(Json(json!({ "workspace": workspace.id() })))
        };
        vec![
            RouteDef::on(Method::GET, "!/landing", landing).public(),
            RouteDef::on(Method::GET, "!/maybe", maybe).public(),
            RouteDef::on(Method::GET, "/whoami", whoami).authenticated(),
        ]
    }
}

pub struct TestRoutesModule;

impl Module for TestRoutesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<TestRoutes>();
    }
}
