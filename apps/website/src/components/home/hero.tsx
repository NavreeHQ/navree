import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import { GridPattern } from "@/components/magicui/grid-pattern";
import { ecosystem } from "@/content/ecosystem";
import crates from "@/generated/crates.json";
import { site } from "@site";

import { HeroTerminal } from "./hero-terminal";
import { PrimaryCta, SecondaryCta } from "./parts";

type Crate = { name: string; layer: number; module: string | null };

const SUB_COPY =
  "Navree gives every backend the same clear structure: modules wired by dependency injection, API-first by default and AI-native from the first command. Auth, workspaces, an ORM, queues, search, real time, monitoring and deployment are already in the box, so you build the product, not the plumbing.";

/**
 * The headline in three lines from `xl` ("The complete / backend
 * framework / for Rust."), so no word is left alone on a line. Any other
 * headline falls back to one balanced block.
 */
function headlineLines(headline: string): string[] {
  const match = /^(\S+ \S+) (\S+ \S+) (.+)$/.exec(headline);
  return match ? match.slice(1) : [headline];
}

/** Navree's own facts, counted from the workspace and the catalogue. */
function facts() {
  const all = crates as Crate[];
  const layers = new Set(all.map((c) => c.layer)).size;
  const modules = all.filter((c) => c.module).length;
  return [
    { value: String(all.length), label: "crates", text: `In ${layers} layers, under one facade` },
    { value: String(modules), label: "core modules", text: "Config and HTTP to auth and workspaces" },
    { value: String(ecosystem.length), label: "packages and tools", text: "First-party, open source and free" },
    { value: "1", label: "binary", text: "Web, workers and scheduler" },
    { value: "MIT", label: "or Apache-2.0", text: "Open source, free and self-hosted" },
  ];
}

export function Hero() {
  return (
    <section aria-labelledby="hero-title" className="relative overflow-hidden">
      {/* Background: a hairline grid fading out from the top right, and one soft bloom behind the terminal. */}
      <div aria-hidden className="pointer-events-none absolute inset-0">
        <GridPattern
          width={48}
          height={48}
          squares={[
            [21, 2],
            [24, 4],
            [18, 5],
            [27, 1],
          ]}
          className="[mask-image:radial-gradient(ellipse_70%_75%_at_78%_8%,black_10%,transparent_75%)] max-xl:[mask-image:radial-gradient(ellipse_120%_55%_at_50%_0%,black,transparent_80%)]"
        />
        <div className="hero-bloom absolute top-[-14rem] right-[-12rem] h-[52rem] w-[60rem] max-xl:top-auto max-xl:right-[-20rem] max-xl:bottom-[-10rem] max-xl:h-[36rem] max-xl:w-[44rem]" />
      </div>

      <div className="container-site relative grid items-center gap-x-10 gap-y-14 pt-14 pb-16 sm:pt-20 lg:pt-24 lg:pb-24 xl:grid-cols-[minmax(0,1fr)_27rem] xl:pt-28 min-[90rem]:grid-cols-[minmax(0,1fr)_33rem]">
        <div className="min-w-0">
          <Link
            href="/docs/prologue/release-notes/"
            className="group inline-flex max-w-full items-center gap-2.5 rounded-full border border-line-2 bg-ink-1/80 py-1 pr-3.5 pl-1 text-sm text-fg-1 shadow-bezel transition-colors hover:border-fg-3"
          >
            <span className="rounded-full border border-accent/25 bg-accent/10 px-2.5 py-0.5 font-mono text-xs font-medium text-accent">
              0.2
            </span>
            <span className="truncate sm:hidden">Read the release notes</span>
            <span className="truncate max-sm:hidden">Navree&apos;s own data layer, tighter security and supply-chain checks</span>
            <ArrowRightIcon
              aria-hidden
              className="size-3.5 shrink-0 text-fg-3 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5 group-hover:text-fg"
            />
          </Link>

          <h1 id="hero-title" className="mt-8 text-display-2xl font-semibold text-balance text-fg sm:mt-10">
            {headlineLines(site.headline).map((line, i) => (
              <span key={line} className="xl:block">
                {i > 0 ? " " : null}
                {line}
              </span>
            ))}
          </h1>

          <p className="mt-7 max-w-[38rem] text-lg leading-relaxed text-pretty text-fg-2">{SUB_COPY}</p>

          <div className="mt-10 flex flex-wrap items-center gap-3">
            <PrimaryCta href={site.getStarted}>Get started</PrimaryCta>
            <SecondaryCta href="/docs/">Read the docs</SecondaryCta>
          </div>
        </div>

        <div className="relative min-w-0 max-w-[40rem] xl:max-w-none">
          <div className="bezel shadow-glow-lg">
            <HeroTerminal />
          </div>
        </div>
      </div>

      <div className="relative border-t border-line bg-ink-0">
        <div className="container-site">
          {/* The 1px gaps show the line color through: crisp dividers at every column count. */}
          <dl className="grid grid-cols-2 gap-px bg-line lg:grid-cols-5">
            {facts().map((fact, i, all) => (
              <div
                key={fact.label}
                className={`flex flex-col gap-1.5 bg-ink-0 py-6 pr-4 sm:gap-2 sm:py-9 lg:pr-6 ${i % 2 === 1 ? "pl-5 sm:pl-8" : "pl-0"} ${
                  i === 0 ? "" : "lg:pl-8"
                } ${i === all.length - 1 ? "max-lg:col-span-2" : ""}`}
              >
                <dt className="order-2 text-[0.9375rem] leading-snug">
                  <span className="font-medium text-fg-1">{fact.label}</span>
                  <span className="mt-1 block text-sm text-balance text-fg-3 max-sm:hidden">{fact.text}</span>
                </dt>
                <dd className="order-1 font-display text-display-md font-semibold text-fg">{fact.value}</dd>
              </div>
            ))}
          </dl>
        </div>
      </div>
    </section>
  );
}
