//! Shared fixtures: a `deals` table, abilities, and `CrmMcp`, an MCP server
//! written with `#[mcp_server]` like the documentation's.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, ActionMarker, Authorized, MANAGE, READ,
    Read, Rules, UPDATE, Update, field,
};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture, Principal};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt};
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::{McpContext, McpModule, ModuleBuilderMcpExt, PromptMessage, ResourceContents};
use navree_orm::Model as _;
use navree_orm::access::AccessibleQuery as _;
use navree_testing::{TestApp, TestAppBuilder, principal};
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
        pub stage: String,
        pub amount_cents: i64,
    }
}

pub use deal::Deal;

/// The `move` action on deals.
pub struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = "move";
}

/// Creates `deals`.
pub struct CreateDeals;

impl Migration for CreateDeals {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("deals", |t| {
                t.uuid("id").primary();
                t.uuid("owner_id");
                t.string("title");
                t.string("stage");
                t.big_integer("amount_cents");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await
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

impl From<Deal> for DealResource {
    fn from(model: Deal) -> Self {
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
    #[validate(length(max = 100))]
    pub query: Option<String>,
    /// Only deals in this stage.
    #[validate(skip)]
    pub stage: Option<Stage>,
    /// Only the user's own deals.
    #[validate(skip)]
    #[serde(default)]
    pub mine: bool,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateDeal {
    /// The deal's title.
    #[validate(length(min = 1, max = 100))]
    pub title: String,
    /// The amount, in cents.
    #[validate(range(min = 0))]
    pub amount_cents: i64,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct MoveDealArgs {
    /// The deal to move.
    #[validate(skip)]
    pub deal_id: Uuid,
    /// The stage to move it to.
    #[validate(skip)]
    pub stage: Stage,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RenameDeal {
    /// The deal to rename.
    #[validate(skip)]
    pub deal_id: Uuid,
    /// Its new title.
    #[validate(length(min = 1))]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct FollowUpArgs {
    /// The deal to follow up on.
    #[validate(skip)]
    pub deal_id: Uuid,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct SummarizeArgs {
    /// How many days back.
    #[validate(range(min = 1, max = 90))]
    pub days: u32,
    /// Whether lost deals count.
    #[validate(skip)]
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
        let mut query = cx.accessible::<Deal, Read>().query().order_by(Deal::TITLE);
        if let Some(stage) = args.stage {
            query = query.where_(Deal::STAGE.eq(stage.as_str()));
        }
        if let Some(words) = args.query {
            query = query.where_(Deal::TITLE.like(format!("%{words}%")));
        }
        if args.mine {
            query = query.where_(Deal::OWNER_ID.eq(cx.user_id()));
        }
        let rows = query.get(cx.uow()).await?.into_models();
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
        let model = Deal::create(
            cx.uow(),
            Deal {
                id: Uuid::now_v7(),
                owner_id: cx.user_id(),
                title: args.title,
                stage: "lead".to_owned(),
                amount_cents: args.amount_cents,
            },
        )
        .await?
        .into_inner();
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
        let mut deal = deal.into_inner();
        deal.stage = args.stage.as_str().to_owned();
        deal.save(cx.uow()).await?;
        Ok(deal.into_inner().into())
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
        let mut deal = deal.into_inner();
        deal.title = args.title;
        deal.save(cx.uow()).await?;
        Ok(deal.into_inner().into())
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
        Ok(deal.into_inner().into_inner().into())
    }

    /// The pipeline: how many deals each stage has.
    #[mcp_resource("crm://pipeline", mime = "application/json", title = "Pipeline")]
    #[can(Read, Deal)]
    async fn pipeline(&self, cx: &McpContext) -> Result<Value, AppError> {
        let rows = cx
            .accessible::<Deal, Read>()
            .query()
            .get(cx.uow())
            .await?
            .into_models();
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
        m.import(AccessModule::for_feature::<Deal>())
            .migrations(MigrationSet::new("deals").migration(SchemaMigration::new(
                "m20261026_000001_create_deals",
                CreateDeals,
            )))
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
pub async fn insert_deal(app: &TestApp, owner: Uuid, title: &str, stage: &str) -> Deal {
    let db = app.get::<Db>().await;
    Deal::create(
        &*db,
        Deal {
            id: Uuid::now_v7(),
            owner_id: owner,
            title: title.to_owned(),
            stage: stage.to_owned(),
            amount_cents: 10_000,
        },
    )
    .await
    .expect("insert deal")
    .into_inner()
}

/// Every deal in the database.
pub async fn all_deals(app: &TestApp) -> Vec<Deal> {
    let db = app.get::<Db>().await;
    Deal::query()
        .order_by(Deal::TITLE)
        .get(&*db)
        .await
        .unwrap()
        .into_models()
}

/// Tool names, sorted.
pub fn names(tools: &[navree_mcp::protocol::Tool]) -> Vec<String> {
    let mut names: Vec<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
    names.sort();
    names
}

/// `Update` re-exported for tests that build specs by hand.
pub type UpdateAction = Update;
