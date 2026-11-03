import type { MetadataRoute } from "next";

import { aiCrawlers } from "@/lib/crawlers";
import { site } from "@site";

export const dynamic = "force-static";

/** `/robots.txt`: everything may be crawled, by search engines and by the AI crawlers in `src/lib/crawlers.ts`; the sitemap lists the pages. */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: [{ userAgent: "*", allow: "/" }, ...aiCrawlers.map((userAgent) => ({ userAgent, allow: "/" }))],
    sitemap: `${site.url}/sitemap.xml`,
  };
}
