//! The examples of the Helm docs page (`tools/helm.mdx`), compiled and run
//! through the facade: navree-orm models, resources written with
//! `#[injectable]` and the prelude, registered with `m.resource::<R>()`,
//! and the page's test.
#![cfg(feature = "sqlite")]

use navree::BoxFuture;
use navree::helm::{
    Action, ActionResult, Field, Filter, FormData, HelmPage, Metric, RecordOf, Resource,
    SaveContext,
};
use navree::orm::Builder;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::json;

mod models {
    use chrono::{DateTime, NaiveDate, Utc};
    use navree::prelude::*;
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// Where a deal is in the pipeline, stored as its serde name.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Stage {
        #[default]
        Lead,
        Won,
        Lost,
    }

    impl Stage {
        pub const ALL: [Stage; 3] = [Stage::Lead, Stage::Won, Stage::Lost];

        pub fn as_str(self) -> &'static str {
            match self {
                Stage::Lead => "lead",
                Stage::Won => "won",
                Stage::Lost => "lost",
            }
        }

        pub fn label(self) -> &'static str {
            match self {
                Stage::Lead => "Lead",
                Stage::Won => "Won",
                Stage::Lost => "Lost",
            }
        }

        pub fn probability(self) -> u8 {
            match self {
                Stage::Lead => 10,
                Stage::Won => 100,
                Stage::Lost => 0,
            }
        }
    }

    // The docs' models: each is an access subject.
    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
    pub struct Deal {
        pub id: Uuid,
        pub title: String,
        pub amount_cents: i64,
        pub currency: String,
        #[model(cast = "string_enum")]
        pub stage: Stage,
        pub company_id: Uuid,
        pub contact_id: Option<Uuid>,
        pub owner_id: Option<Uuid>,
        pub expected_close_date: Option<NaiveDate>,
        pub lost_reason: Option<String>,
        pub closed_at: Option<DateTime<Utc>>,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(
        table = "companies",
        subject = "Company",
        permission_prefix = "companies"
    )]
    pub struct Company {
        pub id: Uuid,
        pub name: String,
        pub website: Option<String>,
        pub city: Option<String>,
        pub owner_id: Option<Uuid>,
        pub deleted_at: Option<DateTime<Utc>>,
    }

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(
        table = "contacts",
        subject = "Contact",
        permission_prefix = "contacts"
    )]
    pub struct Contact {
        pub id: Uuid,
        pub name: String,
    }
}

use models::{Company, Contact, Deal, Stage};

/// The `move` action on deals.
struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = "move";
}

#[derive(Clone, Debug, Default)]
struct MoveDeal {
    stage: Option<Stage>,
    lost_reason: Option<String>,
    /// Where in the stage's column (unused here).
    position: Option<u32>,
}

#[derive(Debug, serde::Deserialize, navree::validation::Validate)]
struct UpdateDeal {
    #[validate(length(min = 1, max = 200))]
    title: Option<String>,
}

/// The app's service, which the resource saves through.
#[injectable]
struct DealService;

impl DealService {
    async fn move_to(
        &self,
        uow: &Uow,
        _user: uuid::Uuid,
        mut deal: Record<Deal>,
        input: MoveDeal,
    ) -> Result<Record<Deal>> {
        let _ = input.position;
        deal.update(uow, |d| {
            if let Some(stage) = input.stage {
                d.stage = stage;
            }
            if input.lost_reason.is_some() {
                d.lost_reason = input.lost_reason;
            }
        })
        .await?;
        Ok(deal)
    }

    async fn update_model(
        &self,
        uow: &Uow,
        _user: uuid::Uuid,
        mut deal: Record<Deal>,
        input: UpdateDeal,
    ) -> Result<Record<Deal>> {
        if let Some(title) = input.title {
            deal.update(uow, |d| d.title = title).await?;
        }
        Ok(deal)
    }
}

#[injectable]
struct UserResource;

impl Resource for UserResource {
    type Model = navree::auth::User;
    const LABEL: &'static str = "Users";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "email"];

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name").sortable(), Field::email("email")]
    }
}

#[injectable]
struct ContactResource;

impl Resource for ContactResource {
    type Model = Contact;
    const LABEL: &'static str = "Contacts";
    const TITLE: &'static str = "name";

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name")]
    }
}

#[injectable]
struct CompanyResource;

impl Resource for CompanyResource {
    type Model = Company;
    const LABEL: &'static str = "Companies";
    const SINGULAR: &'static str = "Company";
    const GROUP: &'static str = "Sales";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name"];

    // The docs' "has_many" example.
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().sortable(),
            Field::url("website"),
            Field::text("city").sortable(),
            Field::belongs_to::<UserResource>("owner_id").label("Owner"),
            Field::has_many::<DealResource>("company_id").label("Deals"),
        ]
    }

    // The docs' "Scoping the query" example.
    fn query(&self, query: Builder<Company>) -> Builder<Company> {
        query.where_(Company::DELETED_AT.is_null())
    }
}

