//! Fixtures: a `deals` entity with access rules, a `notes` table, message
//! and event handlers written with `#[message_handlers]`, and app builders.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use garde::Validate;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Authorized, READ, Read, Rules, Subject,
    UPDATE, Update, field,
};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_http::{AppError, CurrentPrincipal, Guard, Result};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, message_handlers};
use navree_micro::{MessageContext, ModuleBuilderMicroExt, Payload, ServiceClient};
use navree_telemetry::TelemetryContext;
use navree_testing::TestApp;
use navree_validation::Valid;
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DbBackend, Set, Statement};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// The `deals` entity.
pub mod deal {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "deals")]
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

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// Creates `deals` and `notes`.
pub struct CreateTables;

impl MigrationName for CreateTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_deals_and_notes"
    }
}

#[async_trait]
impl MigrationTrait for CreateTables {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), sea_orm::DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("deals")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .col(ColumnDef::new("title").string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("notes")
                    .col(ColumnDef::new("id").string().not_null().primary_key())
                    .col(ColumnDef::new("body").string().not_null())
                    .col(ColumnDef::new("actor").string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), sea_orm::DbErr> {
        for table in ["notes", "deals"] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
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

impl From<deal::Model> for DealView {
    fn from(model: deal::Model) -> Self {
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
    #[garde(skip)]
    pub id: Uuid,
    #[garde(length(min = 2, max = 50))]
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
    #[garde(length(min = 1))]
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
    #[can(Read, deal::Entity, from = "id")]
    async fn get(&self, deal: Authorized<deal::Entity, Read>) -> Result<DealView> {
        Ok(deal.into_inner().into())
    }

    /// Renames a deal the caller may update, and emits `deal.renamed`.
    #[message_pattern("deals.rename")]
    #[can(Update, deal::Entity, from = "id")]
    async fn rename(
        &self,
        uow: Uow,
        deal: Authorized<deal::Entity, Update>,
        Valid(Payload(input)): Valid<Payload<Rename>>,
    ) -> Result<DealView> {
        let mut model: deal::ActiveModel = deal.into_inner().into();
        model.title = Set(input.title.clone());
        let updated = model.update(&*uow.tx().await?).await?;
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
pub async fn insert_note(uow: &Uow, body: &str) -> std::result::Result<(), sea_orm::DbErr> {
    let tx = uow.tx().await?;
    tx.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
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

async fn count_notes(uow: &Uow) -> std::result::Result<i64, sea_orm::DbErr> {
    let tx = uow.tx().await?;
    let row = tx
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM notes",
        ))
        .await?
        .expect("a count");
    row.try_get("", "n")
}

/// The notes in the database: `(body, actor)`.
pub async fn notes(app: &TestApp) -> Vec<(String, Option<String>)> {
    let db = app.get::<Db>().await;
    db.conn()
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT body, actor FROM notes ORDER BY id",
        ))
        .await
        .expect("notes")
        .iter()
        .map(|row| {
            (
                row.try_get("", "body").unwrap(),
                row.try_get("", "actor").unwrap(),
            )
        })
        .collect()
}

/// The deals feature.
pub struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<deal::Entity>())
            .migrations(MigrationSet::new("deals").migration(CreateTables))
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
pub async fn insert_deal(app: &TestApp, owner: Uuid, title: &str) -> deal::Model {
    let db = app.get::<Db>().await;
    deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        owner_id: Set(owner),
        title: Set(title.to_owned()),
    }
    .insert(db.conn())
    .await
    .expect("insert deal")
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
