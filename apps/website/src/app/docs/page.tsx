import type { Metadata } from "next";
import Link from "next/link";

import { ArrowRightIcon } from "@/components/icons";
import { InlineMarkdown } from "@/components/inline-markdown";
import { navigation, sectionIndexes } from "@/lib/docs";
import { pageMetadata } from "@/lib/metadata";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: "Documentation",
  description: `Everything ${site.name} does, from installation to the backend, the screens, the tools and your coding agent.`,
  path: "/docs/",
});

/** The first steps, above the full index. */
const startHere = [
  { href: "/docs/getting-started/installation/", title: "Install the CLI", text: "Rust, the `navree` command and a new app." },
  { href: "/docs/getting-started/your-first-app/", title: "Build your first app", text: "A model, routes, auth and tests." },
  { href: "/docs/ui/choosing-a-ui/", title: "Choose a UI", text: "Navree UI in Rust, or Next.js or Nuxt." },
  { href: "/docs/patterns/overview/", title: "Choose an architecture", text: "Monolith, services, multi-tenant and more." },
  { href: "/docs/ai/introduction/", title: "Set up your agent", text: "Skills, Navigator and docs for models." },
  { href: "/docs/getting-started/bring-your-product/", title: "Bring what you have", text: "An old product or a design, rebuilt in Navree." },
];

export default function DocsIndex() {
  const nav = navigation();
  const hrefs = new Map(sectionIndexes().map((section) => [section.slug, section.href]));
  const first = nav.sections[0]?.pages[0];
  return (
    <div className="max-w-[60rem] py-9 lg:py-12">
      <h1 className="text-[2.25rem] leading-[1.12] font-semibold tracking-[-0.03em] text-fg sm:text-[2.625rem]">Documentation</h1>
      <p className="mt-4 max-w-[40rem] text-lg leading-relaxed text-fg-2">
        Everything {site.name} does, from installing the CLI to queues, screens, monitoring and testing. New here? Start with
        the{" "}
        {first ? (
          <Link href={first.href} className="link">
            {first.title.toLowerCase()}
          </Link>
        ) : (
          "introduction"
        )}
        , then build{" "}
        <Link href="/docs/getting-started/your-first-app/" className="link">
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

      <div className="mt-14 grid gap-x-12 gap-y-10 sm:gap-y-12 md:grid-cols-2">
        {nav.sections.map((section) => (
          <section key={section.slug} aria-labelledby={`section-${section.slug}`}>
            <h2 id={`section-${section.slug}`} className="border-b border-line pb-3 text-lg font-semibold text-fg">
              <Link href={hrefs.get(section.slug) ?? "/docs/"} className="group/section flex items-center justify-between gap-3 transition-colors hover:text-accent">
                {section.title}
                <ArrowRightIcon
                  aria-hidden
                  className="size-4 shrink-0 text-fg-3 transition-[transform,color] duration-300 ease-(--ease-out-quint) group-hover/section:translate-x-0.5 group-hover/section:text-accent"
                />
              </Link>
            </h2>
            <ul className="mt-2">
              {section.pages.map((page) => (
                <li key={page.slug}>
                  <Link
                    href={page.href}
                    className="group -mx-3 block rounded-lg px-3 py-2 transition-colors hover:bg-ink-1 sm:py-2.5"
                  >
                    <span className="font-medium text-fg group-hover:text-accent">{page.title}</span>
                    {/* On phones the index lists titles only; the pages say the rest. */}
                    <span className="mt-0.5 block text-sm leading-relaxed text-fg-2 max-sm:hidden">
                      <InlineMarkdown text={page.description} />
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
