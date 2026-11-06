import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import type { Showcase } from "@/app/ui/showcases";

import { ShowcaseImage } from "./showcase-image";

/**
 * An example app drawn with Navree UI, as a card: its picture, what it is, a
 * sentence on what it does, four facts and the way to its page. The whole card
 * is the link. One column on a phone; lying on its side from a tablet's width
 * to a laptop's; standing up three across on a wide screen. Used by
 * `/showcase/` and the homepage, so an app looks the same in both.
 * `headingLevel` is the level of its title: 3 under a section's heading, 4
 * under a block's.
 */
export function ExampleAppCard({ app, priority = false, headingLevel = 3 }: { app: Showcase; priority?: boolean; headingLevel?: 3 | 4 }) {
  const Title = headingLevel === 3 ? "h3" : "h4";
  return (
    <article className="group relative flex h-full flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-200 hover:border-line-2 md:flex-row xl:flex-col">
      <div className="aspect-[16/10] shrink-0 overflow-hidden border-b border-line bg-ink-0 md:aspect-auto md:w-[44%] md:border-r md:border-b-0 xl:aspect-[16/10] xl:w-auto xl:border-r-0 xl:border-b">
        <ShowcaseImage
          name={app.still}
          priority={priority}
          sizes="(min-width: 1440px) 450px, (min-width: 1280px) 31vw, (min-width: 768px) 44vw, calc(100vw - 34px)"
          className="size-full object-cover object-top-left"
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col p-6 sm:p-7">
        <p className="font-mono text-xs text-fg-3">{app.name}</p>
        <Title className="mt-2 font-display text-display-sm font-semibold text-fg transition-colors group-hover:text-accent">
          <Link href={`/showcase/${app.slug}/`} prefetch={false} className="after:absolute after:inset-0 after:content-['']">
            {app.kind}
          </Link>
        </Title>
        <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{app.summary}</p>
        <div className="mt-auto">
          <dl className="mt-6 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line">
            {app.facts.map((f) => (
              <div key={f.label} className="bg-ink-1 px-4 py-3">
                <dt className="text-xs text-fg-3">{f.label}</dt>
                <dd className="mt-1 text-[0.9375rem] font-medium text-fg-1">{f.value}</dd>
              </div>
            ))}
          </dl>
          <span
            aria-hidden
            className="inline-flex items-center gap-1.5 pt-6 text-[0.9375rem] font-medium text-fg-1 transition-colors group-hover:text-accent"
          >
            See the screens, the scale and how it is built
            <ArrowRightIcon className="size-4 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5" />
          </span>
        </div>
      </div>
    </article>
  );
}
