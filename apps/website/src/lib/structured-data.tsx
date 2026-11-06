import type { Metadata } from "next";

import { founder } from "@/content/about";
import { site } from "@site";

import { ogImage } from "./metadata";
import { platforms } from "./site-facts.ts";
import { docSectionPath, infoFor, pageModified, trailFor, type PageKind } from "./site-pages.ts";
import { navreeVersion } from "./version.ts";

/**
 * schema.org structured data (JSON-LD) for search engines and AI answers.
 *
 * The site graph (root layout, every page) declares the organisation (with
 * its founder), the website, the software and its source code once, with
 * stable `@id`s. Each page adds its own graph that refers to those ids:
 *
 * - every page: a `WebPage` (or `CollectionPage` for a page that lists
 *   others, `AboutPage` for `/about/`) and its `BreadcrumbList`
 *   (`PageSchema`);
 * - a page with a film (the homepage): the film as a `VideoObject` that the
 *   page node points at (`PageSchema`'s `video`);
 * - docs pages: a `TechArticle` with `dateModified` from git, its
 *   breadcrumb and a link to its Markdown copy (`docData`), and a `HowTo`
 *   where the page is a list of steps (`howToData`);
 * - pages with questions: a `FAQPage` from the same data that draws the
 *   visible answers (`faqData`).
 *
 * `scripts/check-jsonld.ts` reads the built pages back and checks all of it.
 */

export const ORGANIZATION_ID = `${site.url}/#organization`;
export const WEBSITE_ID = `${site.url}/#website`;
export const SOFTWARE_ID = `${site.url}/#software`;
export const CODE_ID = `${site.url}/#source-code`;
export const LOGO_ID = `${site.url}/#logo`;
/** The person who built Navree, described on `/about/`. */
export const FOUNDER_ID = `${site.url}/about/#founder`;

const abs = (path: string) => new URL(path, site.url).toString();

type Json = Record<string, unknown>;

/** The licence URLs of `site.license` (`MIT OR Apache-2.0`). */
const LICENSES: Record<string, string> = {
  MIT: "https://opensource.org/licenses/MIT",
  "Apache-2.0": "https://www.apache.org/licenses/LICENSE-2.0",
};
const licenseUrls = () => site.license.split(" OR ").map((id) => LICENSES[id] ?? id);

/** The GitHub organisation that owns the repository: `https://github.com/NavreeHQ`. */
const githubOrg = () => {
  const { origin, pathname } = new URL(site.repoUrl);
  return `${origin}/${pathname.split("/").filter(Boolean)[0]}`;
};

/** A `<script type="application/ld+json">`, with `<` escaped so the JSON can't close the tag. */
export function JsonLd({ data }: { data: Json }) {
  return (
    <script
      type="application/ld+json"
      // The JSON is built from this repository's own data and every `<` is escaped,
      // so it cannot close the tag.
      // nosemgrep: navree-raw-html
      dangerouslySetInnerHTML={{ __html: JSON.stringify(data).replace(/</g, "\\u003c") }}
    />
  );
}

/**
 * The organisation, the website, the software and its source code (root
 * layout). They ride on every page so the `@id` references in the page's own
 * graph resolve inside that page.
 *
 * The website has no `SearchAction` on purpose: the site search is a dialog
 * that runs in the browser, and no URL runs a search for a query.
 */
