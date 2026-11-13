/**
 * The documentation's navigation: sections in sidebar order, and each
 * section's pages in order. This file is the single source of truth for
 * the sidebar (with the groups of a long section as a second level),
 * previous/next links, the search index and the static pages that get built.
 *
 * A page `{ section: "basics", page: "validation" }` lives at
 * `content/docs/basics/validation.mdx` and is served at
 * `/docs/basics/validation/`. Its title and description come from the
 * file's front-matter.
 *
 * To add a page: create the `.mdx` file, then add its slug here. A slug in
 * this list without a file fails the build, and a file that is not listed
 * here is not published.
 */

export type DocsSection = {
  /** The directory under `content/docs/` and the URL segment. */
  slug: string;
  /** The heading shown in the sidebar. */
  title: string;
  /** What the section covers, in one or two sentences of 70 to 160 characters: the section page's lead and meta description. */
  description: string;
  /** Page slugs in order: `content/docs/<section>/<page>.mdx`. */
  pages: string[];
  /**
   * Groups that cut a long section into parts: each group starts at the page
   * `from` and runs to the next group, so a group's pages are one run of
   * `pages` and `from` values follow the order of `pages`. The sidebar shows
   * each group as a second level (a heading that opens and closes its pages,
   * the group with the current page open), the section's own page draws them
   * as headings, and the page's breadcrumb names its group. Pages before the
   * first group have no heading. Group titles are short, in sentence case,
   * and unique within the section. A section of about eight pages or fewer
   * needs no groups; a longer one should have them.
   */
  groups?: { title: string; from: string }[];
};

/** A sidebar entry that links outside the docs content, e.g. `/performance/`. */
export type DocsLink = {
  title: string;
  href: string;
};

