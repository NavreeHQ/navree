//! The examples of `content/docs/microservices/graphql.mdx`, compiled and
//! run: the deal types, `DealResolver`, `CompanyLoader`, `MoveDealInput`,
//! the margin field and the documented test. They name `navree_graphql`
//! and `navree_macros` where the docs name `navree::graphql` and the
//! prelude (the facade cannot be a dependency of this crate); the facade's
//! own `graphql` test covers those paths.

// The examples' types are `pub`, as an app's are.
#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::futures_util::{Stream, StreamExt};
use async_graphql::{ComplexObject, Enum, InputObject, SimpleObject};
use navree_access::{AbilityCtx, AccessModule, AccessOptions, ActionMarker, Read, Rules, field};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, Db, DbError, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_events::EventsModule;
use navree_graphql::{
    Context, ContextExt, EventStream, GraphqlModule, GraphqlSchema, Loader,
    ModuleBuilderGraphqlExt, Result,
};
use navree_kernel::ModuleBuilder;
use navree_macros::{injectable, module, resolver};
use navree_orm::Model as _;
use navree_orm::access::AccessibleQuery as _;
use navree_testing::{TestApp, principal};
use navree_validation::Valid;
use navree_validation::Validate;
use serde_json::{Value, json};
use uuid::Uuid;

/// What `crm_contracts` holds in the docs.
mod crm_contracts {
    pub mod deals {
        use serde::{Deserialize, Serialize};
        use uuid::Uuid;

        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Stage {
            Lead,
            Qualified,
            Proposal,
            Negotiation,
            Won,
            Lost,
        }

        impl Stage {
            pub fn as_str(self) -> &'static str {
                match self {
                    Stage::Lead => "lead",
                    Stage::Qualified => "qualified",
                    Stage::Proposal => "proposal",
                    Stage::Negotiation => "negotiation",
                    Stage::Won => "won",
                    Stage::Lost => "lost",
                }
            }

            pub fn parse(text: &str) -> Stage {
                match text {
                    "qualified" => Stage::Qualified,
                    "proposal" => Stage::Proposal,
                    "negotiation" => Stage::Negotiation,
                    "won" => Stage::Won,
                    "lost" => Stage::Lost,
                    _ => Stage::Lead,
                }
            }
        }

        #[derive(Clone, Debug)]
        pub struct DealView {
            pub id: Uuid,
            pub title: String,
            pub amount_cents: i64,
            pub currency: String,
            pub stage: Stage,
            pub owner_id: Uuid,
            pub company_id: Uuid,
        }

        #[derive(Clone, Debug)]
        pub struct MoveDeal {
            pub id: Uuid,
            pub stage: Stage,
            pub position: Option<i64>,
            pub lost_reason: Option<String>,
        }

        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub struct DealUpdated {
            pub id: Uuid,
            pub title: String,
            pub stage: Stage,
        }

        impl navree_events::Event for DealUpdated {
            const NAME: &'static str = "deal.updated";
        }
    }
}

use crm_contracts::deals::{DealUpdated, DealView, MoveDeal};

/// What `crm_core` holds in the docs: the models and the `Move` action.
mod models {
    pub mod deal {
        use serde::Serialize;
        use uuid::Uuid;

        #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
        #[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
        pub struct Deal {
            pub id: Uuid,
            pub title: String,
            pub amount_cents: i64,
            pub currency: String,
            pub stage: String,
            pub owner_id: Uuid,
            pub company_id: Uuid,
        }
    }

    pub mod company {
        use chrono::{DateTime, Utc};
        use serde::Serialize;
        use uuid::Uuid;

        #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
        #[model(table = "companies")]
        pub struct Company {
            pub id: Uuid,
            pub name: String,
            pub deleted_at: Option<DateTime<Utc>>,
        }
    }
}

use models::company::Company;
use models::deal::Deal;

/// The `move` action on deals.
struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = "move";
}

struct CreateTables;

