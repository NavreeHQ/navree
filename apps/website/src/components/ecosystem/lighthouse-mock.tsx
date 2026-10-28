"use client";

import {
  ActivityIcon,
  BellIcon,
  BugIcon,
  DatabaseIcon,
  GlobeIcon,
  LayoutDashboardIcon,
  LighthouseIcon,
  ListChecksIcon,
  SettingsIcon,
  TagIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";

import { AppShell, Card, Segmented, StatusDot, TopBar, type SidebarSpec } from "./app-shell";
import { Sparkline } from "./charts";
import { MockFrame } from "./frame";
import { issueKinds, lighthouseFeed, lighthouseFigures, lighthouseIssues, lighthouseTimeline, type IssueKind } from "./mock-data";
import { useFlipList } from "./use-flip";
import { useSteps } from "./use-steps";

const toneText = { danger: "text-danger", amber: "text-amber" } as const;
const toneBg = { danger: "bg-danger", amber: "bg-amber" } as const;

export function KindLabel({ kind, className }: { kind: IssueKind; className?: string }) {
  const { label, tone } = issueKinds[kind];
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-xs leading-4 font-medium whitespace-nowrap", toneText[tone], className)}>
      <span className={cn("size-2 rounded-[2px]", toneBg[tone])} />
      {label}
    </span>
  );
}

const ages = ["now", "6s", "14s", "31s", "52s", "1m", "2m", "3m", "4m", "6m"];

/**
 * The live feed of new occurrences: one arrives on top every couple of
 * seconds while the feed is on screen, pushing the others down. Its height
 * is fixed, so arrivals never move the page.
 */
export function LiveFeed({ rows = 5, interval = 2400, className, dense = false }: { rows?: number; interval?: number; className?: string; dense?: boolean }) {
  const n = lighthouseFeed.length;
  const { ref, step } = useSteps(n, { interval, hold: interval, still: 0 });
  const items = Array.from({ length: rows }, (_, i) => lighthouseFeed[(step - i + n * 4) % n]!);
  useFlipList(ref, [step]);
  return (
    <div ref={ref} className={cn("flex flex-col gap-2 overflow-hidden", className)}>
      {items.map((item, i) => (
        <div key={item.id} data-key={item.id} className="mx-auto w-full">
          <div
            className={cn(
              "rounded-md border transition-colors duration-700",
              dense ? "px-3 py-2.5" : "px-3 py-2.5",
              i === 0 ? "border-line-2 bg-ink-2" : "border-line bg-ink-1",
            )}
          >
            <div className="flex items-center justify-between gap-3">
              <KindLabel kind={item.kind} />
              <span className="text-xs text-fg-3 tabular-nums">{ages[i]}</span>
            </div>
            <div className="mt-1.5 truncate font-mono text-[0.8125rem] leading-5 text-fg-1">{item.title}</div>
            {dense ? null : <div className="truncate font-mono text-xs leading-5 text-fg-3">{item.culprit}</div>}
          </div>
        </div>
      ))}
    </div>
  );
}

function Timeline() {
  const { requests, exceptions } = lighthouseTimeline;
  const maxR = Math.max(...requests);
  const maxE = Math.max(...exceptions);
  const bw = 480 / requests.length;
  return (
    <svg viewBox="0 0 480 100" preserveAspectRatio="none" className="block h-28 w-full" aria-hidden>
      {[6, 28, 50].map((y) => (
        <line key={y} x1="0" x2="480" y1={y + 0.5} y2={y + 0.5} className="stroke-line" strokeWidth="1" strokeDasharray="2 3" vectorEffect="non-scaling-stroke" />
      ))}
      <line x1="0" x2="480" y1="70.5" y2="70.5" className="stroke-line-2" strokeWidth="1" vectorEffect="non-scaling-stroke" />
      {requests.map((v, i) => {
        const h = (v / maxR) * 64;
        return <rect key={`r${i}`} x={i * bw + 1.8} y={69 - h} width={bw - 3.6} height={h} rx="0.6" className="fill-fg-3/25" />;
      })}
      {exceptions.map((v, i) =>
        v ? <rect key={`e${i}`} x={i * bw + 1.4} y={72} width={bw - 2.8} height={(v / maxE) * 26} rx="0.6" className="fill-danger/80" /> : null,
      )}
    </svg>
  );
}

const sidebar: SidebarSpec = {
  app: { logo: <LighthouseIcon strokeWidth={2} />, name: "Lighthouse", context: "crm-api · production" },
  groups: [
    {
      label: "Monitor",
      items: [
        { icon: LayoutDashboardIcon, label: "Overview", active: true },
        { icon: BugIcon, label: "Issues", badge: "7" },
        { icon: ActivityIcon, label: "Requests" },
        { icon: DatabaseIcon, label: "Queries" },
        { icon: ListChecksIcon, label: "Jobs" },
        { icon: GlobeIcon, label: "Outgoing" },
      ],
    },
    {
      label: "Configure",
      items: [
        { icon: BellIcon, label: "Alerts", badge: "2" },
        { icon: TagIcon, label: "Releases" },
        { icon: SettingsIcon, label: "Settings" },
      ],
    },
  ],
  user: { initials: "PS", name: "Priya Shah", email: "priya@acme.test" },
};

