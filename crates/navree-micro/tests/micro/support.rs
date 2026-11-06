//! Fixtures: a `deals` model with access rules, a `notes` table, message
//! and event handlers written with `#[message_handlers]`, and app builders.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Authorized, READ, Read, Rules, UPDATE,
    Update, field,
};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{
    Db, DbError, Executor, ExecutorExt, MigrationSet, ModuleBuilderDbExt, Statement, Uow,
};
use navree_http::{AppError, CurrentPrincipal, Guard, Result};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, message_handlers};
use navree_micro::{MessageContext, ModuleBuilderMicroExt, Payload, ServiceClient};
use navree_orm::Model as _;
use navree_telemetry::TelemetryContext;
use navree_testing::TestApp;
use navree_validation::Valid;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// The `deals` model: the access subject `Deal`.
pub mod deal {
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
    pub struct Deal {
        pub id: Uuid,
        pub owner_id: Uuid,
        pub title: String,
    }
}

pub use deal::Deal;

/// The name of the migration that creates `deals` and `notes`.
pub const CREATE_TABLES: &str = "m20261026_000001_create_deals_and_notes";

/// Creates `deals` and `notes`.
pub struct CreateTables;

impl CreateTables {
    /// The migration, as `MigrationSet::migration` takes it.
    pub fn migration() -> SchemaMigration<CreateTables> {
        SchemaMigration::new(CREATE_TABLES, CreateTables)
    }
}

impl Migration for CreateTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("deals", |t| {
                t.uuid("id").primary();
                t.uuid("owner_id");
                t.string("title");
            })
            .await?;
        schema
            .create("notes", |t| {
                t.string("id").primary();
                t.string("body");
                t.string("actor").nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in ["notes", "deals"] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}

/// Users read and update their own deals; database permissions apply.
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    rules.can(READ, "Deal").when(field("owner_id").eq(user.id));
    rules
        .can(UPDATE, "Deal")
        .when(field("owner_id").eq(user.id));
}

/// A deal as other services see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealView {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
}

impl From<Deal> for DealView {
    fn from(model: Deal) -> Self {
        Self {
            id: model.id,
            owner_id: model.owner_id,
            title: model.title,
        }
    }
}

/// `deals.get`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetDeal {
    pub id: Uuid,
}

/// `deals.rename`.
#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Rename {
    #[validate(skip)]
    pub id: Uuid,
    #[validate(length(min = 2, max = 50))]
    pub title: String,
}

/// `deal.renamed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealRenamed {
    pub deal_id: Uuid,
    pub title: String,
}

impl navree_events::Event for DealRenamed {
    const NAME: &'static str = "deal.renamed";
}

/// A note to write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Validate)]
pub struct NewNote {
    #[validate(length(min = 1))]
    pub body: String,
}

/// Lets a message through only with `x-service-key: s3cret`.
#[derive(Debug, Default)]
#[injectable]
pub struct InternalOnly;

impl Guard for InternalOnly {
    fn check<'a>(
        &'a self,
        parts: &'a mut http::request::Parts,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let ok = parts
            .headers
            .get("x-service-key")
            .is_some_and(|value| value == "s3cret");
        Box::pin(async move {
            if ok {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        })
    }
}

/// The deals service's message handlers.
#[injectable]
pub struct DealsHandlers {
    client: Arc<ServiceClient>,
    internal: Arc<InternalOnly>,
}

#[message_handlers]
impl DealsHandlers {
    /// One deal, if the caller may read it.
    #[message_pattern("deals.get")]
    #[can(Read, Deal, from = "id")]
    async fn get(&self, deal: Authorized<Deal, Read>) -> Result<DealView> {
        Ok(deal.into_inner().into_inner().into())
    }

    /// Renames a deal the caller may update, and emits `deal.renamed`.
    #[message_pattern("deals.rename")]
    #[can(Update, Deal, from = "id")]
    async fn rename(
        &self,
        uow: Uow,
        deal: Authorized<Deal, Update>,
        Valid(Payload(input)): Valid<Payload<Rename>>,
    ) -> Result<DealView> {
        let mut updated = deal.into_inner();
        updated.title = input.title.clone();
        updated.save(&uow).await?;
        let updated = updated.into_inner();
        self.client
            .emit(
                "deal.renamed",
                &DealRenamed {
                    deal_id: updated.id,
                    title: updated.title.clone(),
                },
            )
            .key(updated.id)
            .await?;
        Ok(updated.into())
    }

    /// Writes a note, then fails: the note rolls back.
    #[message_pattern("notes.write_then_fail")]
    #[public]
    async fn write_then_fail(&self, uow: Uow, Payload(note): Payload<NewNote>) -> Result<()> {
        insert_note(&uow, &note.body).await?;
        self.client.emit("note.written", &note).await?;
        Err(AppError::conflict("not today"))
    }

    /// Writes a note; `dedupe` answers a repeated message id with the
    /// stored reply.
    #[message_pattern("notes.add", dedupe)]
    #[public]
    async fn add_note(
        &self,
        uow: Uow,
        Valid(Payload(note)): Valid<Payload<NewNote>>,
    ) -> Result<Value> {
        insert_note(&uow, &note.body).await?;
        Ok(json!({ "notes": count_notes(&uow).await? }))
    }

