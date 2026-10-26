"use client";

import {
  ActivityIcon,
  BugIcon,
  DatabaseIcon,
  GlobeIcon,
  LayoutDashboardIcon,
  ListChecksIcon,
  LighthouseIcon,
  type LucideIcon,
} from "lucide-react";
import { AnimatePresence } from "motion/react";

import { AnimatedListItem } from "@/components/magicui/animated-list";
import { cn } from "@/lib/utils";

import { Sparkline } from "./charts";
import { Figure, LiveLight, MockFrame } from "./frame";
import { issueKinds, lighthouseFeed, lighthouseFigures, lighthouseIssues, lighthouseTimeline, type IssueKind } from "./mock-data";
import { useSteps } from "./use-steps";

const toneText = { danger: "text-danger", amber: "text-amber" } as const;
const toneBg = { danger: "bg-danger", amber: "bg-amber" } as const;

export function KindLabel({ kind, className }: { kind: IssueKind; className?: string }) {
  const { label, tone } = issueKinds[kind];
  return (
    <span className={cn("inline-flex items-center gap-1.5 font-mono text-xs leading-4 whitespace-nowrap", toneText[tone], className)}>
      <span className={cn("size-1.5 rounded-[2px]", toneBg[tone])} />
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
  return (
    <div ref={ref} className={cn("flex flex-col gap-2 overflow-hidden", className)}>
      <AnimatePresence initial={false}>
        {items.map((item, i) => (
          <AnimatedListItem key={item.id}>
            <div
              className={cn(
                "rounded-xl border bg-ink-1 transition-colors duration-700",
                dense ? "px-3 py-2.5" : "px-3.5 py-3",
                i === 0 ? "border-line-2 bg-ink-2/80" : "border-line",
              )}
            >
              <div className="flex items-center justify-between gap-3">
                <KindLabel kind={item.kind} />
                <span className="font-mono text-xs text-fg-3 tabular-nums">{ages[i]}</span>
              </div>
              <div className="mt-1.5 truncate font-mono text-[0.8125rem] leading-5 text-fg-1">{item.title}</div>
              {dense ? null : <div className="truncate font-mono text-xs leading-5 text-fg-3">{item.culprit}</div>}
            </div>
          </AnimatedListItem>
        ))}
      </AnimatePresence>
    </div>
  );
}

function SideItem({ icon: Icon, label, count, active }: { icon: LucideIcon; label: string; count?: string; active?: boolean }) {
  return (
    <div className={cn("flex h-8 items-center gap-2.5 rounded-lg px-2.5 text-[0.8125rem]", active ? "bg-ink-3 text-fg" : "text-fg-2")}>
      <Icon className={cn("size-4", active ? "text-accent" : "text-fg-3")} strokeWidth={1.5} />
      <span className="flex-1">{label}</span>
      {count ? <span className="font-mono text-xs text-fg-3">{count}</span> : null}
    </div>
  );
}

function Timeline() {
  const { requests, exceptions } = lighthouseTimeline;
  const maxR = Math.max(...requests);
  const maxE = Math.max(...exceptions);
  const bw = 480 / requests.length;
  return (
    <svg viewBox="0 0 480 100" preserveAspectRatio="none" className="block h-24 w-full" aria-hidden>
      <line x1="0" x2="480" y1="70.5" y2="70.5" className="stroke-line-2" strokeWidth="1" vectorEffect="non-scaling-stroke" />
      {requests.map((v, i) => {
        const h = (v / maxR) * 64;
        return <rect key={`r${i}`} x={i * bw + 1.6} y={69 - h} width={bw - 3.2} height={h} rx="1" className="fill-accent/25" />;
      })}
      {exceptions.map((v, i) =>
        v ? <rect key={`e${i}`} x={i * bw + 1.6} y={72} width={bw - 3.2} height={(v / maxE) * 26} rx="1" className="fill-danger/80" /> : null,
      )}
    </svg>
  );
}

/** The Lighthouse dashboard at /_lighthouse: figures, a timeline, issues and the live feed. */
export function LighthouseMock({ className }: { className?: string }) {
  return (
    <MockFrame
      url="crm.example.com/_lighthouse"
      status={<LiveLight />}
      className={className}
      bodyClassName="@container"
      label="The Lighthouse dashboard of the example CRM: requests per minute, error rate, p95 latency, failed jobs and open issues; a 24-hour timeline of requests and exceptions; issues such as a slow contacts query on the deal board and a declined card when charging an invoice; and a live feed of new occurrences."
    >
      <div className="grid min-h-full @5xl:grid-cols-[11.5rem_minmax(0,1fr)_18.5rem]">
        <aside className="hidden flex-col gap-0.5 border-r border-line bg-ink-0/40 p-3 @5xl:flex">
          <div className="mb-3 flex items-center gap-2 px-2.5 pt-1 text-[0.8125rem] font-semibold text-fg">
            <LighthouseIcon className="size-4 text-accent" strokeWidth={1.5} />
            Lighthouse
          </div>
          <SideItem icon={LayoutDashboardIcon} label="Overview" active />
          <SideItem icon={BugIcon} label="Issues" count="7" />
          <SideItem icon={ActivityIcon} label="Requests" />
          <SideItem icon={DatabaseIcon} label="Queries" />
          <SideItem icon={ListChecksIcon} label="Jobs" />
          <SideItem icon={GlobeIcon} label="Outgoing" />
          <div className="mt-auto border-t border-line px-2.5 pt-3 font-mono text-xs leading-5 text-fg-3">
            <div className="text-fg-2">crm-api</div>
            <div className="truncate">production · 3f2a9c1</div>
          </div>
        </aside>

        <div className="min-w-0 p-4 @xl:p-5">
          <div className="flex items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2.5">
              <span className="text-[0.9375rem] font-semibold text-fg">Overview</span>
              <span className="hidden rounded-md border border-line px-1.5 font-mono text-xs leading-5 text-fg-3 @md:inline">production</span>
            </div>
            <div className="flex rounded-lg border border-line p-0.5 font-mono text-xs">
              <span className="rounded-md px-2 py-0.5 text-fg-3">1h</span>
              <span className="rounded-md bg-ink-3 px-2 py-0.5 text-fg">24h</span>
              <span className="rounded-md px-2 py-0.5 text-fg-3">7d</span>
            </div>
          </div>

          <div className="mt-4 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line @2xl:grid-cols-5">
            {lighthouseFigures.map((f, i) => (
              <div key={f.label} className={cn("relative bg-ink-1 p-3", i === 4 && "col-span-2 @2xl:col-span-1")}>
                <Figure label={f.label} value={f.value} sub={f.sub} />
                {"spark" in f ? <Sparkline data={[...f.spark]} className="absolute right-3 bottom-3 h-6 w-14 text-accent/70" /> : null}
              </div>
            ))}
          </div>

          <div className="mt-3 rounded-xl border border-line bg-ink-1 p-3.5">
            <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-xs text-fg-3">
              <span className="text-fg-2">Requests and exceptions, last 24 hours</span>
              <span className="flex items-center gap-3">
                <span className="inline-flex items-center gap-1.5">
                  <span className="size-2 rounded-[2px] bg-accent/50" />
                  Requests
                </span>
                <span className="inline-flex items-center gap-1.5">
                  <span className="size-2 rounded-[2px] bg-danger/80" />
                  Exceptions
                </span>
              </span>
            </div>
            <div className="mt-3">
              <Timeline />
            </div>
            <div className="mt-1 flex justify-between font-mono text-xs text-fg-3">
              <span>09:00</span>
              <span className="hidden @md:inline">15:00</span>
              <span>21:00</span>
              <span className="hidden @md:inline">03:00</span>
              <span>now</span>
            </div>
          </div>

          <div className="mt-3 overflow-hidden rounded-xl border border-line bg-ink-1">
            <div className="grid grid-cols-[minmax(0,1fr)_3.5rem] gap-4 border-b border-line px-3.5 py-2 text-xs text-fg-3 @2xl:grid-cols-[minmax(0,1fr)_5.5rem_3.5rem_3rem_3.75rem]">
              <span>Issue</span>
              <span className="hidden @2xl:block">24 hours</span>
              <span className="text-right">Events</span>
              <span className="hidden text-right @2xl:block">Users</span>
              <span className="hidden text-right @2xl:block">Last seen</span>
            </div>
            {lighthouseIssues.map((issue) => (
              <div
                key={issue.title}
                className="grid grid-cols-[minmax(0,1fr)_3.5rem] items-center gap-4 border-b border-line px-3.5 py-2.5 last:border-b-0 @2xl:grid-cols-[minmax(0,1fr)_5.5rem_3.5rem_3rem_3.75rem]"
              >
                <div className="min-w-0">
                  <KindLabel kind={issue.kind} />
                  <div className="mt-1 truncate font-mono text-[0.8125rem] leading-5 text-fg-1">{issue.title}</div>
                  <div className="truncate font-mono text-xs leading-5 text-fg-3">{issue.culprit}</div>
                </div>
                <Sparkline
                  data={issue.spark}
                  area
                  className={cn("hidden h-7 w-full @2xl:block", issueKinds[issue.kind].tone === "danger" ? "text-danger/80" : "text-amber/80")}
                />
                <span className="text-right font-mono text-[0.8125rem] text-fg-1 tabular-nums">{issue.events}</span>
                <span className="hidden text-right font-mono text-[0.8125rem] text-fg-2 tabular-nums @2xl:block">{issue.users}</span>
                <span className="hidden text-right font-mono text-xs text-fg-3 @2xl:block">{issue.seen}</span>
              </div>
            ))}
          </div>
        </div>

        <div className="relative min-w-0 border-t border-line bg-ink-0/30 p-4 @5xl:border-t-0 @5xl:border-l">
          <div className="mb-3 flex items-center justify-between">
            <span className="text-[0.8125rem] font-semibold text-fg">New occurrences</span>
            <LiveLight label="streaming" />
          </div>
          {/* Absolutely placed on wide screens so the feed never sets the row's height. */}
          <LiveFeed
            rows={8}
            className="h-[22rem] [mask-image:linear-gradient(to_bottom,black_80%,transparent)] @5xl:absolute @5xl:inset-x-4 @5xl:top-[3.25rem] @5xl:bottom-0 @5xl:h-auto"
          />
        </div>
      </div>
    </MockFrame>
  );
}
