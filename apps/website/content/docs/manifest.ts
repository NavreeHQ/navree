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

/** A sidebar entry that links outside the docs content, e.g. `/benchmarks`. */
export type DocsLink = {
  title: string;
  href: string;
};

export const sections: DocsSection[] = [
  {
    // Owner: A
    slug: "prologue",
    title: "Prologue",
    pages: ["introduction", "release-notes", "roadmap", "contributing"],
  },
  {
    // Owner: A
    slug: "getting-started",
    title: "Getting started",
    pages: ["installation", "your-first-app", "configuration", "directory-structure", "deployment"],
  },
  {
    // Owner: B
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
    // Owner: B
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
    // Owner: C
    slug: "security",
    title: "Security",
    pages: ["authentication", "account-flows", "authorization", "encryption-and-hashing", "audit-log"],
  },
  {
    // Owner: D
    slug: "database",
    title: "Database",
    pages: [
      "getting-started",
      "entities-and-repositories",
      "unit-of-work",
      "migrations",
      "seeding-and-factories",
      "pagination",
    ],
  },
  {
    slug: "digging-deeper",
    title: "Digging deeper",
    pages: [
      // Owner: E
      "cache",
      "events",
      "queues",
      "scheduling",
      "mail",
      "notifications",
      // Owner: F
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
    // Owner: D
    slug: "testing",
    title: "Testing",
    pages: ["getting-started", "http-tests", "database-tests", "fakes"],
  },
  {
    // Owner: F
    slug: "example-app",
    title: "Example application",
    pages: ["crm-tour"],
  },
];

/** Links shown after the sections in the sidebar. */
export const links: DocsLink[] = [{ title: "Benchmarks", href: "/benchmarks/" }];
