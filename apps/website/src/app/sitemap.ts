import type { MetadataRoute } from "next";

import { allDocs } from "@/lib/docs";
import { lastModified, lastModifiedUnder } from "@/lib/last-modified";
import { site } from "@site";

export const dynamic = "force-static";

/**
 * `/sitemap.xml`: every public page, with the date its content last changed
 * (from git). Leaves out the styleguide (an authoring preview), the
 * `/benchmarks/` redirect and anything else marked noindex.
 */
export default function sitemap(): MetadataRoute.Sitemap {
  const url = (path: string) => `${site.url}${path}`;
  const pages: [string, string[]][] = [
    ["/", ["src/app/page.tsx", "src/components/home", "src/content", "site.config.ts"]],
    ["/docs/", ["src/app/docs/page.tsx", "content/docs/manifest.ts"]],
    ["/ecosystem/", ["src/app/ecosystem", "src/components/ecosystem", "src/content/ecosystem.ts"]],
    ["/architecture/", ["src/app/architecture", "src/components/diagrams"]],
    ["/ai/", ["src/app/ai", "src/components/ai"]],
    ["/performance/", ["src/app/performance", "src/components/bench", "src/lib/bench.ts"]],
    ["/showcase/", ["src/app/showcase/page.tsx"]],
    ["/showcase/crm/", ["src/app/showcase/crm", "src/components/showcase", "public/images/crm"]],
  ];
  return [
    ...pages.map(([path, sources]) => ({ url: url(path), lastModified: lastModifiedUnder(...sources) })),
    ...allDocs().map((doc) => ({ url: url(doc.href), lastModified: lastModified(`content/docs/${doc.file}`) })),
  ];
}
