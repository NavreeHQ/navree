import { ArrowRightIcon, CheckIcon } from "lucide-react";
import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";
import type { ReactNode } from "react";

import { BuiltIn } from "@/components/architecture/built-in";
import { rules } from "@/components/architecture/rules";
import { Screens } from "@/components/architecture/screens";
import { DiagramFigure, diagrams } from "@/components/diagrams";
import { patterns, type Pattern } from "@/components/diagrams/patterns";
import { GrowThumb, PatternThumb } from "@/components/diagrams/thumbs";
import { Faq } from "@/components/faq";
import { InlineMarkdown } from "@/components/inline-markdown";
import { CopyButton } from "@/components/mdx/copy-button";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { cn } from "@/lib/utils";

import { CodeExcerpt } from "./code-excerpt";
import { PatternNav } from "./pattern-nav";

export const metadata: Metadata = pageMetadata({
  title: "Architecture: from monolith to microservices",
  description:
    "Build a modular monolith, microservices, a multi-tenant SaaS, an event-driven system, an API-first backend or a backend for frontend with one framework.",
  path: "/architecture/",
});

/** The green call to action with its arrow in a nested circle. */
function PrimaryLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link
      href={href}
      className="group inline-flex h-12 items-center gap-2.5 rounded-full bg-accent pr-1.5 pl-6 text-base font-semibold text-accent-ink transition-[box-shadow,transform] duration-200 ease-(--ease-out-quint) hover:shadow-glow active:scale-[0.98]"
    >
      {children}
      <span className="inline-flex size-9 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
        <ArrowRightIcon aria-hidden className="size-4" />
      </span>
    </Link>
  );
}

function SecondaryLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link
      href={href}
      className="inline-flex h-12 items-center rounded-full border border-line-2 px-6 text-base font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2 active:scale-[0.98]"
    >
      {children}
    </Link>
  );
}

function GuideLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link
      href={href}
      className="group inline-flex h-10 items-center gap-2 rounded-full border border-line-2 pr-1.5 pl-4 text-[0.9375rem] font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
    >
      {children}
      <span className="inline-flex size-7 items-center justify-center rounded-full bg-ink-3 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
        <ArrowRightIcon aria-hidden className="size-3.5" />
      </span>
    </Link>
  );
}

const number = (i: number) => String(i + 1).padStart(2, "0");

/** The page's backdrop, from the generated image series (public/images/generated). */
const hero = { src: "/images/generated/hero-archipelago.webp", width: 3200, height: 1358 };

function Hero() {
  return (
    <section aria-labelledby="architecture-title" className="relative isolate overflow-hidden border-b border-line">
      {/*
        The backdrop: modular islands linked by green light, on the right
        from lg, faded into the page on its left and bottom edges so the
        words sit on empty black. Decorative.
      */}
      <div aria-hidden className="pointer-events-none absolute inset-y-0 right-0 -z-10 hidden w-[46%] lg:block xl:w-[64%]">
        <Image
          src={hero.src}
          width={hero.width}
          height={hero.height}
          alt=""
          unoptimized
          className="h-full w-full object-cover object-[70%_30%] opacity-90 [mask-image:linear-gradient(to_right,transparent,black_38%),linear-gradient(to_bottom,black_55%,transparent)] [mask-composite:intersect]"
        />
      </div>
      <div className="container-site pt-14 pb-16 sm:pt-20 lg:pt-28 lg:pb-24">
        <div className="max-w-xl lg:min-h-[23rem] xl:max-w-[36rem]">
          <p className="eyebrow">Architecture</p>
          <h1 id="architecture-title" className="mt-5 text-display-xl font-semibold text-balance text-fg">
            <span className="block">One framework,</span>
            <span className="block text-fg-2">every architecture.</span>
          </h1>
          <p className="mt-6 text-lg leading-relaxed text-pretty text-fg-2">
            Navree builds every shape of backend from the same unit: the module. Arrange modules one way and you have a
            modular monolith; another way, and you have services, a multi-tenant SaaS or an event-driven system. Start as one
            binary and change shape later without rewriting a module.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <PrimaryLink href="/docs/patterns/overview/">Choose an architecture</PrimaryLink>
            <SecondaryLink href="/docs/architecture/modules/">How modules work</SecondaryLink>
          </div>
        </div>

        <nav aria-label="The architectures" className="mt-14 lg:mt-20">
          <ol className="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-6">
            {patterns.map((p, i) => (
              <li key={p.id}>
                <a
                  href={`#${p.id}`}
                  className="group flex h-full flex-col rounded-2xl border border-line bg-ink-1 p-1.5 shadow-bezel transition-colors duration-200 hover:border-line-2"
                >
                  <span className="block rounded-[0.7rem] border border-line bg-ink-0/70 bg-[radial-gradient(circle,var(--color-line)_1px,transparent_1.25px)] bg-size-[12px_12px] px-3 py-3 transition-colors duration-300 group-hover:bg-ink-0/40">
                    <PatternThumb name={p.id} />
                  </span>
                  <span className="flex items-baseline gap-2 px-2.5 pt-3 pb-2">
                    <span className="font-mono text-xs text-fg-3 tabular-nums">{number(i)}</span>
                    <span className="text-[0.9375rem] font-medium text-fg">{p.name}</span>
                  </span>
                </a>
              </li>
            ))}
          </ol>
        </nav>
      </div>
    </section>
  );
}

