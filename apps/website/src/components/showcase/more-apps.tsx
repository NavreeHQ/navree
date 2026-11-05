import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import { appCards } from "@/app/ui/showcases";
import { SectionHeading } from "@/components/home/parts";

/**
 * The foot of an example app's page: the other apps, one line each, so a
 * reader who has seen one can reach the rest. Read from `showcases.ts`, so an
 * app added there appears on every page.
 */
export function MoreApps({ current }: { current: string }) {
  const others = appCards.filter((card) => card.id !== current);
  return (
    <section aria-labelledby="more-apps-heading" className="cv-auto border-t border-line">
      <div className="container-site section-y">
        <SectionHeading id="more-apps-heading" eyebrow="Keep looking" title="More example apps." />
        <ul className="mt-10 divide-y divide-line border-y border-line">
          {others.map((card) => (
            <li key={card.id}>
              <Link
                href={`/showcase/${card.slug}/`}
                className="group flex flex-col gap-1.5 py-5 transition-colors sm:flex-row sm:items-center sm:justify-between sm:gap-8"
              >
                <span className="min-w-0">
                  <span className="block font-display text-display-sm font-semibold text-fg transition-colors group-hover:text-accent">
                    {card.kind}
                  </span>
                  <span className="mt-0.5 block font-mono text-xs text-fg-3">
                    {card.name} · {card.ui}
                  </span>
                </span>
                <span className="flex items-center gap-3 text-[0.9375rem] leading-snug text-pretty text-fg-2 sm:max-w-md sm:justify-end sm:text-right">
                  {card.hardest}
                  <ArrowRightIcon
                    aria-hidden
                    className="size-4 shrink-0 text-fg-1 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:text-accent"
                  />
                </span>
              </Link>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
