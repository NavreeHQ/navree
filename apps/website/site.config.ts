/**
 * Every site-wide string lives here: change the name, the links or the
 * install commands in one place and the whole site follows.
 */
export const site = {
  name: "Navree",
  domain: "navree.com",
  url: "https://navree.com",
  tagline: "Laravel's completeness, NestJS's structure, Rust's speed and footprint.",
  description:
    "Navree is a web application framework for Rust. Routing, validation, an ORM, auth, queues, mail, real time and testing come in the box, organised into modules like NestJS, running on Tokio and Axum.",
  // TODO(user): set the real repository URL
  repoUrl: "https://github.com/navree-sh/navree",
  /** The branch that "Edit this page" links point at. */
  repoBranch: "main",
  /** Where this site lives inside the repository. */
  websiteDir: "apps/website",
  license: "MIT OR Apache-2.0",
  install: {
    /** Installs the `navree` command-line tool. */
    cli: "cargo install navree-cli",
    /** Creates a new application. */
    newApp: "navree new blog",
    /** Adds the framework to an existing Cargo project. */
    crate: "cargo add navree",
  },
  /** The minimum supported Rust version. */
  rustVersion: "1.95",
} as const;

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
