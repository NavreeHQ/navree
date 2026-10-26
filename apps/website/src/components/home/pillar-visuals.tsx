import { CheckIcon, ChevronsUpDownIcon, PlusIcon, SparkleIcon, TerminalIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/*
 * Small, real product mocks for the pillar tiles. Every name, command and
 * message is Navree's own: the DI error is the kernel's `NotExported`
 * message, the OpenAPI rows are the CRM's document, the skills are in
 * skills/ at the repository root.
 */

/** The inner panel every visual sits in (concentric with its tile: 20 px card, 8 px inset). */
export function Panel({ children, className, label }: { children: ReactNode; className?: string; label?: ReactNode }) {
  return (
    <div className={cn("overflow-hidden rounded-[12px] border border-line bg-ink-0/70", className)}>
      {label ? (
        <div className="flex h-9 items-center justify-between gap-3 border-b border-line px-3.5 font-mono text-xs text-fg-3">{label}</div>
      ) : null}
      {children}
    </div>
  );
}

/* 01: a module tree. */
export function ModuleTree() {
  const rows: [string, string, string?][] = [
    ["src/", ""],
    ["├─ app.rs", "AppModule", "root"],
    ["└─ modules/", ""],
    ["   ├─ companies/", "CompaniesModule"],
    ["   ├─ deals/", "DealsModule", "here"],
    ["   │  ├─ controller.rs", ""],
    ["   │  └─ service.rs", ""],
    ["   └─ reports/", "ReportsModule"],
  ];
  return (
    <Panel className="py-3">
      <ul className="font-mono text-[0.78rem] leading-[1.75]">
        {rows.map(([path, mod, tag]) => (
          <li key={path} className={cn("flex items-center justify-between gap-3 px-3.5 whitespace-pre", tag === "here" && "bg-ink-2/80")}>
            <span className={mod ? "text-fg-1" : "text-fg-3"}>{path}</span>
            {mod ? <span className={cn("truncate", tag === "here" ? "text-fg" : "text-fg-3")}>{mod}</span> : null}
          </li>
        ))}
      </ul>
    </Panel>
  );
}

/* 02: the boot refusing a private dependency, then passing. */
export function BootError() {
  return (
    <Panel
      label={
        <>
          <span className="inline-flex items-center gap-2">
            <TerminalIcon aria-hidden className="size-3.5" />
            ~/acme
          </span>
          <span>navree check</span>
        </>
      }
    >
      <div className="mock-lines space-y-0.5 p-3.5 sm:p-4">
        <p className="text-fg">$ navree check</p>
        <p className="text-fg-2">
          <span className="font-semibold text-danger">error</span> module <span className="text-fg">`ReportsModule`</span> cannot use{" "}
          <span className="text-fg">`app::deals::DealService`</span>: it is private to module <span className="text-fg">`DealsModule`</span>; add it
          to the exports of <span className="text-fg">`DealsModule`</span>
        </p>
        <div className="my-3 rounded-lg border border-line bg-ink-1 py-2 text-fg-3">
          <p className="px-3">#[module(</p>
          <p className="px-3">{"    providers = [DealService],"}</p>
          <p className="bg-accent/[0.07] px-3 text-fg-1">
            <span className="text-accent">+</span>
            {"   exports = [DealService],"}
          </p>
          <p className="px-3">)]</p>
        </div>
        <p className="text-fg">$ navree check</p>
        <p className="text-accent">module graph OK: 12 modules, 41 providers</p>
      </div>
    </Panel>
  );
}

/* 03: the generated OpenAPI document and the one error shape. */
const endpoints = [
  { method: "GET", path: "/api/v1/deals", summary: "List deals", codes: ["200", "400", "401", "403"] },
  { method: "POST", path: "/api/v1/deals", summary: "Create a deal", codes: ["201", "400", "401", "403"] },
  { method: "PATCH", path: "/api/v1/deals/{id}", summary: "Update a deal", codes: ["200", "400", "403", "404"] },
];

