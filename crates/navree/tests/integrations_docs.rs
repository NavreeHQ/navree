//! The examples of `docs/packages/integrations`, through the facade with the
//! `integrations` feature (in `full`): the testing example as written, the
//! scope set, the controller with `Connection<MicrosoftGraph,
//! DirectoryRead>` (macros), the job-side client, a provider type of your
//! own, the report on the connections table and the `ConnectionUnhealthy`
//! listener.
#![cfg(feature = "sqlite")]
#![cfg(feature = "integrations")]
// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use http::Method;
use navree::access::{AbilityCtx, AccessOptions, Rules};
use navree::contracts::{AuthVia, Principal};
use navree::db::Db;
use navree::events::{Listener, ModuleBuilderEventsExt};
use navree::http_client::FakeResponse;
use navree::integrations::{
    ConnectCallback, ConnectOptions, Connection, ConnectionUnhealthy, GoogleWorkspace,
    IntegrationClient, IntegrationConnection, Integrations, IntegrationsError, MicrosoftGraph,
    NewServiceAccount, Owner, Provider, ScopeSet, ServiceAccountKey,
};
use navree::orm::{Model as _, OrmError};
use navree::prelude::*;
use navree::testing::TestApp;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// "Testing": the page's test, as written.

#[tokio::test]
async fn the_directory_is_read_from_the_connected_tenant() {
    let (integrations, fake) = Integrations::fake();
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("integrations.routes", false)
        .config(
            "integrations.redirect_base",
            "https://app.acme.test/integrations/callback",
        )
        .config("integrations.providers.microsoft.client_id", "test-client")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(IntegrationsModule::from_config())
        .override_value(Arc::new(integrations))
        .build()
        .await;
    let integrations = app.get::<Integrations>().await;

    // An admin consents for their tenant.
    let redirect = integrations
        .connect_url(Owner::App, "microsoft", ConnectOptions::default())
        .await
        .unwrap();
    let callback =
        ConnectCallback::admin_consent(redirect.state, "9188040d-6c67-4c5b-b112-36a304b66dad");
    integrations
        .complete(Owner::App, "microsoft", &callback, None)
        .await
        .unwrap();

    fake.api().on(
        Method::GET,
        "https://graph.microsoft.com/v1.0/users*",
        FakeResponse::new(200).json(&json!({ "value": [{ "id": "u1", "mail": "ada@acme.test" }] })),
    );
    let graph = integrations
        .client_for::<MicrosoftGraph>(Owner::App)
        .await
        .unwrap();
    let users: Vec<serde_json::Value> = graph.paginate("/users").await.unwrap();
    assert_eq!(users[0]["mail"], "ada@acme.test");
    assert_eq!(fake.minted()[0].grant_type, "client_credentials");
}

// ---------------------------------------------------------------------------
// "Scope sets" and "Calling the APIs": the feature module.

/// Read users and groups.
pub struct DirectoryRead;

impl ScopeSet for DirectoryRead {
    const NAME: &'static str = "directory";
    const DESCRIPTION: &'static str = "Read users and groups";

    fn scopes(provider: &str) -> &'static [&'static str] {
        match provider {
            "microsoft" => &[
                "https://graph.microsoft.com/User.Read.All",
                "https://graph.microsoft.com/Group.Read.All",
            ],
            "google" => &["https://www.googleapis.com/auth/admin.directory.user.readonly"],
            _ => &[],
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct GraphUser {
    pub id: String,
    pub mail: Option<String>,
}

#[injectable]
pub struct DirectoryController;

#[controller("/directory", version = "1")]
impl DirectoryController {
    /// `GET /api/v1/directory/users`: everyone in the tenant.
    #[get("/users")]
    #[permissions("directory.read")]
    async fn users(
        &self,
        graph: Connection<MicrosoftGraph, DirectoryRead>,
    ) -> Result<Json<Vec<GraphUser>>> {
        Ok(Json(graph.paginate("/users?$select=id,mail").await?))
    }
}

pub async fn sync_google(
    integrations: &Integrations,
    workspace_id: uuid::Uuid,
) -> Result<usize, IntegrationsError> {
    let google = integrations
        .client_for::<GoogleWorkspace>(Owner::Workspace(workspace_id))
        .await?;
    let users: Vec<Value> = google
        .paginate(
            "https://admin.googleapis.com/admin/directory/v1/users?customer=my_customer",
            "users",
        )
        .await?;
    Ok(users.len())
}

// ---------------------------------------------------------------------------
// "The connections table": a report through the ORM model.

/// Unhealthy connections, by provider, for the operations dashboard.
pub async fn unhealthy_by_provider(db: &Db) -> Result<Vec<(String, u64)>, OrmError> {
    let mut counts = Vec::new();
    for provider in ["microsoft", "google", "slack"] {
        let unhealthy = IntegrationConnection::query()
            .where_(IntegrationConnection::PROVIDER.eq(provider))
            .where_(IntegrationConnection::STATUS.eq("unhealthy"))
            .count(db)
            .await?;
        counts.push((provider.to_owned(), unhealthy));
    }
    Ok(counts)
}

/// `[integrations.providers.hubspot]`.
pub struct HubSpot;

impl Provider for HubSpot {
    const KEY: &'static str = "hubspot";
    type Client = IntegrationClient;
}

// ---------------------------------------------------------------------------
// "Health checks": the listener.

#[injectable]
pub struct WarnAdmins;

impl Listener<ConnectionUnhealthy> for WarnAdmins {
    fn handle<'a>(
        &'a self,
        event: &'a ConnectionUnhealthy,
        _ctx: &'a AppContext,
    ) -> navree::BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            tracing::warn!(provider = %event.provider, error = %event.error, "an integration stopped working");
            Ok(())
        })
    }
}