/** The Lighthouse dashboard at /_lighthouse: figures, a timeline, issues and the live feed. */
export function LighthouseMock({ className }: { className?: string }) {
  return (
    <MockFrame
      url="crm.example.com/_lighthouse"
      className={className}
      bodyClassName="@container"
      label="The Lighthouse dashboard of the example CRM: requests per minute, error rate, p95 latency, failed jobs and open issues; a 24-hour timeline of requests and exceptions; issues such as a slow contacts query on the deal board and a declined card when charging an invoice; and a live feed of new occurrences."
    >
      <AppShell sidebar={sidebar}>
        <TopBar crumbs={["crm-api", "Overview"]}>
          <span className="hidden items-center gap-2 pr-2 text-[0.8125rem] text-fg-2 @md:flex">
            <StatusDot pulse />
            Live
          </span>
          <Segmented items={["1h", "24h", "7d"]} active={1} />
        </TopBar>

        <div className="grid flex-1 @5xl:grid-cols-[minmax(0,1fr)_18.5rem]">
          <div className="min-w-0 p-4 @xl:p-6">
            <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
              <div className="text-xl font-semibold tracking-[-0.01em] text-fg">Overview</div>
              <div className="text-[0.8125rem] text-fg-3">
                Release <span className="font-mono text-xs text-fg-2">3f2a9c1</span> · deployed 18 min ago
              </div>
            </div>

            <div className="mt-5 grid grid-cols-2 gap-px overflow-hidden rounded-md border border-line bg-line @2xl:grid-cols-5">
              {lighthouseFigures.map((f, i) => (
                <div key={f.label} className={cn("bg-ink-1 p-4", i === 4 && "hidden @2xl:block")}>
                  <div className="truncate text-[0.8125rem] text-fg-2">{f.label}</div>
                  <div className="mt-1.5 flex items-end justify-between gap-2">
                    <span className="text-2xl leading-tight font-semibold tracking-[-0.02em] whitespace-nowrap text-fg tabular-nums">{f.value}</span>
                    {"spark" in f ? <Sparkline data={[...f.spark]} className="mb-1.5 h-5 w-10 shrink text-fg-3" /> : null}
                  </div>
                  <div className="mt-1 truncate text-[0.8125rem] text-fg-3">{f.sub}</div>
                </div>
              ))}
            </div>

            <Card className="mt-3 p-4">
              <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
                <span className="text-sm font-medium text-fg">Requests and exceptions</span>
                <span className="flex items-center gap-4 text-[0.8125rem] text-fg-3">
                  <span className="inline-flex items-center gap-1.5">
                    <span className="size-2 rounded-[2px] bg-fg-3/50" />
                    Requests
                  </span>
                  <span className="inline-flex items-center gap-1.5">
                    <span className="size-2 rounded-[2px] bg-danger/80" />
                    Exceptions
                  </span>
                </span>
              </div>
              <div className="mt-4">
                <Timeline />
              </div>
              <div className="mt-1.5 flex justify-between text-xs text-fg-3 tabular-nums">
                <span>09:00</span>
                <span className="hidden @md:inline">15:00</span>
                <span>21:00</span>
                <span className="hidden @md:inline">03:00</span>
                <span>Now</span>
              </div>
            </Card>

            <div className="mt-3 overflow-hidden rounded-md border border-line">
              <div className="grid h-10 grid-cols-[minmax(0,1fr)_3.5rem] items-center gap-4 border-b border-line bg-ink-1 px-3 text-[0.8125rem] font-medium text-fg-2 @2xl:grid-cols-[minmax(0,1fr)_5.5rem_3.5rem_3rem_4rem]">
                <span>Issue</span>
                <span className="hidden @2xl:block">24 hours</span>
                <span className="text-right">Events</span>
                <span className="hidden text-right @2xl:block">Users</span>
                <span className="hidden text-right @2xl:block">Last seen</span>
              </div>
              {lighthouseIssues.map((issue, i) => (
                <div
                  key={issue.title}
                  className={cn(
                    "grid grid-cols-[minmax(0,1fr)_3.5rem] items-center gap-4 border-b border-line px-3 py-2.5 last:border-b-0 @2xl:grid-cols-[minmax(0,1fr)_5.5rem_3.5rem_3rem_4rem]",
                    // Narrow frames list the top three; the class comes last so it wins over `grid`.
                    i > 2 && "hidden @xl:grid",
                    i === 2 && "@max-xl:border-b-0",
                  )}
                >
                  <div className="min-w-0">
                    <KindLabel kind={issue.kind} />
                    <div className="mt-1 truncate font-mono text-[0.8125rem] leading-5 text-fg">{issue.title}</div>
                    <div className="truncate font-mono text-xs leading-5 text-fg-3">{issue.culprit}</div>
                  </div>
                  <Sparkline
                    data={issue.spark}
                    area
                    className={cn("hidden h-7 w-full @2xl:block", issueKinds[issue.kind].tone === "danger" ? "text-danger/80" : "text-amber/80")}
                  />
                  <span className="text-right text-sm text-fg-1 tabular-nums">{issue.events}</span>
                  <span className="hidden text-right text-sm text-fg-2 tabular-nums @2xl:block">{issue.users}</span>
                  <span className="hidden text-right text-[0.8125rem] text-fg-3 @2xl:block">{issue.seen}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="relative min-w-0 border-t border-line p-4 @xl:px-6 @5xl:border-t-0 @5xl:border-l @5xl:px-4 @5xl:pt-6">
            <div className="mb-3 flex items-center justify-between">
              <span className="text-sm font-medium text-fg">New occurrences</span>
              <span className="inline-flex items-center gap-2 text-xs text-fg-3">
                <StatusDot />
                Streaming
              </span>
            </div>
            {/* Absolutely placed on wide screens so the feed never sets the row's height. */}
            <LiveFeed
              rows={8}
              className="h-[14.5rem] [mask-image:linear-gradient(to_bottom,black_75%,transparent)] @xl:h-[22rem] @5xl:absolute @5xl:inset-x-4 @5xl:top-[3.75rem] @5xl:bottom-0 @5xl:h-auto"
            />
          </div>
        </div>
      </AppShell>
    </MockFrame>
  );
}
