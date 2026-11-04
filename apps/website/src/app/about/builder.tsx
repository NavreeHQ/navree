import { ArrowRightIcon } from "lucide-react";

import { HomeSection, SectionHeading } from "@/components/home/parts";
import { GitHubIcon } from "@/components/icons";
import { career, facts, founder } from "@/content/about";

/** Who built Navree: the stack of fifteen years, the numbers of his work, his GitHub, and (once filled in) his career. */
export function Builder() {
  return (
    <HomeSection id="builder">
      <SectionHeading
        split
        id="builder"
        eyebrow="The builder"
        title="Fifteen years, six startups, one framework."
        lead={
          <p>
            {founder.short} has worked across the whole stack: Laravel and NestJS on the server, React and Next.js in the
            browser, and the cloud and DevOps that keep a product running.
          </p>
        }
      />

      <div className="mt-12 border-y border-line lg:mt-16">
        {/* The 1px gaps show the line color through: crisp dividers at every column count. */}
        <dl className="grid grid-cols-2 gap-px bg-line lg:grid-cols-4">
          {facts.map((fact, i) => (
            <div
              key={fact.label}
              className={`flex flex-col gap-1.5 bg-ink-0 py-7 pr-4 sm:gap-2 sm:py-9 lg:pr-6 ${i % 2 === 1 ? "pl-5 sm:pl-8" : "pl-0"} ${i === 0 ? "" : "lg:pl-8"}`}
            >
              <dt className="order-2 text-[0.9375rem] leading-snug">
                <span className="font-medium text-fg-1">{fact.label}</span>
                <span className="mt-1 block text-sm text-balance text-fg-3">{fact.text}</span>
              </dt>
              <dd className="order-1 font-display text-display-md font-semibold text-fg">{fact.value}</dd>
            </div>
          ))}
        </dl>
      </div>

      <div className="mt-12 grid items-start gap-x-12 gap-y-8 lg:mt-14 lg:grid-cols-12">
        <p className="max-w-2xl text-lg leading-relaxed text-pretty text-fg-2 lg:col-span-7">
          Navree is where that experience comes together, written in Rust. Each lesson of those years became a part of it,
          and the next section names them, one by one.
        </p>
        <a
          href={founder.github.url}
          className="group flex items-center gap-4 rounded-2xl border border-line bg-ink-1 p-5 transition-colors hover:border-line-2 lg:col-span-5"
        >
          <GitHubIcon aria-hidden className="size-6 shrink-0 text-fg" />
          <span className="min-w-0 flex-1">
            <span className="block font-mono text-[0.9375rem] text-fg">{founder.github.handle}</span>
            <span className="mt-0.5 block text-sm text-fg-3">On GitHub since {founder.github.since}</span>
          </span>
          <ArrowRightIcon
            aria-hidden
            className="size-4 text-fg-2 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:text-fg"
          />
        </a>
      </div>

      {career.length ? (
        <div className="mt-14 lg:mt-20">
          <h3 className="font-mono text-eyebrow text-fg-3 uppercase">Career</h3>
          <ol className="mt-6 border-t border-line">
            {career.map((entry) => (
              <li key={`${entry.years}-${entry.company}`} className="grid gap-x-12 gap-y-1 border-b border-line py-6 lg:grid-cols-12">
                <p className="font-mono text-sm text-fg-3 tabular-nums lg:col-span-3">{entry.years}</p>
                <div className="lg:col-span-9">
                  <p className="text-lg font-medium text-fg">
                    {entry.role}, <span className="text-fg-1">{entry.company}</span>
                  </p>
                  {entry.text ? <p className="mt-1 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{entry.text}</p> : null}
                </div>
              </li>
            ))}
          </ol>
        </div>
      ) : null}
    </HomeSection>
  );
}
