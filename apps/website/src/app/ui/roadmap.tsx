import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { cn } from "@/lib/utils";
import { uiRoadmap, type RoadmapStatus } from "@/lib/ui-page-data";

const columns: { status: RoadmapStatus; title: string; text: string; dot: string }[] = [
  { status: "Building", title: "Building now", text: "Under way.", dot: "bg-amber" },
  { status: "Next", title: "Next", text: "Starts as the modules being built ship.", dot: "bg-fg-2" },
  { status: "Later", title: "Later", text: "Follows, in this order.", dot: "bg-fg-3" },
];

/**
 * "Coming next": the roadmap, read from the table on the "Coming next" docs
 * page, so the two never disagree. Shipped modules are in the gallery above.
 */
export function UiRoadmap() {
  const items = uiRoadmap();
  return (
    <HomeSection id="coming-next">
      <SectionHeading
        split
        id="coming-next"
        eyebrow="Coming next"
        title="Where Navree UI goes next."
        lead={
          <p>
            Navree UI grows module by module, and each module ships only when it is finished. Until one lands, a Next.js or Nuxt
            front end beside the same API covers what isn&apos;t here yet.
          </p>
        }
        action={<ArrowLink href="/docs/ui/coming-next/">The roadmap in the docs</ArrowLink>}
      />

      <div className="mt-12 grid grid-cols-1 gap-3 lg:mt-14 lg:grid-cols-3">
        {columns.map((col) => {
          const rows = items.filter((i) => i.status === col.status);
          return (
            <section
              key={col.status}
              aria-labelledby={`roadmap-${col.status.toLowerCase()}`}
              className="flex flex-col rounded-2xl border border-line bg-ink-1 shadow-bezel"
            >
              <header className="flex items-center justify-between gap-3 border-b border-line px-5 py-4">
                <h3 id={`roadmap-${col.status.toLowerCase()}`} className="flex items-center gap-2.5 font-semibold text-fg">
                  <span
                    aria-hidden
                    className={cn("size-2 rounded-full", col.dot, col.status === "Building" && "motion-safe:animate-pulse")}
                  />
                  {col.title}
                </h3>
                <span className="font-mono text-xs text-fg-3 numeric">{rows.length}</span>
              </header>
              <p className="px-5 pt-3 text-sm text-fg-3">{col.text}</p>
              <ol className="px-2 pt-2 pb-3">
                {rows.map((row) => (
                  <li key={row.order} className="flex gap-3 rounded-xl px-3 py-2.5">
                    <span className="mt-0.5 shrink-0 font-mono text-xs text-fg-3 numeric">{String(row.order).padStart(2, "0")}</span>
                    <span className="text-[0.9375rem] leading-snug text-pretty text-fg-1">
                      <InlineMarkdown text={row.text} />
                    </span>
                  </li>
                ))}
              </ol>
            </section>
          );
        })}
      </div>
    </HomeSection>
  );
}
