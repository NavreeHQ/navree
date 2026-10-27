//! Shared fixtures: `companies` and `deals` with their resources, the
//! rules of a small sales team, and helpers to seed users and records.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use garde::Validate;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, ActionMarker, CREATE, DELETE, READ,
    Rules, Subject, UPDATE, field,
};
use navree_contracts::{Clock, FixedClock};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt, Repo};
use navree_helm::{
    Action, ActionResult, Field, Filter, FormData, HelmModule, Metric, ModelOf,
    ModuleBuilderHelmExt, Resource, SaveContext,
};
use navree_http::AppError;
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, ColumnTrait, QueryFilter, Select, Set};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// The super-admin role.
pub const ADMIN: &str = "admin";

/// "Now" in every test app.
pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()
}

pub mod company {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "companies")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub name: String,
        pub city: Option<String>,
        pub website: Option<String>,
        pub owner_id: Option<Uuid>,
        pub deleted_at: Option<DateTimeUtc>,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod deal {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "deals")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        pub amount_cents: i64,
        pub currency: String,
        pub stage: String,
        pub company_id: Option<Uuid>,
        pub owner_id: Option<Uuid>,
        pub expected_close_date: Option<Date>,
        pub notes: Option<String>,
        pub priority: bool,
        pub closed_at: Option<DateTimeUtc>,
        pub lost_reason: Option<String>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl Subject for company::Entity {
    const SUBJECT: &'static str = "Company";
    const PERMISSION_PREFIX: &'static str = "companies";
}

impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// The `move` action on deals.
#[derive(Clone, Copy, Debug, Default)]
pub struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = "move";
}

pub const STAGES: [(&str, &str); 4] = [
    ("lead", "Lead"),
    ("qualified", "Qualified"),
    ("won", "Won"),
    ("lost", "Lost"),
];

pub struct CreateTables;

impl MigrationName for CreateTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_crm_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("companies")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("city").string().null())
                    .col(ColumnDef::new("website").string().null())
                    .col(ColumnDef::new("owner_id").uuid().null())
                    .col(
                        ColumnDef::new("deleted_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("deals")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("title").string().not_null())
                    .col(ColumnDef::new("amount_cents").big_integer().not_null())
                    .col(ColumnDef::new("currency").string().not_null())
                    .col(ColumnDef::new("stage").string().not_null())
                    .col(ColumnDef::new("company_id").uuid().null())
                    .col(ColumnDef::new("owner_id").uuid().null())
                    .col(ColumnDef::new("expected_close_date").date().null())
                    .col(ColumnDef::new("notes").text().null())
                    .col(ColumnDef::new("priority").boolean().not_null())
                    .col(
                        ColumnDef::new("closed_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("lost_reason").string().null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new("updated_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .drop_table(Table::drop().table("deals").to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table("companies").to_owned())
            .await
    }
}

/// The team's rules:
///
/// - everyone gets their database permissions (`helm.view`, and for
///   managers `deals.*` and `companies.*`);
/// - reps read every company and deal, create deals they own, update and
///   `move` only their own deals (but never their `owner_id`), and delete
///   nothing;
/// - `limited` users read deals and may change only the title of their own;
/// - `own-only` users read their own deals; `no-secrets` users read deals
///   but not their notes.
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    if user.has_role("rep") {
        rules.can(READ, "Company");
        rules.can(READ, "Deal");
        rules
            .can(CREATE, "Deal")
            .when(field("owner_id").eq(user.id));
        rules
            .can(UPDATE, "Deal")
            .when(field("owner_id").eq(user.id));
        rules.cannot(UPDATE, "Deal").fields(["owner_id"]);
        rules
            .can("move", "Deal")
            .when(field("owner_id").eq(user.id));
    }
    if user.has_role("limited") {
        rules.can(READ, "Deal");
        rules
            .can(UPDATE, "Deal")
            .fields(["title"])
            .when(field("owner_id").eq(user.id));
    }
    if user.has_role("own-only") {
        rules.can(READ, "Deal").when(field("owner_id").eq(user.id));
    }
    if user.has_role("no-secrets") {
        rules.can(READ, "Deal");
        rules.cannot(READ, "Deal").fields(["notes"]);
    }
    let _ = DELETE;
}

pub fn options() -> AccessOptions {
    AccessOptions {
        abilities,
        super_admin_role: Some(ADMIN.to_owned()),
    }
}

/// The input of a new deal, validated like the API's DTO.
#[derive(Debug, Deserialize, Validate)]
pub struct NewDeal {
    #[garde(length(min = 1, max = 200))]
    pub title: String,
    #[garde(range(min = 0))]
    pub amount_cents: i64,
    #[garde(skip)]
    pub currency: Option<String>,
    #[garde(skip)]
    pub stage: Option<String>,
    #[garde(skip)]
    pub company_id: Option<Uuid>,
    #[garde(skip)]
    pub expected_close_date: Option<NaiveDate>,
    #[garde(skip)]
    pub notes: Option<String>,
    #[garde(skip)]
    pub priority: Option<bool>,
}

/// Deals in the panel. Creating saves through a DTO (the "service" path)
/// and makes the user the owner.
pub struct DealResource;

impl Injectable for DealResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealResource) })
    }
}

