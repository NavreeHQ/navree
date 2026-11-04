import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/*
 * Small pictures for the library cards: one concrete detail each, in the
 * example CRM's terms. Decorative (the card's words carry the meaning), so
 * hidden from assistive technology.
 */

function Panel({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div aria-hidden className={cn("rounded-md border border-line bg-ink-0/60", className)}>
      {children}
    </div>
  );
}

/** A 4px chip. `selected` is ink (selection never turns green); `positive` is a success state in green. */
function Chip({ children, tone = "plain" }: { children: ReactNode; tone?: "plain" | "selected" | "positive" }) {
  return (
    <span
      className={cn(
        "rounded-sm border px-2 font-mono text-xs leading-5",
        tone === "positive" ? "border-accent/25 bg-accent/10 text-accent" : tone === "selected" ? "border-line-2 bg-ink-3 text-fg" : "border-line text-fg-2",
      )}
    >
      {children}
    </span>
  );
}

function Chips({ items, on }: { items: string[]; on?: number }) {
  return (
    <div aria-hidden className="flex flex-wrap gap-1.5">
      {items.map((item, i) => (
        <Chip key={item} tone={i === on ? "selected" : "plain"}>
          {item}
        </Chip>
      ))}
    </div>
  );
}

/** Search: a typo still finds the contact and the deal. */
export function SearchVisual() {
  return (
    <Panel className="overflow-hidden">
      <div className="flex items-center gap-2 border-b border-line px-3 py-1.5 font-mono text-xs leading-5 text-fg">
        <span className="text-fg-3">q</span>
        dhilon
        <span className="ml-auto text-fg-3">2 hits · 4 ms</span>
      </div>
      <div className="px-3 py-1.5 text-xs leading-6">
        <div className="flex justify-between gap-3">
          <span className="truncate text-fg-1">
            Jaspreet <mark className="rounded-[2px] bg-accent/15 px-0.5 text-fg">Dhillon</mark>
          </span>
          <span className="shrink-0 text-fg-3">Contact</span>
        </div>
        <div className="flex justify-between gap-3">
          <span className="truncate text-fg-1">
            <mark className="rounded-[2px] bg-accent/15 px-0.5 text-fg">Dhillon</mark> renewal
          </span>
          <span className="shrink-0 text-fg-3">Deal</span>
        </div>
      </div>
    </Panel>
  );
}

/** Billing: a workspace on a seat plan with a metered price. */
export function BillingVisual() {
  return (
    <Panel className="px-3 py-2">
      <div className="flex items-center justify-between gap-3 text-xs leading-6">
        <span className="text-fg-1">
          sutlej <span className="text-fg-3">on</span> Pro
        </span>
        <Chip tone="positive">active</Chip>
      </div>
      <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 font-mono text-xs leading-6">
        <span className="text-fg-3">seats</span>
        <span className="truncate text-right text-fg-1">12 members</span>
        <span className="text-fg-3">ai_summaries</span>
        <span className="truncate text-right text-fg-1">1,840 this period</span>
      </div>
    </Panel>
  );
}

/** Feature flags: a rollout to a quarter of workspaces. */
export function FlagsVisual() {
  const on = new Set([1, 6, 9, 14, 18]);
  return (
    <Panel className="px-3 py-2.5">
      <div className="flex items-center justify-between gap-3 font-mono text-xs leading-5">
        <span className="text-fg-1">new-pipeline</span>
        <span className="text-fg-3">25% of workspaces</span>
      </div>
      <div className="mt-2 grid grid-cols-[repeat(20,minmax(0,1fr))] gap-0.5">
        {Array.from({ length: 20 }, (_, i) => (
          <span key={i} className={cn("h-2 rounded-[2px]", on.has(i) ? "bg-accent/80" : "bg-line-2")} />
        ))}
      </div>
    </Panel>
  );
}

/** OAuth server: the scopes a third-party app asks for. */
export function OAuthVisual() {
  return (
    <Panel className="px-3 py-2.5">
      <div className="text-xs leading-5 text-fg-2">
        <span className="text-fg-1">Doaba Sync</span> asks to
      </div>
      <div className="mt-2">
        <Chips items={["deals:read", "deals:write", "contacts:read"]} />
      </div>
    </Panel>
  );
}

/** Social sign-in: the providers. */
export function SocialVisual() {
  return <Chips items={["google", "github", "apple", "microsoft", "gitlab", "slack", "linkedin", "discord"]} />;
}

