import { ArrowRightIcon, CheckIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

import { CopyButton } from "@/components/mdx/copy-button";
import { PlannedBadge } from "@/components/site/planned-badge";
import type { EcosystemEntry } from "@/content/ecosystem";
import { cn } from "@/lib/utils";
import { site } from "@site";

import { ProductMark } from "./marks";

/** The words of a tool block: mark, name, what it is, what it does and where to read more. */
function ToolCopy({ entry, wide = false }: { entry: EcosystemEntry; wide?: boolean }) {
  const planned = site.showPlannedBadges && entry.status === "planned";
  const header = (
    <div>
      <div className="flex items-center gap-3">
        <ProductMark slug={entry.slug} size="lg" />
        {entry.mount ? <span className="rounded-md border border-line bg-ink-1 px-2 py-0.5 font-mono text-xs text-fg-2">{entry.mount}</span> : null}
        {planned ? <PlannedBadge /> : null}
      </div>
      <h3 id={`${entry.slug}-title`} className="mt-6 font-display text-display-md font-semibold text-balance text-fg">
        {entry.name}
      </h3>
      <p className="mt-2 text-lg leading-snug text-pretty text-fg-1">{entry.tagline}.</p>
    </div>
  );

  const body = (
    <div>
      <p className="max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-base">{entry.summary}</p>
      <ul className={cn("mt-6 grid gap-x-8", wide ? "sm:grid-cols-2" : "")}>
        {entry.features.map((feature) => (
          <li key={feature} className="flex items-start gap-3 border-t border-line py-2.5 text-[0.9375rem] text-fg-1">
            <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
            {feature}
          </li>
        ))}
      </ul>
    </div>
  );

  const actions = (
    <div className="flex flex-wrap items-center gap-3">
        <Link
          href={entry.href}
          className="group inline-flex h-10 items-center gap-2 rounded-full border border-line-2 pr-1.5 pl-4 text-[0.9375rem] font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
        >
          Read the {entry.name} docs
          <span className="inline-flex size-7 items-center justify-center rounded-full bg-ink-3 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
            <ArrowRightIcon aria-hidden className="size-3.5" />
          </span>
        </Link>
        <span className="inline-flex h-10 max-w-full min-w-0 items-center gap-1 rounded-full border border-line bg-ink-1 pr-1 pl-4">
          <code className="min-w-0 truncate font-mono text-[0.8125rem] text-fg-1">{entry.command}</code>
          <CopyButton text={entry.command} label={`Copy ${entry.command}`} />
        </span>
    </div>
  );

  if (wide) {
    return (
      <div className="grid grid-cols-1 gap-8 lg:grid-cols-12 lg:gap-12">
        <div className="flex min-w-0 flex-col gap-8 lg:col-span-5">
          {header}
          <div className="hidden lg:block">{actions}</div>
        </div>
        <div className="min-w-0 lg:col-span-7 lg:pt-[4.5rem]">
          {body}
          <div className="mt-6 lg:hidden">{actions}</div>
        </div>
      </div>
    );
  }
  return (
    <div className="flex min-w-0 flex-col gap-6">
      {header}
      {body}
      {actions}
    </div>
  );
}

/**
 * A tool's feature block: its words and a working mock of its UI.
 * `wide` puts the mock full width under the words; otherwise words and mock
 * sit side by side, mock on the right unless `reverse`.
 */
export function ToolBlock({
  entry,
  mock,
  layout = "split",
  reverse = false,
}: {
  entry: EcosystemEntry;
  mock: ReactNode;
  layout?: "split" | "wide";
  reverse?: boolean;
}) {
  return (
    <article id={entry.slug} aria-labelledby={`${entry.slug}-title`} className="scroll-mt-[calc(var(--header-h)+5rem)]">
      {layout === "wide" ? (
        <>
          <ToolCopy entry={entry} wide />
          <div className="mt-10 lg:mt-14">{mock}</div>
        </>
      ) : (
        <div className="grid grid-cols-1 items-start gap-10 lg:grid-cols-12 lg:gap-12 xl:gap-16">
          <div className={cn("min-w-0 lg:sticky lg:top-[calc(var(--header-h)+5.5rem)] lg:col-span-5 xl:col-span-4", reverse && "lg:order-2")}>
            <ToolCopy entry={entry} />
          </div>
          <div className={cn("min-w-0 lg:col-span-7 xl:col-span-8", reverse && "lg:order-1")}>{mock}</div>
        </div>
      )}
    </article>
  );
}
