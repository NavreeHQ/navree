//! Shared fixtures: a `deals` table, abilities, and `CrmMcp`, an MCP server
//! written with `#[mcp_server]` like the documentation's.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use garde::Validate;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, ActionMarker, Authorized, MANAGE, READ,
    Read, Rules, Subject, UPDATE, Update, field,
};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture, Principal};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt};
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::{McpContext, McpModule, ModuleBuilderMcpExt, PromptMessage, ResourceContents};
use navree_testing::{TestApp, TestAppBuilder, principal};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
        pub stage: String,
        pub amount_cents: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub type Deal = deal::Entity;

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// The `move` action on deals.
pub struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = "move";
}

/// Creates `deals`.
pub struct CreateDeals;

impl MigrationName for CreateDeals {
    fn name(&self) -> &str {
        "m20261026_000001_create_deals"
    }
}

#[async_trait]
impl MigrationTrait for CreateDeals {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("deals")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("owner_id").uuid().not_null())
                    .col(ColumnDef::new("title").string().not_null())
                    .col(ColumnDef::new("stage").string().not_null())
                    .col(ColumnDef::new("amount_cents").big_integer().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .drop_table(Table::drop().table("deals").to_owned())
            .await
    }
}

/// Everyone signed in reads every deal but secret ones; owners update and
/// move their own; `admin` manages everything (last rule wins);
/// permissions come from roles.
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    rules.can(READ, "Deal");
    rules.cannot(READ, "Deal").when(field("stage").eq("secret"));
    rules
        .can(UPDATE, "Deal")
        .when(field("owner_id").eq(user.id));
    rules
        .can("move", "Deal")
        .when(field("owner_id").eq(user.id));
    if user.has_role("admin") {
        rules.can(MANAGE, "Deal");
    }
}

/// A deal as tools and resources return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DealResource {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub stage: String,
    pub amount_cents: i64,
}

impl From<deal::Model> for DealResource {
    fn from(model: deal::Model) -> Self {
        Self {
            id: model.id,
            owner_id: model.owner_id,
            title: model.title,
            stage: model.stage,
            amount_cents: model.amount_cents,
        }
    }
}

