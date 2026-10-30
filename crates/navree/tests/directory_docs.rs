//! The examples of `docs/packages/directory-sync`, through the facade with
//! the `directory` feature (in `full`): the recipients sink on an ORM model,
//! the feature module, a source of your own, the welcome listener,
//! connections made in code, and the page's test as written.
#![cfg(feature = "sqlite")]
#![cfg(feature = "directory")]
// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use navree::db::Db;
use navree::directory::{
    Directory, DirectoryError, DirectorySink, DirectorySource, DirectoryUser, DirectoryUserCreated,
    NewConnection, Owner, PullRequest, Pulled, RunStatus, SinkContext, SyncOptions, UserChange,
};
use navree::events::{Listener, ModuleBuilderEventsExt};
use navree::prelude::*;
use navree::testing::TestApp;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// "Mapping the directory into your model": the model, its table, the sink
// and the feature module.

#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "recipients")]
pub struct Recipient {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub directory_user_id: Uuid,
    pub email: String,
    pub name: String,
    pub department: Option<String>,
    pub active: bool,
}

mod m20261028_090000_create_recipients_table {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("recipients", |t| {
                    t.uuid_primary();
                    t.uuid("workspace_id").nullable();
                    t.uuid("directory_user_id").unique();
                    t.string("email");
                    t.string("name");
                    t.string("department").nullable();
                    t.boolean("active");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("recipients").await
        }
    }
}

#[injectable]
pub struct RecipientSink;

impl DirectorySink for RecipientSink {
    fn upsert_user<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a UserChange,
    ) -> navree::BoxFuture<'a, Result<Option<String>, BoxError>> {
        Box::pin(async move {
            let user = &change.user;
            let uow = ctx.uow();
            let mut recipient = match Recipient::query()
                .where_(Recipient::DIRECTORY_USER_ID.eq(change.directory_user_id))
                .first(uow)
                .await?
            {
                Some(recipient) => recipient,
                None => Record::new(Recipient {
                    workspace_id: ctx.owner().id(),
                    directory_user_id: change.directory_user_id,
                    ..Recipient::default()
                }),
            };
            recipient.email = user.primary_email().unwrap_or(&user.user_name).to_owned();
            recipient.name = user.full_name();
            recipient.department = user.department.clone();
            recipient.active = user.active;
            recipient.save(uow).await?;
            Ok(Some(recipient.id.to_string()))
        })
    }

    fn deactivate_user<'a>(
        &'a self,
        ctx: &'a SinkContext,
        change: &'a UserChange,
    ) -> navree::BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let uow = ctx.uow();
            Recipient::query()
                .where_(Recipient::DIRECTORY_USER_ID.eq(change.directory_user_id))
                .update(uow, [Recipient::ACTIVE.set(false)])
                .await?;
            Ok(())
        })
    }
}

pub struct RecipientsModule;

impl Module for RecipientsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports(DirectoryModule::for_feature::<RecipientSink>())
            .migrations(
                MigrationSet::new("recipients").migration(navree::schema::migration!(
                    m20261028_090000_create_recipients_table
                )),
            )
            .listen::<DirectoryUserCreated, Welcome>();
    }
}

// ---------------------------------------------------------------------------
// "Events": the welcome listener.

#[injectable]
pub struct Welcome;

impl Listener<DirectoryUserCreated> for Welcome {
    fn handle<'a>(
        &'a self,
        event: &'a DirectoryUserCreated,
        _ctx: &'a AppContext,
    ) -> navree::BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if event.user.active
                && let Some(email) = event.user.primary_email()
            {
                tracing::info!(%email, "a new colleague: send the welcome mail");
            }
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// "A source of your own": people from an HR export.

pub struct HrSource;

impl DirectorySource for HrSource {
    fn check_settings(
        &self,
        settings: &serde_json::Value,
        _has_secret: bool,
    ) -> Result<(), String> {
        match settings.get("export_url").and_then(|url| url.as_str()) {
            Some(url) if url.starts_with("https://") => Ok(()),
            _ => Err("settings.export_url must be an https:// URL".to_owned()),
        }
    }

    fn pull<'a>(
        &'a self,
        request: &'a PullRequest,
    ) -> navree::BoxFuture<'a, Result<Pulled, DirectoryError>> {
        Box::pin(async move {
            // Read request.setting("export_url") with your HTTP client; here,
            // two people.
            let _export = request.setting("export_url");
            let people = vec![
                DirectoryUser::new("E-1001", "ada@acme.test")
                    .name("Ada", "Lovelace")
                    .email("ada@acme.test")
                    .department("Engineering"),
                DirectoryUser::new("E-1002", "grace@acme.test")
                    .name("Grace", "Hopper")
                    .email("grace@acme.test"),
            ];
            Ok(Pulled::snapshot(people, Vec::new()))
        })
    }
}

