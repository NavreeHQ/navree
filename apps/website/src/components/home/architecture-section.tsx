import { ArrowRightIcon, CheckIcon } from "lucide-react";
import Link from "next/link";

import { DiagramFigure, diagrams } from "@/components/diagrams";
import { patterns } from "@/components/diagrams/patterns";
import { ArrowLink } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

/**
 * The homepage's architecture section: a segmented control over the six
 * shapes Navree builds, each with its diagram, a short explanation and a
 * link to /architecture/. On large screens every panel sits in the same
 * grid cell (inactive ones invisible), so switching never moves the page.
 */
export function ArchitectureSection() {
  return (
    <section aria-labelledby="home-architecture-heading" className="container-site section-y">
      <header className="grid gap-6 lg:grid-cols-12 lg:items-end lg:gap-12">
        <div className="lg:col-span-7">
          <p className="eyebrow">Architecture</p>
          <h2 id="home-architecture-heading" className="mt-5 text-display-lg font-semibold text-balance text-fg">
            One framework, every architecture.
          </h2>
        </div>
        <div className="lg:col-span-5 lg:pb-1">
          <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
            Every shape of backend is built from the same unit: the module. Start as one binary, then add workspaces, events
            or services when the product needs them, without rewriting a module.
          </p>
          <ArrowLink href="/architecture/" className="mt-5">
            Explore every architecture
          </ArrowLink>
        </div>
      </header>

      <Tabs defaultValue={patterns[0].id} className="mt-12 gap-8 lg:mt-14">
        <div className="-mx-4 overflow-x-auto px-4 [scrollbar-width:none] max-sm:[mask-image:linear-gradient(to_right,black_85%,transparent)] sm:mx-0 sm:px-0 [&::-webkit-scrollbar]:hidden">
          <TabsList aria-label="Architectures" className="h-11 group-data-[orientation=horizontal]/tabs:h-11">
            {patterns.map((p) => (
              <TabsTrigger key={p.id} value={p.id} className="px-4">
                {p.short}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>

        <div className="lg:grid">
          {patterns.map((p, i) => (
            <TabsContent
              key={p.id}
              value={p.id}
              forceMount
              className="max-lg:data-[state=inactive]:hidden lg:col-start-1 lg:row-start-1 lg:data-[state=inactive]:invisible lg:data-[state=inactive]:opacity-0 motion-safe:lg:transition-opacity motion-safe:lg:duration-300 motion-safe:lg:ease-(--ease-out-quint)"
            >
              <div className="grid h-full grid-cols-1 gap-8 lg:grid-cols-12 lg:gap-12">
                <div className="flex flex-col lg:col-span-4 lg:py-2">
                  <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">
                    <span className="tabular-nums">{String(i + 1).padStart(2, "0")}</span> · {p.name}
                  </p>
                  <h3 className="mt-3 font-display text-display-md font-semibold text-balance text-fg">{p.headline}</h3>
                  <p className="mt-4 text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-base">{p.teaser}</p>
                  <ul className="mt-6">
                    {p.how.slice(0, 3).map((item) => (
                      <li key={item} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
                        <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
                        <span className="sm:[&_code]:whitespace-nowrap">
                          <InlineMarkdown text={item} />
                        </span>
                      </li>
                    ))}
                  </ul>
                  <div className="pt-8">
                    <Link
                      href={p.docs}
                      className="group inline-flex h-10 items-center gap-2 rounded-full border border-line-2 pr-1.5 pl-4 text-[0.9375rem] font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
                    >
                      {p.name} guide
                      <span className="inline-flex size-7 items-center justify-center rounded-full bg-ink-3 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
                        <ArrowRightIcon aria-hidden className="size-3.5" />
                      </span>
                    </Link>
                  </div>
                </div>
                <div className="min-w-0 lg:col-span-8">
                  <DiagramFigure spec={diagrams[p.id]} name={p.id} fill maxScale={1.2} />
                </div>
              </div>
            </TabsContent>
          ))}
        </div>
      </Tabs>
    </section>
  );
}