// The docs' complete resource, verbatim apart from the module paths.
#[injectable]
struct DealResource;

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
            Field::select("stage")
                .options(Stage::ALL.iter().map(|s| (s.as_str(), s.label())))
                .badge()
                .sortable(),
            Field::belongs_to::<CompanyResource>("company_id")
                .label("Company")
                .required(),
            Field::belongs_to::<ContactResource>("contact_id").label("Contact"),
            Field::belongs_to::<UserResource>("owner_id").label("Owner"),
            Field::date("expected_close_date").sortable(),
            Field::computed("probability", |deal: &Deal| {
                format!("{}%", deal.stage.probability())
            }),
            Field::textarea("lost_reason").hide_on_index(),
            Field::datetime("closed_at").readonly(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<Deal>> {
        vec![
            Filter::select("stage").options(Stage::ALL.iter().map(|s| (s.as_str(), s.label()))),
            Filter::belongs_to::<UserResource>("owner_id").label("Owner"),
            Filter::date_range("expected_close_date"),
            Filter::boolean("open", "Open deals only", |query, on| {
                if on {
                    query.where_(Deal::CLOSED_AT.is_null())
                } else {
                    query
                }
            }),
        ]
    }

    fn actions(&self) -> Vec<Action<Deal>> {
        vec![
            Action::new("mark-won", "Mark as won")
                .ability(Move)
                .confirm("Mark the selected deals as won?")
                .handle(|ctx, deals| async move {
                    let service = ctx.get::<DealService>().await?;
                    let count = deals.len();
                    for deal in deals {
                        let input = MoveDeal {
                            stage: Some(Stage::Won),
                            ..MoveDeal::default()
                        };
                        service
                            .move_to(ctx.uow(), ctx.user_id(), deal, input)
                            .await?;
                    }
                    Ok(ActionResult::message(format!(
                        "{count} deals marked as won."
                    )))
                }),
            // The docs' action with a form.
            Action::new("mark-lost", "Mark as lost")
                .ability(Move)
                .fields(vec![
                    Field::textarea("lost_reason").label("Reason").required(),
                ])
                .handle(|ctx, deals| async move {
                    let reason: String = ctx.input("lost_reason")?;
                    let service = ctx.get::<DealService>().await?;
                    for deal in deals {
                        let input = MoveDeal {
                            stage: Some(Stage::Lost),
                            lost_reason: Some(reason.clone()),
                            ..MoveDeal::default()
                        };
                        service
                            .move_to(ctx.uow(), ctx.user_id(), deal, input)
                            .await?;
                    }
                    Ok(ActionResult::message("Deals marked as lost."))
                }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Deal>> {
        vec![
            Metric::value("Open pipeline")
                .sum(Deal::AMOUNT_CENTS)
                .filter(Deal::CLOSED_AT.is_null())
                .money("USD")
                .on_dashboard(),
            Metric::trend("New deals")
                .count()
                .by_day(Deal::CREATED_AT, 30),
            Metric::partition("Deals by stage")
                .count()
                .group_by(Deal::STAGE),
        ]
    }

    // The docs' "Saving through your services" example.
    fn update<'a>(
        &'a self,
        ctx: &'a SaveContext,
        deal: RecordOf<Self>,
        form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move {
            let deals = ctx.get::<DealService>().await?;
            let input: UpdateDeal = form.parse().await?; // validation, errors under each field
            deals
                .update_model(ctx.uow(), ctx.user_id(), deal, input)
                .await
        })
    }
}

/// The sales tables, with the schema builder.
mod m20261026_000001_create_sales_tables {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("companies", |t| {
                    t.uuid("id").primary();
                    t.string("name");
                    t.string("website").nullable();
                    t.string("city").nullable();
                    t.uuid("owner_id").nullable();
                    t.timestamp_tz("deleted_at").nullable();
                })
                .await?;
            schema
                .create("contacts", |t| {
                    t.uuid("id").primary();
                    t.string("name");
                })
                .await?;
            schema
                .create("deals", |t| {
                    t.uuid("id").primary();
                    t.string("title");
                    t.big_integer("amount_cents");
                    t.string("currency");
                    t.string("stage");
                    t.uuid("company_id");
                    t.uuid("contact_id").nullable();
                    t.uuid("owner_id").nullable();
                    t.date("expected_close_date").nullable();
                    t.string("lost_reason").nullable();
                    t.timestamp_tz("closed_at").nullable();
                    t.timestamp_tz("created_at");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("deals").await?;
            schema.drop_if_exists("contacts").await?;
            schema.drop_if_exists("companies").await
        }
    }
}

#[module(providers = [DealService], register = Self::wire)]
struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("sales").after(["auth"]).migration(
            navree::schema::migration!(m20261026_000001_create_sales_tables),
        ));
        m.resource::<DealResource>();
        m.resource::<CompanyResource>();
        m.resource::<ContactResource>();
        m.resource::<UserResource>();
        // The docs' custom page (its template needs ViewModule).
        m.helm_page(
            HelmPage::new("pipeline", "Pipeline board")
                .group("Sales")
                .icon("kanban")
                .template("admin/pipeline")
                .can("read", "Deal"),
        );
        m.templates(&[("admin/pipeline.html", "<p>{{ user.name }}: {{ title }}</p>")]);
        // navree:wire
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else { return };
    if user.has_role("rep") {
        rules.can(READ, "Deal");
        rules.can(READ, "Company");
        rules.can(READ, "User");
        rules
            .can(UPDATE, "Deal")
            .when(field("owner_id").eq(user.id));
        rules
            .can("move", "Deal")
            .when(field("owner_id").eq(user.id));
    }
}