impl Resource for DealResource {
    type Entity = deal::Entity;

    const LABEL: &'static str = "Deals";
    const SINGULAR: &'static str = "Deal";
    const GROUP: &'static str = "Sales";
    const ICON: &'static str = "handshake";
    const TITLE: &'static str = "title";
    const SEARCH: &'static [&'static str] = &["title"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required().max_length(200).sortable(),
            Field::money("amount_cents", "currency")
                .label("Amount")
                .min(0)
                .sortable(),
            Field::select("currency")
                .options([("USD", "US dollar"), ("EUR", "Euro")])
                .default("USD")
                .hide_on_index(),
            Field::select("stage")
                .options(STAGES)
                .badge()
                .default("lead")
                .sortable(),
            Field::belongs_to::<CompanyResource>("company_id").label("Company"),
            Field::text("owner_id").label("Owner").hide_on_index(),
            Field::date("expected_close_date").sortable(),
            Field::markdown("notes").hide_on_index(),
            Field::boolean("priority"),
            Field::computed("probability", |deal: &deal::Model| {
                let percent = match deal.stage.as_str() {
                    "won" => 100,
                    "qualified" => 25,
                    "lost" => 0,
                    _ => 10,
                };
                format!("{percent}%")
            }),
            Field::textarea("lost_reason").hide_on_index(),
            Field::datetime("closed_at").readonly().hide_on_index(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<deal::Entity>> {
        vec![
            Filter::select("stage").options(STAGES),
            Filter::belongs_to::<CompanyResource>("company_id").label("Company"),
            Filter::date_range("expected_close_date"),
            Filter::boolean("open", "Open deals only", |select, on| {
                if on {
                    select.filter(deal::Column::ClosedAt.is_null())
                } else {
                    select
                }
            }),
            Filter::custom(
                "min_amount",
                "At least (cents)",
                |select, value| match value.parse::<i64>() {
                    Ok(min) => select.filter(deal::Column::AmountCents.gte(min)),
                    Err(_) => select,
                },
            ),
        ]
    }

    fn actions(&self) -> Vec<Action<deal::Entity>> {
        vec![
            Action::new("mark-won", "Mark as won")
                .ability(Move)
                .confirm("Mark the selected deals as won?")
                .handle(|ctx, deals: Vec<deal::Model>| async move {
                    let count = deals.len();
                    for deal in deals {
                        let mut active: deal::ActiveModel = deal.into();
                        active.stage = Set("won".to_owned());
                        active.closed_at = Set(Some(now()));
                        Repo::<deal::Entity>::update(ctx.uow(), active).await?;
                    }
                    Ok(ActionResult::message(format!(
                        "{count} deals marked as won."
                    )))
                }),
            Action::new("mark-lost", "Mark as lost")
                .ability(Move)
                .fields(vec![
                    Field::textarea("lost_reason").label("Reason").required(),
                ])
                .handle(|ctx, deals: Vec<deal::Model>| async move {
                    let reason: String = ctx.input("lost_reason")?;
                    for deal in deals {
                        let mut active: deal::ActiveModel = deal.into();
                        active.stage = Set("lost".to_owned());
                        active.lost_reason = Set(Some(reason.clone()));
                        Repo::<deal::Entity>::update(ctx.uow(), active).await?;
                    }
                    Ok(ActionResult::message("Deals marked as lost."))
                }),
            Action::new("export", "Export")
                .queued()
                .only_on_index()
                .handle(|ctx, deals: Vec<deal::Model>| async move {
                    let mut active: deal::ActiveModel = deals[0].clone().into();
                    active.notes = Set(Some(format!("exported by {}", ctx.user_id())));
                    Repo::<deal::Entity>::update(ctx.uow(), active).await?;
                    Ok(ActionResult::download("/files/deals.csv"))
                }),
            Action::new("fail", "Fail").only_on_detail().handle(
                |ctx, deals: Vec<deal::Model>| async move {
                    let mut active: deal::ActiveModel = deals[0].clone().into();
                    active.title = Set("changed before failing".to_owned());
                    Repo::<deal::Entity>::update(ctx.uow(), active).await?;
                    Err(AppError::conflict("This deal is locked."))
                },
            ),
        ]
    }

    fn metrics(&self) -> Vec<Metric<deal::Entity>> {
        vec![
            Metric::value("Open pipeline")
                .sum(deal::Column::AmountCents)
                .filter(deal::Column::ClosedAt.is_null())
                .money("USD")
                .on_dashboard(),
            Metric::value("New deals")
                .count()
                .over(deal::Column::CreatedAt),
            Metric::trend("Deals by day")
                .count()
                .by_day(deal::Column::CreatedAt, 7),
            Metric::partition("Deals by stage")
                .count()
                .group_by(deal::Column::Stage),
            Metric::partition("Pipeline by company")
                .sum(deal::Column::AmountCents)
                .group_by(deal::Column::CompanyId)
                .money("USD"),
        ]
    }

    fn create<'a>(
        &'a self,
        ctx: &'a SaveContext,
        form: FormData,
    ) -> BoxFuture<'a, Result<ModelOf<Self>, AppError>> {
        Box::pin(async move {
            let input: NewDeal = form.parse().await?;
            let at = now();
            let deal = deal::ActiveModel {
                id: Set(Uuid::now_v7()),
                title: Set(input.title),
                amount_cents: Set(input.amount_cents),
                currency: Set(input.currency.unwrap_or_else(|| "USD".to_owned())),
                stage: Set(input.stage.unwrap_or_else(|| "lead".to_owned())),
                company_id: Set(input.company_id),
                owner_id: Set(Some(ctx.user_id())),
                expected_close_date: Set(input.expected_close_date),
                notes: Set(input.notes),
                priority: Set(input.priority.unwrap_or(false)),
                closed_at: Set(None),
                lost_reason: Set(None),
                created_at: Set(at),
                updated_at: Set(at),
            };
            Ok(Repo::<deal::Entity>::insert(ctx.uow(), deal).await?)
        })
    }
}

