import type { MetadataRoute } from "next";

import { allDocs, sectionIndexes } from "@/lib/docs";
import { lastModified } from "@/lib/last-modified";
import { pageModified, pageRoutes, sectionModified } from "@/lib/site-pages";
import { site } from "@site";

export const dynamic = "force-static";

/**
 * `/sitemap.xml`: every public page, with the date its content last changed
 * (from git). The pages that are not docs pages are found by walking
 * `src/app` (`pageRoutes`), so a new one needs no edit here; a page marked
 * noindex (the styleguide, the `/benchmarks/` redirect) is left out. Docs
 * pages and the page of each docs section come from the manifest.
 */
export default function sitemap(): MetadataRoute.Sitemap {
  const url = (path: string) => `${site.url}${path}`;
  return [
    ...pageRoutes().map((route) => ({ url: url(route), lastModified: pageModified(route) })),
    ...sectionIndexes().map((section) => ({ url: url(section.href), lastModified: sectionModified(section.slug) })),
    ...allDocs().map((doc) => ({ url: url(doc.href), lastModified: lastModified(`content/docs/${doc.file}`) })),
  ];
}