export function siteGraph(): Json {
  const version = navreeVersion();
  const requirement = `Rust ${site.rustVersion} or newer`;
  return {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "Organization",
        "@id": ORGANIZATION_ID,
        name: site.name,
        url: site.url,
        description: site.description,
        slogan: site.headline,
        logo: {
          "@type": "ImageObject",
          "@id": LOGO_ID,
          url: abs("/icon-512.png"),
          contentUrl: abs("/icon-512.png"),
          width: 512,
          height: 512,
        },
        sameAs: [githubOrg()],
        founder: {
          "@type": "Person",
          "@id": FOUNDER_ID,
          name: founder.name,
          url: abs("/about/"),
          description: `${founder.role.replace(/^a /, "A ")} in ${founder.place}, and the author of ${site.name}.`,
          homeLocation: { "@type": "Place", name: founder.place },
          knowsAbout: [...founder.knowsAbout],
          sameAs: [founder.github.url],
        },
      },
      {
        "@type": "WebSite",
        "@id": WEBSITE_ID,
        name: site.name,
        url: site.url,
        description: site.tagline,
        inLanguage: "en",
        publisher: { "@id": ORGANIZATION_ID },
      },
      {
        "@type": "SoftwareApplication",
        "@id": SOFTWARE_ID,
        name: site.name,
        description: site.description,
        url: site.url,
        applicationCategory: "DeveloperApplication",
        applicationSubCategory: "Application framework",
        operatingSystem: platforms.join(", "),
        softwareVersion: version,
        softwareRequirements: requirement,
        releaseNotes: abs("/docs/prologue/release-notes/"),
        installUrl: abs(site.getStarted),
        license: licenseUrls(),
        isAccessibleForFree: true,
        offers: { "@type": "Offer", price: "0", priceCurrency: "INR" },
        featureList: [
          "Modules and dependency injection, checked when the app boots",
          "An ORM and migrations for TimescaleDB, PostgreSQL, MySQL, SQLite and MongoDB",
          "Authentication, access control and workspaces",
          "Queues, scheduling, search, real time and workflows",
          "Navree UI: server-rendered components written in Rust",
          "First-party tools for monitoring, queues, admin and debugging",
          "Agent skills, an MCP server and documentation as Markdown",
        ],
        keywords: [
          "Rust",
          "application framework",
          "backend",
          "server-rendered UI components",
          "ORM",
          "multi-tenancy",
          "monitoring",
          "admin panel",
          "MCP",
          "AI agents",
        ],
        image: abs(ogImage.url),
        author: { "@id": ORGANIZATION_ID },
        publisher: { "@id": ORGANIZATION_ID },
      },
      {
        "@type": "SoftwareSourceCode",
        "@id": CODE_ID,
        name: `${site.name} source code`,
        description: `The source code of ${site.name}, an open-source Rust framework for complete applications.`,
        url: site.repoUrl,
        codeRepository: site.repoUrl,
        programmingLanguage: { "@type": "ComputerLanguage", name: "Rust", url: "https://www.rust-lang.org/" },
        runtimePlatform: requirement,
        version,
        license: licenseUrls(),
        isAccessibleForFree: true,
        targetProduct: { "@id": SOFTWARE_ID },
        author: { "@id": ORGANIZATION_ID },
        publisher: { "@id": ORGANIZATION_ID },
      },
    ],
  };
}

/** A breadcrumb trail as a `BreadcrumbList` node. */
function breadcrumb(id: string, trail: { name: string; path: string }[]): Json {
  return {
    "@type": "BreadcrumbList",
    "@id": id,
    itemListElement: trail.map((step, i) => ({
      "@type": "ListItem",
      position: i + 1,
      name: step.name,
      item: abs(step.path),
    })),
  };
}

/** A page title as a string, whatever form Next's metadata gives it. */
function plainTitle(title: Metadata["title"]): string {
  if (!title) return site.name;
  if (typeof title === "string") return title;
  if ("absolute" in title && title.absolute) return title.absolute;
  if ("default" in title && title.default) return title.default;
  return site.name;
}

/** A film on a page, as `PageSchema` turns it into a `VideoObject`. Paths start with a slash. */
export type PageVideo = {
  /** The node's fragment: `<page url>#<id>`. */
  id: string;
  name: string;
  description: string;
  /** The picture shown before it plays. */
  thumbnail: string;
  /** The film's file, and its media type. */
  file: string;
  encodingFormat: string;
  /** The day it was made, `YYYY-MM-DD`. */
  uploadDate: string;
  /** ISO 8601, `PT1M5S`. */
  duration: string;
  width: number;
  height: number;
};

/**
 * The graph every non-docs page carries: the page itself (`WebPage`, or
 * `CollectionPage` where the registry in `site-pages.ts` says it lists
 * others), when it last changed (from git), its image and its breadcrumb.
 * Title and description come from the page's own `metadata` export, so they
 * can't differ from the tags in the head. `mainEntity` names the node the
 * page is mainly about (the about page: its founder). `video` is a film the
 * page plays, described as a `VideoObject` the page node points at.
 *
 *     <PageSchema path="/ui/" metadata={metadata} />
 */
