import { ArrowRightIcon } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { DockVisual, StarterKitsVisual, WorkspacesVisual } from "@/components/ecosystem/card-visuals";
import {
  AiVisual,
  BillingVisual,
  BrowserVisual,
  FlagsVisual,
  GraphqlVisual,
  GrpcVisual,
  I18nVisual,
  McpVisual,
  MicroVisual,
  OAuthVisual,
  RealtimeVisual,
  SearchVisual,
  SkillsVisual,
  SocialVisual,
  TestingVisual,
  WebhooksVisual,
} from "@/components/ecosystem/card-visuals-more";
import { CategoryNav } from "@/components/ecosystem/category-nav";
import { FleetMock } from "@/components/ecosystem/fleet-mock";
import { HarborMock } from "@/components/ecosystem/harbor-mock";
import { HelmMock } from "@/components/ecosystem/helm-mock";
import { LibraryCard } from "@/components/ecosystem/library-card";
import { LighthouseMock } from "@/components/ecosystem/lighthouse-mock";
import { LogbookMock } from "@/components/ecosystem/logbook-mock";
import { Manifest } from "@/components/ecosystem/manifest";
import { NavigatorMock } from "@/components/ecosystem/navigator-mock";
import { ToolBlock } from "@/components/ecosystem/tool-block";
import { FlickeringGrid } from "@/components/magicui/flickering-grid";
import { GridPattern } from "@/components/magicui/grid-pattern";
import { CopyButton } from "@/components/mdx/copy-button";
import { categories, ecosystem, entriesIn, entry, type EcosystemCategory } from "@/content/ecosystem";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { cn } from "@/lib/utils";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: "Ecosystem: first-party tools and packages",
  description:
    "Monitoring, an admin panel, queue supervision, debugging, deployment, search, billing, OAuth and AI: first-party, open source and self-hosted.",
  path: "/ecosystem/",
});

/** The tools that get a feature block with a working mock, by slug. */
const blocks: Record<string, { mock: ReactNode; layout?: "split" | "wide"; reverse?: boolean }> = {
  lighthouse: { mock: <LighthouseMock />, layout: "wide" },
  fleet: { mock: <FleetMock /> },
  harbor: { mock: <HarborMock />, reverse: true },
  helm: { mock: <HelmMock />, layout: "wide" },
  logbook: { mock: <LogbookMock /> },
  navigator: { mock: <NavigatorMock />, reverse: true },
};

/** Cards with a small picture, and cards that span two columns. */
const visuals: Record<string, ReactNode> = {
  workspaces: <WorkspacesVisual />,
  "starter-kits": <StarterKitsVisual />,
  dock: <DockVisual />,
  search: <SearchVisual />,
  billing: <BillingVisual />,
  flags: <FlagsVisual />,
  oauth: <OAuthVisual />,
  social: <SocialVisual />,
  webhooks: <WebhooksVisual />,
  i18n: <I18nVisual />,
  microservices: <MicroVisual />,
  grpc: <GrpcVisual />,
  graphql: <GraphqlVisual />,
  realtime: <RealtimeVisual />,
  skills: <SkillsVisual />,
  ai: <AiVisual />,
  mcp: <McpVisual />,
  testing: <TestingVisual />,
  "browser-testing": <BrowserVisual />,
};
const featured = new Set(["workspaces"]);

/** Card grid columns per category, by how many cards it has. */
const gridFor: Record<EcosystemCategory, string> = {
  operate: "md:grid-cols-2",
  build: "md:grid-cols-2",
  capabilities: "md:grid-cols-2 lg:grid-cols-3",
  connect: "md:grid-cols-2 xl:grid-cols-3",
  ai: "md:grid-cols-2 lg:grid-cols-3",
  test: "md:grid-cols-2",
};

const promises = [
  { figure: String(ecosystem.length), label: "first-party packages and tools", text: "Documented here, built to the framework's conventions." },
  { figure: "$0", label: "paid add-ons", text: "No tiers, no seats, no hosted plan. MIT OR Apache-2.0." },
  { figure: "1", label: "version number", text: "Tested, documented and released together." },
  { figure: "100%", label: "self-hosted", text: "Dashboards run in your app; the data stays on your servers." },
];

