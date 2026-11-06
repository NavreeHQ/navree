import type { Metadata } from "next";
import Link from "next/link";

import { ArrowRightIcon } from "@/components/icons";
import { InlineMarkdown } from "@/components/inline-markdown";
import { sectionIndexes } from "@/lib/docs";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: "Documentation for the Navree framework",
  description: `Everything ${site.name} does, from installation to the backend, the screens, the tools and your coding agent.`,
  path: "/docs/",
});

/** How many pages of a section its card lists; the section's own page lists them all. */
const PAGES_PER_CARD = 5;

/** The first steps, above the sections. */
const startHere = [
  { href: "/docs/getting-started/installation/", title: "Install the CLI", text: "Rust, the `navree` command and a new app." },
  { href: "/docs/getting-started/your-first-app/", title: "Build your first app", text: "A model, routes, auth and tests." },
  { href: "/docs/ui/choosing-a-ui/", title: "Choose a UI", text: "Navree UI in Rust, or Next.js or Nuxt." },
  { href: "/docs/patterns/overview/", title: "Choose an architecture", text: "Monolith, services, multi-tenant and more." },
  { href: "/docs/ai/introduction/", title: "Set up your agent", text: "Skills, Navigator and docs for models." },
  { href: "/docs/getting-started/bring-your-product/", title: "Bring what you have", text: "An old product or a design, rebuilt in Navree." },
];

export default function DocsIndex() {
  const sections = sectionIndexes();
  const first = sections[0]?.pages[0];
  return (
    <div className="max-w-[60rem] py-9 lg:py-12">
      <h1 className="text-[2.25rem] leading-[1.12] font-semibold tracking-[-0.03em] text-fg sm:text-[2.625rem]">Documentation</h1>
      <p className="mt-4 max-w-[40rem] text-lg leading-relaxed text-fg-2">
        Everything {site.name} does, from installing the CLI to queues, screens, monitoring and testing. New here? Start with
        the{" "}
        {first ? (
          <Link href={first.href} prefetch={false} className="link">
            {first.title.toLowerCase()}
          </Link>
        ) : (
          "introduction"
        )}
        , then build{" "}
        <Link href="/docs/getting-started/your-first-app/" prefetch={false} className="link">
          your first app
        </Link>
        .
      </p>

      <nav aria-label="Start here" className="mt-10">
        <ol className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {startHere.map((step, i) => (
            <li key={step.href}>
              <Link
                href={step.href}
                prefetch={false}
                className="group flex h-full flex-col rounded-2xl border border-line bg-ink-1 p-4 shadow-bezel transition-colors duration-200 hover:border-line-2 hover:bg-ink-2"
              >
                <span className="flex items-center justify-between font-mono text-xs text-fg-3 numeric">
                  {String(i + 1).padStart(2, "0")}
                  <ArrowRightIcon
                    aria-hidden
                    className="size-4 transition-[transform,color] duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:text-accent"
                  />
                </span>
                <span className="mt-3 font-medium text-fg">{step.title}</span>
                <span className="mt-1 text-sm leading-relaxed text-pretty text-fg-3">
                  <InlineMarkdown text={step.text} />
                </span>
              </Link>
            </li>
          ))}
        </ol>
      </nav>

      {/* One card for each section: what it covers, its first pages and a link to the rest. None of these links prefetches, so the index doesn't fetch every page as it scrolls past. */}
      <div className="mt-14 grid gap-4 md:grid-cols-2">
        {sections.map((section) => {
          const shown = section.pages.slice(0, PAGES_PER_CARD);
          return (
            <section
              key={section.slug}
              aria-labelledby={`section-${section.slug}`}
              className="flex flex-col rounded-2xl border border-line bg-ink-1 p-5 shadow-bezel"
            >
              <h2 id={`section-${section.slug}`} className="text-lg font-semibold text-fg">
                <Link
                  href={section.href}
                  prefetch={false}
                  className="group/section flex items-center justify-between gap-3 transition-colors hover:text-accent"
                >
                  {section.title}
                  <ArrowRightIcon
                    aria-hidden
                    className="size-4 shrink-0 text-fg-3 transition-[transform,color] duration-300 ease-(--ease-out-quint) group-hover/section:translate-x-0.5 group-hover/section:text-accent"
                  />
                </Link>
              </h2>
              {/* On phones the card lists titles only; the section's page says the rest. */}
              <p className="mt-1.5 text-sm leading-relaxed text-pretty text-fg-3 max-sm:hidden">
                <InlineMarkdown text={section.description} />
              </p>
              <ul className="mt-3 border-t border-line pt-2">
                {shown.map((page) => (
                  <li key={page.slug}>
                    <Link
                      href={page.href}
                      prefetch={false}
                      className="-mx-2 block rounded-md px-2 py-1.5 text-sm text-fg-2 transition-colors hover:bg-ink-2 hover:text-accent"
                    >
                      {page.title}
                    </Link>
                  </li>
                ))}
              </ul>
              {section.pages.length > shown.length ? (
                <Link href={section.href} prefetch={false} className="link mt-auto pt-3 text-sm">
                  All {section.pages.length} pages
                </Link>
              ) : null}
            </section>
          );
        })}
      </div>
      <PageSchema path="/docs/" metadata={metadata} />
    </div>
  );
}