pub struct HrModule;

impl Module for HrModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.directory_source("hr", HrSource);
    }
}

// ---------------------------------------------------------------------------
// "Testing": the page's test, as written.

#[tokio::test]
async fn directory_people_become_recipients() {
    let (directory, fake) = Directory::fake();
    let app = TestApp::builder()
        .temp_database()
        .config("directory.routes", false)
        .module(DatabaseModule::from_config())
        .module(DirectoryModule::from_config())
        .module(RecipientsModule)
        .override_value(Arc::new(directory))
        .build()
        .await;
    fake.user(
        DirectoryUser::new("00u1", "ada@acme.test")
            .name("Ada", "Lovelace")
            .email("ada@acme.test"),
    )
    .user(DirectoryUser::new("00u2", "grace@acme.test").inactive());

    let directory = app.get::<Directory>().await;
    let connection = directory
        .create_connection(Owner::App, NewConnection::new("Acme Okta", "okta"))
        .await
        .unwrap()
        .connection;
    let run = directory
        .sync_now(connection.id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.summary.users_created, 2);

    let db = app.get::<Db>().await;
    let ada = Recipient::query()
        .where_(Recipient::EMAIL.eq("ada@acme.test"))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    assert!(ada.active);

    // Ada leaves the company.
    fake.remove_user("00u1");
    directory
        .sync_now(connection.id, SyncOptions::default())
        .await
        .unwrap();
    let ada = Recipient::query()
        .where_(Recipient::EMAIL.eq("ada@acme.test"))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    assert!(!ada.active);
}

// ---------------------------------------------------------------------------
// "Connections" in code, a dry run and the registered source.

#[tokio::test]
async fn connections_are_made_in_code_and_custom_sources_sync() {
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("directory.routes", false)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(DirectoryModule::from_config())
        .module(RecipientsModule)
        .module(HrModule)
        .build()
        .await;
    let directory = app.get::<Directory>().await;
    let workspace = Owner::Workspace(Uuid::now_v7());

    // A SCIM connection: give the identity provider the URL and the token.
    let scim = directory
        .create_connection(workspace, NewConnection::new("Acme Entra ID", "scim"))
        .await
        .unwrap();
    let token = scim.scim_token.unwrap();
    assert!(token.expose().starts_with("nvscim_"));
    assert_eq!(scim.scim_url.as_deref(), Some("/scim/v2"));

    // An LDAP connection: the bind password is sealed with the app key.
    let ldap = directory
        .create_connection(
            workspace,
            NewConnection::new("Acme Active Directory", "ldap")
                .settings(json!({
                    "url": "ldaps://dc1.acme.test",
                    "base_dn": "DC=acme,DC=test",
                    "bind_dn": "CN=navree-sync,OU=Service Accounts,DC=acme,DC=test"
                }))
                .secret("bind password from your secret store")
                .interval(30),
        )
        .await
        .unwrap();
    assert!(ldap.connection.has_secret);
    assert_eq!(ldap.connection.sync_interval_mins, 30);

    // The HR source: checked when saved, then pulled.
    let refused = directory
        .create_connection(
            workspace,
            NewConnection::new("HR", "hr").settings(json!({ "export_url": "ftp://hr" })),
        )
        .await;
    assert!(matches!(refused, Err(DirectoryError::InvalidSettings(_))));
    let hr = directory
        .create_connection(
            workspace,
            NewConnection::new("HR", "hr")
                .settings(json!({ "export_url": "https://hr.acme.test/export" })),
        )
        .await
        .unwrap()
        .connection;
    let plan = directory
        .sync_now(hr.id, SyncOptions::dry_run())
        .await
        .unwrap();
    assert_eq!(plan.status, RunStatus::Succeeded);
    assert_eq!(plan.summary.users_created, 2);
    let changes = directory.changes(plan.id, 0, 50).await.unwrap();
    assert_eq!(changes[0].label, "ada@acme.test");
    let run = directory
        .sync_now(hr.id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.summary.users_created, 2);
    let db = app.get::<Db>().await;
    let ada = Recipient::query()
        .where_(Recipient::EMAIL.eq("ada@acme.test"))
        .first(&*db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ada.workspace_id, workspace.id());
    assert_eq!(ada.department.as_deref(), Some("Engineering"));
}
