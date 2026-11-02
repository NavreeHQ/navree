//! Shared fixtures: `companies` and `deals` as navree-orm models with their
//! resources, the rules of a small sales team, and helpers to seed users
//! and records, on the database `NAVREE_TEST_DATABASE` names (see
//! [`servers`](crate::servers)).

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, ActionMarker, CREATE, DELETE, READ,
    Rules, UPDATE, field,
};
use navree_contracts::{Clock, FixedClock};
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt};
use navree_helm::{
    Action, ActionResult, Field, Filter, FormData, HelmModule, Metric, ModuleBuilderHelmExt,
    RecordOf, Resource, SaveContext,
};
use navree_http::AppError;
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::{Builder, Model, Record};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The super-admin role.
pub const ADMIN: &str = "admin";

/// "Now" in every test app.
pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()
}

/// A company. Its API token is a secret: hidden from serialized records
/// (`#[model(hidden)]`), so it never reaches a change event or the audit
/// log, even when the panel edits it.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "companies",
    subject = "Company",
    permission_prefix = "companies"
)]
pub struct Company {
    pub id: Uuid,
    pub name: String,
    pub city: Option<String>,
    pub website: Option<String>,
    pub owner_id: Option<Uuid>,
    #[model(hidden)]
    pub api_token: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A deal.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    pub currency: String,
    pub stage: String,
    pub company_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    pub expected_close_date: Option<NaiveDate>,
    pub notes: Option<String>,
    pub priority: bool,
    pub closed_at: Option<DateTime<Utc>>,
    pub lost_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

/// A UUID column: 16 bytes on MySQL, as navree-db binds UUIDs there.
pub fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

pub struct CreateTables;

impl Migration for CreateTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("companies", |t| {
                uuid(t, "id", mysql).primary();
                t.string("name");
                t.string("city").nullable();
                t.string("website").nullable();
                uuid(t, "owner_id", mysql).nullable();
                t.string("api_token").nullable();
                t.timestamp_tz("deleted_at").nullable();
                t.timestamp_tz("created_at");
            })
            .await?;
        schema
            .create("deals", |t| {
                uuid(t, "id", mysql).primary();
                t.string("title");
                t.big_integer("amount_cents");
                t.string("currency");
                t.string("stage");
                uuid(t, "company_id", mysql).nullable();
                uuid(t, "owner_id", mysql).nullable();
                t.date("expected_close_date").nullable();
                t.text("notes").nullable();
                t.boolean("priority");
                t.timestamp_tz("closed_at").nullable();
                t.string("lost_reason").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await?;
        schema.drop_if_exists("companies").await
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
///   but not their notes;
/// - `no-title`, `no-amount` and the like read deals and companies but not that
///   column; `title-if-owner` users read every deal but the title of their own
///   only.
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
    for (role, column) in [
        ("no-title", "title"),
        ("no-amount", "amount_cents"),
        ("no-stage", "stage"),
        ("no-company", "company_id"),
        ("no-close-date", "expected_close_date"),
        ("no-currency", "currency"),
        ("no-probability", "probability"),
    ] {
        if user.has_role(role) {
            rules.can(READ, "Deal");
            rules.can(READ, "Company");
            rules.cannot(READ, "Deal").fields([column]);
        }
    }
    if user.has_role("no-company-name") {
        rules.can(READ, "Deal");
        rules.can(READ, "Company");
        rules.cannot(READ, "Company").fields(["name"]);
    }
    if user.has_role("title-if-owner") {
        rules.can(READ, "Deal");
        rules
            .cannot(READ, "Deal")
            .fields(["title"])
            .when(field("owner_id").ne(user.id));
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
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(range(min = 0))]
    pub amount_cents: i64,
    #[validate(skip)]
    pub currency: Option<String>,
    #[validate(skip)]
    pub stage: Option<String>,
    #[validate(skip)]
    pub company_id: Option<Uuid>,
    #[validate(skip)]
    pub expected_close_date: Option<NaiveDate>,
    #[validate(skip)]
    pub notes: Option<String>,
    #[validate(skip)]
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
    type Model = Deal;

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
            Field::computed("probability", |deal: &Deal| {
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

    fn filters(&self) -> Vec<Filter<Deal>> {
        vec![
            Filter::select("stage").options(STAGES),
            Filter::belongs_to::<CompanyResource>("company_id").label("Company"),
            Filter::date_range("expected_close_date"),
            Filter::boolean("open", "Open deals only", |query, on| {
                if on {
                    query.where_(Deal::CLOSED_AT.is_null())
                } else {
                    query
                }
            }),
            Filter::custom("min_amount", "At least (cents)", |query, value| match value
                .parse::<i64>()
            {
                Ok(min) => query.where_(Deal::AMOUNT_CENTS.gte(min)),
                Err(_) => query,
            })
            .reads([Deal::AMOUNT_CENTS]),
        ]
    }

    fn actions(&self) -> Vec<Action<Deal>> {
        vec![
            Action::new("mark-won", "Mark as won")
                .ability(Move)
                .confirm("Mark the selected deals as won?")
                .handle(|ctx, deals: Vec<Record<Deal>>| async move {
                    let count = deals.len();
                    for mut deal in deals {
                        deal.update(ctx.uow(), |d| {
                            d.stage = "won".to_owned();
                            d.closed_at = Some(now());
                        })
                        .await?;
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
                .handle(|ctx, deals: Vec<Record<Deal>>| async move {
                    let reason: String = ctx.input("lost_reason")?;
                    for mut deal in deals {
                        deal.update(ctx.uow(), |d| {
                            d.stage = "lost".to_owned();
                            d.lost_reason = Some(reason.clone());
                        })
                        .await?;
                    }
                    Ok(ActionResult::message("Deals marked as lost."))
                }),
            Action::new("export", "Export")
                .queued()
                .only_on_index()
                .handle(|ctx, mut deals: Vec<Record<Deal>>| async move {
                    let user = ctx.user_id();
                    deals[0]
                        .update(ctx.uow(), |d| {
                            d.notes = Some(format!("exported by {user}"));
                        })
                        .await?;
                    Ok(ActionResult::download("/files/deals.csv"))
                }),
            Action::new("fail", "Fail").only_on_detail().handle(
                |ctx, mut deals: Vec<Record<Deal>>| async move {
                    deals[0]
                        .update(ctx.uow(), |d| d.title = "changed before failing".to_owned())
                        .await?;
                    Err(AppError::conflict("This deal is locked."))
                },
            ),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Deal>> {
        vec![
            Metric::value("Open pipeline")
                .sum(Deal::AMOUNT_CENTS)
                .filter(Deal::CLOSED_AT.is_null())
                .money("USD")
                .on_dashboard(),
            Metric::value("New deals").count().over(Deal::CREATED_AT),
            Metric::trend("Deals by day")
                .count()
                .by_day(Deal::CREATED_AT, 7),
            Metric::partition("Deals by stage")
                .count()
                .group_by(Deal::STAGE),
            Metric::partition("Pipeline by company")
                .sum(Deal::AMOUNT_CENTS)
                .group_by(Deal::COMPANY_ID)
                .money("USD"),
        ]
    }

    fn create<'a>(
        &'a self,
        ctx: &'a SaveContext,
        form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>, AppError>> {
        Box::pin(async move {
            let input: NewDeal = form.parse().await?;
            let at = now();
            let mut deal = Record::new(Deal {
                id: Uuid::now_v7(),
                title: input.title,
                amount_cents: input.amount_cents,
                currency: input.currency.unwrap_or_else(|| "USD".to_owned()),
                stage: input.stage.unwrap_or_else(|| "lead".to_owned()),
                company_id: input.company_id,
                owner_id: Some(ctx.user_id()),
                expected_close_date: input.expected_close_date,
                notes: input.notes,
                priority: input.priority.unwrap_or(false),
                closed_at: None,
                lost_reason: None,
                created_at: at,
                updated_at: at,
            });
            deal.save(ctx.uow()).await?;
            Ok(deal)
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
    type Model = Company;

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
            Field::text("api_token").label("API token").hide_on_index(),
            Field::has_many::<DealResource>("company_id").label("Deals"),
        ]
    }

    fn query(&self, query: Builder<Company>) -> Builder<Company> {
        query.where_(Company::DELETED_AT.is_null())
    }
}

/// The CRM feature: its tables and its resources.
pub struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("crm").migration(SchemaMigration::new(
            "m20261026_000001_create_crm_tables",
            CreateTables,
        )))
        .resource::<DealResource>()
        .resource::<CompanyResource>();
    }
}

/// The test app builder: a fresh database on the current server, a fixed
/// clock, the access module with [`options`], the CRM feature and the
/// panel.
pub fn builder() -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
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
pub async fn company(app: &TestApp, name: &str, city: Option<&str>) -> Company {
    let db = app.get::<Db>().await;
    let company = Company {
        id: Uuid::now_v7(),
        name: name.to_owned(),
        city: city.map(str::to_owned),
        website: None,
        owner_id: None,
        api_token: None,
        deleted_at: None,
        created_at: now(),
    };
    Company::insert(&*db, [company.clone()]).await.unwrap();
    company
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
pub async fn deal(app: &TestApp, seed: DealSeed) -> Deal {
    let db = app.get::<Db>().await;
    let deal = Deal {
        id: Uuid::now_v7(),
        title: seed.title.to_owned(),
        amount_cents: seed.amount_cents,
        currency: "USD".to_owned(),
        stage: seed.stage.to_owned(),
        company_id: seed.company,
        owner_id: seed.owner,
        expected_close_date: seed.expected_close_date,
        notes: Some("**Secret** notes".to_owned()),
        priority: false,
        closed_at: seed.closed.then(now),
        lost_reason: None,
        created_at: seed.created_at,
        updated_at: seed.created_at,
    };
    Deal::insert(&*db, [deal.clone()]).await.unwrap();
    deal
}

/// Reads a deal back.
pub async fn reload(app: &TestApp, id: Uuid) -> Option<Deal> {
    let db = app.get::<Db>().await;
    Deal::find(&*db, id).await.unwrap().map(Record::into_inner)
}

/// Reads a company back (soft-deleted ones too).
pub async fn reload_company(app: &TestApp, id: Uuid) -> Option<Company> {
    let db = app.get::<Db>().await;
    Company::find(&*db, id)
        .await
        .unwrap()
        .map(Record::into_inner)
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
