import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { component, example, groups } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { cn } from "@/lib/utils";

import { GroupList } from "./gallery-group";

/** How many cards a group shows on a phone before "Show all". */
const SHOWN_ON_PHONES = 3;

/** A line per group of the catalogue. A group added later shows without one until it gets its line here. */
const blurbs: Record<string, string> = {
  actions: "What a click does: buttons, groups of them and toggles.",
  inputs: "Every field a form needs, with its label, help and error, and the form drawn from a struct.",
  display: "Cards, badges, avatars, items, text and the parts of a conversation.",
  feedback: "Alerts, empty states, progress, skeletons and spinners.",
  overlays: "Dialogs, sheets, drawers, popovers, tooltips and hover cards, on the browser's own elements.",
  layout: "Stacks, rows and grids on the 4 px grid, and the parts that fold, scroll and divide a page.",
  navigation: "The app shell, breadcrumbs and pagination.",
  data: "Tables, the server-side DataTable, stats and bars.",
};

/**
 * Every component in the UI catalogue (`content/ui/catalog.json`, written
 * from the toolkit by `cargo run -p ui-docs`), by group, each as its real
 * preview and a link to its page. A component that lands in the toolkit
 * shows here on the next build, with nothing to edit. On a phone each group
 * shows its first cards and a button for the rest.
 */
export function ComponentGallery() {
  const count = groups.reduce((n, g) => n + g.components.length, 0);
  return (
    <HomeSection id="components">
      <SectionHeading
        split
        id="components"
        eyebrow="Components"
        title={`${count} components, drawn by the toolkit.`}
        lead={
          <p>
            Every preview below is the real component, rendered by navree-ui from Rust with its own stylesheet. Open one for its
            variants, states, parts, API and keyboard notes.
          </p>
        }
        action={<ArrowLink href="/docs/components/overview/">All components in the docs</ArrowLink>}
      />

      <nav aria-label="Component groups" className="mt-10 lg:mt-12">
        <ul className="flex flex-wrap gap-2">
          {groups.map((g) => (
            <li key={g.id}>
              <a
                href={`#components-${g.id}`}
                className="inline-flex h-9 items-center gap-2 rounded-full border border-line-2 bg-ink-1 px-3.5 text-sm text-fg-1 transition-colors hover:border-fg-3 hover:text-fg"
              >
                {g.title}
                <span className="font-mono text-xs text-fg-3 numeric">{g.components.length}</span>
              </a>
            </li>
          ))}
        </ul>
      </nav>

      {/* Without JavaScript the groups' buttons can't open, so every card shows. */}
      <noscript>
        <style>{".gallery-more{display:block!important}.gallery-toggle{display:none!important}"}</style>
      </noscript>
      <div className="mt-10 space-y-14 lg:mt-12 lg:space-y-16">
        {groups.map((g) => (
          <section
            key={g.id}
            id={`components-${g.id}`}
            aria-labelledby={`components-${g.id}-title`}
            className="grid scroll-mt-[calc(var(--header-h)+1.5rem)] gap-6 border-t border-line pt-8 lg:grid-cols-12 lg:gap-12"
          >
            <div className="lg:col-span-3">
              <div className="lg:sticky lg:top-[calc(var(--header-h)+2rem)]">
                <p className="font-mono text-xs text-fg-3 numeric">{String(g.components.length).padStart(2, "0")}</p>
                <h3 id={`components-${g.id}-title`} className="mt-2 font-display text-display-sm font-semibold text-fg">
                  {g.title}
                </h3>
                {blurbs[g.id] ? <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{blurbs[g.id]}</p> : null}
              </div>
            </div>
            <GroupList id={`components-${g.id}-list`} count={g.components.length} shown={SHOWN_ON_PHONES} group={g.title}>
              {g.components.map((id, at) => {
                const c = component(id);
                const thumb = example(c.thumb);
                return (
                  <li
                    key={id}
                    className={cn("min-w-0", at >= SHOWN_ON_PHONES && "gallery-more max-sm:hidden max-sm:group-data-[open]/list:block")}
                  >
                    <Link
                      href={c.url}
                      prefetch={false}
                      className="group flex h-full flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-200 hover:border-line-2"
                    >
                      <div className="relative h-[8.5rem] overflow-hidden border-b border-line bg-ink-0 sm:h-[10rem]" inert>
                        <PreviewFrame
                          src={thumb.src}
                          title=""
                          height={thumb.height}
                          width={thumb.width}
                          fit={false}
                          thumb
                          className={thumb.width ? undefined : "h-full"}
                        />
                      </div>
                      <div className="flex flex-1 flex-col gap-1 px-4 py-3.5">
                        <span className="flex items-center justify-between gap-2 font-medium text-fg">
                          {c.name}
                          <ArrowRightIcon
                            aria-hidden
                            className="size-4 shrink-0 text-fg-3 transition-[transform,color] duration-300 group-hover:translate-x-0.5 group-hover:text-accent"
                          />
                        </span>
                        <span className="line-clamp-2 text-[0.875rem] leading-snug text-fg-3">
                          <InlineMarkdown text={c.summary} />
                        </span>
                      </div>
                    </Link>
                  </li>
                );
              })}
            </GroupList>
          </section>
        ))}
      </div>
    </HomeSection>
  );
}