export function ApiDocument({ errorHtml }: { errorHtml: string }) {
  return (
    <div className="grid gap-2">
      <Panel
        label={
          <>
            <span className="truncate">GET /api/openapi.json</span>
            <span className="shrink-0 rounded-full border border-line-2 px-2 py-px text-[0.6875rem] text-fg-2">OpenAPI 3.1</span>
          </>
        }
      >
        <ul className="divide-y divide-line">
          {endpoints.map((e) => (
            <li
              key={e.method + e.path}
              className="grid grid-cols-[3.25rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5 px-3.5 py-2.5 sm:grid-cols-[3.25rem_minmax(0,1fr)_auto]"
            >
              <span className="font-mono text-xs font-medium text-fg-2">{e.method}</span>
              <span className="flex min-w-0 items-baseline gap-3">
                <span className="truncate font-mono text-[0.8125rem] text-fg">{e.path}</span>
                <span className="hidden truncate text-xs text-fg-3 md:inline">{e.summary}</span>
              </span>
              <span className="col-start-2 flex gap-1 sm:col-start-auto">
                {e.codes.map((c) => (
                  <span
                    key={c}
                    className={cn(
                      "rounded-md border px-1.5 font-mono text-[0.6875rem] leading-[1.125rem]",
                      c.startsWith("2") ? "border-line-2 text-fg-1" : "border-line text-fg-3",
                    )}
                  >
                    {c}
                  </span>
                ))}
              </span>
            </li>
          ))}
        </ul>
      </Panel>
      <Panel
        label={
          <>
            <span>400 Bad Request</span>
            <span>every error, one shape</span>
          </>
        }
      >
        <pre
          className="shiki navree-night scroll-thin overflow-x-auto bg-transparent! px-3.5 py-3 font-mono text-[0.78rem] leading-[1.7]"
          dangerouslySetInnerHTML={{ __html: errorHtml }}
        />
      </Panel>
    </div>
  );
}

/* 04: types all the way through. */
const typed: [string, string][] = [
  ["Valid<Json<CreateDeal>>", "request"],
  ["Select<deal::Entity>", "query"],
  ["SendTaskReminders: Job", "job"],
  ["DealWon: Event", "event"],
  ["WorkspaceConfig", "config"],
];

export function TypedList() {
  return (
    <Panel>
      <ul className="divide-y divide-line">
        {typed.map(([type, kind]) => (
          <li key={type} className="flex items-center justify-between gap-3 px-3.5 py-2.5">
            <code className="truncate font-mono text-[0.78rem] text-fg-1">{type}</code>
            <span className="shrink-0 font-mono text-[0.6875rem] tracking-[0.08em] text-fg-3 uppercase">{kind}</span>
          </li>
        ))}
      </ul>
    </Panel>
  );
}

/* 05: what ships on. */
const secure = [
  "Deny by default",
  "Rotating refresh tokens",
  "Two-factor (TOTP)",
  "Argon2id hashing",
  "Encrypted cookies",
  "CSRF protection",
  "Rate limits",
  "Security headers",
  "Audit log",
  "SSRF guard",
];

export function SecureList() {
  return (
    <Panel className="p-3.5">
      <ul className="flex flex-wrap gap-x-4 gap-y-2 min-[25rem]:grid min-[25rem]:grid-cols-2">
        {secure.map((s) => (
          <li key={s} className="flex items-center gap-2 text-[0.8125rem] text-fg-1">
            <CheckIcon aria-hidden className="size-3.5 shrink-0 text-accent/80" strokeWidth={2.5} />
            {s}
          </li>
        ))}
      </ul>
    </Panel>
  );
}

/* 06: a workspace switcher. */
const workspaces = [
  { initial: "A", name: "Acme Inc", role: "Owner", active: true },
  { initial: "G", name: "Globex", role: "Admin", active: false },
  { initial: "I", name: "Initech", role: "Viewer", active: false },
];