export function PageSchema({
  path,
  metadata,
  kind,
  mainEntity,
  list,
  video,
}: {
  path: string;
  metadata: Metadata;
  kind?: PageKind;
  mainEntity?: string;
  /** Pages this page shows a card for, as an `ItemList` (`/ui/`: the example apps). */
  list?: { id: string; name: string; items: { name: string; description: string; path: string }[] };
  /** A film the page plays (the homepage: the launch film). */
  video?: PageVideo;
}) {
  const info = infoFor(path);
  const url = abs(path);
  const og = metadata.openGraph?.title;
  const name = typeof og === "string" ? og : plainTitle(metadata.title);
  const home = path === "/";
  const itemList: Json[] = list
    ? [
        {
          "@type": "ItemList",
          "@id": `${url}#${list.id}`,
          name: list.name,
          numberOfItems: list.items.length,
          itemListOrder: "https://schema.org/ItemListOrderAscending",
          itemListElement: list.items.map((item, i) => ({
            "@type": "ListItem",
            position: i + 1,
            name: item.name,
            description: item.description,
            item: abs(item.path),
          })),
        },
      ]
    : [];
  const videoNode: Json[] = video
    ? [
        {
          "@type": "VideoObject",
          "@id": `${url}#${video.id}`,
          name: video.name,
          description: video.description,
          thumbnailUrl: abs(video.thumbnail),
          contentUrl: abs(video.file),
          encodingFormat: video.encodingFormat,
          uploadDate: video.uploadDate,
          duration: video.duration,
          width: video.width,
          height: video.height,
          isAccessibleForFree: true,
          about: { "@id": SOFTWARE_ID },
          publisher: { "@id": ORGANIZATION_ID },
        },
      ]
    : [];
  const data: Json = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": kind ?? info.kind,
        "@id": `${url}#webpage`,
        url,
        name,
        description: typeof metadata.description === "string" ? metadata.description : site.description,
        inLanguage: "en",
        dateModified: pageModified(path),
        isPartOf: { "@id": WEBSITE_ID },
        about: { "@id": SOFTWARE_ID },
        ...(home ? { mainEntity: { "@id": SOFTWARE_ID } } : mainEntity ? { mainEntity: { "@id": mainEntity } } : {}),
        primaryImageOfPage: {
          "@type": "ImageObject",
          url: abs(ogImage.url),
          width: ogImage.width,
          height: ogImage.height,
        },
        ...(video ? { video: { "@id": `${url}#${video.id}` } } : {}),
        breadcrumb: { "@id": `${url}#breadcrumb` },
      },
      breadcrumb(`${url}#breadcrumb`, trailFor(path, home ? undefined : info.name)),
      ...itemList,
      ...videoNode,
    ],
  };
  return <JsonLd data={data} />;
}

/** A docs page as a technical article, with its breadcrumb trail and its Markdown copy. */
export function docData(doc: {
  title: string;
  description: string;
  href: string;
  section: string;
  sectionTitle: string;
  dateModified?: string;
  /** The page's Markdown copy, `/docs/<section>/<page>.md`. */
  markdown?: string;
}): Json {
  const url = abs(doc.href);
  return {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "WebPage",
        "@id": `${url}#webpage`,
        url,
        name: doc.title,
        description: doc.description,
        inLanguage: "en",
        ...(doc.dateModified ? { dateModified: doc.dateModified } : {}),
        isPartOf: { "@id": WEBSITE_ID },
        about: { "@id": SOFTWARE_ID },
        primaryImageOfPage: { "@type": "ImageObject", url: abs(ogImage.url), width: ogImage.width, height: ogImage.height },
        breadcrumb: { "@id": `${url}#breadcrumb` },
        mainEntity: { "@id": `${url}#article` },
      },
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        headline: doc.title,
        description: doc.description,
        url,
        mainEntityOfPage: url,
        articleSection: doc.sectionTitle,
        inLanguage: "en",
        isAccessibleForFree: true,
        ...(doc.dateModified ? { dateModified: doc.dateModified } : {}),
        image: abs(ogImage.url),
        author: { "@id": ORGANIZATION_ID },
        publisher: { "@id": ORGANIZATION_ID },
        isPartOf: { "@id": WEBSITE_ID },
        about: { "@id": SOFTWARE_ID },
        ...(doc.markdown
          ? { encoding: { "@type": "MediaObject", encodingFormat: "text/markdown", contentUrl: abs(doc.markdown) } }
          : {}),
      },
      breadcrumb(`${url}#breadcrumb`, [
        { name: site.name, path: "/" },
        { name: "Docs", path: "/docs/" },
        { name: doc.sectionTitle, path: docSectionPath(doc.section) },
        { name: doc.title, path: doc.href },
      ]),
    ],
  };
}

