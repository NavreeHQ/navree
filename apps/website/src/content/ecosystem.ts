/**
 * Navree's first-party ecosystem: every package and tool, in one list.
 * The homepage, /ecosystem/ and the docs read from here, so names, crates
 * and commands stay the same everywhere. The canonical catalogue is in
 * docs/design §5.
 *
 * `status` is the truth in the repository. The site renders everything as
 * available unless `site.showPlannedBadges` is true, in which case planned
 * entries get an "In development" badge.
 */

export type EcosystemStatus = "stable" | "planned";

export type EcosystemKind = "tool" | "library" | "core";

export type EcosystemCategory = "operate" | "build" | "capabilities" | "connect" | "ai" | "test";

export type EcosystemEntry = {
  slug: string;
  name: string;
  kind: EcosystemKind;
  category: EcosystemCategory;
  /** One line, no trailing full stop. */
  tagline: string;
  /** Two or three sentences for cards that have room. */
  summary: string;
  /** Short, concrete capabilities. */
  features: string[];
  /** The crate, or the command for CLI-only tools. */
  crate: string;
  /** A one-line install or run command, shown in monospace. */
  command: string;
  /** The docs page. */
  href: string;
  status: EcosystemStatus;
  /** Optional: one line of code that shows the key API, for cards. */
  signature?: string;
  /** Optional: where a tool lives, the path it serves or the command that runs it. */
  mount?: string;
};

export type EcosystemCategoryInfo = {
  id: EcosystemCategory;
  title: string;
  lead: string;
  /** Optional: a section heading for pages that give the category its own section. */
  heading?: string;
};

export const categories: EcosystemCategoryInfo[] = [
  {
    id: "operate",
    title: "Run in production",
    lead: "Monitoring, queue supervision and deployment, built into the framework and hosted by you.",
    heading: "See what production is doing, and ship to it without downtime.",
  },
  {
    id: "build",
    title: "Build faster",
    lead: "Starter kits, an admin panel, a local debugger and local services, ready on day one.",
    heading: "Start with a product, not an empty directory.",
  },
  {
    id: "capabilities",
    title: "Product capabilities",
    lead: "The features every product grows into, designed to work with workspaces, access control and the queue.",
    heading: "Search, billing, sign‑in and flags, ready when you need them.",
  },
  {
    id: "connect",
    title: "Connect services",
    lead: "Messages, events, gRPC, GraphQL and real time between your services and your clients.",
    heading: "Services and clients, connected by the same modules.",
  },
  {
    id: "ai",
    title: "AI",
    lead: "For the agents that write your code and the AI features inside your product.",
    heading: "An MCP server for your coding agent, an AI SDK for your product.",
  },
  {
    id: "test",
    title: "Testing",
    lead: "From unit tests with fakes to a real browser driving full user journeys.",
    heading: "Test every layer, up to a real browser.",
  },
];