// The docs' test helpers.
async fn boot_app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .fake_auth()
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: Some("admin".into()),
        }))
        .module(ViewModule::from_config())
        .module(DealsModule)
        .module(HelmModule::from_config())
        .build()
        .await
}

async fn create_user(app: &TestApp, email: &str, role: &str) -> navree::auth::User {
    let user = app
        .get::<AuthService>()
        .await
        .create_user("Priya", email, "a long password")
        .await
        .unwrap();
    let db = app.get::<navree::db::Db>().await;
    let access = app.get::<navree::access::AccessService>().await;
    let uow = db.uow(None, None).await;
    if access.role(role).await.unwrap().is_none() {
        access.create_role(&uow, role, None).await.unwrap();
        access.grant(&uow, role, "helm.view").await.unwrap();
    }
    access.assign_role(&uow, user.id, role).await.unwrap();
    uow.commit().await.unwrap();
    user
}

async fn create_deal(app: &TestApp, owner: uuid::Uuid, title: &str) -> Deal {
    let db = app.get::<navree::db::Db>().await;
    let company = Company {
        id: uuid::Uuid::now_v7(),
        name: "Globex".into(),
        ..Company::default()
    };
    Company::insert(&*db, [company.clone()]).await.unwrap();
    let deal = Deal {
        id: uuid::Uuid::now_v7(),
        title: title.into(),
        amount_cents: 1_250_000,
        currency: "USD".into(),
        stage: Stage::Lead,
        company_id: company.id,
        owner_id: Some(owner),
        created_at: chrono::Utc::now(),
        ..Deal::default()
    };
    Deal::insert(&*db, [deal.clone()]).await.unwrap();
    deal
}

#[tokio::test]
async fn reps_read_every_deal_but_edit_only_their_own() {
    // `boot_app`, `create_user` and `create_deal` are your tests' own helpers.
    let app = boot_app().await;
    let priya = create_user(&app, "priya@example.com", "rep").await;
    let tom = create_user(&app, "tom@example.com", "rep").await;
    let toms_deal = create_deal(&app, tom.id, "Globex renewal").await;

    let rep = app.acting_as(principal(priya.id));
    rep.get("/admin/api/resources/deals")
        .query(&[("search", "Globex")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", 1.into());

    rep.patch(&format!("/admin/api/resources/deals/{}", toms_deal.id))
        .json(&json!({ "title": "Mine now" }))
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn the_docs_resource_serves_its_screens() {
    let app = boot_app().await;
    let priya = create_user(&app, "priya@example.com", "rep").await;
    let own = create_deal(&app, priya.id, "Initech upgrade").await;
    let rep = app.acting_as(principal(priya.id));

    let body = rep
        .get(&format!("/admin/api/resources/deals/{}", own.id))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"]["display"]["amount_cents"], "$12,500.00");
    assert_eq!(body["data"]["display"]["stage"], "Lead");
    assert_eq!(body["data"]["display"]["company_id"], "Globex");
    assert_eq!(body["data"]["display"]["owner_id"], "Priya");
    assert_eq!(body["data"]["values"]["probability"], "10%");

    // Saving through the service, with the DTO's validation rules.
    rep.patch(&format!("/admin/api/resources/deals/{}", own.id))
        .json(&json!({ "title": "Initech upgrade, phase 2" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.title", json!("Initech upgrade, phase 2"));

    rep.post("/admin/api/resources/deals/actions/mark-won")
        .json(&json!({ "ids": [own.id] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("message", json!("1 deals marked as won."));
    rep.post("/admin/api/resources/deals/actions/mark-lost")
        .json(&json!({ "ids": [own.id], "input": { "lost_reason": "Budget" } }))
        .send()
        .await
        .assert_status(200);
    let db = app.get::<navree::db::Db>().await;
    let lost = Deal::find(&*db, own.id).await.unwrap().unwrap();
    assert_eq!(lost.stage, Stage::Lost);
    assert_eq!(lost.lost_reason.as_deref(), Some("Budget"));

    rep.get("/admin/api/resources/deals/metrics/deals-by-stage")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("groups.0.label", json!("Lost"));
    rep.get("/admin/api/resources/deals")
        .query(&[("filters[stage]", "lost")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", 1.into());
    rep.get("/admin/api/resources/companies")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", 1.into());

    let html = rep
        .get("/admin/pages/pipeline")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(html.contains("<p>Priya: Pipeline board</p>"), "{html}");
}