/** Webhooks: a delivery's attempts, retried with backoff. */
export function WebhooksVisual() {
  const attempts = [
    { n: 1, after: "at once", status: "503", ok: false },
    { n: 2, after: "after 5 s", status: "503", ok: false },
    { n: 3, after: "after 5 min", status: "200", ok: true },
  ];
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      <div className="truncate text-fg-1">deal.won → hooks.sutlej.example</div>
      {attempts.map((a) => (
        <div key={a.n} className="grid grid-cols-[4.75rem_minmax(0,1fr)_auto] gap-2">
          <span className="text-fg-3">attempt {a.n}</span>
          <span className="truncate text-fg-3">{a.after}</span>
          <span className={a.ok ? "text-accent" : "text-fg-2"}>{a.status}</span>
        </div>
      ))}
    </Panel>
  );
}

/** Localization: one message, three locales. */
export function I18nVisual() {
  const lines = [
    { locale: "en", text: "3 deals created" },
    { locale: "hi", text: "3 सौदे बनाए गए" },
    { locale: "pa", text: "3 ਸੌਦੇ ਬਣਾਏ ਗਏ" },
  ];
  return (
    <Panel className="px-3 py-2">
      {lines.map((l) => (
        <div key={l.locale} className="grid grid-cols-[1.75rem_minmax(0,1fr)] text-xs leading-6">
          <span className="font-mono text-fg-3">{l.locale}</span>
          <span className="text-fg-1">{l.text}</span>
        </div>
      ))}
    </Panel>
  );
}

/** Microservices: the transports. */
export function MicroVisual() {
  return <Chips items={["nats", "kafka", "rabbitmq", "redis streams", "mqtt", "tcp"]} on={0} />;
}

/** gRPC: a service from its .proto. */
export function GrpcVisual() {
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      <div className="text-fg-1">
        <span className="text-fg-3">service</span> Deals {"{"}
      </div>
      <div className="truncate pl-4 text-fg-2">
        <span className="text-fg-3">rpc</span> Get(GetDeal) <span className="text-fg-3">returns</span> (Deal);
      </div>
      <div className="text-fg-1">{"}"}</div>
    </Panel>
  );
}

/** GraphQL: a query with nested fields batched by a dataloader. */
export function GraphqlVisual() {
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      <div className="text-fg-1">
        deals(stage: NEGOTIATION) {"{"}
      </div>
      <div className="pl-4 text-fg-2">title amountCents</div>
      <div className="truncate pl-4 text-fg-2">
        company {"{"} name {"}"} <span className="text-fg-3"># batched</span>
      </div>
      <div className="text-fg-1">{"}"}</div>
    </Panel>
  );
}

/** Real time: channels and who is here. */
export function RealtimeVisual() {
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      {[
        { ch: "deals", who: "public" },
        { ch: "private-workspace.sutlej", who: "private" },
        { ch: "presence-deal.0199c1d2", who: "3 here" },
      ].map((c) => (
        <div key={c.ch} className="flex justify-between gap-3">
          <span className="truncate text-fg-1">{c.ch}</span>
          <span className="shrink-0 text-fg-3">{c.who}</span>
        </div>
      ))}
    </Panel>
  );
}

/** Agent skills: a few of the skills. */
export function SkillsVisual() {
  return <Chips items={["navree-modules-and-di", "navree-database", "navree-auth", "navree-testing"]} />;
}

/** AI SDK: the providers it fails over between. */
export function AiVisual() {
  return <Chips items={["anthropic", "openai", "gemini", "mistral", "ollama", "bedrock"]} on={0} />;
}

/** MCP servers: the CRM's tools for AI clients. */
export function McpVisual() {
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      <div className="text-fg-3">/mcp · streamable HTTP</div>
      {["list_deals", "move_deal", "log_activity"].map((t) => (
        <div key={t} className="text-fg-1">
          <span className="text-accent">tool</span> {t}
        </div>
      ))}
    </Panel>
  );
}

/** Testing: `cargo test`. */
export function TestingVisual() {
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      <div className="text-fg">
        <span className="text-fg-3">$ </span>cargo test
      </div>
      {["reps_change_only_their_own_deals", "board_groups_by_stage_with_totals"].map((t) => (
        <div key={t} className="flex justify-between gap-3">
          <span className="truncate text-fg-2">test {t}</span>
          <span className="shrink-0 text-accent">ok</span>
        </div>
      ))}
    </Panel>
  );
}

/** Browser testing: a journey through the real front end. */
export function BrowserVisual() {
  const steps: [string, string][] = [
    ["visit", '"/login"'],
    ["type_into", '"@email", "priya@…"'],
    ["press", '"Sign in"'],
    ["assert_see", '"Pipeline"'],
  ];
  return (
    <Panel className="px-3 py-2 font-mono text-xs leading-6">
      {steps.map(([fn, arg]) => (
        <div key={fn} className="flex justify-between gap-3">
          <span className="truncate">
            <span className="text-fg-1">{fn}</span>
            <span className="text-fg-3">({arg})</span>
          </span>
          <span className="shrink-0 text-accent">✓</span>
        </div>
      ))}
    </Panel>
  );
}