export function SwitcherMock() {
  return (
    <Panel className="p-2">
      <div className="flex items-center gap-2.5 rounded-lg border border-line-2 bg-ink-2 px-2.5 py-2">
        <span className="inline-flex size-6 items-center justify-center rounded-md bg-fg font-display text-xs font-semibold text-ink-0">A</span>
        <span className="flex-1 text-[0.8125rem] font-medium text-fg">Acme Inc</span>
        <ChevronsUpDownIcon aria-hidden className="size-3.5 text-fg-3" />
      </div>
      <ul className="mt-1.5 space-y-0.5">
        {workspaces.map((w) => (
          <li key={w.name} className={cn("flex items-center gap-2.5 rounded-lg px-2.5 py-1.5", w.active && "bg-ink-2/70")}>
            <span className="inline-flex size-6 items-center justify-center rounded-md border border-line-2 bg-ink-1 font-display text-xs font-semibold text-fg-1">
              {w.initial}
            </span>
            <span className="flex-1 text-[0.8125rem] text-fg-1">{w.name}</span>
            <span className="font-mono text-[0.6875rem] text-fg-3">{w.role}</span>
            <CheckIcon aria-hidden className={cn("size-3.5 text-accent", !w.active && "invisible")} />
          </li>
        ))}
        <li className="flex items-center gap-2.5 border-t border-line px-2.5 pt-2 pb-1 text-[0.8125rem] text-fg-3">
          <span className="inline-flex size-6 items-center justify-center">
            <PlusIcon aria-hidden className="size-3.5" />
          </span>
          Create workspace
        </li>
      </ul>
    </Panel>
  );
}

/* 07: a coding agent using Navree's skills and Navigator. */
export function AgentMock() {
  return (
    <Panel className="mock-lines p-3.5">
      <p className="text-fg">
        <span className="text-fg-3">›</span> Add a billing module with plans
      </p>
      <p className="mt-1.5 text-fg-3">
        <SparkleIcon aria-hidden className="mr-1.5 inline size-3 -translate-y-px text-fg-2" />
        skill <span className="text-fg-1">navree-modules-and-di</span>
      </p>
      <p className="text-fg-3">
        <SparkleIcon aria-hidden className="mr-1.5 inline size-3 -translate-y-px text-fg-2" />
        tool <span className="text-fg-1">navigator · module graph</span>
      </p>
      <p className="mt-1.5 text-fg-2">$ navree make:module billing --crud</p>
      <p className="text-fg-2">$ navree check</p>
      <p className="text-accent">module graph OK: 13 modules, 44 providers</p>
    </Panel>
  );
}

/* 08: logs with trace ids. */
const logs: [string, string, string][] = [
  ["INFO", "http", "GET /api/v1/deals 200 14ms"],
  ["INFO", "queue", "crm.task_reminders done 82ms"],
  ["WARN", "db", "slow query 412ms deals.index"],
];

export function LogMock() {
  return (
    <Panel className="py-2.5">
      <ul className="font-mono text-[0.75rem] leading-[1.9]">
        {logs.map(([level, target, text]) => (
          <li key={text} className="flex gap-2.5 px-3.5 whitespace-nowrap">
            <span className={level === "WARN" ? "text-amber" : "text-fg-3"}>{level}</span>
            <span className="w-10 shrink-0 text-fg-3">{target}</span>
            <span className="truncate text-fg-1">{text}</span>
          </li>
        ))}
      </ul>
    </Panel>
  );
}

/* 09: one binary today, services over a broker tomorrow. */
const transports = ["NATS", "Kafka", "RabbitMQ", "Redis Streams", "gRPC"];

export function TransportChips() {
  return (
    <Panel className="p-3.5">
      <ul className="flex flex-wrap gap-1.5">
        {transports.map((t) => (
          <li key={t} className="rounded-md border border-line-2 bg-ink-1 px-2 py-0.5 font-mono text-[0.75rem] text-fg-1">
            {t}
          </li>
        ))}
      </ul>
      <p className="mt-3 font-mono text-[0.75rem] text-fg-3">
        #[message_pattern(<span className="text-fg-1">&quot;deals.get&quot;</span>)]
      </p>
    </Panel>
  );
}

/* 10: Navree's own measurements (the performance page's numbers). */
export function FrugalStats({ peakMb, startupMs }: { peakMb: number; startupMs: number }) {
  return (
    <Panel className="grid grid-cols-3 divide-x divide-line">
      {[
        [String(peakMb), "MB", "peak memory under load"],
        [String(startupMs), "ms", "from start to ready"],
        ["1", "", "binary"],
      ].map(([value, unit, label]) => (
        <div key={label} className="px-3.5 py-3">
          <p className="font-display text-xl font-semibold tracking-[-0.02em] text-fg">
            {value}
            {unit ? <span className="ml-0.5 text-sm font-medium text-fg-2">{unit}</span> : null}
          </p>
          <p className="mt-0.5 text-xs leading-snug text-fg-3">{label}</p>
        </div>
      ))}
    </Panel>
  );
}
