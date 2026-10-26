import type { DiagramName } from "@/components/mdx/diagram";

/**
 * The six architectures, in words: the /architecture/ page and the
 * homepage section both read this. `code` spans in strings are written
 * with backticks and rendered by `InlineMarkdown`.
 */
export type Pattern = {
  /** Anchor on /architecture/, and the diagram's name. */
  id: Exclude<DiagramName, "request-lifecycle" | "module-graph">;
  name: string;
  /** The tab label on the homepage. */
  short: string;
  /** The section heading. */
  headline: string;
  /** What it is, in two or three sentences. */
  what: string;
  /** One sentence for the homepage panel. */
  teaser: string;
  /** When to choose it. */
  choose: string[];
  /** How Navree supports it. */
  how: string[];
  code: { file: string; lang: "rust" | "toml" | "ts"; source: string };
  docs: string;
};

export const patterns: Pattern[] = [
  {
    id: "modular-monolith",
    name: "Modular monolith",
    short: "Monolith",
    headline: "One binary, hard boundaries.",
    what: "Every feature is a module that declares what it imports, provides and exports. The app ships as one binary with one database, calls between features are function calls, and the kernel refuses to boot when a module reaches for something it wasn't given.",
    teaser:
      "Every feature is a module with declared imports and exports. One binary, one database, and a kernel that refuses to boot when a boundary is crossed.",
    choose: [
      "You're starting. Every Navree app begins here.",
      "One team, or several teams on one release.",
      "A request touches three features and should commit once.",
    ],
    how: [
      "`#[module]` with imports, providers, controllers and exports",
      "A type-keyed DI container, checked at boot and by `check` in CI",
      "`navree make:module` writes a module and registers it",
      "A crate per module when teams want Cargo to hold the lines",
    ],
    code: {
      file: "src/modules/deals/mod.rs",
      lang: "rust",
      source: `#[module(
    imports = [CompaniesModule],
    providers = [DealService],
    controllers = [DealsController],
    exports = [DealService],
)]
pub struct DealsModule;`,
    },
    docs: "/docs/patterns/modular-monolith/",
  },
  {
    id: "microservices",
    name: "Microservices",
    short: "Microservices",
    headline: "Split out a service. Keep the module.",
    what: "A module that needs its own deploys, scaling or on-call moves into a binary of its own. Services answer messages and react to events over NATS, Kafka, RabbitMQ, Redis Streams, MQTT, TCP or gRPC, each owns its database, and a message handler is written like a route.",
    teaser:
      "Move a module into its own service. Messages and events travel over NATS, Kafka, RabbitMQ, Redis Streams, MQTT, TCP or gRPC, and handlers are written like routes.",
    choose: [
      "One feature scales or ships unlike the rest.",
      "A feature's failures or data must be kept apart.",
      "A team owns a feature end to end, on-call included.",
    ],
    how: [
      "`MicroModule`, with the transport in configuration",
      "`#[message_pattern]` and `#[event_pattern]` handlers with guards, `#[can]` and a unit of work",
      "`ServiceClient::send` and `emit`, with an outbox for events",
      "Request id, caller, workspace and trace context on every message",
    ],
    code: {
      file: "services/deals/src/handlers.rs",
      lang: "rust",
      source: `#[message_handlers]
impl DealsHandlers {
    #[message_pattern("deals.get")]
    #[can(Read, Deal, from = "id")]
    async fn get(&self, uow: Uow, deal: Authorized<Deal, Read>) -> Result<DealView> {
        self.deals.view(&uow, deal.into_inner()).await
    }
}`,
    },
    docs: "/docs/patterns/microservices/",
  },
  {
    id: "multi-tenancy",
    name: "Multi-tenant SaaS",
    short: "Multi-tenant",
    headline: "Every customer gets a workspace.",
    what: "Workspaces, members, roles and invitations are part of the framework and on by default. Each request resolves its workspace, checks membership and scopes its unit of work, so every query sees one customer's data, with PostgreSQL row-level security underneath.",
    teaser:
      "Workspaces are built in. Every request resolves its workspace, checks membership and scopes its queries, with isolation by row, schema or database.",
    choose: [
      "Many organizations share one deployment.",
      "Roles differ per customer: an owner here, a viewer there.",
      "Some customers need their own schema or database.",
    ],
    how: [
      "`WorkspaceModule`, in the default features",
      "The `CurrentWorkspace` extractor and `WorkspaceRepo`",
      "Isolation by row, schema or database, set in configuration",
      "Workspace roles feed `#[permissions]` and `#[can]`",
    ],
    code: {
      file: "config/app.toml",
      lang: "toml",
      source: `[workspace]
resolvers = ["header", "path"]
# row | schema | database
isolation = "row"

[workspace.roles.member]
permissions = ["projects.*"]`,
    },
    docs: "/docs/patterns/multi-tenancy/",
  },
  {
    id: "event-driven",
    name: "Event-driven",
    short: "Event-driven",
    headline: "React to changes without coupling modules.",
    what: "Modules emit typed events and listeners react, in the request or on a worker. Jobs and outgoing events are written in the same transaction as the change and leave only after it commits, so nothing hears about a change that rolled back.",
    teaser:
      "Modules emit typed events; listeners react in the request or on a worker. Jobs and outgoing events commit with the change and leave only after it.",
    choose: [
      "Side effects must not slow or fail the request.",
      "Screens should update live.",
      "Other services must hear about every committed change.",
    ],
    how: [
      "Typed events (`Event`) and `m.listen::<E, L>()`",
      "Queued listeners and jobs on `queue:work`, with retries and backoff",
      "A transactional outbox: `jobs` and `micro_outbox` rows commit with the change",
      "Broadcasting over the built-in Pusher-protocol WebSocket server",
    ],
    code: {
      file: "crates/crm-deals/src/lib.rs",
      lang: "rust",
      source: `impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<DealAssigned, NotifyDealOwner>()
            .on::<DealWon, NotifyDealOwner>();
        m.forward_event::<DealWon>(); // also publish deal.won to other services
    }
}`,
    },
    docs: "/docs/patterns/event-driven/",
  },
  {
    id: "api-first",
    name: "API-first",
    short: "API-first",
    headline: "The contract comes from the code.",
    what: "Routes declare their version and access rule, inputs are validated DTOs, outputs are typed resources and every error has one JSON shape. Navree builds an OpenAPI 3.1 document from all of it at boot, so the document, the validation and the generated clients change together.",
    teaser:
      "Versioned routes, validated DTOs and one JSON error shape. The OpenAPI 3.1 document is built from the code, and typed clients are built from the document.",
    choose: [
      "Mobile apps, partners, other teams or AI agents use your API.",
      "A breaking change should fail CI, not a client.",
      "The docs must never drift from the code.",
    ],
    how: [
      "`#[controller(\"/deals\", version = \"1\")]` and `#[api(...)]`",
      "`Valid<Json<T>>` with garde rules: field errors before the handler runs",
      "`ApiModule` serves `/api/openapi.json` and `/api/docs`",
      "`openapi:export` for typed clients in any language",
    ],
    code: {
      file: "crates/crm-deals-v2/src/lib.rs",
      lang: "rust",
      source: `#[controller("/deals", version = "2")]
impl DealsV2Controller {
    #[get("/{id}")]
    #[can(Read, Deal, from = "id")]
    #[api(summary = "Show a deal", response = DealV2Resource, responses(401, 403, 404))]
    async fn show(
        &self,
        uow: Uow,
        deal: Authorized<Deal, Read>,
    ) -> Result<Json<DealV2Resource>> {
        let deal = self.deals.show(&uow, deal.into_inner()).await?;
        Ok(Json(deal.into()))
    }
}`,
    },
    docs: "/docs/patterns/api-first/",
  },
  {
    id: "backend-for-frontend",
    name: "Backend for frontend",
    short: "BFF",
    headline: "Keep every token out of the browser.",
    what: "Your web app talks only to its own server, on the same origin. That server keeps the access and refresh tokens in httpOnly cookies, rotates them and calls the Navree API with a bearer token. The API stays the plain bearer-token API every other client uses.",
    teaser:
      "The browser talks to its own server, which keeps the tokens in httpOnly cookies, rotates them and calls the Navree API with a bearer token.",
    choose: [
      "You ship your own web app.",
      "One cross-site scripting bug must not leak every session.",
      "Mobile apps and partners call the same API with tokens.",
    ],
    how: [
      "Refresh tokens that rotate on every use, with reuse detection",
      "A Next.js BFF in the `saas` starter kit and `navree new --ui next`",
      "Real time straight from the WebSocket server, channels authorized through the BFF",
      "A browser client generated from the OpenAPI document",
    ],
    code: {
      file: "src/app/api/[...path]/route.ts",
      lang: "ts",
      source: `import { bff } from "@/lib/bff/server";

/** Every /api/* call is forwarded with the bearer token from the httpOnly cookie. */
function handle(request: Request): Promise<Response> {
  return bff.proxy(request, new URL(request.url).pathname);
}

export { handle as DELETE, handle as GET, handle as PATCH, handle as POST, handle as PUT };`,
    },
    docs: "/docs/patterns/backend-for-frontend/",
  },
];