/**
 * A docs section's own page (`/docs/<section>/`) as a `CollectionPage` whose
 * main entity is the list of the section's pages, with its breadcrumb trail
 * and its Markdown copy.
 */
export function sectionData(section: {
  title: string;
  description: string;
  href: string;
  dateModified: string;
  pages: { title: string; description: string; href: string }[];
  /** The section's Markdown copy, `/docs/<section>.md`. */
  markdown?: string;
}): Json {
  const url = abs(section.href);
  return {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "CollectionPage",
        "@id": `${url}#webpage`,
        url,
        name: section.title,
        description: section.description,
        inLanguage: "en",
        dateModified: section.dateModified,
        isPartOf: { "@id": WEBSITE_ID },
        about: { "@id": SOFTWARE_ID },
        primaryImageOfPage: { "@type": "ImageObject", url: abs(ogImage.url), width: ogImage.width, height: ogImage.height },
        breadcrumb: { "@id": `${url}#breadcrumb` },
        mainEntity: { "@id": `${url}#pages` },
        ...(section.markdown
          ? { encoding: { "@type": "MediaObject", encodingFormat: "text/markdown", contentUrl: abs(section.markdown) } }
          : {}),
      },
      {
        "@type": "ItemList",
        "@id": `${url}#pages`,
        name: `Pages in ${section.title}`,
        numberOfItems: section.pages.length,
        itemListOrder: "https://schema.org/ItemListOrderAscending",
        itemListElement: section.pages.map((page, i) => ({
          "@type": "ListItem",
          position: i + 1,
          name: page.title,
          description: page.description,
          item: abs(page.href),
        })),
      },
      breadcrumb(`${url}#breadcrumb`, [
        { name: site.name, path: "/" },
        { name: "Docs", path: "/docs/" },
        { name: section.title, path: section.href },
      ]),
    ],
  };
}

/** A list of questions and answers as a `FAQPage`. Answers are plain text. */
export function faqData(path: string, items: { question: string; answer: string }[]): Json {
  const url = abs(path);
  return {
    "@context": "https://schema.org",
    "@type": "FAQPage",
    "@id": `${url}#faq`,
    url,
    inLanguage: "en",
    isPartOf: { "@id": WEBSITE_ID },
    mainEntity: items.map((item) => ({
      "@type": "Question",
      name: item.question,
      acceptedAnswer: { "@type": "Answer", text: item.answer },
    })),
  };
}

/** A page that is a list of steps as a `HowTo`. */
export function howToData(
  path: string,
  how: { name: string; description: string; steps: { name: string; text: string; anchor: string }[]; tools?: string[] },
): Json {
  const url = abs(path);
  return {
    "@context": "https://schema.org",
    "@type": "HowTo",
    "@id": `${url}#howto`,
    name: how.name,
    description: how.description,
    inLanguage: "en",
    isPartOf: { "@id": WEBSITE_ID },
    ...(how.tools?.length ? { tool: how.tools.map((name) => ({ "@type": "HowToTool", name })) } : {}),
    step: how.steps.map((step, i) => ({
      "@type": "HowToStep",
      position: i + 1,
      name: step.name,
      text: step.text,
      url: `${url}#${step.anchor}`,
    })),
  };
}