function PatternSection({ pattern, index }: { pattern: Pattern; index: number }) {
  const reverse = index % 2 === 1;
  return (
    <section
      id={pattern.id}
      aria-labelledby={`${pattern.id}-heading`}
      className="cv-auto scroll-mt-[calc(var(--header-h)+3.25rem)] border-b border-line"
    >
      <div className="container-site section-y">
        {/*
          Three grid items, so a phone reads intro, picture, then details;
          from xl the picture spans both rows of its own column.
        */}
        <div className="grid grid-cols-1 items-start gap-y-10 xl:grid-cols-12 xl:grid-rows-[auto_1fr] xl:gap-x-16 xl:gap-y-0">
          <div className={cn("xl:col-span-5 xl:row-start-1", reverse ? "xl:col-start-8" : "xl:col-start-1")}>
            <p className="eyebrow">
              <span className="text-fg-3 tabular-nums">{number(index)}</span> {pattern.name}
            </p>
            <h2 id={`${pattern.id}-heading`} className="mt-5 text-display-lg font-semibold text-balance text-fg">
              {pattern.headline}
            </h2>
            <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">
              <InlineMarkdown text={pattern.what} />
            </p>
          </div>

          <div
            className={cn(
              "min-w-0 space-y-4 xl:col-span-7 xl:row-span-2 xl:row-start-1",
              reverse ? "xl:col-start-1" : "xl:col-start-6",
            )}
          >
            <DiagramFigure spec={diagrams[pattern.id]} name={pattern.id} />
            <CodeExcerpt file={pattern.code.file} lang={pattern.code.lang} source={pattern.code.source} />
          </div>

          <div className={cn("xl:col-span-5 xl:row-start-2 xl:pt-10", reverse ? "xl:col-start-8" : "xl:col-start-1")}>
            <div className="grid max-w-2xl gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-1">
              <div>
                <h3 className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">Choose it when</h3>
                <ul className="mt-3">
                  {pattern.choose.map((item) => (
                    <li key={item} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
                      <span aria-hidden className="mt-[0.5rem] size-1 shrink-0 rounded-full bg-fg-3" />
                      {item}
                    </li>
                  ))}
                </ul>
              </div>
              <div>
                <h3 className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">How Navree supports it</h3>
                <ul className="mt-3">
                  {pattern.how.map((item) => (
                    <li key={item} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
                      <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
                      <span className="sm:[&_code]:whitespace-nowrap">
                        <InlineMarkdown text={item} />
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            </div>

            <div className="mt-8">
              <GuideLink href={pattern.docs}>{pattern.name} guide</GuideLink>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

const growth = [
  {
    stage: 1 as const,
    title: "Start as a modular monolith",
    text: "`navree new` writes one binary, and `make:module` adds a module per feature. Calls between features are function calls, and one transaction covers a request.",
    file: "Terminal",
    lang: "sh" as const,
    source: `navree new crm
navree make:module companies --crud
navree make:module deals --crud`,
  },
  {
    stage: 2 as const,
    title: "Add workspaces",
    text: "Add `WorkspaceModule`, give tenant tables a `workspace_id` and take `CurrentWorkspace` where a handler needs it. Routes, services and access rules stay as they are.",
    file: "src/app.rs",
    lang: "rust" as const,
    source: `.module(AuthModule::from_config())
.module(WorkspaceModule::from_config()) // [!code ++]
.module(CompaniesModule)
.module(DealsModule)`,
  },
  {
    stage: 3 as const,
    title: "Move a module into a service",
    text: "When deals need their own deploys, `DealsModule` moves into its own binary with `MicroModule`. Its service, rules and tests come along; callers send a message instead of calling a method.",
    file: "src/modules/reports/service.rs",
    lang: "rust" as const,
    source: `self.deals.show(&uow, id).await? // [!code --]
self.client // [!code ++]
    .send("deals.get", &GetDeal { id }) // [!code ++]
    .await? // [!code ++]`,
  },
];

function Grow() {
  return (
    <section id="grow" aria-labelledby="grow-heading" className="cv-auto scroll-mt-[calc(var(--header-h)+3.25rem)] border-b border-line">
      <div className="container-site section-y">
        <header className="grid gap-6 lg:grid-cols-12 lg:items-end lg:gap-12">
          <div className="lg:col-span-7">
            <p className="eyebrow">Grow without rewriting</p>
            <h2 id="grow-heading" className="mt-5 text-display-lg font-semibold text-balance text-fg">
              Start as one binary. Grow into the shape the product needs.
            </h2>
          </div>
          <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2 lg:col-span-5 lg:pb-1">
            A module keeps its providers, rules, migrations and tests whether it runs in the monolith, inside a workspace or
            in a service of its own. What changes is the wiring around it.
          </p>
        </header>

        <ol className="mt-14 grid grid-cols-1 gap-3 lg:mt-16 lg:grid-cols-3">
          {growth.map((step) => (
            <li key={step.stage} className="flex min-w-0 flex-col rounded-[1.25rem] border border-line bg-ink-1/60 p-1.5">
              <div className="rounded-[0.875rem] border border-line bg-ink-0/60 bg-[radial-gradient(circle,var(--color-line)_1px,transparent_1.25px)] bg-size-[12px_12px] px-6 py-5">
                <GrowThumb stage={step.stage} className="mx-auto max-w-[17rem]" />
              </div>
              <div className="px-4 pt-5 pb-6 sm:px-5">
                <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">Step {step.stage}</p>
                <h3 className="mt-2 font-display text-display-sm font-semibold text-fg">{step.title}</h3>
                <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
                  <InlineMarkdown text={step.text} />
                </p>
              </div>
              <CodeExcerpt
                file={step.file}
                lang={step.lang}
                source={step.source}
                className="mt-auto rounded-[0.875rem] [&_.line]:pr-4 [&_.line]:pl-5"
              />
            </li>
          ))}
        </ol>

        <dl className="mt-3 grid overflow-hidden rounded-2xl border border-line bg-ink-1/60 sm:grid-cols-2">
          <div className="p-6 sm:p-7">
            <dt className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">What changes</dt>
            <dd className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-1">
              The wiring: which binary a module runs in, its transport, the isolation of its tables, how callers reach it.
            </dd>
          </div>
          <div className="border-t border-line p-6 sm:border-t-0 sm:border-l sm:p-7">
            <dt className="font-mono text-xs tracking-[0.12em] text-accent uppercase">What stays</dt>
            <dd className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-1">
              Every line of module code: its services, routes and handlers, its access rules, its migrations and its tests.
            </dd>
          </div>
        </dl>
      </div>
    </section>
  );
}

const kits = [
  {
    command: "navree new cloudworks --kit saas",
    title: "Multi-tenant SaaS",
    text: "Workspaces, billing and an admin panel, with a Next.js BFF that keeps tokens in httpOnly cookies.",
  },
  {
    command: "navree new cloudworks --kit api",
    title: "API-first monolith",
    text: "Versioned routes, an OpenAPI 3.1 document and a module per feature, ready for any client.",
  },
  {
    command: "navree new deals --kit micro",
    title: "Microservice",
    text: "One module with a message handler, an event handler and NATS configured for development.",
  },
];

function Kits() {
  return (
    <section aria-labelledby="kits-heading" className="cv-auto">
      <div className="container-site section-y">
        <div className="max-w-3xl">
          <p className="eyebrow">Start</p>
          <h2 id="kits-heading" className="mt-5 text-display-lg font-semibold text-balance text-fg">
            Start from the shape you need.
          </h2>
          <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">
            Each starter kit is a working app in one of these shapes, built from the same modules as everything on this page.
          </p>
        </div>
        <ul className="mt-12 grid grid-cols-1 gap-3 lg:grid-cols-3">
          {kits.map((kit) => (
            <li key={kit.command} className="flex flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel">
              <h3 className="font-display text-display-sm font-semibold text-fg">{kit.title}</h3>
              <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{kit.text}</p>
              <div className="mt-auto pt-6">
                <div className="flex h-11 items-center gap-1 rounded-full border border-line bg-ink-0 pr-1 pl-4">
                  <code className="min-w-0 flex-1 truncate font-mono text-[0.8125rem] text-fg-1">
                    <span className="text-fg-3 select-none">$ </span>
                    {kit.command}
                  </code>
                  <CopyButton text={kit.command} label={`Copy ${kit.command}`} />
                </div>
              </div>
            </li>
          ))}
        </ul>
        <div className="mt-10 flex flex-wrap items-center gap-3">
          <PrimaryLink href="/docs/getting-started/starter-kits/">Explore the starter kits</PrimaryLink>
          <SecondaryLink href="/docs/patterns/overview/">Read the patterns guide</SecondaryLink>
        </div>
      </div>
    </section>
  );
}

export default function ArchitecturePage() {
  const nav = [
    ...patterns.map((p) => ({ id: p.id, label: p.name })),
    { id: "grow", label: "Grow without rewriting" },
    ...(rules().length ? [{ id: "built-in", label: "Built in" }] : []),
  ];
  return (
    <>
      <Hero />
      <PatternNav items={nav} />
      {patterns.map((p, i) => (
        <PatternSection key={p.id} pattern={p} index={i} />
      ))}
      <Screens />
      <Grow />
      <BuiltIn />
      <Kits />
      <Faq name="architecture" />
      <PageSchema path="/architecture/" metadata={metadata} />
    </>
  );
}
