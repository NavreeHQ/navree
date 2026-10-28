import { site } from "@site";

/**
 * schema.org structured data (JSON-LD) for search engines. The organisation
 * and the website are declared once, in the root layout, with stable `@id`s
 * that the page-level data (the software on the homepage, the docs articles
 * and their breadcrumbs) refers to.
 */

export const ORGANIZATION_ID = `${site.url}/#organization`;
export const WEBSITE_ID = `${site.url}/#website`;
export const SOFTWARE_ID = `${site.url}/#software`;

const abs = (path: string) => new URL(path, site.url).toString();

type Json = Record<string, unknown>;

/** A `<script type="application/ld+json">`, with `<` escaped so the JSON can't close the tag. */
export function JsonLd({ data }: { data: Json }) {
  return <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(data).replace(/</g, "\\u003c") }} />;
}

/** The organisation behind Navree and the website itself (root layout). */
export function siteGraph(): Json {
  return {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "Organization",
        "@id": ORGANIZATION_ID,
        name: site.name,
        url: site.url,
        logo: { "@type": "ImageObject", url: abs("/icon-512.png"), width: 512, height: 512 },
        sameAs: [site.repoUrl],
      },
      {
        "@type": "WebSite",
        "@id": WEBSITE_ID,
        name: site.name,
        url: site.url,
        description: site.description,
        inLanguage: "en",
        publisher: { "@id": ORGANIZATION_ID },
      },
    ],
  };
}

/** Navree as software: the framework's source code and where it lives (homepage). */
export function softwareData(): Json {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareSourceCode",
    "@id": SOFTWARE_ID,
    name: site.name,
    description: site.description,
    url: site.url,
    codeRepository: site.repoUrl,
    programmingLanguage: { "@type": "ComputerLanguage", name: "Rust", url: "https://www.rust-lang.org/" },
    runtimePlatform: `Rust ${site.rustVersion} or newer`,
    license: ["https://opensource.org/licenses/MIT", "https://www.apache.org/licenses/LICENSE-2.0"],
    isAccessibleForFree: true,
    author: { "@id": ORGANIZATION_ID },
    publisher: { "@id": ORGANIZATION_ID },
    image: abs("/og.png"),
  };
}

/** A docs page as a technical article, with its breadcrumb trail. */
export function docData(doc: {
  title: string;
  description: string;
  href: string;
  section: string;
  sectionTitle: string;
  dateModified?: string;
}): Json {
  const url = abs(doc.href);
  return {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        headline: doc.title,
        description: doc.description,
        url,
        mainEntityOfPage: url,
        articleSection: doc.sectionTitle,
        inLanguage: "en",
        ...(doc.dateModified ? { dateModified: doc.dateModified } : {}),
        image: abs("/og.png"),
        author: { "@id": ORGANIZATION_ID },
        publisher: { "@id": ORGANIZATION_ID },
        isPartOf: { "@id": WEBSITE_ID },
        about: { "@id": SOFTWARE_ID },
      },
      {
        "@type": "BreadcrumbList",
        "@id": `${url}#breadcrumb`,
        itemListElement: [
          { "@type": "ListItem", position: 1, name: site.name, item: site.url + "/" },
          { "@type": "ListItem", position: 2, name: "Docs", item: abs("/docs/") },
          { "@type": "ListItem", position: 3, name: doc.sectionTitle, item: abs(`/docs/#section-${doc.section}`) },
          { "@type": "ListItem", position: 4, name: doc.title, item: url },
        ],
      },
    ],
  };
}