struct DirectoryModule;

impl Module for DirectoryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports(IntegrationsModule::for_feature::<DirectoryRead>())
            .controller::<DirectoryController>()
            .listen::<ConnectionUnhealthy, WarnAdmins>();
    }
}

const ADMIN: uuid::Uuid = uuid::Uuid::from_u128(1);

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.as_ref().is_some_and(|user| user.id == ADMIN) {
        rules.can(navree::access::READ, "directory");
    }
}

fn service_account() -> ServiceAccountKey {
    let pem = include_str!("../../navree-integrations/tests/fixtures/rsa-2048.pem");
    ServiceAccountKey::from_value(&json!({
        "type": "service_account",
        "client_email": "navree@acme-prod.iam.gserviceaccount.com",
        "private_key": pem,
        "private_key_id": "4f2a9c"
    }))
    .unwrap()
}

#[tokio::test]
async fn the_feature_module_reads_the_directory_through_its_scope_set() {
    let (integrations, fake) = Integrations::fake();
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("integrations.routes", false)
        .config(
            "integrations.redirect_base",
            "https://app.acme.test/integrations/callback",
        )
        .config("integrations.providers.microsoft.client_id", "test-client")
        .config(
            "integrations.providers.google.scopes",
            json!(["https://www.googleapis.com/auth/admin.directory.user.readonly"]),
        )
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(IntegrationsModule::from_config())
        .module(DirectoryModule)
        .override_value(Arc::new(integrations))
        .build()
        .await;
    let integrations = app.get::<Integrations>().await;
    let admin = app.acting_as(Principal {
        user_id: ADMIN,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    });

    admin
        .get("/api/v1/directory/users")
        .send()
        .await
        .assert_error_code(404, "integration_not_connected");
    let redirect = integrations
        .connect_url(
            Owner::App,
            "microsoft",
            ConnectOptions::default().scope_set("directory"),
        )
        .await
        .unwrap();
    let microsoft = integrations
        .complete(
            Owner::App,
            "microsoft",
            &ConnectCallback::admin_consent(redirect.state, "contoso.onmicrosoft.com"),
            None,
        )
        .await
        .unwrap()
        .connection;
    fake.api().on(
        Method::GET,
        "https://graph.microsoft.com/v1.0/users*",
        FakeResponse::new(200).json(&json!({
            "value": [{ "id": "u1", "mail": "ada@acme.test" }, { "id": "u2", "mail": null }]
        })),
    );
    admin
        .get("/api/v1/directory/users")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("0.mail", json!("ada@acme.test"))
        .assert_json_path("1.id", json!("u2"));

    // The job-side client of a workspace that has no connection.
    let workspace = uuid::Uuid::from_u128(77);
    assert!(matches!(
        sync_google(&integrations, workspace).await,
        Err(IntegrationsError::NotConnected { .. })
    ));
    integrations
        .connect_service_account(
            Owner::Workspace(workspace),
            "google",
            NewServiceAccount::new(service_account(), Some("admin@acme.test".into())),
        )
        .await
        .unwrap();
    fake.api().on(
        Method::GET,
        "https://admin.googleapis.com/admin/directory/v1/users*",
        FakeResponse::new(200).json(&json!({ "users": [{ "primaryEmail": "liz@acme.test" }] })),
    );
    assert_eq!(sync_google(&integrations, workspace).await.unwrap(), 1);

    // A provider type of your own names its key.
    assert_eq!(HubSpot::KEY, "hubspot");

    // The report counts the connection that stopped working.
    let db = app.get::<Db>().await;
    fake.refuse_tokens("microsoft", "unauthorized_client");
    integrations.check(microsoft.id).await.unwrap();
    assert_eq!(
        unhealthy_by_provider(&db).await.unwrap(),
        [
            ("microsoft".to_owned(), 1),
            ("google".to_owned(), 0),
            ("slack".to_owned(), 0)
        ]
    );
}
