/**
 * The landing page's code tabs. The first four are taken from the CRM
 * (apps/crm-api): the deals module, controller and `CreateDeal` DTO
 * (crates/crm-deals), and a job around crm-activities' `TaskReminders`.
 *
 * The ORM tab shows a `Deal` model and two controller methods on it: a page
 * of won deals with their companies in one `IN (..)` query, and a save that
 * writes only the columns that changed.
 *
 * The Workspaces and Testing tabs are written from the crates' APIs:
 * `CurrentWorkspace`, `WorkspaceScoped` and `WorkspaceRepo` from the
 * prelude and the `workspace` config section (navree-workspace); `TestApp`,
 * `acting_as` and `assert_error` (navree-testing). Recheck every tab when
 * the crates behind it change. Keep the tabs short, and keep them true to
 * the APIs.
 */
export type Showcase = {
  id: string;
  label: string;
  file: string;
  code: string;
  /** An optional second block, e.g. the HTTP response. */
  extra?: { file: string; lang: string; code: string };
  title: string;
  text: string;
  href: string;
  linkText: string;
};

export const showcases: Showcase[] = [
  {
    id: "module",
    label: "Modules",
    file: "src/modules/deals/mod.rs",
    title: "An app is a set of modules",
    text: "Each module declares what it imports, the services it provides, its controllers and what it exports. The container wires them by type when the app boots, and refuses to start if a dependency is missing or private, naming the module and the type. main.rs lists the modules; nothing else is global.",
    href: "/docs/architecture/modules/",
    linkText: "Read about modules",
    code: `use navree::prelude::*;

/// The sales pipeline: its routes, its service, and what
/// other modules may use.
#[module(
    imports = [CrmCoreModule],
    providers = [DealService],
    controllers = [DealsController],
    exports = [DealService],
    register = Self::wire,
)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        // A queue worker tells owners about their deals.
        m.listen::<DealAssigned, NotifyDealOwner>()
            .on::<DealWon, NotifyDealOwner>();
    }
}`,
    extra: {
      file: "src/main.rs",
      lang: "rust",
      code: `fn main() -> Result<(), BoxError> {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::defaults()))
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(AuthModule::from_config())
        // ... mail, access and the rest of the platform
        .module(DealsModule)
        .run()
}`,
    },
  },
  {
    id: "controller",
    label: "Controllers",
    file: "src/modules/deals/controller.rs",
    title: "Every route says who may call it",
    text: "A route that declares no access doesn't compile. #[public] opens a route up, #[authenticated] and #[roles] check who is signed in, and #[can] checks abilities: on a list it turns the rules into a SQL filter, and on a record it loads and checks the row before your handler runs.",
    href: "/docs/basics/access-control/",
    linkText: "Read about access control",
    code: `#[injectable]
pub struct DealsController {
    deals: Arc<DealService>,
}

#[controller("/deals", version = "1")]
impl DealsController {
    /// Lists only the deals this user may read: the rules
    /// become a SQL filter.
    #[get("/")]
    #[can(Read, Deal)]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Deal, Read>,
        Valid(Query(filter)): Valid<Query<DealFilter>>,
        page: PageQuery,
    ) -> Result<Page<DealResource>> {
        self.deals.list(&uow, scope, filter, page).await
    }

    /// Loads the deal and checks it before the handler runs:
    /// its owner, a manager or an admin may update it.
    #[patch("/{id}")]
    #[can(Update, Deal, from = "id")]
    async fn update(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        deal: Authorized<Deal, Update>,
        Valid(Json(input)): Valid<Json<UpdateDeal>>,
    ) -> Result<Json<DealResource>> {
        Ok(Json(self.deals.update(&uow, user.id, deal, input).await?))
    }
}`,
  },
  {
    id: "orm",
    label: "ORM",
    file: "src/modules/deals/models/deal.rs",
    title: "Models are structs the compiler checks",
    text: "#[derive(Model)] maps a struct to its table and gives it a typed constant per column, so comparing an amount with a string doesn't compile. Relations load for a whole page in one query, soft-deleted rows stay out, and a saved record writes only the columns that changed, in the request's transaction, with the change recorded for the audit log.",
    href: "/docs/orm/getting-started/",
    linkText: "Read about the ORM",
    code: `use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use super::{Activity, Company, Stage};

/// A deal: the \`deals\` table, one typed column constant per field.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps, soft_deletes)]
pub struct Deal {
    pub id: Uuid,
    pub company_id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[relations]
impl Deal {
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
    }
}`,
    extra: {
      file: "src/modules/deals/controller.rs",
      lang: "rust",
      code: `#[controller("/deals", version = "1")]
impl DealsController {
    /// Won deals over $1,000, newest first, with their companies:
    /// one query for the page's companies, however long the page.
    #[get("/won")]
    #[authenticated]
    async fn won(&self, uow: Uow, page: PageQuery) -> Result<Page<Record<Deal>>> {
        Ok(Deal::query()
            .where_(Deal::STAGE.eq(Stage::Won))
            .where_(Deal::AMOUNT_CENTS.gte(100_000))
            .with("company")
            .latest()
            .paginate(&uow, page)
            .await?)
    }

    /// Loads the deal or answers 404. \`save\` writes only what
    /// changed, in the request's transaction, for the audit log.
    #[patch("/{deal}/win")]
    #[authenticated]
    async fn win(&self, uow: Uow, deal: Bound<Deal>) -> Result<Json<Record<Deal>>> {
        let mut deal = deal.into_inner();
        deal.stage = Stage::Won;
        deal.save(&uow).await?;
        Ok(Json(deal))
    }
}`,
    },
  },
  {
    id: "validation",
    label: "Validation",
    file: "src/modules/deals/dto.rs",
    title: "Bad input never reaches your handler",
    text: "Valid<Json<T>> deserializes the body and runs its validation rules first. A failure is one 400 response that lists every field's errors in Navree's JSON error shape, the same shape as every other error. Below is the answer to a blank title and a negative amount with no company.",
    href: "/docs/basics/validation/",
    linkText: "Read about validation",
    code: `// Options with \`required\`: one response lists every missing field.
#[derive(Deserialize, Validate)]
pub struct CreateDeal {
    #[validate(required, custom(not_blank), length(max = 255))]
    pub title: Option<String>,
    #[validate(required)]
    pub company_id: Option<Uuid>,
    #[validate(required, range(min = 0, max = MAX_AMOUNT_CENTS))]
    pub amount_cents: Option<i64>,
    #[validate(skip)]
    pub stage: Option<Stage>,
    // ...
}

#[post("/")]
#[can(Create, Deal)]
async fn store(
    &self,
    uow: Uow,
    CurrentUser(user): CurrentUser,
    Abilities(ability): Abilities,
    Valid(Json(input)): Valid<Json<CreateDeal>>,
) -> Result<Created<DealResource>> {
    let deal = self.deals.create(&uow, &ability, user.id, input).await?;
    Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
}`,
    extra: {
      file: "400 Bad Request",
      lang: "json",
      code: `{
  "statusCode": 400,
  "message": [
    "amount_cents must not be less than 0",
    "company_id should not be empty",
    "title should not be empty"
  ],
  "error": "Bad Request",
  "errors": {
    "amount_cents": ["amount_cents must not be less than 0"],
    "company_id": ["company_id should not be empty"],
    "title": ["title should not be empty"]
  }
}`,
    },
  },
  {
    id: "queue",
    label: "Queued jobs",
    file: "src/modules/activities/jobs/task_reminders.rs",
    title: "Background work is a struct",
    text: "Workers run jobs with retries, backoff and timeouts, from the database queue. A job dispatched while a request is running waits for that request's transaction: if it rolls back, the job is never queued.",
    href: "/docs/digging-deeper/queues/",
    linkText: "Read about queues",
    code: `use navree::contracts::{Job, JobContext, JobOptions};

/// Reminds everyone of their open tasks due on \`day\`.
#[derive(Serialize, Deserialize)]
pub struct SendTaskReminders {
    pub day: DateTime<Utc>,
}

impl Job for SendTaskReminders {
    const NAME: &'static str = "crm.task_reminders";

    fn options() -> JobOptions {
        JobOptions { queue: "reminders".into(), max_attempts: 5, ..JobOptions::default() }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let reminders = ctx.app.get::<TaskReminders>().await?;
        reminders.send_for(self.day).await?;
        Ok(())
    }
}

// Register it in a module with m.job::<SendTaskReminders>(), then
// dispatch it; \`navree queue:work --queue reminders\` runs it.
queue.dispatch(SendTaskReminders { day: Utc::now() }).await?;`,
  },
  {
    id: "workspaces",
    label: "Workspaces",
    file: "src/modules/deals/controller.rs",
    title: "Every query knows its workspace",
    text: "CurrentWorkspace resolves the workspace from the subdomain, a header, the path or a token claim, loads it and checks that the user is a member, or answers 401, 403 or 404 in the usual JSON shape. WorkspaceRepo scopes every read and write to it, so another workspace's deal is a 404, never a leak.",
    href: "/docs/workspaces/introduction/",
    linkText: "Read about workspaces",
    code: `use navree::prelude::*; // CurrentWorkspace, WorkspaceRepo, WorkspaceScoped

// Deals belong to a workspace: their table has a \`workspace_id\`.
impl WorkspaceScoped for Deal {}

type Deals = WorkspaceRepo<Deal>;

#[controller("/deals", version = "1")]
impl DealsController {
    /// The deals of the workspace this request names.
    #[get("/")]
    #[authenticated]
    async fn index(
        &self,
        ws: CurrentWorkspace,
        uow: Uow,
        page: PageQuery,
    ) -> Result<Page<Record<Deal>>> {
        ws.authorize("deals.read")?;
        Ok(Deals::query(ws.id()).paginate(&uow, page).await?)
    }

    /// Another workspace's deal is a 404.
    #[get("/{id}")]
    #[authenticated]
    async fn show(
        &self,
        ws: CurrentWorkspace,
        uow: Uow,
        Path(id): Path<Uuid>,
    ) -> Result<Json<Record<Deal>>> {
        Ok(Json(Deals::find_or_fail(&uow, ws.id(), id).await?))
    }
}`,
    extra: {
      file: "config/app.toml",
      lang: "toml",
      code: `[workspace]
resolvers = ["subdomain", "header", "path", "claim"]  # first match wins
base_domain = "app.example.com"   # acme.app.example.com -> acme
header = "X-Workspace"            # X-Workspace: acme
path_prefix = "/w"                # /api/w/acme/deals -> acme
isolation = "row"                 # row | schema | database`,
    },
  },
  {
    id: "testing",
    label: "Testing",
    file: "src/modules/deals/tests/mod.rs",
    title: "Tests boot the real app, in-process",
    text: "TestApp boots your modules with a fresh SQLite database (turn on the sqlite feature in your dev-dependencies), and TestClient sends requests through the whole pipeline: request ids, authentication, access checks, the unit of work and the error format. Nothing listens on a port, so tests run in milliseconds and in parallel.",
    href: "/docs/testing/http-tests/",
    linkText: "Read about HTTP tests",
    code: `use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::json;
use uuid::Uuid;

use super::DealsModule;
use crate::modules::companies::CompaniesModule;

#[tokio::test]
async fn deals_are_closed_by_default() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(CompaniesModule)
        .module(DealsModule)
        .build()
        .await;

    // A guest is refused before any handler runs.
    app.client()
        .post("/api/v1/deals")
        .json(&json!({ "title": "Acme renewal" }))
        .send()
        .await
        .assert_error(401, "Unauthorized");

    // Signed in, but holding no permission on deals.
    app.acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/deals")
        .json(&json!({ "title": "Acme renewal" }))
        .send()
        .await
        .assert_error(403, "Forbidden");

    app.shutdown().await.unwrap();
}`,
  },
];
