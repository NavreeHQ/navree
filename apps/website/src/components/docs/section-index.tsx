import Link from "next/link";

import { ArrowLeftIcon, ArrowRightIcon } from "@/components/icons";
import { InlineMarkdown } from "@/components/inline-markdown";
import { sectionIndexes, type SectionIndex } from "@/lib/docs";

/**
 * A docs section's own page (`/docs/<section>/`): what the section covers
 * and every page in it with its description, read from the manifest and the
 * pages' front-matter so it never goes stale.
 */
export function SectionIndexPage({ section }: { section: SectionIndex }) {
  const all = sectionIndexes();
  const at = all.findIndex((s) => s.slug === section.slug);
  const previous = at > 0 ? all[at - 1] : null;
  const next = at >= 0 && at < all.length - 1 ? all[at + 1] : null;
  const first = section.pages[0];

  return (
    <div className="py-9 lg:py-12">
      <header className="max-w-[46rem]">
        <nav aria-label="Breadcrumb" className="text-sm text-fg-3">
          <ol className="flex flex-wrap items-center gap-1.5">
            <li>
              <Link href="/docs/" className="transition-colors hover:text-fg">
                Docs
              </Link>
            </li>
            <li aria-hidden>/</li>
            <li aria-current="page">{section.title}</li>
          </ol>
        </nav>
        <h1 className="mt-3 text-[2.25rem] leading-[1.12] font-semibold tracking-[-0.03em] text-balance text-fg sm:text-[2.625rem]">
          {section.title}
        </h1>
        <p className="mt-4 text-lg leading-relaxed text-pretty text-fg-2">
          <InlineMarkdown text={section.description} />
        </p>
        {first ? (
          <p className="mt-5 text-sm text-fg-3">
            {section.pages.length} {section.pages.length === 1 ? "page" : "pages"}. Start with{" "}
            <Link href={first.href} className="link">
              {first.title}
            </Link>
            .
          </p>
        ) : null}
      </header>

      <div className="mt-10 max-w-[60rem] space-y-10">
        {section.groups.map((group, i) => (
          <section key={group.title ?? i} aria-labelledby={group.title ? `group-${i}` : undefined}>
            {group.title ? (
              <h2 id={`group-${i}`} className="border-b border-line pb-3 text-lg font-semibold text-fg">
                {group.title}
              </h2>
            ) : null}
            <ul className={`grid gap-3 sm:grid-cols-2 ${group.title ? "mt-4" : ""}`}>
              {group.pages.map((page) => (
                <li key={page.slug}>
                  <Link
                    href={page.href}
                    className="group flex h-full flex-col rounded-2xl border border-line bg-ink-1 p-4 shadow-bezel transition-colors duration-200 hover:border-line-2 hover:bg-ink-2"
                  >
                    <span className="flex items-start justify-between gap-3 font-medium text-fg">
                      {page.title}
                      <ArrowRightIcon
                        aria-hidden
                        className="mt-0.5 size-4 shrink-0 text-fg-3 transition-[transform,color] duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:text-accent"
                      />
                    </span>
                    <span className="mt-1 text-sm leading-relaxed text-pretty text-fg-2">
                      <InlineMarkdown text={page.description} />
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>

      <nav aria-label="Previous and next sections" className="mt-14 grid max-w-[60rem] gap-4 border-t border-line pt-8 sm:grid-cols-2">
        {previous ? (
          <Link href={previous.href} className="pager-link">
            <span className="flex items-center gap-1.5 text-[0.8125rem] text-fg-3">
              <ArrowLeftIcon className="size-3.5" /> Previous section
            </span>
            <span className="mt-1 block font-medium text-fg">{previous.title}</span>
          </Link>
        ) : (
          <span className="hidden sm:block" />
        )}
        {next ? (
          <Link href={next.href} className="pager-link sm:text-right">
            <span className="flex items-center gap-1.5 text-[0.8125rem] text-fg-3 sm:justify-end">
              Next section <ArrowRightIcon className="size-3.5" />
            </span>
            <span className="mt-1 block font-medium text-fg">{next.title}</span>
          </Link>
        ) : null}
      </nav>
    </div>
  );
}
