import type { Metadata } from "next";
import Link from "next/link";

import { InlineMarkdown } from "@/components/inline-markdown";
import { navigation } from "@/lib/docs";
import { site } from "@site";

export const metadata: Metadata = {
  title: "Documentation",
  description: `Everything ${site.name} does, from installation to queues, real time and testing.`,
  alternates: { canonical: "/docs/" },
};

export default function DocsIndex() {
  const nav = navigation();
  const first = nav.sections[0]?.pages[0];
  return (
    <div className="max-w-[60rem] py-9 lg:py-12">
      <h1 className="text-[2.25rem] leading-[1.12] font-semibold tracking-[-0.03em] text-fg sm:text-[2.625rem]">Documentation</h1>
      <p className="mt-4 max-w-[40rem] text-lg leading-relaxed text-fg-2">
        Everything {site.name} does, from installing the CLI to queues, real time and testing. New here? Start with the{" "}
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

      <div className="mt-12 grid gap-x-12 gap-y-12 md:grid-cols-2">
        {nav.sections.map((section) => (
          <section key={section.slug} aria-labelledby={`section-${section.slug}`}>
            <h2 id={`section-${section.slug}`} className="border-b border-line pb-3 text-lg font-semibold text-fg">
              {section.title}
            </h2>
            <ul className="mt-2">
              {section.pages.map((page) => (
                <li key={page.slug}>
                  <Link href={page.href} className="group -mx-3 block rounded-lg px-3 py-2.5 transition-colors hover:bg-ink-1">
                    <span className="font-medium text-fg group-hover:text-accent">{page.title}</span>
                    <span className="mt-0.5 block text-sm leading-relaxed text-fg-2">
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