/// A deal stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Lead,
    Qualified,
    Won,
    Lost,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Lead => "lead",
            Stage::Qualified => "qualified",
            Stage::Won => "won",
            Stage::Lost => "lost",
        }
    }
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct ListDeals {
    /// Words from the deal title.
    #[garde(length(chars, max = 100))]
    pub query: Option<String>,
    /// Only deals in this stage.
    #[garde(skip)]
    pub stage: Option<Stage>,
    /// Only the user's own deals.
    #[garde(skip)]
    #[serde(default)]
    pub mine: bool,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateDeal {
    /// The deal's title.
    #[garde(length(min = 1, max = 100))]
    pub title: String,
    /// The amount, in cents.
    #[garde(range(min = 0))]
    pub amount_cents: i64,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct MoveDealArgs {
    /// The deal to move.
    #[garde(skip)]
    pub deal_id: Uuid,
    /// The stage to move it to.
    #[garde(skip)]
    pub stage: Stage,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RenameDeal {
    /// The deal to rename.
    #[garde(skip)]
    pub deal_id: Uuid,
    /// Its new title.
    #[garde(length(min = 1))]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct FollowUpArgs {
    /// The deal to follow up on.
    #[garde(skip)]
    pub deal_id: Uuid,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct SummarizeArgs {
    /// How many days back.
    #[garde(range(min = 1, max = 90))]
    pub days: u32,
    /// Whether lost deals count.
    #[garde(skip)]
    #[serde(default)]
    pub include_lost: bool,
}

/// The CRM's MCP server.
#[injectable]
pub struct CrmMcp;

#[mcp_server]
impl CrmMcp {
    /// Lists the deals the user can see, newest first, optionally only one
    /// stage or the user's own.
    #[mcp_tool(read_only)]
    #[can(Read, Deal)]
    async fn list_deals(
        &self,
        cx: &McpContext,
        args: ListDeals,
    ) -> Result<Vec<DealResource>, AppError> {
        let mut select = cx
            .accessible::<Deal, Read>()
            .apply(deal::Entity::find().order_by_asc(deal::Column::Title));
        if let Some(stage) = args.stage {
            select = select.filter(deal::Column::Stage.eq(stage.as_str()));
        }
        if let Some(query) = args.query {
            select = select.filter(deal::Column::Title.contains(query));
        }
        if args.mine {
            select = select.filter(deal::Column::OwnerId.eq(cx.user_id()));
        }
        let rows = select.all(&*cx.uow().tx().await?).await?;
        Ok(rows.into_iter().map(DealResource::from).collect())
    }

    /// Creates a deal owned by the user.
    #[mcp_tool(title = "Create a deal")]
    #[authenticated]
    async fn create_deal(
        &self,
        cx: &McpContext,
        args: CreateDeal,
    ) -> Result<DealResource, AppError> {
        let model = deal::ActiveModel {
            id: Set(Uuid::now_v7()),
            owner_id: Set(cx.user_id()),
            title: Set(args.title),
            stage: Set("lead".to_owned()),
            amount_cents: Set(args.amount_cents),
        }
        .insert(&*cx.uow().tx().await?)
        .await?;
        Ok(model.into())
    }

    /// Creates a deal, then fails: nothing it wrote is kept.
    #[mcp_tool]
    #[authenticated]
    async fn create_then_fail(
        &self,
        cx: &McpContext,
        args: CreateDeal,
    ) -> Result<DealResource, AppError> {
        self.create_deal(cx, args).await?;
        Err(AppError::conflict("The pipeline is locked"))
    }

    /// Moves a deal to another stage.
    #[mcp_tool(destructive)]
    #[can(Update, Deal)]
    async fn move_deal(
        &self,
        cx: &McpContext,
        args: MoveDealArgs,
    ) -> Result<DealResource, AppError> {
        let deal = cx.authorize::<Deal, Move>(args.deal_id).await?;
        let mut model: deal::ActiveModel = deal.into_inner().into();
        model.stage = Set(args.stage.as_str().to_owned());
        Ok(model.update(&*cx.uow().tx().await?).await?.into())
    }

    /// Renames a deal the user may update.
    #[mcp_tool(idempotent)]
    #[can(Update, Deal, from = "deal_id")]
    async fn rename_deal(
        &self,
        cx: &McpContext,
        deal: Authorized<Deal, Update>,
        args: RenameDeal,
    ) -> Result<DealResource, AppError> {
        let mut model: deal::ActiveModel = deal.into_inner().into();
        model.title = Set(args.title);
        Ok(model.update(&*cx.uow().tx().await?).await?.into())
    }

    /// The month's sales, as text.
    #[mcp_tool(name = "report_sales", title = "Sales report", open_world)]
    #[permissions("reports.view")]
    async fn sales_report(&self) -> Result<String, AppError> {
        Ok("Sales are up 12%.".to_owned())
    }

    /// Only for admins.
    #[mcp_tool]
    #[roles("admin")]
    async fn purge_deals(&self) -> Result<u64, AppError> {
        Ok(0)
    }

    /// Exports the deals, reporting progress.
    #[mcp_tool(read_only)]
    #[authenticated]
    async fn export_deals(&self, cx: &McpContext) -> Result<u32, AppError> {
        cx.progress(1.0, Some(2.0), "half way").await;
        cx.progress(2.0, Some(2.0), "").await;
        Ok(2)
    }

    /// Who is calling.
    #[mcp_tool(read_only)]
    #[public]
    async fn whoami(&self, cx: &McpContext) -> Result<Value, AppError> {
        Ok(serde_json::json!({
            "user_id": cx.user_id(),
            "session": cx.session_id().is_some(),
            "client": cx.client().map(|client| client.name.clone()),
            "roles": cx.access().roles,
            "request_id": !cx.request_id().is_empty(),
        }))
    }

    /// Fails with an internal error whose details stay in the log.
    #[mcp_tool]
    #[authenticated]
    async fn explode(&self) -> Result<String, AppError> {
        Err(AppError::internal("the database password is hunter2"))
    }

    /// A deal with its owner, stage and amount.
    #[mcp_resource("crm://deals/{id}", mime = "application/json")]
    #[can(Read, Deal, from = "id")]
    async fn deal(&self, deal: Authorized<Deal, Read>) -> Result<DealResource, AppError> {
        Ok(deal.into_inner().into())
    }

    /// The pipeline: how many deals each stage has.
    #[mcp_resource("crm://pipeline", mime = "application/json", title = "Pipeline")]
    #[can(Read, Deal)]
    async fn pipeline(&self, cx: &McpContext) -> Result<Value, AppError> {
        let rows = cx
            .accessible::<Deal, Read>()
            .apply(deal::Entity::find())
            .all(&*cx.uow().tx().await?)
            .await?;
        let mut counts = serde_json::Map::new();
        for row in rows {
            let count = counts.entry(row.stage).or_insert(Value::from(0));
            *count = Value::from(count.as_u64().unwrap_or(0) + 1);
        }
        Ok(Value::Object(counts))
    }

    /// A note, as text.
    #[mcp_resource("crm://notes/{slug}")]
    #[authenticated]
    async fn note(&self, slug: String) -> Result<String, AppError> {
        Ok(format!("Note {slug}"))
    }

    /// A deal's number, typed.
    #[mcp_resource("crm://numbers/{n}", name = "number")]
    #[authenticated]
    async fn numbered(&self, n: u32) -> Result<u32, AppError> {
        Ok(n * 2)
    }

    /// A file from storage.
    #[mcp_resource("crm://files/{name}", mime = "application/pdf")]
    #[authenticated]
    async fn file(&self, name: String) -> Result<ResourceContents, AppError> {
        Ok(ResourceContents::blob(
            format!("%PDF {name}").into_bytes(),
            "application/pdf",
        ))
    }

    /// Statistics for admins.
    #[mcp_resource("crm://admin/stats")]
    #[roles("admin")]
    async fn stats(&self) -> Result<Value, AppError> {
        Ok(serde_json::json!({ "deals": 0 }))
    }

    /// Drafts a follow-up email to a deal's contact.
    #[mcp_prompt(name = "follow-up-email", title = "Draft a follow-up email")]
    #[can(Read, Deal)]
    async fn follow_up(
        &self,
        cx: &McpContext,
        args: FollowUpArgs,
    ) -> Result<Vec<PromptMessage>, AppError> {
        let deal = cx.authorize::<Deal, Read>(args.deal_id).await?;
        Ok(vec![
            PromptMessage::resource(format!("crm://deals/{}", deal.id)),
            PromptMessage::user(format!(
                "Draft a short follow-up email about {}. End with one clear next step.",
                deal.title
            )),
            PromptMessage::assistant("Here is a draft:"),
        ])
    }

    /// Summarizes the recent pipeline.
    #[mcp_prompt]
    #[authenticated]
    async fn summarize(&self, args: SummarizeArgs) -> Result<String, AppError> {
        Ok(format!(
            "Summarize the last {} days{}.",
            args.days,
            if args.include_lost {
                ", lost deals included"
            } else {
                ""
            }
        ))
    }
}

/// The deals feature: its table, subject and MCP server.
pub struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<deal::Entity>())
            .migrations(MigrationSet::new("deals").migration(CreateDeals))
            .mcp_server::<CrmMcp>();
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
            .map(|entry| entry.action.clone())
            .collect()
    }
}

impl AuditSink for RecordingAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// The app builder: a temporary database, access, the deals feature and
/// `McpModule`.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("app.name", "navree-crm")
        .module(DatabaseModule)
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(DealsModule)
        .module(McpModule::from_config())
}