function Hero() {
  return (
    <section aria-labelledby="ecosystem-title" className="relative isolate overflow-hidden border-b border-line">
      <GridPattern
        width={48}
        height={48}
        squares={[
          [17, 2],
          [20, 4],
          [23, 1],
          [26, 6],
          [14, 7],
        ]}
        className="-z-10 [mask-image:radial-gradient(ellipse_70%_60%_at_70%_20%,black,transparent_75%)]"
      />
      <div
        aria-hidden
        className="pointer-events-none absolute -top-40 right-[-10%] -z-10 h-[34rem] w-[54rem] rounded-full bg-[radial-gradient(closest-side,rgb(61_255_143/0.10),transparent)]"
      />

      <div className="container-site pt-14 pb-16 sm:pt-20 lg:pt-24 lg:pb-24">
        <p className="eyebrow">Ecosystem</p>
        <h1 id="ecosystem-title" className="mt-5 text-display-xl font-semibold text-balance text-fg">
          <span className="block">Everything first&#8209;party.</span>
          <span className="block text-fg-2">Everything included.</span>
        </h1>

        <div className="mt-10 grid grid-cols-1 gap-12 lg:mt-12 lg:grid-cols-12 lg:gap-x-10 lg:gap-y-12 xl:gap-x-16">
          <div className="lg:col-span-5">
            <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              Monitoring, an admin panel, queue supervision, a debugging timeline, deployment, search, billing, OAuth, AI and more
              are part of {site.name}. They are open source, they run on your servers, and they are designed, documented and
              released with the framework.
            </p>
            <div className="mt-8 flex flex-wrap items-center gap-3">
              <Link
                href={site.getStarted}
                className="group inline-flex h-12 items-center gap-2.5 rounded-full bg-accent pr-1.5 pl-6 text-base font-semibold text-accent-ink transition-[box-shadow,transform] duration-200 ease-(--ease-out-quint) hover:shadow-glow active:scale-[0.98]"
              >
                Get started
                <span className="inline-flex size-9 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
                  <ArrowRightIcon aria-hidden className="size-4" />
                </span>
              </Link>
              <Link
                href="/docs/"
                className="inline-flex h-12 items-center rounded-full border border-line-2 px-6 text-base font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
              >
                Read the docs
              </Link>
            </div>
          </div>
          <div className="min-w-0 lg:col-span-7 lg:col-start-6 lg:row-span-2 lg:row-start-1">
            <Manifest className="shadow-glow-lg" />
          </div>
          <div className="lg:col-span-5 lg:col-start-1 lg:row-start-2">
            <dl className="grid grid-cols-2 border-t border-line">
              {promises.map((p, i) => (
                <div key={p.label} className={cn("py-5", i % 2 === 1 ? "border-l border-line pl-5" : "pr-5", i > 1 && "border-t border-line")}>
                  <dt>
                    <span className="block font-display text-display-md font-semibold text-fg tabular-nums">{p.figure}</span>
                    <span className="mt-1 block text-sm text-fg-1">{p.label}</span>
                  </dt>
                  <dd className="mt-1.5 text-sm leading-relaxed text-pretty text-fg-3">{p.text}</dd>
                </div>
              ))}
            </dl>
          </div>
        </div>
      </div>
    </section>
  );
}

