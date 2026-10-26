/**
 * The landing page's code tabs. Adapted from real code: the CRM's deals
 * module and controller (apps/crm-api/crates/crm-deals), the benchmark app
 * (bench/frameworks/navis) and the navis-queue crate docs. Keep them short,
 * and keep them true to the APIs.
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
    file: "src/deals/mod.rs",
    title: "An app is a set of modules",
    text: "As in NestJS, each module declares what it imports, the services it provides, its controllers and what it exports. The container wires them by type when the app boots, and refuses to start if a dependency is missing or private, naming the module and the type.",
    href: "/docs/architecture/modules/",
    linkText: "Read about modules",
    code: `use navis::prelude::*;

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
        // Owners hear about their deals from a queue worker.
        m.listen::<DealAssigned, NotifyDealOwner>()
            .on::<DealWon, NotifyDealOwner>();
    }
}

fn main() -> Result<(), BoxError> {
    Navis::app()
        .module(ConfigModule::for_root(ConfigSources::defaults()))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(DealsModule)
        .run()
}`,
  },
  {
    id: "controller",
    label: "Controllers",
    file: "src/deals/controller.rs",
    title: "Every route says who may call it",
    text: "Routes are denied unless they declare access. #[public] opens one up, #[roles] checks roles, and #[can] checks CASL-compatible abilities, loading the record the route acts on before your handler runs.",
    href: "/docs/basics/access-control/",
    linkText: "Read about access control",
    code: `#[injectable]
pub struct DealsController {
    deals: Arc<DealService>,
}

#[controller("/deals", version = "1")]
impl DealsController {
    /// Anyone, signed in or not.
    #[get("/stages")]
    #[public]
    async fn stages(&self) -> Json<&'static [&'static str]> {
        Json(STAGES)
    }

    /// Managers and admins only.
    #[get("/forecast")]
    #[roles("admin", "manager")]
    async fn forecast(&self, uow: Uow) -> Result<Json<Forecast>> {
        Ok(Json(self.deals.forecast(&uow).await?))
    }

    /// Whoever may update this deal: its owner, or a manager.
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
    id: "validation",
    label: "Validation",
    file: "src/deals/dto.rs",
    title: "Bad input never reaches your handler",
    text: "Valid<Json<T>> deserializes the body and runs its garde rules first. A failure is one 400 response listing every field's errors, in the NestJS format your front end already understands.",
    href: "/docs/basics/validation/",
    linkText: "Read about validation",
    code: `#[derive(Deserialize, Validate)]
pub struct CreateDeal {
    #[garde(length(chars, min = 1, max = 200))]
    pub title: String,
    #[garde(range(min = 0, max = MAX_AMOUNT_CENTS))]
    pub amount_cents: i64,
    #[garde(skip)]
    pub stage: Stage,
}

#[post("/")]
#[can(Create, Deal)]
async fn store(
    &self,
    uow: Uow,
    CurrentUser(user): CurrentUser,
    Valid(Json(input)): Valid<Json<CreateDeal>>,
) -> Result<Created<DealResource>> {
    let deal = self.deals.create(&uow, &user, input).await?;
    Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
}`,
    extra: {
      file: "400 Bad Request",
      lang: "json",
      code: `{
  "statusCode": 400,
  "message": ["amount_cents must not be less than 0"],
  "error": "Bad Request",
  "errors": { "amount_cents": ["amount_cents must not be less than 0"] }
}`,
    },
  },
  {
    id: "queue",
    label: "Queued jobs",
    file: "src/billing/jobs.rs",
    title: "Background work is a struct",
    text: "Workers run jobs with retries, backoff and timeouts, from the database queue. A job dispatched while a request is running waits for that request's transaction: if it rolls back, the job is never queued.",
    href: "/docs/digging-deeper/queues/",
    linkText: "Read about queues",
    code: `use navis::contracts::{Job, JobContext, JobOptions};

#[derive(Serialize, Deserialize)]
pub struct SendInvoice {
    pub invoice_id: u64,
}

impl Job for SendInvoice {
    const NAME: &'static str = "billing.send_invoice";

    fn options() -> JobOptions {
        JobOptions { queue: "emails".into(), max_attempts: 5, ..JobOptions::default() }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let invoices = ctx.app.get::<InvoiceService>().await?;
        invoices.send(self.invoice_id).await?;
        Ok(())
    }
}

// Register it in a module: m.job::<SendInvoice>();
// Then, anywhere (a worker runs it: \`app queue:work --queue emails\`):
queue.dispatch(SendInvoice { invoice_id: 42 }).await?;`,
  },
];
