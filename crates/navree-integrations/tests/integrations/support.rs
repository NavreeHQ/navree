//! Helpers shared by the navree-integrations tests: app builders with the
//! fake and with real providers on wiremock, principals, workspaces, an
//! event inbox and a fixed clock.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, MANAGE, Rules};
use navree_contracts::{AuthVia, Clock, FixedClock, Principal};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::{DatabaseModule, Db};
use navree_events::{Event, EventsModule, Listener, ModuleBuilderEventsExt};
use navree_http_client::{HttpClientModule, HttpClientOptions, SsrfPolicy};
use navree_integrations::{
    ConnectionRestored, ConnectionUnhealthy, IntegrationConnected, IntegrationDisconnected,
    Integrations, IntegrationsFake, IntegrationsModule, Owner, ServiceAccountKey,
};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_workspace::{
    NewOrganization, NewWorkspace, Organizations, Workspace, WorkspaceModule, Workspaces,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// The front end's callback page.
pub const REDIRECT_BASE: &str = "https://app.acme.test/settings/integrations/callback";

/// An admin who manages integrations app-wide.
pub const ADMIN: Uuid = Uuid::from_u128(1);
/// A user without the permission (except in workspaces they own).
pub const OTHER: Uuid = Uuid::from_u128(2);
/// A second admin.
pub const DEPUTY: Uuid = Uuid::from_u128(3);

/// The RSA key of the fixture service account.
pub const PEM: &str = include_str!("../fixtures/rsa-2048.pem");

/// 2026-10-27T08:00:00Z.
pub fn start() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-27T08:00:00Z")
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

/// The fixture service-account key file.
pub fn service_account_json() -> Value {
    json!({
        "type": "service_account",
        "project_id": "acme-prod",
        "private_key_id": "4f2a9c",
        "private_key": PEM,
        "client_email": "navree@acme-prod.iam.gserviceaccount.com",
        "client_id": "109318734729811",
        "token_uri": "https://oauth2.googleapis.com/token"
    })
}

pub fn service_account() -> ServiceAccountKey {
    ServiceAccountKey::from_value(&service_account_json()).unwrap()
}

/// The events the integrations module emitted, by name, with payloads.
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
    IntegrationConnected,
    IntegrationDisconnected,
    ConnectionUnhealthy,
    ConnectionRestored
);

/// Subscribes the collector to the module's four events.
pub struct InboxModule;

impl Module for InboxModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<IntegrationConnected, Collector>()
            .on::<IntegrationDisconnected, Collector>()
            .on::<ConnectionUnhealthy, Collector>()
            .on::<ConnectionRestored, Collector>();
    }
}

/// The admin manages integrations app-wide; everyone else has only the
/// permissions of their roles (in a workspace: their workspace role's).
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx
        .user
        .as_ref()
        .is_some_and(|user| user.id == ADMIN || user.id == DEPUTY)
    {
        rules.can(MANAGE, "integrations");
    }
}

/// The providers every test app configures (endpoints point at the real
/// providers; [`real`] points them at a mock server).
fn providers(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("integrations.redirect_base", REDIRECT_BASE)
        .config(
            "integrations.providers.microsoft",
            json!({
                "client_id": "0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f",
                "client_secret": "ms-secret",
                "apps": { "mail_send": { "client_id": "ms-mail-app", "client_secret": "ms-mail-secret" } }
            }),
        )
        .config(
            "integrations.providers.google",
            json!({
                "scopes": ["https://www.googleapis.com/auth/admin.directory.user.readonly"]
            }),
        )
        .config(
            "integrations.providers.slack",
            json!({
                "client_id": "4129031.6713",
                "client_secret": "slack-secret",
                "scopes": ["chat:write", "users:read"]
            }),
        )
        .config(
            "integrations.providers.hubspot",
            json!({
                "type": "oauth2",
                "name": "HubSpot",
                "client_id": "hub-client",
                "client_secret": "hub-secret",
                "authorize_url": "https://app.hubspot.com/oauth/authorize",
                "token_url": "https://api.hubapi.com/oauth/v1/token",
                "revoke_url": "https://api.hubapi.com/oauth/v1/revoke",
                "api_url": "https://api.hubapi.com",
                "scopes": ["crm.objects.contacts.read"]
            }),
        )
        .config(
            "integrations.providers.edr",
            json!({
                "type": "api_key",
                "name": "Sentinel EDR",
                "header": "X-Api-Key",
                "scheme": "",
                "health_url": "/v1/health"
            }),
        )
}