/// A booted app.
pub async fn app() -> TestApp {
    builder().build().await
}

/// A new user.
pub fn user() -> Principal {
    principal(Uuid::now_v7())
}

/// Gives `user` the role `role` with `permissions`.
pub async fn give_role(app: &TestApp, user: Uuid, role: &str, permissions: &[&str]) {
    let service = app.get::<AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    if service.role(role).await.unwrap().is_none() {
        service.create_role(&uow, role, None).await.unwrap();
    }
    for permission in permissions {
        service.grant(&uow, role, permission).await.unwrap();
    }
    service.assign_role(&uow, user, role).await.unwrap();
    uow.commit().await.unwrap();
}

/// Inserts a deal owned by `owner`.
pub async fn insert_deal(app: &TestApp, owner: Uuid, title: &str, stage: &str) -> deal::Model {
    let db = app.get::<Db>().await;
    deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        owner_id: Set(owner),
        title: Set(title.to_owned()),
        stage: Set(stage.to_owned()),
        amount_cents: Set(10_000),
    }
    .insert(db.conn())
    .await
    .expect("insert deal")
}

/// Every deal in the database.
pub async fn all_deals(app: &TestApp) -> Vec<deal::Model> {
    let db = app.get::<Db>().await;
    deal::Entity::find()
        .order_by_asc(deal::Column::Title)
        .all(db.conn())
        .await
        .unwrap()
}

/// Tool names, sorted.
pub fn names(tools: &[navree_mcp::protocol::Tool]) -> Vec<String> {
    let mut names: Vec<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
    names.sort();
    names
}

/// `Update` re-exported for tests that build specs by hand.
pub type UpdateAction = Update;
