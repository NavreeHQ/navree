//! Shared fixtures: mock provider servers, options for a provider at a mock
//! URL, scripted response sequences, and a test app with users, abilities
//! and a `notes` table for agents.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, TimeZone, Utc};
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Rules, Subject, field,
};
use navree_ai::{AiModule, AiOptions, ModelConfig, ProviderConfig};
use navree_auth::{AuthModule, AuthService};
use navree_contracts::{Clock, FixedClock};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TestApp, TestAppBuilder};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::Value;
use uuid::Uuid;
use wiremock::{Request, Respond, ResponseTemplate};

/// When the test clock starts.
pub fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap()
}

/// Options with one model `name` of provider `driver` at `url` (fast
/// retries, short timeouts).
pub fn provider_options(driver: &str, url: &str, name: &str, model: &str) -> AiOptions {
    let mut provider = ProviderConfig {
        driver: Some(driver.to_owned()),
        url: Some(url.to_owned()),
        api_key: Some("test-key".to_owned()),
        ..ProviderConfig::default()
    };
    if driver == "bedrock" {
        provider.region = Some("us-east-1".to_owned());
        provider.access_key_id = Some("AKIDEXAMPLE".to_owned());
        provider.secret_access_key = Some("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned());
    }
    AiOptions {
        default: name.to_owned(),
        timeout_secs: 5,
        retry_base_ms: 5,
        providers: [(driver.to_owned(), provider)].into(),
        models: [(name.to_owned(), ModelConfig::new(driver, model))].into(),
        ..AiOptions::default()
    }
}

/// Answers each request with the next template, then repeats the last.
pub struct Sequence {
    templates: Vec<ResponseTemplate>,
    next: AtomicUsize,
}

impl Sequence {
    pub fn new(templates: Vec<ResponseTemplate>) -> Self {
        Self {
            templates,
            next: AtomicUsize::new(0),
        }
    }
}

impl Respond for Sequence {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let index = self.next.fetch_add(1, Ordering::SeqCst);
        self.templates[index.min(self.templates.len() - 1)].clone()
    }
}

/// A JSON response.
pub fn json(status: u16, body: Value) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(body)
}

/// A server-sent events response with `events` (`(event name, data)`).
pub fn sse(events: &[(&str, Value)]) -> ResponseTemplate {
    let body: String = events
        .iter()
        .map(|(name, data)| {
            if name.is_empty() {
                format!("data: {data}\n\n")
            } else {
                format!("event: {name}\ndata: {data}\n\n")
            }
        })
        .collect();
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

/// The JSON bodies `server` received.
pub async fn bodies(server: &wiremock::MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
        .collect()
}

/// The `notes` entity agents work on.
pub mod note {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub owner_id: Uuid,
        pub title: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl Subject for note::Entity {
    const SUBJECT: &'static str = "Note";
    const PERMISSION_PREFIX: &'static str = "notes";
}

struct CreateNotes;

impl MigrationName for CreateNotes {
    fn name(&self) -> &str {
        "m20261026_000001_create_notes"
    }
}

#[async_trait]
impl MigrationTrait for CreateNotes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("notes")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .col(ColumnDef::new("title").string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("notes").to_owned())
            .await
    }
}

/// Everyone may read and create notes and update their own; managers
/// manage every note.
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    let Some(user) = &ctx.user else { return };
    rules.can("read", "Note");
    rules.can("create", "Note");
    rules
        .can("update", "Note")
        .when(field("owner_id").eq(user.id));
    if ctx.has_role("manager") {
        rules.can("manage", "Note");
    }
    ctx.grant_permissions(rules);
}

/// The notes table and subject.
pub struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .import(AccessModule::for_feature::<note::Entity>());
    }
}

/// A test app with users, roles, the notes table and `AiModule` with the
/// fake, plus `extra` modules.
pub fn agent_app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("ai.fake", true)
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(start())))
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(AiModule::from_config())
        .module(NotesModule)
}

/// Creates a user (with `roles`) and returns their id.
pub async fn user(app: &TestApp, name: &str, roles: &[&str]) -> Uuid {
    let auth = app.get::<AuthService>().await;
    let user = auth
        .create_user(
            name,
            &format!("{}@example.test", name.to_lowercase()),
            "password123",
        )
        .await
        .unwrap();
    if !roles.is_empty() {
        let access = app.get::<AccessService>().await;
        let db = app.get::<Db>().await;
        let uow = db.uow(None, None).await;
        for role in roles {
            let _ = access.create_role(&uow, role, None).await;
            access.assign_role(&uow, user.id, role).await.unwrap();
        }
        uow.commit().await.unwrap();
    }
    user.id
}