    /// What the handler knows about its message.
    #[message_pattern("deals.echo")]
    #[public]
    async fn echo(&self, msg: MessageContext, Payload(payload): Payload<Value>) -> Result<Value> {
        Ok(json!({
            "payload": payload,
            "pattern": msg.pattern(),
            "service": msg.service(),
            "message_id": msg.message_id(),
            "request_id": msg.request_id(),
            "attempt": msg.attempt(),
            "deadline": msg.deadline().is_some(),
            "traceparent": msg.header("traceparent"),
            // The telemetry context the handler runs in.
            "trace_id": TelemetryContext::current()
                .and_then(|context| context.trace().map(|trace| trace.trace_id.to_string())),
            "parent_span_id": TelemetryContext::current().and_then(|context| {
                context.trace().and_then(|trace| trace.parent_span_id.map(|span| span.to_string()))
            }),
            "request_id_seen": TelemetryContext::current()
                .and_then(|context| context.request_id().map(str::to_owned)),
        }))
    }

    /// The authenticated caller.
    #[message_pattern("deals.whoami")]
    #[authenticated]
    async fn whoami(&self, CurrentPrincipal(me): CurrentPrincipal) -> Result<Uuid> {
        Ok(me.user_id)
    }

    /// Admins only.
    #[message_pattern("deals.admin")]
    #[roles("admin")]
    async fn admin(&self) -> Result<&'static str> {
        Ok("admin area")
    }

    /// Internal services only.
    #[message_pattern("deals.internal")]
    #[public]
    #[guard(self.internal.clone())]
    async fn internal(&self) -> Result<String> {
        Ok("internal".to_owned())
    }

    /// Takes longer than its time limit.
    #[message_pattern("deals.slow")]
    #[public]
    #[timeout("100ms")]
    async fn slow(&self, uow: Uow) -> Result<()> {
        insert_note(&uow, "slow").await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    /// Asks another service, inheriting this message's deadline.
    #[message_pattern("deals.relay")]
    #[public]
    async fn relay(&self, Payload(target): Payload<String>) -> Result<Value> {
        let reply: Value = self.client.send(&target, &json!({})).await?;
        Ok(reply)
    }

    /// Fails with a 500.
    #[message_pattern("deals.broken")]
    #[public]
    async fn broken(&self) -> Result<()> {
        Err(AppError::internal("the database password is hunter2"))
    }
}

/// Writes a note in `uow`.
pub async fn insert_note(uow: &Uow, body: &str) -> std::result::Result<(), DbError> {
    uow.execute(&Statement::portable(
        "INSERT INTO notes (id, body, actor) VALUES (?, ?, ?)",
        [
            Uuid::now_v7().to_string().into(),
            body.into(),
            uow.actor().map(|actor| actor.to_string()).into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn count_notes(uow: &Uow) -> std::result::Result<i64, DbError> {
    uow.fetch_scalar(&Statement::text("SELECT COUNT(*) FROM notes"))
        .await
}

/// The notes in the database: `(body, actor)`.
pub async fn notes(app: &TestApp) -> Vec<(String, Option<String>)> {
    let db = app.get::<Db>().await;
    db.fetch_all(&Statement::text(
        "SELECT body, actor FROM notes ORDER BY id",
    ))
    .await
    .expect("notes")
    .iter()
    .map(|row| (row.get("body").unwrap(), row.get("actor").unwrap()))
    .collect()
}

/// The deals feature.
pub struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Deal>())
            .migrations(MigrationSet::new("deals").migration(CreateTables::migration()))
            .provider::<InternalOnly>()
            .message_handlers::<DealsHandlers>();
    }
}

/// An `AuditSink` that keeps every entry.
#[derive(Clone, Default)]
pub struct RecordingAudit(Arc<Mutex<Vec<AuditEntry>>>);

impl RecordingAudit {
    pub fn actions(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.action.clone())
            .collect()
    }

    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0.lock().unwrap().clone()
    }
}

impl AuditSink for RecordingAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, std::result::Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// Inserts a deal owned by `owner`.
pub async fn insert_deal(app: &TestApp, owner: Uuid, title: &str) -> Deal {
    let db = app.get::<Db>().await;
    Deal::create(
        &*db,
        Deal {
            id: Uuid::now_v7(),
            owner_id: owner,
            title: title.to_owned(),
        },
    )
    .await
    .expect("insert deal")
    .into_inner()
}

/// Creates `role` with `permissions` and gives it to `user`.
pub async fn give_role(app: &TestApp, user: Uuid, role: &str, permissions: &[&str]) {
    let db = app.get::<Db>().await;
    let service = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    if service.role(role).await.expect("role").is_none() {
        service
            .create_role(&uow, role, None)
            .await
            .expect("create role");
    }
    for permission in permissions {
        service.grant(&uow, role, permission).await.expect("grant");
    }
    service.assign_role(&uow, user, role).await.expect("assign");
    uow.commit().await.expect("commit");
}

/// The access options of the test apps.
pub fn access() -> AccessOptions {
    AccessOptions {
        abilities,
        super_admin_role: None,
    }
}

/// A counter shared with a handler.
#[derive(Debug, Default)]
pub struct Counter(pub AtomicU32);

impl Counter {
    pub fn bump(&self) -> u32 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn get(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

/// The values of `variables`, the settings of a server `test` needs, or
/// `None` after a notice when one is not set; `NAVREE_TEST_REQUIRE_SERVICES=1`
/// makes a missing one a failure.
pub fn services<const N: usize>(test: &str, variables: [&str; N]) -> Option<[String; N]> {
    let mut values = Vec::with_capacity(N);
    for variable in variables {
        match std::env::var(variable) {
            Ok(value) if !value.trim().is_empty() => values.push(value),
            _ => {
                assert!(
                    std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                    "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
                );
                eprintln!("SKIPPED {test}: set {variable}");
                return None;
            }
        }
    }
    values.try_into().ok()
}
