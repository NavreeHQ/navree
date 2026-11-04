import fs from "node:fs";
import path from "node:path";

import { lastModifiedUnder, latestCommit } from "./last-modified.ts";

/**
 * The site's pages that are not docs pages (`src/app/<route>/page.tsx`),
 * for the sitemap, the breadcrumbs and the structured data. Every route that
 * has a `page.tsx` is found by walking `src/app`, so a new page is in the
 * sitemap with no edit here; this file only says more about a page than the
 * folder tells:
 *
 * - `name`: the short name the breadcrumb trail shows;
 * - `kind`: `CollectionPage` for a page whose job is to list others,
 *   `AboutPage` for the page about who built Navree;
 * - `parent`: the page above it in the trail (default: the homepage);
 * - `sources`: the files whose last change dates the page, in addition to
 *   its own folder (`lastmod` in the sitemap, `dateModified` in the data).
 *
 * A page left out of the registry gets its folder name as its name, kind
 * `WebPage`, the homepage as its parent and its own folder as its source.
 */
export type PageKind = "WebPage" | "CollectionPage" | "AboutPage";

export type PageInfo = {
  name: string;
  kind?: PageKind;
  parent?: string;
  sources?: string[];
};

export const pageInfo: Record<string, PageInfo> = {
  "/": {
    name: "Navree",
    sources: ["src/components/home", "src/content", "site.config.ts"],
  },
  "/docs/": { name: "Docs", kind: "CollectionPage", sources: ["content/docs/manifest.ts"] },
  "/ecosystem/": {
    name: "Ecosystem",
    kind: "CollectionPage",
    sources: ["src/components/ecosystem", "src/content/ecosystem.ts"],
  },
  "/architecture/": {
    name: "Architecture",
    sources: ["src/components/architecture", "src/components/diagrams", "src/content/faq.ts"],
  },
  "/ai/": { name: "AI", sources: ["src/components/ai"] },
  "/ui/": {
    name: "UI",
    sources: [
      "src/components/ui-docs",
      "src/components/showcase",
      "src/lib/ui-page-data.ts",
      "content/ui",
      "content/docs/ui/coming-next.mdx",
      "content/showcase/store.json",
      "public/images/showcase",
    ],
  },
  "/performance/": { name: "Performance", sources: ["src/components/bench", "src/lib/bench.ts"] },
  "/showcase/": { name: "Showcase", kind: "CollectionPage" },
  "/showcase/crm/": {
    name: "CRM",
    parent: "/showcase/",
    sources: ["src/components/showcase", "public/images/crm"],
  },
  "/showcase/store/": {
    name: "Store",
    parent: "/showcase/",
    sources: ["src/app/ui/showcases.ts", "content/showcase/store.json", "content/ui/catalog.json"],
  },
  "/showcase/inbox/": {
    name: "Support inbox",
    parent: "/showcase/",
    sources: ["src/app/showcase/inbox", "src/components/showcase/inbox-numbers.ts", "content/ui/catalog.json"],
  },
  "/changelog/": { name: "Changelog", kind: "CollectionPage", sources: ["src/lib/changelog.ts"] },
  "/about/": { name: "About", kind: "AboutPage", sources: ["src/components/about", "src/content/about.ts", "src/content/faq.ts"] },
};

const WEBSITE = process.cwd();
const APP = path.join(WEBSITE, "src", "app");

/** Routes under `src/app` that are not pages of the sitemap. */
const NOT_PAGES = new Set(["docs/[...slug]"]);

/** The route of every `page.tsx` under `src/app` that is not marked noindex, as `/ui/`. */
export function pageRoutes(): string[] {
  const routes: string[] = [];
  const walk = (dir: string, route: string) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.name.startsWith("_") || entry.name.startsWith("@")) continue;
      const next = route ? `${route}/${entry.name}` : entry.name;
      if (!NOT_PAGES.has(next)) {
        const page = path.join(dir, entry.name, "page.tsx");
        if (fs.existsSync(page) && !/index:\s*false/.test(fs.readFileSync(page, "utf8"))) routes.push(`/${next}/`);
      }
      walk(path.join(dir, entry.name), next);
    }
  };
  if (fs.existsSync(path.join(APP, "page.tsx"))) routes.push("/");
  walk(APP, "");
  return routes.sort((a, b) => a.length - b.length || a.localeCompare(b));
}

/** What the registry says about a route, with the defaults filled in. */
export function infoFor(route: string): Required<Omit<PageInfo, "sources">> & { sources: string[] } {
  const known = pageInfo[route];
  const folder = route === "/" ? "src/app/page.tsx" : `src/app${route.replace(/\/$/, "")}`;
  const segments = route.split("/").filter(Boolean);
  return {
    name: known?.name ?? (segments.at(-1) ?? "Navree").replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase()),
    kind: known?.kind ?? "WebPage",
    parent: known?.parent ?? (route === "/" ? "" : segments.length > 1 ? `/${segments.slice(0, -1).join("/")}/` : "/"),
    sources: [folder, ...(known?.sources ?? [])],
  };
}

/** When a non-docs page last changed (ISO date): the latest commit to its folder or its sources. */
export function pageModified(route: string): string {
  if (route === "/changelog/") return latestCommit();
  return lastModifiedUnder(...infoFor(route).sources);
}

/** When a docs section's own page (`/docs/<section>/`) last changed: its manifest entry, its pages or the page layout. */
export function sectionModified(section: string): string {
  return lastModifiedUnder("content/docs/manifest.ts", `content/docs/${section}`, "src/components/docs/section-index.tsx");
}

/** The trail from the homepage to a page: `[{ name: "Navree", path: "/" }, ..., { name, path }]`. */
export function trailFor(route: string, ownName?: string): { name: string; path: string }[] {
  const trail: { name: string; path: string }[] = [];
  let current = route;
  const seen = new Set<string>();
  while (current && !seen.has(current)) {
    seen.add(current);
    const info = infoFor(current);
    trail.unshift({ name: current === route && ownName ? ownName : info.name, path: current });
    current = info.parent;
  }
  return trail;
}

/** Where a docs section's breadcrumb step points: the section's own page, `/docs/<section>/`. */
export function docSectionPath(section: string): string {
  return `/docs/${section}/`;
}
