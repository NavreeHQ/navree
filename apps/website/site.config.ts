/**
 * Every site-wide string lives here: change the name, the links or the
 * install commands in one place and the whole site follows.
 */
export const site = {
  name: "Navree",
  domain: "navree.com",
  url: "https://navree.com",
  /** The category line: the homepage H1 and the default page title. */
  headline: "The complete backend framework for Rust.",
  tagline: "Modular, API-first and AI-native. Everything your backend needs, checked by the compiler.",
  description:
    "Navree is a batteries-included Rust framework for building secure, scalable server-side applications. Modules and dependency injection give every app the same clear structure, and auth, workspaces, an ORM, queues, search, real time, monitoring, deployment and AI come in the box.",
  repoUrl: "https://github.com/NavreeHQ/navree",
  /** The branch that "Edit this page" links point at. */
  repoBranch: "main",
  /** Where this site lives inside the repository. */
  websiteDir: "apps/website",
  license: "MIT OR Apache-2.0",
  // The crates aren't on crates.io yet, so installs come from git. After the
  // first release: `cargo install navree-cli` and `cargo add navree`.
  install: {
    /** Installs the `navree` command-line tool. */
    cli: "cargo install --git https://github.com/NavreeHQ/navree navree-cli",
    /** Creates a new application. */
    newApp: "navree new blog",
    /** Adds the framework to an existing Cargo project. */
    crate: "cargo add --git https://github.com/NavreeHQ/navree navree",
  },
  /** The minimum supported Rust version. */
  rustVersion: "1.95",
  /**
   * Pages and catalogue entries for things not built yet carry
   * `status: "planned"`. While this is false the site shows everything as
   * available; set it to true to show an "In development" badge on each.
   */
  showPlannedBadges: false as boolean,
  /** The main navigation, read by the header and the footer. */
  nav: [
    { label: "Docs", href: "/docs/", match: "/docs" },
    { label: "Ecosystem", href: "/ecosystem/", match: "/ecosystem" },
    { label: "Architecture", href: "/architecture/", match: "/architecture" },
    { label: "AI", href: "/ai/", match: "/ai" },
    { label: "Performance", href: "/performance/", match: "/performance" },
  ],
  /** The "Get started" button in the header. */
  getStarted: "/docs/getting-started/installation/",
} as const;

export type NavItem = (typeof site.nav)[number];

/** A link to `path` (repository-relative) on the default branch. */
export function repoBlob(path: string): string {
  return `${site.repoUrl}/blob/${site.repoBranch}/${path}`;
}

/** A link to `path` (repository-relative) as a directory on the default branch. */
export function repoTree(path: string): string {
  return `${site.repoUrl}/tree/${site.repoBranch}/${path}`;
}

/** The "Edit this page" link of a docs file, relative to `content/docs`. */
export function editUrl(contentPath: string): string {
  return `${site.repoUrl}/edit/${site.repoBranch}/${site.websiteDir}/content/docs/${contentPath}`;
}
