import fs from "node:fs";
import path from "node:path";

import matter from "gray-matter";

import { links, sections } from "@content/docs/manifest";

/** The directory that holds the docs: `content/docs/<section>/<page>.mdx`. */
const DOCS_DIR = path.join(process.cwd(), "content", "docs");

export type DocMeta = {
  section: string;
  sectionTitle: string;
  page: string;
  /** `basics/validation`. */
  slug: string;
  /** `/docs/basics/validation/`. */
  href: string;
  title: string;
  description: string;
  /** Relative to `content/docs`, for the "Edit this page" link. */
  file: string;
};

export type Doc = DocMeta & { source: string };

export type NavSection = { slug: string; title: string; pages: DocMeta[] };

let cache: Doc[] | null = null;

/** Every page in the manifest, in navigation order. */
export function allDocs(): Doc[] {
  if (cache && process.env.NODE_ENV === "production") return cache;
  const docs: Doc[] = [];
  for (const section of sections) {
    for (const page of section.pages) {
      const file = `${section.slug}/${page}.mdx`;
      const full = path.join(DOCS_DIR, file);
      if (!fs.existsSync(full)) {
        throw new Error(`content/docs/${file} is listed in content/docs/manifest.ts but does not exist`);
      }
      const { data, content } = matter(fs.readFileSync(full, "utf8"));
      docs.push({
        section: section.slug,
        sectionTitle: section.title,
        page,
        slug: `${section.slug}/${page}`,
        href: `/docs/${section.slug}/${page}/`,
        title: String(data.title ?? page),
        description: String(data.description ?? ""),
        file,
        source: content,
      });
    }
  }
  cache = docs;
  return docs;
}

export function findDoc(slug: string[]): Doc | null {
  const key = slug.join("/");
  return allDocs().find((doc) => doc.slug === key) ?? null;
}

/** The sidebar: sections with their pages (without the MDX source). */
export function navigation(): { sections: NavSection[]; links: typeof links } {
  const docs = allDocs();
  return {
    sections: sections.map((section) => ({
      slug: section.slug,
      title: section.title,
      pages: docs.filter((doc) => doc.section === section.slug).map(meta),
    })),
    links,
  };
}

/** The previous and next pages, across sections. */
export function neighbours(slug: string): { previous: DocMeta | null; next: DocMeta | null } {
  const docs = allDocs();
  const index = docs.findIndex((doc) => doc.slug === slug);
  return {
    previous: index > 0 ? meta(docs[index - 1]) : null,
    next: index >= 0 && index < docs.length - 1 ? meta(docs[index + 1]) : null,
  };
}

export function meta(doc: Doc): DocMeta {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const { source, ...rest } = doc;
  return rest;
}

/** Plain text of a front-matter string: inline code marks removed. */
export function plain(text: string): string {
  return text.replace(/`/g, "");
}