export const ecosystem: EcosystemEntry[] = [
  // Run in production
  {
    slug: "lighthouse",
    name: "Lighthouse",
    kind: "tool",
    category: "operate",
    tagline: "Application monitoring, built in",
    summary:
      "Exceptions, slow requests, slow queries, failed jobs and outgoing HTTP, grouped into issues and shown in a dashboard inside your app. Alerts go to email, Slack or a webhook, and your coding agent can read and resolve issues over MCP.",
    features: ["Issues grouped by fingerprint", "Slow request and query traces", "Alerts to email, Slack and webhooks", "MCP tools for agents", "OpenTelemetry export"],
    crate: "navree-lighthouse",
    command: "navree::LighthouseModule::from_config()",
    href: "/docs/tools/lighthouse/",
    status: "planned",
    signature: "LighthouseModule::from_config()",
    mount: "/_lighthouse",
  },
  {
    slug: "fleet",
    name: "Fleet",
    kind: "tool",
    category: "operate",
    tagline: "Queue dashboard and worker supervisor",
    summary:
      "Run supervised workers per queue that balance themselves under load. Watch throughput, wait times and runtimes, and retry or forget failed jobs from one dashboard.",
    features: ["Auto-balanced workers", "Throughput and wait-time metrics", "Retry or forget failed jobs", "Pause and continue queues", "Long-wait alerts"],
    crate: "navree-fleet",
    command: "navree fleet",
    href: "/docs/tools/fleet/",
    status: "planned",
    signature: "navree fleet",
    mount: "/_fleet",
  },
  {
    slug: "harbor",
    name: "Harbor",
    kind: "tool",
    category: "operate",
    tagline: "Servers and zero-downtime deployments",
    summary:
      "Provision a server over SSH with TLS, PostgreSQL, Redis and systemd units for your web, worker and scheduler processes, then ship atomic releases that only go live after a health check.",
    features: ["Provision any Ubuntu server", "Atomic, health-checked releases", "One-command rollbacks", "Encrypted environment secrets", "Container images and push-to-deploy"],
    crate: "navree-cli",
    command: "navree deploy",
    href: "/docs/tools/harbor/",
    status: "planned",
    signature: "navree deploy",
    mount: "navree deploy",
  },
  // Build faster
  {
    slug: "starter-kits",
    name: "Starter kits",
    kind: "tool",
    category: "build",
    tagline: "Production-ready apps from one command",
    summary:
      "The SaaS kit gives you a Next.js and shadcn/ui front end with sign-in, workspaces, billing, settings and an admin panel, wired to a Navree API. API and microservice kits start leaner.",
    features: ["SaaS, API and microservice kits", "Sign-in, workspaces and billing", "A BFF that keeps tokens in httpOnly cookies", "Tests included"],
    crate: "navree-cli",
    command: "navree new acme --kit saas",
    href: "/docs/getting-started/starter-kits/",
    status: "planned",
    signature: "navree new acme --kit saas",
    mount: "navree new",
  },
  {
    slug: "helm",
    name: "Helm",
    kind: "tool",
    category: "build",
    tagline: "An admin panel generated from your entities",
    summary:
      "Describe a resource once and get list, detail and edit screens with filters, bulk actions and metrics. Every screen respects the access rules your API already enforces.",
    features: ["Resources from entities", "Filters, actions and metrics", "Uses your access policies", "Custom fields and pages"],
    crate: "navree-helm",
    command: "navree make:resource deals Deal",
    href: "/docs/tools/helm/",
    status: "planned",
    signature: "impl Resource for DealResource",
    mount: "/admin",
  },
  {
    slug: "logbook",
    name: "Logbook",
    kind: "tool",
    category: "build",
    tagline: "A local debugging timeline",
    summary:
      "Every request in development with the queries it ran, the events it fired, the jobs it queued, the mail it sent and the log lines it wrote, in one timeline you can filter and replay.",
    features: ["Requests, queries, jobs, mail and events", "Filter by tag or user", "Replay a request", "Development only"],
    crate: "navree-logbook",
    command: "navree::LogbookModule::from_config()",
    href: "/docs/tools/logbook/",
    status: "planned",
    signature: "LogbookModule::from_config()",
    mount: "/_logbook",
  },
  {
    slug: "dock",
    name: "Dock",
    kind: "tool",
    category: "build",
    tagline: "Local services in one command",
    summary: "Start PostgreSQL, Redis, Meilisearch, Mailpit, MinIO and NATS in containers, with your app's environment already pointing at them.",
    features: ["PostgreSQL, Redis, Meilisearch", "Mailpit, MinIO and NATS", "Docker or Podman", "Same setup in CI"],
    crate: "navree-dock",
    command: "navree dock up",
    href: "/docs/tools/dock/",
    status: "stable",
    signature: "navree dock up",
    mount: "navree dock up",
  },
  // Product capabilities
  {
    slug: "workspaces",
    name: "Workspaces",
    kind: "core",
    category: "capabilities",
    tagline: "Multi-tenancy at the workspace level",
    summary:
      "Workspaces, members, roles and invitations, with every query scoped to the current workspace. Isolate data by row, by schema or by database. On by default.",
    features: ["Header, subdomain, path or token resolution", "Owner, admin, member, viewer and custom roles", "Email invitations", "Row, schema or database isolation"],
    crate: "navree-workspace",
    command: "navree::WorkspaceModule::from_config()",
    href: "/docs/workspaces/introduction/",
    status: "stable",
    signature: "async fn index(ws: CurrentWorkspace, uow: Uow)",
  },
  {
    slug: "search",
    name: "Search",
    kind: "library",
    category: "capabilities",
    tagline: "Full-text and vector search over your entities",
    summary:
      "Derive Searchable on an entity and its index stays in sync through the queue. Query PostgreSQL full text, Meilisearch, Typesense or an embedded Tantivy index, and store vectors in pgvector or Qdrant.",
    features: ["#[derive(Searchable)]", "PostgreSQL, Meilisearch, Typesense, Tantivy", "Vectors in pgvector or Qdrant", "Synced through the queue"],
    crate: "navree-search",
    command: "navree search:import Deal",
    href: "/docs/packages/search/",
    status: "planned",
    signature: "search.query::<Contact>(\"ada\").paginate(page)",
  },
  {
    slug: "billing",
    name: "Billing",
    kind: "library",
    category: "capabilities",
    tagline: "Subscriptions and invoices with Stripe or Paddle",
    summary:
      "Make a user or a workspace billable, then handle plans, trials, metered usage, invoices and the customer portal. Payment webhooks are verified and processed on the queue.",
    features: ["Plans, trials and metered usage", "Per-user or per-workspace billing", "Invoices and the customer portal", "Verified webhooks"],
    crate: "navree-billing",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features billing",
    href: "/docs/packages/billing/",
    status: "planned",
    signature: "BillingModule::for_feature::<Workspace>()",
  },
  {
    slug: "flags",
    name: "Feature flags",
    kind: "library",
    category: "capabilities",
    tagline: "Roll out features per user, workspace or percentage",
    summary: "Define a flag as a type, check it anywhere, or guard a whole route with it. Flags live in your database and are cached.",
    features: ["Flags as types", "User, workspace and percentage rollouts", "#[requires_flag] on routes", "Cached lookups"],
    crate: "navree-flags",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features flags",
    href: "/docs/packages/feature-flags/",
    status: "planned",
    signature: "flags.active::<NewPipeline>().await?",
  },
  {
    slug: "oauth",
    name: "OAuth server",
    kind: "library",
    category: "capabilities",
    tagline: "OAuth 2.1 and OpenID Connect provider",
    summary:
      "Let third-party apps act on behalf of your users with authorization code and PKCE, client credentials and device flows, scopes, a consent screen and personal access tokens.",
    features: ["Authorization code with PKCE", "Client credentials and device flow", "Scopes and consent", "Personal access tokens"],
    crate: "navree-oauth",
    command: "navree oauth:client \"Acme mobile\"",
    href: "/docs/packages/oauth-server/",
    status: "planned",
    signature: "navree oauth:client \"Zapier\" --scopes deals:read",
  },
  {
    slug: "social",
    name: "Social sign-in",
    kind: "library",
    category: "capabilities",
    tagline: "Sign in with Google, GitHub, Apple and more",
    summary: "Redirect to a provider, handle the callback and get a Navree user with linked accounts and normal Navree tokens.",
    features: ["Google, GitHub, Apple, Microsoft", "GitLab, Slack, LinkedIn, Discord", "Linked accounts", "Issues Navree tokens"],
    crate: "navree-social",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features social",
    href: "/docs/packages/social-sign-in/",
    status: "planned",
    signature: "self.social.redirect(\"github\").await?",
  },
  {
    slug: "webhooks",
    name: "Webhooks",
    kind: "library",
    category: "capabilities",
    tagline: "Signed outgoing webhooks and verified incoming ones",
    summary: "Send HMAC-signed webhooks with retries and a delivery log, and verify incoming webhooks from Stripe, GitHub and any Standard Webhooks sender.",
    features: ["HMAC signatures", "Queue-backed retries with backoff", "Delivery log", "Stripe, GitHub and Standard Webhooks verifiers"],
    crate: "navree-webhooks",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features webhooks",
    href: "/docs/packages/webhooks/",
    status: "planned",
    signature: "impl WebhookEvent for DealWon",
  },
  {
    slug: "i18n",
    name: "Localization",
    kind: "library",
    category: "capabilities",
    tagline: "Translations, plurals and locale negotiation",
    summary: "Write translations in Fluent files and pick the locale from the user, the workspace or the browser. Validation messages and mail are translated too.",
    features: ["Fluent translation files", "Plurals and variables", "Locale from user, workspace or browser", "Translated validation messages"],
    crate: "navree-i18n",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features i18n",
    href: "/docs/packages/localization/",
    status: "planned",
    signature: "t!(\"deal-created\", count = 3)",
  },
  // Connect services
  {
    slug: "microservices",
    name: "Microservices",
    kind: "library",
    category: "connect",
    tagline: "Request-response and events between services",
    summary:
      "Handle messages with the same modules, providers and guards as your HTTP routes. Pick TCP, NATS, Kafka, RabbitMQ, Redis Streams or MQTT, and run HTTP and messages in one binary.",
    features: ["#[message_pattern] and #[event_pattern]", "NATS, Kafka, RabbitMQ, Redis Streams, MQTT, TCP", "Hybrid HTTP and message apps", "Trace context across services"],
    crate: "navree-micro",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features micro",
    href: "/docs/microservices/introduction/",
    status: "planned",
    signature: "#[event_pattern(\"deal.won\")]",
  },
  {
    slug: "grpc",
    name: "gRPC",
    kind: "library",
    category: "connect",
    tagline: "gRPC and Connect services from your modules",
    summary: "Register a tonic service in a module and it shares the container, the guards and the access rules of the rest of your app.",
    features: ["tonic services in modules", "Guards and access rules", "gRPC-Web and Connect", "Typed clients"],
    crate: "navree-grpc",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features grpc",
    href: "/docs/microservices/grpc/",
    status: "planned",
    signature: "#[grpc_service]",
  },
  {
    slug: "graphql",
    name: "GraphQL",
    kind: "library",
    category: "connect",
    tagline: "GraphQL with dataloaders and subscriptions",
    summary: "Write resolvers on async-graphql inside your modules, batch with per-request dataloaders and push subscriptions over WebSockets.",
    features: ["#[resolver] in modules", "Per-request dataloaders", "Subscriptions over WebSockets", "GraphiQL in development"],
    crate: "navree-graphql",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features graphql",
    href: "/docs/microservices/graphql/",
    status: "planned",
    signature: "#[resolver]",
  },
  {
    slug: "realtime",
    name: "Real time",
    kind: "core",
    category: "connect",
    tagline: "A WebSocket server that speaks the Pusher protocol",
    summary: "Broadcast events to public, private and presence channels from your own binary. pusher-js and any Pusher client connects.",
    features: ["Public, private and presence channels", "Broadcast from events", "Channel authorization with access rules", "pusher-js compatible"],
    crate: "navree-realtime",
    command: "navree::RealtimeModule::from_config()",
    href: "/docs/digging-deeper/broadcasting/",
    status: "stable",
    signature: "self.broadcaster.broadcast(channels, event, data)",
  },
  // AI
  {
    slug: "navigator",
    name: "Navigator",
    kind: "tool",
    category: "ai",
    tagline: "The MCP server for your coding agent",
    summary:
      "Give Claude Code, Cursor, Codex or any MCP client a live view of your app: routes, the module graph, entity schemas, configuration, recent logs and docs that match your Navree version.",
    features: ["Routes and the module graph", "Entity schemas and read-only SQL", "Recent logs and the last error", "Version-matched docs search"],
    crate: "navree-cli",
    command: "navree mcp",
    href: "/docs/ai/navigator/",
    status: "planned",
    signature: "claude mcp add navree -- navree mcp",
    mount: "navree mcp",
  },
  {
    slug: "skills",
    name: "Agent skills",
    kind: "tool",
    category: "ai",
    tagline: "Teach your agent the Navree way",
    summary: "Skills for modules, routes, validation, the database, auth, workspaces, queues, tests and more, so agents write code the way the framework expects.",
    features: ["A skill for every common task", "Verified against the code", "Works with any skills-aware agent", "AGENTS.md in every new app"],
    crate: "skills/",
    command: "navree skills:install",
    href: "/docs/ai/agent-skills/",
    status: "stable",
    signature: "navree skills:install",
  },
  {
    slug: "ai",
    name: "AI SDK",
    kind: "library",
    category: "ai",
    tagline: "Text, structured output, agents and RAG",
    summary:
      "Generate text or typed structs from Anthropic, OpenAI, Gemini, Mistral, Ollama or Bedrock, build agents with tools and memory, and fail over between models. Every capability has a fake for tests.",
    features: ["Typed structured output", "Agents with #[tool] methods", "Embeddings and vector stores", "Provider failover and fakes"],
    crate: "navree-ai",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features ai",
    href: "/docs/ai/ai-sdk/",
    status: "planned",
    signature: "ai.structured::<ContractTerms>(prompt).await?",
  },
  {
    slug: "mcp",
    name: "MCP servers",
    kind: "library",
    category: "ai",
    tagline: "Expose your app to AI clients",
    summary: "Turn service methods into MCP tools, resources and prompts, served over streamable HTTP or stdio, behind your auth and per-tool permissions.",
    features: ["#[mcp_tool], #[mcp_resource], #[mcp_prompt]", "Streamable HTTP and stdio", "Auth and per-tool permissions", "Testing helpers"],
    crate: "navree-mcp",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features mcp",
    href: "/docs/ai/mcp-servers/",
    status: "stable",
    signature: "#[mcp_tool(read_only)]",
  },
  // Testing
  {
    slug: "testing",
    name: "Testing",
    kind: "core",
    category: "test",
    tagline: "In-process HTTP tests, a database per test, and fakes",
    summary: "Boot the real app in-process, call it without a network, and swap the queue, mail, clock, HTTP client and storage for fakes you can assert on.",
    features: ["In-process HTTP tests", "A fresh database per test", "Fakes for queue, mail, clock and HTTP", "Factories and seeders"],
    crate: "navree-testing",
    command: "cargo test",
    href: "/docs/testing/getting-started/",
    status: "stable",
    signature: "app.acting_as(rep).get(\"/api/v1/deals\")",
  },
  {
    slug: "browser-testing",
    name: "Browser testing",
    kind: "library",
    category: "test",
    tagline: "Drive a real browser from your tests",
    summary: "Visit pages, fill forms, click and assert on what a user sees, over the Chrome DevTools Protocol, with a screenshot saved on every failure.",
    features: ["visit, type_into, press, assert_see", "Automatic waiting", "Screenshots on failure", "Runs headless in CI"],
    crate: "navree-browser",
    command: "cargo add --dev --git https://github.com/NavreeHQ/navree navree-browser",
    href: "/docs/packages/browser-testing/",
    status: "planned",
    signature: "browser.visit(web.url(\"/login\")).await?",
  },
];

export function entriesIn(category: EcosystemCategory): EcosystemEntry[] {
  return ecosystem.filter((entry) => entry.category === category);
}

export function entry(slug: string): EcosystemEntry {
  const found = ecosystem.find((e) => e.slug === slug);
  if (!found) throw new Error(`No ecosystem entry "${slug}"`);
  return found;
}
