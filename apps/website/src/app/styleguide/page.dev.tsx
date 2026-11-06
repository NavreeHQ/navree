import fs from "node:fs";
import path from "node:path";

import matter from "gray-matter";
import type { Metadata } from "next";

import { Toc } from "@/components/docs/toc";
import { renderMdx } from "@/lib/mdx";

import { SiteComponents } from "./site-components";

export const metadata: Metadata = {
  title: "Components preview",
  description: "Every docs component and Markdown element, rendered with the site's styles. Shown only while the site runs in development.",
  robots: { index: false, follow: false },
};

/**
 * Renders `content/styleguide.mdx` (every docs component, for authors),
 * then the site building blocks: shadcn/ui and Magic UI, as restyled. The
 * file is `page.dev.tsx`, so this route exists only under `next dev` (see
 * `next.config.ts`).
 */
export default async function Styleguide() {
  const file = fs.readFileSync(path.join(process.cwd(), "content", "styleguide.mdx"), "utf8");
  const { data, content: source } = matter(file);
  const { content, headings } = await renderMdx(source);
  return (
    <div className="mx-auto w-full max-w-[90rem] px-4 py-12 sm:px-6 lg:px-8 xl:grid xl:grid-cols-[minmax(0,46rem)_13.5rem] xl:justify-center xl:gap-16">
      <article className="min-w-0">
        <h1 className="text-[2.25rem] leading-[1.12] font-semibold tracking-[-0.03em] text-fg sm:text-[2.625rem]">{String(data.title)}</h1>
        <p className="mt-4 mb-9 text-lg leading-relaxed text-fg-2">{String(data.description)}</p>
        <div className="prose-navree">{content}</div>
        <SiteComponents />
      </article>
      <aside className="hidden xl:block">
        <div className="sticky top-[calc(var(--header-h)+3rem)]">
          <Toc headings={headings} />
        </div>
      </aside>
    </div>
  );
}