export const sections: DocsSection[] = [
  {
    slug: "prologue",
    title: "Prologue",
    description: "What Navree is, what 1.0 ships, how it is versioned and supported, what comes next, and how to send a change.",
    pages: ["introduction", "release-notes", "versioning-and-support", "roadmap", "contributing"],
  },
  {
    slug: "getting-started",
    title: "Getting started",
    description: "Install the CLI, build your first app, and learn how an app is configured, laid out, started and deployed, or bring an existing product or design.",
    pages: [
      "installation",
      "your-first-app",
      "configuration",
      "directory-structure",
      "starter-kits",
      // The two ways into Navree: from an existing product (reverse
      // engineering) and from an existing design (to Navree UI pages).
      "bring-your-product",
      "bring-your-design",
      "deployment",
    ],
  },
  {
    slug: "configuration",
    title: "Configuration",
    description: "Files per environment and per section, the commands that show and check the configuration, and the reference of every section the framework reads.",
    pages: ["environments-and-files", "reference"],
  },
  {
    slug: "architecture",
    title: "Architecture concepts",
    description: "How a request travels through a Navree app to the screen, and how modules, providers, lifecycle hooks and crates fit together.",
    pages: [
      "request-lifecycle",
      "ui-runtime",
      "modules",
      "providers",
      "lifecycle-hooks",
      "crate-hierarchy",
      "facade-and-features",
    ],
  },
  {
    slug: "patterns",
    title: "Architecture patterns",
    description: "Choose a shape for your application: a modular monolith, microservices, multi-tenancy, events, API first, a backend for a front end or server-rendered pages.",
    pages: [
      "overview",
      "modular-monolith",
      "microservices",
      "multi-tenancy",
      "event-driven",
      "api-first",
      "backend-for-frontend",
      "server-rendered-app",
    ],
  },
  {
    slug: "basics",
    title: "The basics",
    description: "Routes, requests, validation, list queries, responses, errors, middleware, logging and sessions: the parts every HTTP app uses.",
    pages: [
      // Routing and requests
      "routing",
      "access-control",
      "middleware",
      "requests",
      // Input
      "validation",
      "international-input",
      "phone-numbers",
      "list-queries",
      // Responses and errors
      "responses",
      "errors",
      "logging",
      // Protecting requests
      "rate-limiting",
      "cors-and-security-headers",
      "sessions-and-csrf",
    ],
    groups: [
      { title: "Routing and requests", from: "routing" },
      { title: "Input", from: "validation" },
      { title: "Responses and errors", from: "responses" },
      { title: "Protecting requests", from: "rate-limiting" },
    ],
  },
  {
    slug: "security",
    title: "Security",
    description: "Sign-in, sessions, passwordless and single sign-on, authorization, encryption, signatures and the audit log.",
    pages: [
      "secure-by-default",
      // Signing in
      "authentication",
      "account-flows",
      "sessions",
      "passwordless",
      "single-sign-on",
      // Access and audit
      "authorization",
      "impersonation",
      "audit-log",
      // Encryption and signing
      "encryption-and-hashing",
      "signatures-and-keys",
    ],
    groups: [
      { title: "Signing in", from: "authentication" },
      { title: "Access and audit", from: "authorization" },
      { title: "Encryption and signing", from: "encryption-and-hashing" },
    ],
  },
  {
    slug: "workspaces",
    title: "Workspaces",
    description: "Give each customer, team or organization its own space: how a workspace is found, who can join it and how its data stays apart.",
    pages: [
      "introduction",
      // Members and access
      "resolution",
      "members-and-roles",
      "invitations",
      "organizations",
      // Data
      "isolation",
      "partitions",
      "shared-libraries",
      "demo-workspaces",
    ],
    groups: [
      { title: "Members and access", from: "resolution" },
      { title: "Data", from: "isolation" },
    ],
  },
  {
    slug: "database",
    title: "Database",
    description: "Entities, repositories, migrations, the schema builder, TimescaleDB, seeding and pagination on PostgreSQL, MySQL and SQLite.",
    pages: [
      "getting-started",
      // Engines
      "postgresql",
      "mysql",
      "sqlite",
      "timescaledb",
      // Working with data
      "entities-and-repositories",
      "unit-of-work",
      "pagination",
      "seeding-and-factories",
      // Schema and migrations
      "migrations",
      "running-migrations",
      "schema-builder",
      "columns",
      "indexes-and-foreign-keys",
      "squashing-migrations",
    ],
    groups: [
      { title: "Engines", from: "postgresql" },
      { title: "Working with data", from: "entities-and-repositories" },
      { title: "Schema and migrations", from: "migrations" },
    ],
  },
  {
    slug: "orm",
    title: "ORM",
    description: "Models, the query builder, relationships, eager loading, events, soft deletes and factories, on SQL databases and MongoDB.",
    pages: [
      "getting-started",
      // Reading and writing
      "models",
      "query-builder",
      "relationships",
      "eager-loading",
      "collections",
      "pagination-and-streaming",
      // Shaping records
      "casts-and-accessors",
      "serialization",
      "events-and-observers",
      "scopes-and-soft-deletes",
      // Connections and your app
      "connections-and-transactions",
      "mongodb",
      "route-model-binding",
      "workspaces-and-access",
      // Testing
      "factories",
      "testing",
    ],
    groups: [
      { title: "Reading and writing", from: "models" },
      { title: "Shaping records", from: "casts-and-accessors" },
      { title: "Connections and your app", from: "connections-and-transactions" },
      { title: "Testing", from: "factories" },
    ],
  },
  {
    slug: "digging-deeper",
    title: "Digging deeper",
    description: "Cache, events, queues, scheduling, mail, notifications, storage, real time, OpenAPI, health checks and the command line.",
    pages: [
      // Cache and Redis
      "cache",
      "redis",
      // Events and background work
      "events",
      "queues",
      "batches-and-operations",
      "scheduling",
      "time-zones",
      // Mail
      "mail",
      "mail-delivery",
      "inbound-mail",
      "views",
      // Notifications
      "notifications",
      "notification-channels",
      "notification-preferences",
      // Files, HTTP and real time
      "storage",
      "http-client",
      "broadcasting",
      "openapi",
      // Running your app
      "health-and-metrics",
      "telemetry",
      "cli",
      "maintenance-mode",
    ],
    groups: [
      { title: "Cache and Redis", from: "cache" },
      { title: "Events and background work", from: "events" },
      { title: "Mail", from: "mail" },
      { title: "Notifications", from: "notifications" },
      { title: "Files, HTTP and real time", from: "storage" },
      { title: "Running your app", from: "health-and-metrics" },
    ],
  },
  {
    slug: "ui",
    title: "UI",
    description: "Navree UI draws your screens as typed Rust components on the server: foundations, the app shell, forms, the DataTable and what is still to come.",
    pages: [
      "choosing-a-ui",
      "overview",
      // Foundations
      "foundations",
      "themes",
      "theme-builder",
      "app-styles",
      "icons",
      "flags",
      // Pages and layout
      "layout",
      "app-shell",
      "errors-and-states",
      "marketing-sites",
      "seo",
      // Forms and data
      "forms",
      "data-table",
      "described",
      "dates",
      "reports",
      // Interaction
      "commands-and-ops",
      "regions",
      "live-hub",
      "drag-and-drop",
      "islands",
      // What is next
      "coming-next",
    ],
    groups: [
      { title: "Foundations", from: "foundations" },
      { title: "Pages and layout", from: "layout" },
      { title: "Forms and data", from: "forms" },
      { title: "Interaction", from: "commands-and-ops" },
      { title: "What is next", from: "coming-next" },
    ],
  },
  {
    slug: "components",
    title: "UI components",
    description: "Every Navree UI component with a live preview, its parts, its options and its keyboard behavior.",
    pages: [
      "overview",
      // Actions
      "button",
      "button-group",
      "toggle",
      "toggle-group",
      "dropdown-menu",
      "context-menu",
      "menubar",
      // Inputs
      "field",
      "input",
      "input-group",
      "input-otp",
      "textarea",
      "native-select",
      "select",
      "combobox",
      "multi-select",
      "checkbox",
      "switch",
      "radio-group",
      "phone-input",
      "color-picker",
      "questionnaire",
      "upload",
      // Date and time
      "calendar",
      "date-picker",
      "date-range-picker",
      "time-picker",
      "date-time-picker",
      // Display
      "card",
      "badge",
      "avatar",
      "avatar-group",
      "trace",
      "item",
      "typography",
      "kbd",
      "attachment",
      "picture",
      "markdown",
      "code-block",
      "message",
      "timeline",
      "activity-feed",
      "fact-card",
      // Feedback
      "alert",
      "toast",
      "empty",
      "empty-state",
      "error-page",
      "forbidden",
      "not-found",
      "sign-in-required",
      "progress",
      "skeleton",
      "spinner",
      // Overlays
      "dialog",
      "alert-dialog",
      "sheet",
      "drawer",
      "popover",
      "tooltip",
      "hover-card",
      // Layout
      "split-pane",
      "accordion",
      "collapsible",
      "scroll-area",
      "separator",
      "aspect-ratio",
      "carousel",
      "direction",
      // Navigation
      "tabs",
      "navigation-menu",
      "breadcrumb",
      "pagination",
      "command",
      // Steps, ranges and trees
      "stepper",
      "wizard",
      "slider",
      "tree-view",
      "toolbar",
      // Data
      "table",
      "lazy-list",
      "stat",
      "sortable",
      // Charts
      "line-chart",
      "area-chart",
      "bar-chart",
      "donut-chart",
      "sparkline",
      "funnel",
      "ring",
      "rings",
      "gauge",
      "day-clock",
      "meter",
      "mini-columns",
      "color-strip",
      "range-scale",
      "trend",
      "calendar-heatmap",
      "matrix-heatmap",
      "relation-list",
      "scatter-chart",
      // App blocks
      "info-tip",
      "chart-skeleton",
      "breathing-guide",
      "dock",
      "day-pager",
      "copy-field",
      "auth-layout",
      // Marketing
      "site-page",
      "site-header",
      "hero",
      "section",
      "logo-cloud",
      "feature-grid",
      "feature-rows",
      "stat-band",
      "device-frames",
      "testimonials",
      "pricing",
      "faq",
      "cta-band",
      "newsletter",
      "legal-page",
      "site-footer",
      // Assistant
      "conversation",
      "chat-message",
      "prompt-input",
      "tool-call-card",
      "approval-card",
      "surface-frame",
      "suggestions",
      "sources",
      "thread-list",
    ],
    groups: [
      { title: "Actions", from: "button" },
      { title: "Inputs", from: "field" },
      { title: "Date and time", from: "calendar" },
      { title: "Display", from: "card" },
      { title: "Feedback", from: "alert" },
      { title: "Overlays", from: "dialog" },
      { title: "Layout", from: "split-pane" },
      { title: "Navigation", from: "tabs" },
      { title: "Steps, ranges and trees", from: "stepper" },
      { title: "Data", from: "table" },
      { title: "Charts", from: "line-chart" },
      { title: "App blocks", from: "info-tip" },
      { title: "Marketing", from: "site-page" },
      { title: "Assistant", from: "conversation" },
    ],
  },
  {
    slug: "frontend",
    title: "Front end",
    description: "Draw your screens in Next.js (React) or Nuxt (Vue) beside the same API: the typed client, sign-in, tables, theming and translations.",
    pages: [
      "introduction",
      "typed-client",
      "auth-and-bff",
      "nextjs",
      "nuxt",
      "data-tables",
      "theming",
      "localization",
    ],
  },
  {
    slug: "microservices",
    title: "Microservices",
    description: "Split an app into services that talk over message transports, gRPC and GraphQL, with one trace across them.",
    pages: ["introduction", "transports", "request-response", "events", "grpc", "graphql", "tracing"],
  },
  {
    slug: "ai",
    title: "AI",
    description: "Skills, the Navigator MCP server and Markdown docs for your coding agent, and the SDK, agents, embeddings and MCP servers for your app.",
    pages: [
      "introduction",
      // For your coding agent
      "agent-skills",
      "navigator",
      "docs-for-agents",
      // In your app
      "ai-sdk",
      "agents-and-tools",
      "generative-ui",
      "embeddings-and-rag",
      "mcp-servers",
    ],
    groups: [
      { title: "For your coding agent", from: "agent-skills" },
      { title: "In your app", from: "ai-sdk" },
    ],
  },
  {
    slug: "packages",
    title: "Packages",
    description: "Add-ons for real products: search, billing, feature flags, settings, OAuth, webhooks, integrations, documents, media, workflows and more.",
    pages: [
      // Product features
      "search",
      "billing",
      "feature-flags",
      "settings",
      "localization",
      // Identity
      "oauth-server",
      "social-sign-in",
      "directory-sync",
      "custom-domains",
      // Integrations and automation
      "webhooks",
      "integrations",
      "workflows",
      "automations",
      // Files and documents
      "documents",
      "documents-limits",
      "media",
      "imports-and-exports",
      "imports-and-exports-limits",
      // Testing
      "browser-testing",
    ],
    groups: [
      { title: "Product features", from: "search" },
      { title: "Identity", from: "oauth-server" },
      { title: "Integrations and automation", from: "webhooks" },
      { title: "Files and documents", from: "documents" },
      { title: "Testing", from: "browser-testing" },
    ],
  },
  {
    slug: "tools",
    title: "Tools",
    description: "The first-party tools: Lighthouse, Logbook, Fleet and Helm run inside your app, Harbor deploys it and Dock runs its local services.",
    pages: ["lighthouse", "logbook", "fleet", "helm", "harbor", "dock"],
  },
  {
    slug: "testing",
    title: "Testing",
    description: "Test routes, Navree UI pages, the database and workspace isolation with fakes, and run the conformity checks that keep an app consistent.",
    pages: [
      "getting-started",
      "http-tests",
      "ui-pages",
      "database-tests",
      "fakes",
      "isolation-tests",
      "conformity-checks",
    ],
  },
  {
    slug: "example-app",
    title: "Example application",
    description: "A tour of the example CRM, and the example apps drawn with Navree UI: a store, a board and a support inbox.",
    pages: ["crm-tour", "ui-apps", "store-tour", "board-tour", "inbox-tour"],
  },
];

/** Links shown after the sections in the sidebar. */
export const links: DocsLink[] = [
  { title: "UI", href: "/ui/" },
  { title: "Ecosystem", href: "/ecosystem/" },
  { title: "Architecture", href: "/architecture/" },
  { title: "AI", href: "/ai/" },
  { title: "Performance", href: "/performance/" },
  { title: "Showcase", href: "/showcase/" },
];
