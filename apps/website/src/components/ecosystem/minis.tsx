import { AgentSession } from "./navigator-mock";
import { Slot } from "./harbor-mock";
import { FleetWorkloadCompact } from "./fleet-mock";
import { Checkbox, StageBadge } from "./helm-mock";
import { LiveFeed } from "./lighthouse-mock";
import { Waterfall } from "./logbook-mock";
import { harborReleases, helmDeals, logbookTimeline } from "./mock-data";

/*
 * Small, mostly still versions of the tool mocks for the homepage bento.
 * Only the Lighthouse feed moves: one signature motion for the section.
 */

export function LighthouseMini() {
  return <LiveFeed rows={7} dense interval={2600} className="h-[19.5rem] [mask-image:linear-gradient(to_bottom,black_75%,transparent)] lg:h-full" />;
}

export function FleetMini() {
  return <FleetWorkloadCompact step={5} />;
}

/** The moment of a deploy: Caddy switched to the healthy new release, the old one drained. */
export function HarborMini() {
  return (
    <div className="rounded-md border border-line bg-ink-0 p-3.5">
      <div className="rounded-md border border-line-2 bg-ink-2 px-3 py-1.5 text-center font-mono text-xs leading-5">
        <span className="text-fg">Caddy</span> <span className="text-fg-3">· crm.example.com</span>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div className="flex justify-center">
          <span className="h-4 w-px bg-line-2" />
        </div>
        <div className="flex justify-center">
          <span className="h-4 w-px bg-accent" />
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <Slot name="blue" port="8081" release={harborReleases[1]!.id} state={{ label: "drained", tone: "off" }} active={false} />
        <Slot name="green" port="8082" release={harborReleases[0]!.id} state={{ label: "live", tone: "live" }} active />
      </div>
      <div className="mt-3 flex items-center justify-between gap-3 font-mono text-xs">
        <span className="truncate text-fg-2">GET /health/ready 200 after 0.4 s</span>
        <span className="shrink-0 text-fg-3">1m 53s</span>
      </div>
    </div>
  );
}

export function HelmMini() {
  const rows = helmDeals.slice(0, 4);
  return (
    <div className="overflow-hidden rounded-md border border-line bg-ink-0">
      <div className="flex h-10 items-center gap-2 border-b border-line bg-ink-1 px-3 text-[0.8125rem]">
        <span className="font-medium text-fg tabular-nums">3 selected</span>
        <span className="ml-auto inline-flex h-7 items-center rounded-sm border border-line-2 bg-ink-1 px-2.5 text-fg-1">Mark as won</span>
        <span className="inline-flex h-7 items-center rounded-sm bg-ink-3 px-2.5 text-fg">Run</span>
      </div>
      {rows.map((deal) => (
        <div
          key={deal.title}
          className={`grid h-10 grid-cols-[1rem_minmax(0,1fr)_auto] items-center gap-2.5 border-b border-line px-3 last:border-b-0 ${deal.pick ? "bg-ink-2" : ""}`}
        >
          <Checkbox checked={!!deal.pick} />
          <span className="truncate text-[0.8125rem] text-fg">{deal.title}</span>
          <StageBadge stage={deal.stage} />
        </div>
      ))}
    </div>
  );
}

export function LogbookMini() {
  const rows = logbookTimeline.filter((r) => ["query", "event", "job", "mail", "response"].includes(r.kind)).slice(2, 9);
  return (
    <div className="rounded-md border border-line bg-ink-0 px-3 py-2">
      <div className="flex items-center justify-between border-b border-line pb-2 font-mono text-xs">
        <span className="text-fg">POST /api/v1/deals</span>
        <span className="text-fg-3">201 · 24 ms</span>
      </div>
      <Waterfall rows={rows} compact animate={false} />
    </div>
  );
}

export function NavigatorMini() {
  return (
    <div className="rounded-md border border-line bg-ink-0 p-3.5">
      <AgentSession step={99} compact />
    </div>
  );
}