impl Migration for CreateTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("companies", |t| {
                t.uuid("id").primary();
                t.string("name");
                t.timestamp_tz("deleted_at").nullable();
            })
            .await?;
        schema
            .create("deals", |t| {
                t.uuid("id").primary();
                t.string("title");
                t.big_integer("amount_cents");
                t.string("currency");
                t.string("stage");
                t.uuid("owner_id");
                t.uuid("company_id");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await?;
        schema.drop_if_exists("companies").await
    }
}

/// Reps read and move their own deals; anyone with `deals.read_margin`
/// reads margins.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    rules
        .can("read", "Deal")
        .when(field("owner_id").eq(user.id));
    rules
        .can("move", "Deal")
        .when(field("owner_id").eq(user.id));
}

// ---------------------------------------------------------------------------
// Types (docs: "Types")
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Deal", complex)]
pub struct DealObject {
    pub id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    pub currency: String,
    pub stage: Stage,
    pub owner_id: Uuid,
    #[graphql(skip)]
    pub company_id: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
#[graphql(remote = "crm_contracts::deals::Stage")]
pub enum Stage {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
    Won,
    Lost,
}

impl From<DealView> for DealObject {
    fn from(deal: DealView) -> Self {
        Self {
            id: deal.id,
            title: deal.title,
            amount_cents: deal.amount_cents,
            currency: deal.currency,
            stage: deal.stage.into(),
            owner_id: deal.owner_id,
            company_id: deal.company_id,
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Company")]
pub struct CompanyObject {
    pub id: Uuid,
    pub name: String,
}

impl From<Company> for CompanyObject {
    fn from(company: Company) -> Self {
        Self {
            id: company.id,
            name: company.name,
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "DealUpdated")]
pub struct DealUpdatedObject {
    pub id: Uuid,
    pub title: String,
    pub stage: Stage,
}

impl From<DealUpdated> for DealUpdatedObject {
    fn from(event: DealUpdated) -> Self {
        Self {
            id: event.id,
            title: event.title,
            stage: event.stage.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Validation (docs: "Validation")
// ---------------------------------------------------------------------------

#[derive(Debug, InputObject, Validate)]
pub struct MoveDealInput {
    #[validate(skip)]
    pub stage: Stage,
    #[validate(range(min = 0))]
    pub position: Option<i64>,
    #[validate(length(max = 1000))]
    pub lost_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// The service the resolver calls
// ---------------------------------------------------------------------------

#[injectable]
pub struct DealService;

fn view(deal: Deal) -> DealView {
    DealView {
        id: deal.id,
        title: deal.title,
        amount_cents: deal.amount_cents,
        currency: deal.currency,
        stage: crm_contracts::deals::Stage::parse(&deal.stage),
        owner_id: deal.owner_id,
        company_id: deal.company_id,
    }
}

impl DealService {
    async fn view(&self, _uow: &Uow, deal: navree_orm::Record<Deal>) -> Result<DealView> {
        Ok(view(deal.into_inner()))
    }

    async fn find(
        &self,
        uow: &Uow,
        scope: navree_access::Accessible<Deal, Read>,
        stage: Option<crm_contracts::deals::Stage>,
        limit: u64,
    ) -> Result<Vec<DealView>> {
        let mut query = scope.query().order_by(Deal::TITLE);
        if let Some(stage) = stage {
            query = query.where_(Deal::STAGE.eq(stage.as_str()));
        }
        let deals = query.limit(limit).get(uow).await?;
        Ok(deals.into_models().into_iter().map(view).collect())
    }

    async fn move_to(
        &self,
        uow: &Uow,
        _me: Uuid,
        mut deal: navree_orm::Record<Deal>,
        to: MoveDeal,
    ) -> Result<DealView> {
        assert_eq!(deal.id, to.id);
        if to.stage == crm_contracts::deals::Stage::Lost && to.lost_reason.is_none() {
            return Err(async_graphql::Error::new("Say why the deal was lost"));
        }
        assert!(to.position.is_none_or(|position| position >= 0));
        deal.stage = to.stage.as_str().to_owned();
        deal.save(uow).await?;
        Ok(view(deal.into_inner()))
    }
}

// ---------------------------------------------------------------------------
// Resolvers (docs: "Resolvers", "Subscriptions")
// ---------------------------------------------------------------------------

#[injectable]
pub struct DealResolver {
    deals: Arc<DealService>,
}

#[resolver]
impl DealResolver {
    /// One deal.
    #[query]
    #[can(Read, Deal, from = "id")]
    async fn deal(&self, ctx: &Context<'_>, id: Uuid) -> Result<DealObject> {
        let _ = id;
        let deal = ctx.authorized::<Deal, Read>()?.into_inner();
        Ok(self.deals.view(&ctx.uow()?, deal).await?.into())
    }

    /// The deals the viewer may read, optionally in one stage.
    #[query]
    #[can(Read, Deal)]
    async fn deals(
        &self,
        ctx: &Context<'_>,
        stage: Option<Stage>,
        #[graphql(default = 50, validator(maximum = 100))] limit: u64,
    ) -> Result<Vec<DealObject>> {
        let uow = ctx.uow()?;
        let scope = ctx.accessible::<Deal, Read>().await?;
        let deals = self
            .deals
            .find(&uow, scope, stage.map(Into::into), limit)
            .await?;
        Ok(deals.into_iter().map(Into::into).collect())
    }

    /// Moves a deal to another stage.
    #[mutation]
    #[can(Move, Deal, from = "id")]
    async fn move_deal(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: Valid<MoveDealInput>,
    ) -> Result<DealObject> {
        let uow = ctx.uow()?;
        let me = ctx.principal()?.user_id;
        let deal = ctx.authorized::<Deal, Move>()?.into_inner();
        let Valid(MoveDealInput {
            stage,
            position,
            lost_reason,
        }) = input;
        let to = MoveDeal {
            id,
            stage: stage.into(),
            position,
            lost_reason,
        };
        Ok(self.deals.move_to(&uow, me, deal, to).await?.into())
    }

    /// Deal changes, optionally for one stage.
    #[subscription]
    #[can(Read, Deal)]
    async fn deal_updated(
        &self,
        ctx: &Context<'_>,
        stage: Option<Stage>,
    ) -> Result<impl Stream<Item = DealUpdatedObject>> {
        let updates = EventStream::<DealUpdated>::new(ctx)?;
        Ok(updates
            .filter(move |event| std::future::ready(stage.is_none_or(|s| event.stage == s.into())))
            .map(DealUpdatedObject::from))
    }
}

// ---------------------------------------------------------------------------
// Access on nested fields and dataloaders (docs: "Access", "Dataloaders")
// ---------------------------------------------------------------------------

#[ComplexObject]
impl DealObject {
    /// The margin, for users who may read it.
    async fn margin_cents(&self, ctx: &Context<'_>) -> Result<Option<i64>> {
        let abilities = ctx.abilities().await?;
        if !abilities.can("read_margin", "Deal", None) {
            return Ok(None);
        }
        Ok(Some(self.amount_cents / 5))
    }

    async fn company(&self, ctx: &Context<'_>) -> Result<Option<CompanyObject>> {
        Ok(ctx
            .loader::<CompanyLoader>()
            .load_one(self.company_id)
            .await?)
    }
}

#[injectable]
pub struct CompanyLoader {
    db: Arc<Db>,
}

impl Loader<Uuid> for CompanyLoader {
    type Value = CompanyObject;
    type Error = Arc<DbError>;

    async fn load(
        &self,
        ids: &[Uuid],
    ) -> std::result::Result<HashMap<Uuid, CompanyObject>, Self::Error> {
        let companies = Company::query()
            .where_in(Company::ID, ids.iter().copied())
            .where_null(Company::DELETED_AT)
            .get(&*self.db)
            .await
            .map_err(|error| match error.db_error() {
                Some(db) => Arc::new(db.clone()),
                None => Arc::new(DbError::new(navree_db::ErrorKind::Other, error.to_string())),
            })?;
        Ok(companies
            .into_models()
            .into_iter()
            .map(|c| (c.id, c.into()))
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Registering resolvers (docs: "Registering resolvers")
// ---------------------------------------------------------------------------

#[module(register = Self::wire, providers = [DealService])]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Deal>()).migrations(
            MigrationSet::new("deals").migration(SchemaMigration::new(
                "m20261026_000001_create_deals_and_companies",
                CreateTables,
            )),
        );
        m.resolver::<DealResolver>().dataloader::<CompanyLoader>();
    }
}

// ---------------------------------------------------------------------------
// Testing (docs: "Testing")
// ---------------------------------------------------------------------------

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule)
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(GraphqlModule::from_config())
        .module(DealsModule)
        .build()
        .await
}

async fn seed_rep_with_deal(app: &TestApp) -> (Uuid, Uuid) {
    let db = app.get::<Db>().await;
    let company = Company::create(
        &*db,
        Company {
            id: Uuid::now_v7(),
            name: "Agra".to_owned(),
            deleted_at: None,
        },
    )
    .await
    .unwrap();
    let rep = Uuid::now_v7();
    let deal = Deal::create(
        &*db,
        Deal {
            id: Uuid::now_v7(),
            title: "Agra renewal".to_owned(),
            amount_cents: 500_000,
            currency: "USD".to_owned(),
            stage: "negotiation".to_owned(),
            owner_id: rep,
            company_id: company.id,
        },
    )
    .await
    .unwrap();
    (rep, deal.id)
}

#[tokio::test]
async fn deal_by_id_respects_access() {
    let app = app().await;
    let (rep, deal_id) = seed_rep_with_deal(&app).await;
    let query = json!({
        "query": "query($id: UUID!) { deal(id: $id) { title stage company { name } } }",
        "variables": { "id": deal_id },
    });

    app.client()
        .post("/graphql")
        .json(&query)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!(null))
        .assert_json_path("errors.0.extensions.code", json!("UNAUTHENTICATED"));

    app.acting_as(principal(rep))
        .post("/graphql")
        .json(&query)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.deal.title", json!("Agra renewal"))
        .assert_json_path("data.deal.stage", json!("NEGOTIATION"))
        .assert_json_path("data.deal.company.name", json!("Agra"));

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_validation_error_has_the_documented_shape() {
    let app = app().await;
    let (rep, deal_id) = seed_rep_with_deal(&app).await;
    let body = app
        .acting_as(principal(rep))
        .post("/graphql")
        .json(&json!({
            "query": "mutation($id: UUID!) {\n  moveDeal(id: $id, input: { stage: WON, position: -1 }) { stage }\n}",
            "variables": { "id": deal_id },
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "data": null,
            "errors": [
                {
                    "message": "position must not be less than 0",
                    "path": ["moveDeal"],
                    "locations": [{ "line": 2, "column": 3 }],
                    "extensions": {
                        "code": "BAD_REQUEST",
                        "statusCode": 400,
                        "error": "Bad Request",
                        "errors": { "input.position": ["position must not be less than 0"] }
                    }
                }
            ]
        })
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_documented_resolver_lists_moves_and_hides_margins() {
    let app = app().await;
    let (rep, deal_id) = seed_rep_with_deal(&app).await;
    let schema = app.get::<GraphqlSchema>().await;
    let request = async_graphql::Request::new(
        "mutation($id: UUID!) { moveDeal(id: $id, input: { stage: WON }) { stage marginCents } }",
    )
    .variables(async_graphql::Variables::from_json(
        json!({ "id": deal_id }),
    ))
    .data(principal(rep));
    let response = schema.execute(request).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "moveDeal": { "stage": "WON", "marginCents": null } })
    );

    let response = schema
        .execute(
            async_graphql::Request::new("{ deals(stage: WON, limit: 10) { title amountCents } }")
                .data(principal(rep)),
        )
        .await;
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "deals": [{ "title": "Agra renewal", "amountCents": 500000 }] })
    );
    let too_many = schema
        .execute(
            async_graphql::Request::new("{ deals(limit: 500) { title } }").data(principal(rep)),
        )
        .await;
    assert_eq!(too_many.errors.len(), 1);

    let sdl = schema.sdl();
    assert!(sdl.contains("amountCents: Int!"), "{sdl}");
    assert!(sdl.contains("NEGOTIATION"), "{sdl}");
    assert!(
        sdl.contains("dealUpdated(stage: Stage): DealUpdated!"),
        "{sdl}"
    );
    app.shutdown().await.unwrap();
}
