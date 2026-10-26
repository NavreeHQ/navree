import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import { InlineMarkdown } from "@/components/inline-markdown";
import { BorderBeam } from "@/components/magicui/border-beam";
import { navigation } from "@/lib/docs";

import { PrimaryCta } from "./parts";

/** The end of the page: every page of the AI docs, from the docs manifest. */
export function AiClosing() {
  const section = navigation().sections.find((s) => s.slug === "ai");
  const pages = section?.pages ?? [];

  return (
    <section aria-labelledby="ai-docs-title" className="border-t border-line">
      <div className="container-site section-y">
        <div className="relative overflow-hidden rounded-[1.5rem] border border-line bg-ink-1 shadow-bezel">
          <BorderBeam size={140} duration={14} />
          <div className="grid grid-cols-1 gap-10 p-6 sm:p-10 lg:grid-cols-12 lg:gap-12 lg:p-14">
            <div className="lg:col-span-4">
              <p className="eyebrow">The AI docs</p>
              <h2 id="ai-docs-title" className="mt-5 text-display-lg font-semibold text-balance text-fg">
                Start with your agent.
              </h2>
              <p className="mt-5 max-w-md text-lg leading-relaxed text-pretty text-fg-2">
                Four commands set up skills, Navigator and the docs. The AI SDK is one feature flag away.
              </p>
              <PrimaryCta href="/docs/ai/introduction/#set-up-your-agent" className="mt-8">
                Set up your agent
              </PrimaryCta>
            </div>
            <ul className="grid grid-cols-1 gap-px self-start overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-2 lg:col-span-8">
              {pages.map((page) => (
                <li key={page.slug} className="bg-ink-1">
                  <Link
                    href={page.href}
                    className="group flex h-full flex-col gap-1.5 p-5 transition-colors duration-150 hover:bg-ink-2"
                  >
                    <span className="flex items-center justify-between gap-3 font-medium text-fg">
                      {page.title}
                      <ArrowRightIcon
                        aria-hidden
                        className="size-4 shrink-0 text-fg-3 transition-[transform,color] duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:text-accent"
                      />
                    </span>
                    <span className="line-clamp-2 text-sm leading-relaxed text-fg-3">
                      <InlineMarkdown text={page.description} />
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
    </section>
  );
}