/// Companies in the panel; soft-deleted ones are hidden.
pub struct CompanyResource;

impl Injectable for CompanyResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(CompanyResource) })
    }
}

impl Resource for CompanyResource {
    type Entity = company::Entity;

    const LABEL: &'static str = "Companies";
    const SINGULAR: &'static str = "Company";
    const GROUP: &'static str = "Sales";
    const ORDER: i32 = -1;
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "city"];
    const DEFAULT_SORT: &'static str = "name";

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().sortable(),
            Field::text("city").sortable(),
            Field::url("website"),
            Field::text("owner_id").label("Owner").hide_on_index(),
            Field::has_many::<DealResource>("company_id").label("Deals"),
        ]
    }

    fn query(&self, select: Select<company::Entity>) -> Select<company::Entity> {
        select.filter(company::Column::DeletedAt.is_null())
    }
}

/// The CRM feature: its tables and its resources.
pub struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("crm").migration(CreateTables))
            .resource::<DealResource>()
            .resource::<CompanyResource>();
    }
}

/// The test app builder: a temporary database, a fixed clock, the access
/// module with [`options`], the CRM feature and the panel.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(now())))
        .module(navree_db::DatabaseModule)
        .module(AccessModule::for_root(options()))
        .module(CrmModule)
        .module(HelmModule::from_config())
}

