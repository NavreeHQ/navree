import type { Metadata } from "next";

import { site } from "@site";

import { docTitle as fitDocTitle, metaDescription, sectionPageTitle } from "./seo-text.ts";

export { metaDescription } from "./seo-text.ts";

/** The social card every page shares (`src/app/og.png/route.tsx`). */
export const ogImage = {
  url: "/og.png",
  width: 1200,
  height: 630,
  alt: `${site.name}: ${site.headline} ${site.tagline}`,
};

/** Where the changelog's feed is, so readers and crawlers can follow what changed. */
export const changelogFeed = { url: "/changelog/feed.xml", title: `${site.name} changelog` };

/**
 * The `<title>` of a docs page, 30 to 60 characters: the page's title, its
 * section and the documentation's name (see `seo-text.ts`).
 */
export function docTitle(title: string, sectionTitle: string): string {
  return fitDocTitle(title, sectionTitle, site.name);
}

/** The `<title>` of a docs section's own page, 30 to 60 characters (see `seo-text.ts`). */
export function sectionTitle(title: string): string {
  return sectionPageTitle(title, site.name);
}

/**
 * A page's title, description, canonical URL and social tags in one place.
 * Next replaces (doesn't merge) the layout's `openGraph` and `twitter` when a
 * page sets them, so every page passes everything here: a page that leaves
 * them out would share the homepage's og:url and title.
 *
 * `path` is the page's URL path with its trailing slash (`/ecosystem/`); the
 * root layout's `metadataBase` makes it absolute.
 *
 * The description is fitted to a search result (`metaDescription`); pass
 * `descriptionContext` for a page whose description can be very short.
 */
export function pageMetadata({
  title,
  description,
  path,
  type = "website",
  absoluteTitle = false,
  htmlTitle,
  descriptionContext,
  modified,
  section,
  extra,
}: {
  title: string;
  description: string;
  path: string;
  type?: "website" | "article";
  /** Use `title` as the whole `<title>`, without the "| Navree" suffix (the homepage). */
  absoluteTitle?: boolean;
  /** The whole `<title>`, when it should differ from the social title (docs pages: `docTitle`). */
  htmlTitle?: string;
  /** A sentence added to a description under 70 characters (`Components in the Navree documentation.`). */
  descriptionContext?: string;
  /** When the page's content last changed (ISO date): `article:modified_time`. */
  modified?: string;
  /** The section an article belongs to: `article:section`. */
  section?: string;
  /** More `<head>` metadata, merged in as is (robots, alternates.types). */
  extra?: Metadata & { alternates?: { types?: Record<string, string> } };
}): Metadata {
  const fitted = metaDescription(description, descriptionContext);
  return {
    ...extra,
    title: htmlTitle ? { absolute: htmlTitle } : absoluteTitle ? { absolute: title } : title,
    description: fitted,
    alternates: {
      ...extra?.alternates,
      canonical: path,
      types: {
        "application/atom+xml": [{ url: changelogFeed.url, title: changelogFeed.title }],
        ...extra?.alternates?.types,
      },
    },
    openGraph: {
      type,
      siteName: site.name,
      locale: "en_US",
      url: path,
      title,
      description: fitted,
      images: [ogImage],
      ...(type === "article" ? { ...(modified ? { modifiedTime: modified } : {}), ...(section ? { section } : {}) } : {}),
    },
    twitter: {
      card: "summary_large_image",
      title,
      description: fitted,
      images: [{ url: ogImage.url, alt: ogImage.alt }],
    },
  };
}