function CategorySection({ index, id }: { index: number; id: EcosystemCategory }) {
  const info = categories.find((c) => c.id === id)!;
  const items = entriesIn(id);
  const tools = items.filter((e) => e.slug in blocks);
  const cards = items.filter((e) => !(e.slug in blocks));
  return (
    <section id={id} aria-labelledby={`${id}-heading`} className="cv-auto scroll-mt-[calc(var(--header-h)+3.25rem)] border-b border-line">
      <div className="container-site section-y">
        <header className="grid grid-cols-1 gap-6 lg:grid-cols-12 lg:items-end lg:gap-12">
          <div className="lg:col-span-7">
            <p className="eyebrow">
              <span className="text-fg-3 tabular-nums">{String(index + 1).padStart(2, "0")}</span> {info.title}
            </p>
            <h2 id={`${id}-heading`} className="mt-5 text-display-lg font-semibold text-balance text-fg">
              {info.heading ?? info.title}
            </h2>
          </div>
          <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2 lg:col-span-5 lg:pb-1">{info.lead}</p>
        </header>

        {tools.length ? (
          <div className="mt-16 space-y-24 lg:mt-20 lg:space-y-32">
            {tools.map((e) => {
              const b = blocks[e.slug]!;
              return <ToolBlock key={e.slug} entry={e} mock={b.mock} layout={b.layout} reverse={b.reverse} />;
            })}
          </div>
        ) : null}

        {cards.length ? (
          <div className={cn("grid grid-cols-1 gap-3", tools.length ? "mt-24 lg:mt-32" : "mt-14 lg:mt-16", gridFor[id])}>
            {cards.map((e) => (
              <LibraryCard key={e.slug} entry={e} className={featured.has(e.slug) ? "lg:col-span-2" : undefined}>
                {visuals[e.slug]}
              </LibraryCard>
            ))}
          </div>
        ) : null}
      </div>
    </section>
  );
}

function Closing() {
  const commands = [site.install.cli, "navree new acme --kit saas"];
  return (
    <section aria-labelledby="ecosystem-start" className="cv-auto relative isolate overflow-hidden">
      <FlickeringGrid
        className="absolute inset-0 -z-10 [mask-image:radial-gradient(ellipse_60%_70%_at_50%_100%,black,transparent_75%)]"
        squareSize={3}
        gridGap={7}
        maxOpacity={0.22}
        flickerChance={0.08}
      />
      <div className="container-site section-y flex flex-col items-center text-center">
        <p className="eyebrow">Start</p>
        <h2 id="ecosystem-start" className="mt-5 max-w-3xl text-display-lg font-semibold text-balance text-fg">
          Start with all of it.
        </h2>
        <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">
          The SaaS starter kit writes an app with sign-in, workspaces, billing, settings and an admin panel, wired to {site.name}.
          Lighthouse and Logbook come with every new app; every other tool is one module away.
        </p>
        <div className="mt-9 w-full max-w-2xl rounded-2xl border border-line bg-ink-1 text-left shadow-bezel" data-copy-root>
          <div className="flex items-start justify-between gap-3 py-3 pr-2 pl-4 sm:pl-5">
            <pre className="scroll-thin min-w-0 overflow-x-auto py-1 font-mono text-[0.8125rem] leading-7 text-fg sm:text-sm">
              <code>
                {commands.map((c) => (
                  <span key={c} className="block">
                    <span className="text-fg-3 select-none">$ </span>
                    {c}
                  </span>
                ))}
              </code>
            </pre>
            <CopyButton text={commands.join("\n")} label="Copy the commands" className="shrink-0" />
          </div>
        </div>
        <div className="mt-9 flex flex-wrap justify-center gap-3">
          <Link
            href={entry("starter-kits").href}
            className="group inline-flex h-12 items-center gap-2.5 rounded-full bg-accent pr-1.5 pl-6 text-base font-semibold text-accent-ink transition-[box-shadow,transform] duration-200 ease-(--ease-out-quint) hover:shadow-glow active:scale-[0.98]"
          >
            Explore the starter kits
            <span className="inline-flex size-9 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5">
              <ArrowRightIcon aria-hidden className="size-4" />
            </span>
          </Link>
          <Link
            href={site.getStarted}
            className="inline-flex h-12 items-center rounded-full border border-line-2 px-6 text-base font-medium text-fg transition-colors duration-200 hover:border-fg-3 hover:bg-ink-2"
          >
            Install {site.name}
          </Link>
        </div>
      </div>
    </section>
  );
}

export default function EcosystemPage() {
  const nav = categories.map((c) => ({ id: c.id, label: c.title, count: entriesIn(c.id).length }));
  return (
    <>
      <Hero />
      {/* The category bar sticks only while the categories are on screen. */}
      <div>
        <CategoryNav items={nav} />
        {categories.map((c, i) => (
          <CategorySection key={c.id} index={i} id={c.id} />
        ))}
      </div>
      <Closing />
      <PageSchema path="/ecosystem/" metadata={metadata} />
    </>
  );
}
