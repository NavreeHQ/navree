import Link from "next/link";
import type { ReactNode } from "react";

import { mb, perf } from "@/lib/bench";
import { highlight } from "@/lib/mdx";
import { cn } from "@/lib/utils";

import { GlowCard } from "./glow-card";
import { HomeSection, SectionHeading } from "./parts";
import {
  AgentMock,
  ApiDocument,
  BootError,
  FrugalStats,
  LogMock,
  ModuleTree,
  SecureList,
  SwitcherMock,
  TransportChips,
  TypedList,
} from "./pillar-visuals";

type Pillar = { n: string; title: string; text: string; href: string };

/** The ten pillars, verbatim from the positioning (spec §3). */
const P: Record<string, Pillar> = {
  modular: {
    n: "01",
    title: "Modular by design.",
    text: "Every feature is a module that declares what it imports, provides and exports. Apps stay navigable at five modules or five hundred.",
    href: "/docs/architecture/modules/",
  },
  di: {
    n: "02",
    title: "Dependency injection.",
    text: "A type-keyed container wires services at boot and refuses to start with a missing or private dependency, naming the module and the type.",
    href: "/docs/architecture/providers/",
  },
  api: {
    n: "03",
    title: "API-first.",
    text: "Versioned routes, validated input, one JSON error shape and an OpenAPI 3.1 document generated from your code.",
    href: "/docs/digging-deeper/openapi/",
  },
  typed: {
    n: "04",
    title: "Type-safe end to end.",
    text: "Requests, queries, jobs, events and configuration are typed. If it compiles, it's wired.",
    href: "/docs/basics/validation/",
  },
  secure: {
    n: "05",
    title: "Secure by default.",
    text: "Routes deny until you allow them. Rotating refresh tokens, two-factor, Argon2id, encrypted cookies, CSRF, rate limits, security headers, an audit log and an SSRF guard ship on.",
    href: "/docs/security/authentication/",
  },
  workspaces: {
    n: "06",
    title: "Workspaces built in.",
    text: "Multi-tenancy at the workspace level: members, roles, invitations, and isolation by row, schema or database.",
    href: "/docs/workspaces/introduction/",
  },
  ai: {
    n: "07",
    title: "AI-native.",
    text: "Agent skills, an MCP server for your coding agent, docs written for models as well as people, and an AI SDK for the product itself.",
    href: "/ai/",
  },
  observable: {
    n: "08",
    title: "Observable.",
    text: "Structured logs, traces, metrics, health checks and Lighthouse monitoring, self-hosted and on by default.",
    href: "/docs/digging-deeper/health-and-metrics/",
  },
  scales: {
    n: "09",
    title: "Scales out.",
    text: "Start as one binary; split into services over NATS, Kafka, RabbitMQ, Redis Streams or gRPC without rewriting your modules.",
    href: "/docs/microservices/introduction/",
  },
  fast: {
    n: "10",
    title: "Fast and frugal.",
    // The canonical line says "about 30 MB"; the bound follows the measured peak so the tile never contradicts its numbers.
    text: `One binary, under ${Math.ceil(perf.peakMemMb / 10) * 10} MB of memory under load and ready in milliseconds.`,
    href: "/performance/",
  },
};

/**
 * One bento tile. The title is the link and stretches over the tile, so
 * the whole tile is one target with one accessible name. Wide tiles put
 * the copy beside the visual from `lg`.
 */
function Tile({ pillar, visual, wide, className }: { pillar: Pillar; visual: ReactNode; wide?: boolean; className?: string }) {
  return (
    <GlowCard className={cn("rounded-[1.25rem] shadow-bezel", className)}>
      <div className={cn("flex h-full flex-col", wide && "lg:flex-row")}>
        <div className={cn("p-6 pb-5 sm:p-7 sm:pb-6", wide && "lg:w-[16.5rem] lg:shrink-0 lg:pr-4 xl:w-[20rem]")}>
          <p className="font-mono text-xs text-fg-3 numeric">{pillar.n}</p>
          <h3 className="mt-4 font-display text-display-sm font-semibold text-fg">
            <Link
              href={pillar.href}
              className="rounded-sm transition-colors after:absolute after:inset-0 after:z-10 after:rounded-[1.25rem] group-hover:text-accent focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-offset-[-3px] focus-visible:after:outline-accent"
            >
              {pillar.title}
            </Link>
          </h3>
          <p className="mt-2.5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{pillar.text}</p>
        </div>
        <div className={cn("mt-auto p-2", wide && "lg:mt-0 lg:flex lg:min-w-0 lg:flex-1 lg:flex-col lg:justify-center")}>{visual}</div>
      </div>
    </GlowCard>
  );
}

const ERROR_JSON = `{
  "statusCode": 400,
  "message": ["title should not be empty"],
  "error": "Bad Request"
}`;

/** The pillars as a bento grid: six columns from `lg`, two from `md`, one on phones. */
export async function Pillars() {
  const errorHtml = (await highlight(ERROR_JSON, "json")).replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
  return (
    <HomeSection id="pillars">
      <SectionHeading
        split
        id="pillars"
        eyebrow="The framework"
        title="Everything a backend needs, on from the first commit."
        lead="Ten things that hold in every Navree app, whether it serves one team or a thousand workspaces."
      />

      <div className="mt-14 grid grid-cols-1 gap-3 md:grid-flow-row-dense md:grid-cols-2 lg:mt-16 lg:grid-cols-6">
        <Tile pillar={P.modular} visual={<ModuleTree />} className="lg:col-span-2" />
        <Tile pillar={P.di} visual={<BootError />} wide className="md:col-span-2 lg:col-span-4" />
        <Tile pillar={P.api} visual={<ApiDocument errorHtml={errorHtml} />} wide className="md:col-span-2 lg:col-span-4" />
        <Tile pillar={P.typed} visual={<TypedList />} className="lg:col-span-2" />
        <Tile pillar={P.secure} visual={<SecureList />} className="lg:col-span-2" />
        <Tile pillar={P.workspaces} visual={<SwitcherMock />} className="lg:col-span-2" />
        <Tile pillar={P.ai} visual={<AgentMock />} className="lg:col-span-2" />
        <Tile pillar={P.observable} visual={<LogMock />} className="lg:col-span-2" />
        <Tile pillar={P.scales} visual={<TransportChips />} className="lg:col-span-2" />
        <Tile pillar={P.fast} visual={<FrugalStats peakMb={mb(perf.peakMemMb)} startupMs={perf.startupMs} />} className="lg:col-span-2" />
      </div>
    </HomeSection>
  );
}
