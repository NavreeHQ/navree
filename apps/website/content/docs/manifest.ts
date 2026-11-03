/**
 * The documentation's navigation: sections in sidebar order, and each
 * section's pages in order. This file is the single source of truth for
 * the sidebar, previous/next links, the search index and the static pages
 * that get built.
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
  /** Page slugs in order: `content/docs/<section>/<page>.mdx`. */
  pages: string[];
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
    pages: ["introduction", "release-notes", "roadmap", "contributing"],
  },
  {
    slug: "getting-started",
    title: "Getting started",
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
    slug: "architecture",
    title: "Architecture concepts",
    pages: [
      "request-lifecycle",
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
    pages: [
      "overview",
      "modular-monolith",
      "microservices",
      "multi-tenancy",
      "event-driven",
      "api-first",
      "backend-for-frontend",
    ],
  },
  {
    slug: "basics",
    title: "The basics",
    pages: [
      "routing",
      "access-control",
      "middleware",
      "requests",
      "validation",
      "responses",
      "errors",
      "logging",
      "rate-limiting",
      "cors-and-security-headers",
      "sessions-and-csrf",
    ],
  },
  {
    slug: "security",
    title: "Security",
    pages: [
      "authentication",
      "account-flows",
      "sessions",
      "passwordless",
      "impersonation",
      "single-sign-on",
      "authorization",
      "encryption-and-hashing",
      "signatures-and-keys",
      "audit-log",
    ],
  },
  {
    slug: "workspaces",
    title: "Workspaces",
    pages: [
      "introduction",
      "resolution",
      "members-and-roles",
      "invitations",
      "isolation",
      "organizations",
      "partitions",
      "shared-libraries",
      "demo-workspaces",
    ],
  },
  {
    slug: "database",
    title: "Database",
    pages: [
      "getting-started",
      "entities-and-repositories",
      "unit-of-work",
      "migrations",
      "running-migrations",
      "schema-builder",
      "columns",
      "indexes-and-foreign-keys",
      "squashing-migrations",
      "timescaledb",
      "seeding-and-factories",
      "pagination",
    ],
  },
  {
    slug: "orm",
    title: "ORM",
    pages: [
      "getting-started",
      "models",
      "query-builder",
      "relationships",
      "eager-loading",
      "collections",
      "pagination-and-streaming",
      "casts-and-accessors",
      "serialization",
      "events-and-observers",
      "scopes-and-soft-deletes",
      "connections-and-transactions",
      "route-model-binding",
      "workspaces-and-access",
      "factories",
      "testing",
      "mongodb",
    ],
  },
  {
    slug: "digging-deeper",
    title: "Digging deeper",
    pages: [
      "cache",
      "redis",
      "events",
      "queues",
      "batches-and-operations",
      "scheduling",
      "time-zones",
      "mail",
      "mail-delivery",
      "inbound-mail",
      "notifications",
      "notification-channels",
      "notification-preferences",
      "storage",
      "http-client",
      "broadcasting",
      "views",
      "openapi",
      "health-and-metrics",
      "cli",
      "maintenance-mode",
    ],
  },
  {
    slug: "ui",
    title: "UI",
    pages: [
      "choosing-a-ui",
      "overview",
      "foundations",
      "icons",
      "layout",
      "app-shell",
      "forms",
      "data-table",
      "commands-and-ops",
      "regions",
      "coming-next",
    ],
  },
  {
    slug: "components",
    title: "UI components",
    pages: [
      "overview",
      // Actions
      "button",
      "button-group",
      "toggle",
      // Inputs
      "field",
      "input",
      "input-group",
      "textarea",
      "native-select",
      "select",
      "combobox",
      "multi-select",
      "checkbox",
      "switch",
      "radio-group",
      // Display
      "card",
      "badge",
      "avatar",
      "item",
      "typography",
      "kbd",
      "attachment",
      "message",
      // Feedback
      "alert",
      "empty",
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
      "accordion",
      "collapsible",
      "scroll-area",
      "separator",
      "aspect-ratio",
      "direction",
      // Navigation
      "breadcrumb",
      "pagination",
      "command",
      // Data
      "table",
      "stat",
    ],
  },
  {
    slug: "frontend",
    title: "Front end",
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
    pages: ["introduction", "transports", "request-response", "events", "grpc", "graphql", "tracing"],
  },
  {
    slug: "ai",
    title: "AI",
    pages: [
      "introduction",
      "agent-skills",
      "navigator",
      "docs-for-agents",
      "ai-sdk",
      "agents-and-tools",
      "embeddings-and-rag",
      "mcp-servers",
    ],
  },
  {
    slug: "packages",
    title: "Packages",
    pages: [
      "search",
      "billing",
      "feature-flags",
      "settings",
      "oauth-server",
      "social-sign-in",
      "webhooks",
      "integrations",
      "directory-sync",
      "custom-domains",
      "documents",
      "media",
      "imports-and-exports",
      "workflows",
      "automations",
      "localization",
      "browser-testing",
    ],
  },
  {
    slug: "tools",
    title: "Tools",
    pages: ["lighthouse", "logbook", "fleet", "helm", "harbor", "dock"],
  },
  {
    slug: "testing",
    title: "Testing",
    pages: [
      "getting-started",
      "http-tests",
      "database-tests",
      "fakes",
      "isolation-tests",
      "conformity-checks",
    ],
  },
  {
    slug: "example-app",
    title: "Example application",
    pages: ["crm-tour"],
  },
];

/** Links shown after the sections in the sidebar. */
export const links: DocsLink[] = [
  { title: "Ecosystem", href: "/ecosystem/" },
  { title: "Architectures", href: "/architecture/" },
  { title: "Performance", href: "/performance/" },
];
