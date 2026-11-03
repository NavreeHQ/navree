import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import { PageActions } from "@/components/docs/page-actions";
import { SectionIndexPage } from "@/components/docs/section-index";
import { Toc } from "@/components/docs/toc";
import { DocFaq } from "@/components/faq";
import { ArrowLeftIcon, ArrowRightIcon, EditIcon } from "@/components/icons";
import { InlineMarkdown } from "@/components/inline-markdown";
import { PlannedBadge } from "@/components/site/planned-badge";
import { allDocs, documentTitle, findDoc, findSection, neighbours, plain, sectionIndexes } from "@/lib/docs";
import { howToFor } from "@/lib/howto";
import { lastModified } from "@/lib/last-modified";
import { renderMdx } from "@/lib/mdx";
import { docTitle, pageMetadata } from "@/lib/metadata";
import { JsonLd, docData, howToData } from "@/lib/structured-data";
import { editUrl, site } from "@site";

/** Every page is also published as Markdown (`scripts/agent-files.ts`). */
const markdownPath = (slug: string) => `/docs/${slug}.md`;

type Props = { params: Promise<{ slug: string[] }> };

export const dynamicParams = false;

/** The pages (`/docs/<section>/<page>/`) and each section's own page (`/docs/<section>/`). */
export function generateStaticParams() {
  return [
    ...sectionIndexes().map((section) => ({ slug: [section.slug] })),
    ...allDocs().map((doc) => ({ slug: doc.slug.split("/") })),
  ];
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const section = slug.length === 1 ? findSection(slug[0]) : null;
  if (section) {
    return pageMetadata({ title: section.documentTitle, description: plain(section.description), path: section.href });
  }
  const doc = findDoc(slug);
  if (!doc) return {};
  return pageMetadata({
    title: documentTitle(doc),
    htmlTitle: docTitle(documentTitle(doc), doc.sectionTitle),
    description: plain(doc.description),
    descriptionContext: `${doc.sectionTitle} in the ${site.name} documentation.`,
    path: doc.href,
    type: "article",
    modified: lastModified(`content/docs/${doc.file}`),
    section: doc.sectionTitle,
    extra: { alternates: { types: { "text/markdown": markdownPath(doc.slug) } } },
  });
}

export default async function DocPage({ params }: Props) {
  const { slug } = await params;
  const section = slug.length === 1 ? findSection(slug[0]) : null;
  if (section) return <SectionIndexPage section={section} />;
  const doc = findDoc(slug);
  if (!doc) notFound();

  const { content, headings } = await renderMdx(doc.source);
  const { previous, next } = neighbours(doc.slug);
  const edit = editUrl(doc.file);
  const howTo = howToFor(doc.slug, doc.source);

  return (
    <div className="py-9 lg:py-12 xl:grid xl:grid-cols-[minmax(0,1fr)_13.5rem] xl:gap-14">
      <article className="min-w-0 max-w-[46rem]">
        <header className="mb-9 flex flex-col">
          {/* Below lg the sticky docs bar already names the section and page, so the
              actions move under the description instead of floating alone above the title. */}
          <div className="flex min-h-8 items-center gap-4 max-lg:order-last max-lg:mt-6 lg:justify-between">
            <nav aria-label="Breadcrumb" className="hidden min-w-0 text-sm text-fg-3 lg:block">
              <ol className="flex flex-wrap items-center gap-1.5">
                <li>
                  <Link href="/docs/" className="transition-colors hover:text-fg">
                    Docs
                  </Link>
                </li>
                <li aria-hidden>/</li>
                <li>
                  <Link href={`/docs/${doc.section}/`} className="transition-colors hover:text-fg">
                    {doc.sectionTitle}
                  </Link>
                </li>
              </ol>
            </nav>
            <PageActions
              title={doc.title}
              markdownPath={markdownPath(doc.slug)}
              markdownUrl={`${site.url}${markdownPath(doc.slug)}`}
            />
          </div>
          {site.showPlannedBadges && doc.status === "planned" ? <PlannedBadge className="mt-4" /> : null}
          <h1 className="mt-3 text-[2.25rem] leading-[1.12] font-semibold tracking-[-0.03em] text-balance text-fg sm:text-[2.625rem]">
            {doc.title}
          </h1>
          {doc.description ? (
            <p className="mt-4 text-lg leading-relaxed text-pretty text-fg-2">
              <InlineMarkdown text={doc.description} />
            </p>
          ) : null}
        </header>

        {headings.length ? (
          <details className="toc-mobile mb-9 rounded-xl border border-line bg-ink-1 xl:hidden">
            <summary className="cursor-pointer list-none px-4 py-3 text-sm font-semibold text-fg">On this page</summary>
            <div className="border-t border-line px-4 py-3">
              <Toc headings={headings} />
            </div>
          </details>
        ) : null}

        <div className="prose-navree">{content}</div>

        <DocFaq slug={doc.slug} />

        <footer className="mt-16 border-t border-line pt-8">
          <a href={edit} className="inline-flex items-center gap-2 text-sm text-fg-2 transition-colors hover:text-fg">
            <EditIcon className="size-4" />
            Edit this page on GitHub
          </a>
          <nav aria-label="Previous and next pages" className="mt-8 grid gap-4 sm:grid-cols-2">
            {previous ? (
              <Link href={previous.href} className="pager-link">
                <span className="flex items-center gap-1.5 text-[0.8125rem] text-fg-3">
                  <ArrowLeftIcon className="size-3.5" /> Previous
                </span>
                <span className="mt-1 block font-medium text-fg">{previous.title}</span>
              </Link>
            ) : (
              <span className="hidden sm:block" />
            )}
            {next ? (
              <Link href={next.href} className="pager-link sm:text-right">
                <span className="flex items-center gap-1.5 text-[0.8125rem] text-fg-3 sm:justify-end">
                  Next <ArrowRightIcon className="size-3.5" />
                </span>
                <span className="mt-1 block font-medium text-fg">{next.title}</span>
              </Link>
            ) : null}
          </nav>
        </footer>
      </article>

      <aside className="hidden xl:block">
        <div className="sticky top-[calc(var(--header-h)+3rem)] max-h-[calc(100dvh-var(--header-h)-4rem)] overflow-y-auto pb-6">
          <Toc headings={headings} />
          <a
            href={edit}
            className={`inline-flex items-center gap-2 text-[0.8125rem] text-fg-3 transition-colors hover:text-fg ${headings.length ? "mt-7 border-t border-line pt-5" : ""}`}
          >
            <EditIcon className="size-3.5" />
            Edit this page
          </a>
        </div>
      </aside>
      <JsonLd
        data={docData({
          title: doc.title,
          description: plain(doc.description),
          href: doc.href,
          section: doc.section,
          sectionTitle: doc.sectionTitle,
          dateModified: lastModified(`content/docs/${doc.file}`),
          markdown: markdownPath(doc.slug),
        })}
      />
      {howTo ? <JsonLd data={howToData(doc.href, howTo)} /> : null}
    </div>
  );
}
