/**
 * Navree's first-party ecosystem: every package and tool, in one list.
 * The homepage, /ecosystem/ and the docs read from here, so names, crates
 * and commands stay the same everywhere.
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
    lead: "Starter kits, the UI toolkit, an admin panel, a local debugger and local services, ready on day one.",
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
    lead: "Messages, events, gRPC, GraphQL and real time between your services and your clients, and the Redis and MongoDB servers behind them.",
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
      "Exceptions, slow requests, slow queries, failed jobs and outgoing HTTP, grouped into issues and shown in a dashboard inside your app. Alerts go to email, Slack or a webhook, scripts read and resolve issues through its JSON API, and your coding agent through its MCP tools.",
    features: ["Issues grouped by fingerprint", "Slow request and query traces", "Alerts to email, Slack and webhooks", "JSON API and MCP tools for agents", "Dashboard served by your app"],
    crate: "navree-lighthouse",
    command: "navree::lighthouse::LighthouseModule::from_config()",
    href: "/docs/tools/lighthouse/",
    status: "stable",
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
    status: "stable",
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
    crate: "navree-harbor",
    command: "navree deploy",
    href: "/docs/tools/harbor/",
    status: "stable",
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
    command: "navree new cloudworks --kit saas",
    href: "/docs/getting-started/starter-kits/",
    status: "stable",
    signature: "navree new cloudworks --kit saas",
    mount: "navree new",
  },
  {
    slug: "ui",
    name: "Navree UI",
    kind: "core",
    category: "build",
    tagline: "Your app's screens, in Rust",
    summary:
      "Typed components rendered to HTML on the server, in the same binary as your API: forms from a validated struct, a server-side DataTable, pickers for choices, dates and phone numbers, menus, uploads, long lists and boards that load and move as they are used, and the app shell. Themes, one stylesheet, a small runtime and a strict content security policy come with it.",
    features: ["Typed Rust components", "Forms, a server-side DataTable and pickers", "Lists, boards and live updates", "Themes and a theme builder", "Pages that work with JavaScript off", "A strict content security policy"],
    crate: "navree-ui",
    command: "navree::ui::UiModule::from_config()",
    href: "/docs/ui/overview/",
    status: "stable",
    signature: "UiModule::from_config()",
  },
  {
    slug: "helm",
    name: "Helm",
    kind: "tool",
    category: "build",
    tagline: "An admin panel generated from your models",
    summary:
      "Describe a resource once and get list, detail and edit screens with filters, bulk actions and metrics. Every screen respects the access rules your API already enforces.",
    features: ["Resources from models", "Filters, actions and metrics", "Uses your access policies", "Custom fields and pages"],
    crate: "navree-helm",
    command: "navree make:resource deals Deal",
    href: "/docs/tools/helm/",
    status: "stable",
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
    command: "navree::logbook::LogbookModule::from_config()",
    href: "/docs/tools/logbook/",
    status: "stable",
    signature: "LogbookModule::from_config()",
    mount: "/_logbook",
  },
  {
    slug: "dock",
    name: "Dock",
    kind: "tool",
    category: "build",
    tagline: "Local services in one command",
    summary: "Start TimescaleDB (or plain PostgreSQL), Redis, Meilisearch, Mailpit, MinIO and NATS in containers, with your app's environment already pointing at them.",
    features: ["TimescaleDB or PostgreSQL, Redis, Meilisearch", "Mailpit, MinIO and NATS", "Docker or Podman", "Same setup in CI"],
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
      "Workspaces, members, roles and invitations, with every query scoped to the current workspace. Isolate data by row, by schema or by database, switch between workspaces from a Navree UI page, and seed demo workspaces from fixtures. On by default.",
    features: ["Header, subdomain, path or token resolution", "Owner, admin, member, viewer and custom roles", "Email invitations", "Row, schema or database isolation", "Demo workspaces, read-only or reset nightly"],
    crate: "navree-workspace",
    command: "navree::workspace::WorkspaceModule::from_config()",
    href: "/docs/workspaces/introduction/",
    status: "stable",
    signature: "async fn index(ws: CurrentWorkspace, uow: Uow)",
  },
  {
    slug: "search",
    name: "Search",
    kind: "library",
    category: "capabilities",
    tagline: "Full-text and vector search over your models",
    summary:
      "Derive Searchable on a model and its index stays in sync through the queue. Query PostgreSQL full text, Meilisearch, Typesense or an embedded Tantivy index, and store vectors in pgvector or Qdrant.",
    features: ["#[derive(Searchable)]", "PostgreSQL, Meilisearch, Typesense, Tantivy", "Vectors in pgvector or Qdrant", "Synced through the queue"],
    crate: "navree-search",
    command: "navree search:import Deal",
    href: "/docs/packages/search/",
    status: "stable",
    signature: "search.query::<Contact>(\"dhillon\").paginate(page)",
  },
  {
    slug: "billing",
    name: "Billing",
    kind: "library",
    category: "capabilities",
    tagline: "Subscriptions, payments and invoices with Stripe, Paddle or your own provider",
    summary:
      "Make a user or a workspace billable, then handle plans, trials, metered usage, invoices, one-time payments, payment links, refunds and the customer portal, through Stripe, Paddle or a provider you register. Writes are idempotent, and payment webhooks are verified and processed on the queue.",
    features: ["Plans, trials and metered usage", "One-time payments, payment links and refunds", "Per-user or per-workspace billing", "Invoices and the customer portal", "Stripe, Paddle or your own provider", "Verified webhooks"],
    crate: "navree-billing",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features billing",
    href: "/docs/packages/billing/",
    status: "stable",
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
    status: "stable",
    signature: "flags.active::<NewPipeline>().await?",
  },
  {
    slug: "settings",
    name: "Settings",
    kind: "library",
    category: "capabilities",
    tagline: "Typed settings per organization, workspace and user",
    summary:
      "Define a group of settings as a struct with #[derive(Settings)] and store it for the whole app, an organization, a workspace or a user. Values are inherited field by field down that chain, every write is validated, and each value reports where it came from.",
    features: ["#[derive(Settings)] groups", "Global, organization, workspace and user scopes", "Inherited field by field", "The source of every value", "Validated writes"],
    crate: "navree-settings",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features settings",
    href: "/docs/packages/settings/",
    status: "stable",
    signature: "settings.resolve::<PortalSettings>(workspace).await?",
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
    command: "navree oauth:client \"Cloudworks mobile\" --public --redirect-uri com.cloudworks.mobile:/callback",
    href: "/docs/packages/oauth-server/",
    status: "stable",
    signature: "navree oauth:client \"Partner app\" --redirect-uri https://partner.example.com/callback --scopes deals:read",
  },
  {
    slug: "social",
    name: "Social sign-in",
    kind: "library",
    category: "capabilities",
    tagline: "Sign in with Google, GitHub, Apple and more",
    summary: "Redirect to a provider, handle the callback and get a Navree user with linked accounts and normal Navree tokens.",
    features: ["Google, GitHub, Apple, Microsoft", "GitLab, Slack, LinkedIn, Discord, X", "Linked accounts", "Issues Navree tokens"],
    crate: "navree-social",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features social",
    href: "/docs/packages/social-sign-in/",
    status: "stable",
    signature: "self.social.redirect(\"github\").await?",
  },
  {
    slug: "sso",
    name: "Single sign-on",
    kind: "library",
    category: "capabilities",
    tagline: "SAML 2.0 and OpenID Connect per organization or workspace",
    summary:
      "Connect each customer's identity provider, route sign-in by verified email domain, refuse passwords where single sign-on is enforced and provision members with roles from their groups.",
    features: ["SAML 2.0 and OpenID Connect", "Verified email domains", "Just-in-time provisioning", "Signatures checked on ring"],
    crate: "navree-sso",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features sso",
    href: "/docs/security/single-sign-on/",
    status: "stable",
    signature: "self.sso.login(\"sutlej\", return_to).await?",
  },
  {
    slug: "webhooks",
    name: "Webhooks",
    kind: "library",
    category: "capabilities",
    tagline: "Signed outgoing webhooks and verified incoming ones",
    summary: "Send HMAC-signed webhooks with retries and a delivery log, and verify incoming webhooks from Stripe, GitHub, Paddle, AWS SNS, Twilio, Slack and any Standard Webhooks sender.",
    features: ["HMAC signatures", "Queue-backed retries with backoff", "Delivery log", "Verifiers for Stripe, GitHub, Paddle, SNS, Twilio and Standard Webhooks"],
    crate: "navree-webhooks",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features webhooks",
    href: "/docs/packages/webhooks/",
    status: "stable",
    signature: "impl WebhookEvent for DealWon",
  },
  {
    slug: "integrations",
    name: "Integrations",
    kind: "library",
    category: "capabilities",
    tagline: "Each customer's Microsoft 365, Google Workspace and Slack, connected",
    summary:
      "Let every workspace or organization connect its own Microsoft 365, Google Workspace, Slack, OAuth 2.0 APIs and API keys. Credentials are encrypted, tokens refresh once under a lock, each feature gets only the scopes it needs, and health checks warn you when a connection stops working.",
    features: ["Admin consent and app-only tokens", "Service accounts with domain-wide delegation", "Least-privilege scope sets per feature", "Token refresh under a lock", "Scheduled health checks", "Typed clients and IntegrationsFake"],
    crate: "navree-integrations",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features integrations",
    href: "/docs/packages/integrations/",
    status: "stable",
    signature: "graph.get(\"/users\").send().await?",
  },
  {
    slug: "directory-sync",
    name: "Directory sync",
    kind: "library",
    category: "capabilities",
    tagline: "Each customer's people, kept in step with their directory",
    summary:
      "Provision people from Microsoft Entra ID and Okta over SCIM 2.0, or pull them from Microsoft Graph, Google Workspace, Okta, LDAP and Active Directory. One normalized directory, mapped into your model by a sink, in runs with dry runs, a deactivation limit and progress.",
    features: ["SCIM 2.0 server with filters and PATCH", "Microsoft Graph delta queries", "Google Workspace and Okta", "LDAP and Active Directory over TLS", "Dry runs and a deactivation limit", "DirectorySink and DirectoryFake"],
    crate: "navree-directory",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features directory",
    href: "/docs/packages/directory-sync/",
    status: "stable",
    signature: "impl DirectorySink for RecipientSink",
  },
  {
    slug: "custom-domains",
    name: "Custom domains",
    kind: "library",
    category: "capabilities",
    tagline: "Each workspace on its own domain, with its own certificate",
    summary:
      "Let customers bring crm.sutlejsupply.example. Navree hands them the DNS records to create, verifies them, gets the certificate through your proxy's on-demand TLS or from Let's Encrypt itself, and resolves each request's workspace from its host.",
    features: ["DNS TXT verification and routing checks", "On-demand TLS through Caddy's ask endpoint", "ACME certificates served by rustls", "The domain workspace resolver", "Managed Cloudflare zones", "Expiry reports to Lighthouse"],
    crate: "navree-domains",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features domains",
    href: "/docs/packages/custom-domains/",
    status: "stable",
    signature: "domains.add(workspace, NewDomain::new(\"crm.sutlejsupply.example\"))",
  },
  {
    slug: "documents",
    name: "Documents",
    kind: "library",
    category: "capabilities",
    tagline: "PDF and Word documents from your templates and data",
    summary:
      "Render certificates, invoices and reports as PDF from HTML templates or documents built in code, laid out in process or printed by headless Chrome, in any script, and write DOCX. Merge, stamp and queue them.",
    features: ["Native and Chrome engines", "Bundled fonts for Arabic, Hebrew and Devanagari", "Tagged PDF with repeating table headers", "DOCX writer", "Merge, stamps and overlays", "Queued renders to storage"],
    crate: "navree-documents",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features documents",
    href: "/docs/packages/documents/",
    status: "stable",
    signature: "documents.pdf(\"certificates/classic\", &data).await?",
  },
  {
    slug: "media",
    name: "Media",
    kind: "library",
    category: "capabilities",
    tagline: "Files on your models, with thumbnails, archives and text",
    summary:
      "Attach uploads and remote files to your models in collections that check types and sizes. Generate thumbnails, WebP copies and responsive variants on the queue, unpack zip and SCORM packages safely, extract text from PDF and DOCX, and process video with FFmpeg.",
    features: ["Collections on any model", "Image conversions and srcset", "Remote files through the SSRF guard", "Zip-slip and size-bomb guards", "Text from PDF, DOCX and HTML", "FFmpeg posters and transcodes"],
    crate: "navree-media",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features media",
    href: "/docs/packages/media/",
    status: "stable",
    signature: "media.add(&course, \"thumbnail\", source).save(&uow).await?",
  },
  {
    slug: "sheets",
    name: "Imports and exports",
    kind: "library",
    category: "capabilities",
    tagline: "CSV and Excel exports and imports from typed rows",
    summary:
      "Export typed rows as CSV with a byte-order mark and escaped formulas or as XLSX, streamed or queued to a disk with a signed link. Import CSV, TSV, XLSX, XLSB, XLS or ODS files in any encoding with header mapping, row validation, a staging preview, duplicate handling and error files.",
    features: ["#[derive(Exportable)] and #[derive(Importable)]", "Imports CSV, TSV, XLSX, XLSB, XLS and ODS", "Encoding detection (UTF-16, Windows-1256)", "Staging preview and duplicate modes", "Batched apply and error files"],
    crate: "navree-sheets",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features sheets",
    href: "/docs/packages/imports-and-exports/",
    status: "stable",
    signature: "Download::csv(\"contacts\", rows)",
  },
  {
    slug: "workflows",
    name: "Workflows",
    kind: "library",
    category: "capabilities",
    tagline: "Durable workflows in code and automations people build",
    summary:
      "Write long-running sequences as async functions whose steps are stored and replayed on resume, with retries, sleeps of minutes or months and event waits. Let people build their own automations from your triggers, conditions and actions, with versions, enrollments and simulation.",
    features: ["#[workflow] with steps, sleeps and event waits", "Runs that survive crashes and keep their version", "#[trigger], #[condition] and #[action] blocks with JSON Schemas", "Versioned automations per workspace", "Enrollment policies, simulation and templates"],
    crate: "navree-workflows",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features workflows",
    href: "/docs/packages/workflows/",
    status: "stable",
    signature: "ctx.wait_for::<CourseCompleted>(filter, 7 * DAY).await?",
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
    status: "stable",
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
      "Handle messages with the same modules, providers and guards as your HTTP routes. Pick TCP, NATS, Kafka (4.0 or later), RabbitMQ, Redis Streams or MQTT, and run HTTP and messages in one binary.",
    features: ["#[message_pattern] and #[event_pattern]", "NATS, Kafka 4.0+, RabbitMQ, Redis Streams, MQTT, TCP", "Hybrid HTTP and message apps", "Trace context across services"],
    crate: "navree-micro",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features micro",
    href: "/docs/microservices/introduction/",
    status: "stable",
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
    status: "stable",
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
    status: "stable",
    signature: "#[resolver]",
  },
  {
    slug: "realtime",
    name: "Real time",
    kind: "core",
    category: "connect",
    tagline: "A WebSocket server for Pusher clients, and a live hub for Navree UI pages",
    summary: "Broadcast events to public, private and presence channels from your own binary; pusher-js and any Pusher client connects. The live hub tells Navree UI pages what changed, scoped to the reader's user and workspace, over server-sent events that resume where they left off.",
    features: ["Public, private and presence channels", "Broadcast from events", "Channel authorization with access rules", "pusher-js compatible", "A live hub for Navree UI pages"],
    crate: "navree-realtime",
    command: "navree::realtime::RealtimeModule::from_config()",
    href: "/docs/digging-deeper/broadcasting/",
    status: "stable",
    signature: "self.broadcaster.broadcast(channels, event, data)",
  },
  {
    slug: "redis",
    name: "Redis",
    kind: "library",
    category: "connect",
    tagline: "One Redis for every process of your app",
    summary:
      "Point the cache, locks, queue, sessions, rate limits and real-time broadcasts at one Redis server, so web and worker processes, and every instance behind a load balancer, share them.",
    features: ["Cache and atomic locks", "The job queue", "Sessions", "Shared rate limits", "Broadcasts across instances"],
    crate: "navree-redis",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features redis",
    href: "/docs/digging-deeper/redis/",
    status: "stable",
    signature: "RedisModule::from_config()",
  },
  {
    slug: "mongodb",
    name: "MongoDB",
    kind: "library",
    category: "connect",
    tagline: "The ORM's models on MongoDB collections",
    summary:
      "Run your models on MongoDB. Queries compile to filters and aggregation pipelines, keys can be ObjectIds, documents embed other documents, units of work run on sessions, and migrations create indexes, validators and time-series collections.",
    features: ["Models on collections", "ObjectId keys and embedded documents", "Filters and pipelines from the query builder", "Units of work on sessions", "Time-series and TTL collections"],
    crate: "navree-mongo",
    command: "cargo add --git https://github.com/NavreeHQ/navree navree --features mongo",
    href: "/docs/orm/mongodb/",
    status: "stable",
    signature: "MongoModule::from_config()",
  },
  // AI
  {
    slug: "navigator",
    name: "Navigator",
    kind: "tool",
    category: "ai",
    tagline: "The MCP server for your coding agent",
    summary:
      "Give Claude Code, Cursor, Codex or any MCP client a live view of your app: routes, the module graph and providers, the database schema, read-only SQL, configuration, recent logs and docs that match your Navree version.",
    features: ["Routes and the module graph", "The database schema and read-only SQL", "Recent logs and the last error", "Version-matched docs search"],
    crate: "navree-navigator",
    command: "navree mcp",
    href: "/docs/ai/navigator/",
    status: "stable",
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
    status: "stable",
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
    status: "stable",
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
