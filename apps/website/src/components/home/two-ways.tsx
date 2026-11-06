import { ArrowRightIcon, CheckIcon } from "lucide-react";
import Link from "next/link";

import { DiagramFigure, diagrams } from "@/components/diagrams";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

/** The two ways in, each an agent skill (docs: getting-started/bring-your-product, bring-your-design). */
const ways = [
  {
    id: "product",
    tab: "Bring your product",
    skill: "navree-reverse-engineering",
    title: "Rebuild a product you run today.",
    text: "Your agent reads the old product, its code and its data, and writes down what it does as specs, with evidence for every line. It plans and tests from the specs, rebuilds the product in Navree module by module, and proves that the new one matches the old.",
    points: [
      "A web app or a desktop back office, whatever it was built with",
      "The old data moves field by field, rehearsed until it reconciles",
      "Five stop points where you decide before it goes on",
    ],
    diagram: "reverse-engineering",
    href: "/docs/getting-started/bring-your-product/",
  },
  {
    id: "design",
    tab: "Bring your design",
    skill: "navree-design-to-ui",
    title: "Turn a design into working pages.",
    text: "Give your agent an HTML mock-up, a design-tool export or screenshots. It maps every element to Navree UI, writes a spec per page, builds the pages in Rust and checks them against the design, by eye and by tool.",
    points: [
      "Keep the design's look or translate it, screen by screen",
      "Every guess comes back to you as a question",
      "Three stop points where you decide before it goes on",
    ],
    diagram: "design-to-ui",
    href: "/docs/getting-started/bring-your-design/",
  },
] as const;

/**
 * The two ways in, as tabs: the two agent skills that bring an existing product
 * or design into Navree, each panel a skill, what it does and its phase
 * diagram. As in the architecture section, every panel sits in the same grid
 * cell on large screens, so switching never moves the page. The homepage's AI
 * section and `/ai/` both draw it, so the diagrams are the same on both. `headingLevel` is the level of each panel's title: 3 under a
 * section's heading, 4 under a block's.
 */
export function WaysTabs({ className, headingLevel = 3 }: { className?: string; headingLevel?: 3 | 4 }) {
  const Title = headingLevel === 3 ? "h3" : "h4";
  return (
    <Tabs defaultValue={ways[0].id} className={cn("gap-8", className)}>
      <TabsList aria-label="Ways in" className="h-11 w-fit group-data-[orientation=horizontal]/tabs:h-11">
        {ways.map((w) => (
          <TabsTrigger key={w.id} value={w.id} className="px-4">
            {w.tab}
          </TabsTrigger>
        ))}
      </TabsList>

      <div className="lg:grid">
        {ways.map((w) => (
          <TabsContent
            key={w.id}
            value={w.id}
            forceMount
            className="max-lg:data-[state=inactive]:hidden lg:col-start-1 lg:row-start-1 lg:data-[state=inactive]:invisible lg:data-[state=inactive]:opacity-0 motion-safe:lg:transition-opacity motion-safe:lg:duration-300 motion-safe:lg:ease-(--ease-out-quint)"
          >
            <div className="grid h-full grid-cols-1 gap-8 lg:grid-cols-12 lg:gap-12">
              <div className="flex flex-col lg:col-span-4 lg:py-2">
                <p className="font-mono text-xs text-fg-3">{w.skill}</p>
                <Title className="mt-3 font-display text-display-md font-semibold text-balance text-fg">{w.title}</Title>
                <p className="mt-4 text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-base">{w.text}</p>
                <ul className="mt-6">
                  {w.points.map((point) => (
                    <li key={point} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
                      <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
                      <span>{point}</span>
                    </li>
                  ))}
                </ul>
                <div className="pt-8">
                  <Link
                    href={w.href}
                    className="group inline-flex h-10 items-center gap-2 rounded-full border border-line-2 pr-1.5 pl-4 text-[0.9375rem] font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
                  >
                    {w.tab}
                    <span className="inline-flex size-7 items-center justify-center rounded-full bg-ink-3 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
                      <ArrowRightIcon aria-hidden className="size-3.5" />
                    </span>
                  </Link>
                </div>
              </div>
              <div className="min-w-0 lg:col-span-8">
                <DiagramFigure spec={diagrams[w.diagram]} name={w.diagram} fill maxScale={1.15} />
              </div>
            </div>
          </TabsContent>
        ))}
      </div>
    </Tabs>
  );
}