/// A booted test app (see [`builder`]).
pub async fn app() -> TestApp {
    builder().build().await
}

/// Creates `role` (if needed) with `permissions` and gives it to `user`.
pub async fn give_role(app: &TestApp, user: Uuid, role: &str, permissions: &[&str]) {
    let db = app.get::<Db>().await;
    let service = app.get::<AccessService>().await;
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

/// A user with `role` and `helm.view`, and a client acting as them.
pub async fn user(app: &TestApp, role: &str) -> (Uuid, TestClient) {
    let id = Uuid::now_v7();
    let mut permissions = vec!["helm.view"];
    if role == "manager" {
        permissions.extend(["deals.*", "companies.*"]);
    }
    give_role(app, id, role, &permissions).await;
    (id, app.acting_as(principal(id)))
}

/// Inserts a company.
pub async fn company(app: &TestApp, name: &str, city: Option<&str>) -> company::Model {
    let db = app.get::<Db>().await;
    company::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(name.to_owned()),
        city: Set(city.map(str::to_owned)),
        website: Set(None),
        owner_id: Set(None),
        deleted_at: Set(None),
        created_at: Set(now()),
    }
    .insert(db.conn())
    .await
    .unwrap()
}

/// What a test deal starts with.
#[derive(Clone, Debug)]
pub struct DealSeed {
    pub title: &'static str,
    pub amount_cents: i64,
    pub stage: &'static str,
    pub owner: Option<Uuid>,
    pub company: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expected_close_date: Option<NaiveDate>,
    pub closed: bool,
}

impl Default for DealSeed {
    fn default() -> Self {
        Self {
            title: "Deal",
            amount_cents: 1000,
            stage: "lead",
            owner: None,
            company: None,
            created_at: now(),
            expected_close_date: None,
            closed: false,
        }
    }
}

/// Inserts a deal.
pub async fn deal(app: &TestApp, seed: DealSeed) -> deal::Model {
    let db = app.get::<Db>().await;
    deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(seed.title.to_owned()),
        amount_cents: Set(seed.amount_cents),
        currency: Set("USD".to_owned()),
        stage: Set(seed.stage.to_owned()),
        company_id: Set(seed.company),
        owner_id: Set(seed.owner),
        expected_close_date: Set(seed.expected_close_date),
        notes: Set(Some("**Secret** notes".to_owned())),
        priority: Set(false),
        closed_at: Set(seed.closed.then(now)),
        lost_reason: Set(None),
        created_at: Set(seed.created_at),
        updated_at: Set(seed.created_at),
    }
    .insert(db.conn())
    .await
    .unwrap()
}

/// Reads a deal back.
pub async fn reload(app: &TestApp, id: Uuid) -> Option<deal::Model> {
    use sea_orm::EntityTrait;
    let db = app.get::<Db>().await;
    deal::Entity::find_by_id(id).one(db.conn()).await.unwrap()
}

/// The ids in a list response.
pub fn ids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The titles in a list response.
pub fn titles(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect()
}
