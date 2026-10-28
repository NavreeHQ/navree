import type { MetadataRoute } from "next";

import { site } from "@site";

export const dynamic = "force-static";

/** `/robots.txt`: everything may be crawled; the sitemap lists the pages. */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: { userAgent: "*", allow: "/" },
    sitemap: `${site.url}/sitemap.xml`,
  };
}