/// A test app: database, crypto, events (recorded in the inbox), a fixed
/// clock, the integrations module from configuration (routes off) and
/// `scope_sets`.
pub fn builder(clock: Arc<FixedClock>, inbox: Arc<Inbox>) -> TestAppBuilder {
    providers(
        TestApp::builder()
            .temp_database()
            .config("app.key", generate_key())
            .config("http.per_ip_rate_limit", Value::Null)
            .config("integrations.routes", false),
    )
    .module(CryptoModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(IntegrationsModule::from_config())
    .module(crate::scopes::FeatureModule)
    .module(InboxModule)
    .override_value(inbox)
    .override_value::<dyn Clock>(clock)
}

/// A booted app with its fake providers.
pub struct Harness {
    pub app: TestApp,
    pub fake: IntegrationsFake,
    pub clock: Arc<FixedClock>,
    pub inbox: Arc<Inbox>,
}

impl Harness {
    pub async fn integrations(&self) -> Arc<Integrations> {
        self.app.get::<Integrations>().await
    }

    pub fn advance(&self, by: Duration) {
        self.clock.advance(by);
    }

    pub async fn workspace(&self, owner: Uuid, slug: &str) -> Workspace {
        let workspaces = self.app.get::<Workspaces>().await;
        let uow = self.app.get::<Db>().await.uow(None, None).await;
        let workspace = workspaces
            .create(
                &uow,
                owner,
                NewWorkspace {
                    name: slug.to_owned(),
                    slug: Some(slug.to_owned()),
                },
            )
            .await
            .unwrap();
        uow.commit().await.unwrap();
        workspace
    }

    /// An organization owned by `owner` with `workspaces` attached.
    pub async fn organization(&self, owner: Uuid, slug: &str, workspaces: &[Uuid]) -> Uuid {
        let organizations = self.app.get::<Organizations>().await;
        let uow = self.app.get::<Db>().await.uow(None, None).await;
        let organization = organizations
            .create(
                &uow,
                owner,
                NewOrganization {
                    name: slug.to_owned(),
                    slug: Some(slug.to_owned()),
                    ..NewOrganization::default()
                },
            )
            .await
            .unwrap();
        for workspace in workspaces {
            organizations
                .attach(&uow, organization.id, *workspace)
                .await
                .unwrap();
        }
        uow.commit().await.unwrap();
        organization.id
    }

    /// Connects `owner` to Microsoft 365 through admin consent.
    pub async fn connect_microsoft(
        &self,
        owner: Owner,
        tenant: &str,
    ) -> navree_integrations::ConnectionInfo {
        let integrations = self.integrations().await;
        let redirect = integrations
            .connect_url(owner, "microsoft", Default::default())
            .await
            .unwrap();
        integrations
            .complete(
                owner,
                "microsoft",
                &navree_integrations::ConnectCallback::admin_consent(redirect.state, tenant),
                None,
            )
            .await
            .unwrap()
            .connection
    }
}

/// A harness with fake providers; `configure` adds configuration and
/// modules.
pub async fn harness(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> Harness {
    let (integrations, fake) = Integrations::fake();
    let clock = Arc::new(FixedClock::new(start()));
    let inbox = Arc::new(Inbox::default());
    let app = configure(builder(clock.clone(), inbox.clone()))
        .override_value(Arc::new(integrations))
        .build()
        .await;
    Harness {
        app,
        fake,
        clock,
        inbox,
    }
}

/// Routes on, with AccessModule (ADMIN manages integrations app-wide) and,
/// with `workspaces`, WorkspaceModule (organizations on).
pub fn with_routes(workspaces: bool) -> impl FnOnce(TestAppBuilder) -> TestAppBuilder {
    move |builder| {
        let builder = builder
            .config("integrations.routes", true)
            .config("workspace.users_table", "")
            .config("workspace.organizations.enabled", true)
            .module(AccessModule::for_root(AccessOptions {
                abilities,
                ..AccessOptions::default()
            }));
        if workspaces {
            builder.module(WorkspaceModule::from_config())
        } else {
            builder
        }
    }
}

/// WorkspaceModule (organizations on) without routes.
pub fn with_workspaces(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("workspace.users_table", "")
        .config("workspace.organizations.enabled", true)
        .config("workspace.routes", false)
        .module(WorkspaceModule::from_config())
}

/// The `HttpClientModule` of the real-provider tests: loopback allowed
/// (the mock servers), no retries.
pub fn loopback_http() -> impl Module {
    HttpClientModule::for_root(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".to_owned()],
        },
        ..HttpClientOptions::default()
    })
}

/// A harness with the real providers pointed at `mock` (a wiremock
/// server's URI).
pub async fn real(
    mock: &str,
    configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder,
) -> RealHarness {
    let clock = Arc::new(FixedClock::new(start()));
    let inbox = Arc::new(Inbox::default());
    let builder = builder(clock.clone(), inbox.clone())
        .module(loopback_http())
        .config("integrations.providers.microsoft.authority_url", mock)
        .config(
            "integrations.providers.microsoft.api_url",
            format!("{mock}/v1.0"),
        )
        .config(
            "integrations.providers.google.token_url",
            format!("{mock}/token"),
        )
        .config(
            "integrations.providers.google.api_url",
            format!("{mock}/google"),
        )
        .config(
            "integrations.providers.slack.token_url",
            format!("{mock}/api/oauth.v2.access"),
        )
        .config(
            "integrations.providers.slack.revoke_url",
            format!("{mock}/api/auth.revoke"),
        )
        .config(
            "integrations.providers.slack.api_url",
            format!("{mock}/api"),
        )
        .config(
            "integrations.providers.hubspot.token_url",
            format!("{mock}/oauth/v1/token"),
        )
        .config(
            "integrations.providers.hubspot.revoke_url",
            format!("{mock}/oauth/v1/revoke"),
        )
        .config(
            "integrations.providers.hubspot.api_url",
            format!("{mock}/hub"),
        )
        .config("integrations.providers.edr.api_url", format!("{mock}/edr"));
    let app = configure(builder).build().await;
    RealHarness { app, clock, inbox }
}

/// A booted app with real providers on a mock server.
pub struct RealHarness {
    pub app: TestApp,
    pub clock: Arc<FixedClock>,
    pub inbox: Arc<Inbox>,
}

impl RealHarness {
    pub async fn integrations(&self) -> Arc<Integrations> {
        self.app.get::<Integrations>().await
    }
}

/// The query parameter `name` of `url`.
pub fn query_param(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// A JSON fixture under `tests/fixtures/`.
pub fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}
