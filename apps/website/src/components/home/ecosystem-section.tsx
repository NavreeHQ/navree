import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { ProductMark } from "@/components/ecosystem/marks";
import { FleetMini, HarborMini, HelmMini, LighthouseMini, LogbookMini, NavigatorMini } from "@/components/ecosystem/minis";
import { Marquee } from "@/components/magicui/marquee";
import { ecosystem, entry, type EcosystemEntry } from "@/content/ecosystem";
import { cn } from "@/lib/utils";
import { site } from "@site";

/** A tool in the bento: its mark, name and line, and a small still of its UI. Links to its block on /ecosystem/. */
function Tile({
  slug,
  className,
  fill = false,
  phone = true,
  children,
}: {
  slug: string;
  className?: string;
  /** Let the visual take the tile's remaining height (the tall tile). */
  fill?: boolean;
  /** Show the visual on phones too. */
  phone?: boolean;
  children: ReactNode;
}) {
  const e = entry(slug);
  return (
    <Link
      href={`/ecosystem/#${slug}`}
      className={cn(
        "group relative flex min-w-0 flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-300 hover:border-line-2 focus-visible:outline-offset-2",
        className,
      )}
    >
      <div className="flex items-start gap-3.5 p-5 sm:p-6">
        <ProductMark slug={slug} size="sm" className="mt-0.5" />
        <div className="min-w-0 flex-1">
          <h3 className="flex items-center gap-2 font-sans text-[1.0625rem] leading-snug font-semibold tracking-[-0.01em] text-fg">
            {e.name}
            <ArrowRightIcon
              aria-hidden
              className="size-3.5 text-fg-3 opacity-0 transition-[opacity,translate] duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:opacity-100 group-focus-visible:opacity-100"
            />
          </h3>
          <p className="mt-1 text-[0.9375rem] leading-snug text-pretty text-fg-2">{e.tagline}.</p>
        </div>
        {e.mount ? (
          <span className="hidden shrink-0 rounded-md border border-line px-1.5 font-mono text-xs leading-5 text-fg-3 sm:inline">{e.mount}</span>
        ) : null}
      </div>
      {/* A tile that fills its height lets its visual take the rest of the tile, absolutely, so it never sets the row's height. */}
      <div aria-hidden className={cn("min-w-0 px-5 pb-5 sm:px-6 sm:pb-6", fill && "relative flex-1 lg:min-h-[19.5rem] lg:p-0", !phone && "hidden sm:block")}>
        {fill ? <div className="lg:absolute lg:inset-x-6 lg:top-0 lg:bottom-6">{children}</div> : children}
      </div>
    </Link>
  );
}

function LibraryPill({ e }: { e: EcosystemEntry }) {
  return (
    <span className="inline-flex h-11 shrink-0 items-center gap-3 rounded-full border border-line bg-ink-1 px-5 whitespace-nowrap shadow-bezel">
      <span className="text-[0.9375rem] font-medium text-fg">{e.name}</span>
      <span className="font-mono text-xs text-fg-3">{e.crate}</span>
    </span>
  );
}

/**
 * The homepage's look at the ecosystem: a bento of the six tools with small
 * stills of their UIs (the Lighthouse feed is live), a strip of the
 * libraries, and the way to /ecosystem/.
 */
export function EcosystemSection() {
  const libraries = ecosystem.filter((e) => e.kind !== "tool");
  const half = Math.ceil(libraries.length / 2);
  const rows = [libraries.slice(0, half), libraries.slice(half)];

  return (
    <section aria-labelledby="home-ecosystem-heading" className="relative isolate overflow-hidden">
      <div className="container-site section-y">
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-12 lg:items-end lg:gap-12">
          <div className="lg:col-span-7">
            <p className="eyebrow">Ecosystem</p>
            <h2 id="home-ecosystem-heading" className="mt-5 text-display-lg font-semibold text-balance text-fg">
              The tools to run it, first-party.
            </h2>
          </div>
          <div className="lg:col-span-5 lg:pb-1">
            <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              Monitoring, an admin panel, queue supervision, a debugging timeline, deployment and an MCP server for your agent
              ship with {site.name}, next to libraries for search, billing, OAuth, AI and more. Open source, self-hosted, no paid
              add-ons.
            </p>
          </div>
        </div>

        <div className="mt-12 grid grid-cols-1 gap-3 md:grid-cols-2 lg:mt-16 lg:grid-cols-6">
          <Tile slug="lighthouse" fill className="md:col-span-2 lg:col-span-3 lg:row-span-2">
            <LighthouseMini />
          </Tile>
          <Tile slug="fleet" className="lg:col-span-3">
            <FleetMini />
          </Tile>
          {/* On phones the deploy is shown once, by the Ship section that follows. */}
          <Tile slug="harbor" phone={false} className="lg:col-span-3">
            <HarborMini />
          </Tile>
          <Tile slug="helm" phone={false} className="lg:col-span-2">
            <HelmMini />
          </Tile>
          <Tile slug="logbook" phone={false} className="lg:col-span-2">
            <LogbookMini />
          </Tile>
          <Tile slug="navigator" phone={false} className="md:col-span-2 lg:col-span-2">
            <NavigatorMini />
          </Tile>
        </div>

        <div className="relative mt-3 flex flex-col gap-3 overflow-hidden rounded-2xl border border-line bg-ink-0 py-5">
          <span className="sr-only">Libraries: </span>
          {rows.map((row, i) => (
            <Marquee key={i} reverse={i === 1} className={cn("[--gap:0.75rem]", i === 0 ? "[--duration:60s]" : "[--duration:70s]")}>
              {row.map((e) => (
                <LibraryPill key={e.slug} e={e} />
              ))}
            </Marquee>
          ))}
          <div aria-hidden className="pointer-events-none absolute inset-y-0 left-0 w-24 bg-linear-to-r from-ink-0 to-transparent sm:w-40" />
          <div aria-hidden className="pointer-events-none absolute inset-y-0 right-0 w-24 bg-linear-to-l from-ink-0 to-transparent sm:w-40" />
        </div>

        <div className="mt-10 flex flex-wrap items-center gap-x-6 gap-y-4">
          <Link
            href="/ecosystem/"
            className="group inline-flex h-12 items-center gap-2.5 rounded-full border border-line-2 pr-1.5 pl-6 text-base font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
          >
            Explore the ecosystem
            <span className="inline-flex size-9 items-center justify-center rounded-full bg-ink-3 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
              <ArrowRightIcon aria-hidden className="size-4" />
            </span>
          </Link>
          <p className="text-sm text-fg-3">
            {ecosystem.length} first-party packages and tools · {site.license}
          </p>
        </div>
      </div>
    </section>
  );
}
