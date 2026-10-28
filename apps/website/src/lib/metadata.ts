import type { Metadata } from "next";

import { site } from "@site";

/** The social card every page shares (`src/app/og.png/route.tsx`). */
export const ogImage = {
  url: "/og.png",
  width: 1200,
  height: 630,
  alt: `${site.name}: ${site.headline} ${site.tagline}`,
};

/**
 * A page's title, description, canonical URL and social tags in one place.
 * Next replaces (doesn't merge) the layout's `openGraph` and `twitter` when a
 * page sets them, so every page passes everything here: a page that leaves
 * them out would share the homepage's og:url and title.
 *
 * `path` is the page's URL path with its trailing slash (`/ecosystem/`); the
 * root layout's `metadataBase` makes it absolute.
 */
export function pageMetadata({
  title,
  description,
  path,
  type = "website",
  absoluteTitle = false,
  extra,
}: {
  title: string;
  description: string;
  path: string;
  type?: "website" | "article";
  /** Use `title` as the whole `<title>`, without the "| Navree" suffix (the homepage). */
  absoluteTitle?: boolean;
  /** More `<head>` metadata, merged in as is (robots, alternates.types). */
  extra?: Metadata & { alternates?: { types?: Record<string, string> } };
}): Metadata {
  return {
    ...extra,
    title: absoluteTitle ? { absolute: title } : title,
    description,
    alternates: { ...extra?.alternates, canonical: path },
    openGraph: {
      type,
      siteName: site.name,
      locale: "en_US",
      url: path,
      title,
      description,
      images: [ogImage],
    },
    twitter: {
      card: "summary_large_image",
      title,
      description,
      images: [{ url: ogImage.url, alt: ogImage.alt }],
    },
  };
}
