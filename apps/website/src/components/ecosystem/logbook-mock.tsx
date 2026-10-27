"use client";

import {
  ArrowLeftRightIcon,
  DatabaseIcon,
  LayersIcon,
  ListChecksIcon,
  MailIcon,
  NotebookTextIcon,
  RotateCcwIcon,
  ScrollTextIcon,
  TriangleAlertIcon,
  ZapIcon,
} from "lucide-react";

import { cn } from "@/lib/utils";

import { AppShell, Badge, Button, SearchField, TopBar, type SidebarSpec } from "./app-shell";
import { MockFrame } from "./frame";
import { logbookBatches, logbookTimeline, type LogKind } from "./mock-data";
import { usePlayOnView } from "./use-steps";

const TOTAL_MS = 24;

/** Bars in ink steps: the request's own work light, what it hands off brighter, the response brightest. */
const barTone: Record<LogKind, string> = {
  request: "bg-fg-3/50",
  access: "bg-fg-3/50",
  query: "bg-fg-3/80",
  model: "bg-fg-2/60",
  event: "bg-fg-2",
  job: "bg-fg-2",
  mail: "bg-fg-2",
  cache: "bg-fg-2/60",
  log: "bg-fg-3/50",
  response: "bg-fg",
};

/**
 * The timeline of one request: every query, event, job, mail, cache call and
 * log line, drawn against the request's 24 ms. Bars grow in once when the
 * timeline scrolls into view.
 */
export function Waterfall({
  rows = logbookTimeline,
  compact = false,
  animate = true,
}: {
  rows?: typeof logbookTimeline;
  compact?: boolean;
  /** Grow the bars in when the timeline scrolls into view. */
  animate?: boolean;
}) {
  const view = usePlayOnView();
  const { ref } = view;
  const armed = animate && view.armed;
  const play = animate && view.play;
  return (
    <div ref={ref} className="@container">
      {rows.map((row, i) => (
        <div
          key={`${row.kind}-${row.start}`}
          className={cn(
            "grid items-center gap-x-3 border-b border-line/70 py-[0.3125rem] last:border-b-0",
            compact ? "grid-cols-[3.5rem_minmax(0,1fr)_32%]" : "grid-cols-[3.75rem_minmax(0,1fr)] @lg:grid-cols-[4rem_minmax(0,1fr)_40%]",
          )}
        >
          <span className="font-mono text-xs text-fg-3">{row.kind}</span>
          <span className="min-w-0 truncate font-mono text-xs leading-5 text-fg-1">{row.text}</span>
          <span className={cn("relative h-2 rounded-[2px] bg-ink-2", compact ? "" : "col-start-2 mt-1 @lg:col-start-3 @lg:mt-0")}>
            <span
              className={cn("absolute inset-y-0 origin-left rounded-[2px] transition-transform duration-700 ease-(--ease-out-quint)", barTone[row.kind])}
              style={{
                left: `${(row.start / TOTAL_MS) * 100}%`,
                width: `${Math.max(compact ? 4 : 1.6, (row.duration / TOTAL_MS) * 100)}%`,
                transform: armed && !play ? "scaleX(0)" : "scaleX(1)",
                transitionDelay: play ? `${120 + i * 70}ms` : "0ms",
              }}
            />
          </span>
        </div>
      ))}
      {compact ? null : (
        <div className="mt-2 grid grid-cols-[3.75rem_minmax(0,1fr)] gap-x-3 @lg:grid-cols-[4rem_minmax(0,1fr)_40%]">
          <span className="col-start-2 flex justify-between text-xs text-fg-3 tabular-nums @lg:col-start-3">
            <span>0</span>
            <span>12</span>
            <span>24 ms</span>
          </span>
        </div>
      )}
    </div>
  );
}

const sidebar: SidebarSpec = {
  app: { logo: <NotebookTextIcon strokeWidth={2} />, name: "Logbook", context: "crm-api · local" },
  groups: [
    {
      label: "Entries",
      items: [
        { icon: ArrowLeftRightIcon, label: "Requests", badge: "128", active: true },
        { icon: DatabaseIcon, label: "Queries", badge: "1.2k" },
        { icon: ListChecksIcon, label: "Jobs", badge: "14" },
        { icon: MailIcon, label: "Mail", badge: "6" },
        { icon: ZapIcon, label: "Events", badge: "42" },
        { icon: LayersIcon, label: "Cache", badge: "88" },
        { icon: ScrollTextIcon, label: "Logs", badge: "310" },
        { icon: TriangleAlertIcon, label: "Exceptions", badge: "2" },
      ],
    },
  ],
  user: { initials: "PS", name: "Priya Shah", email: "priya@acme.test" },
};

const tabs = ["Request", "Response", "Queries 6", "Events 2", "Jobs 1", "Mail 1", "Timeline"];

/** Logbook at /_logbook: the batches on the left, one request's timeline on the right. */
export function LogbookMock({ className }: { className?: string }) {
  return (
    <MockFrame
      url="127.0.0.1:8080/_logbook"
      className={className}
      bodyClassName="@container"
      label="Logbook in development, open on the request POST /api/v1/deals: a list of recent requests and jobs on the left, one marked for duplicate queries; on the right the request's summary and its timeline, with the access check, each query, the deal.created event, the queued job and mail, the cache call, a log line and the 201 response, drawn against its 24 milliseconds."
    >
      <AppShell sidebar={sidebar}>
        <TopBar crumbs={["Requests", "POST /api/v1/deals"]}>
          <SearchField mono placeholder="user:0192f0c1 route:/api/v1/deals" className="hidden w-72 @3xl:flex" />
        </TopBar>

        <div className="grid flex-1 @2xl:grid-cols-[15rem_minmax(0,1fr)]">
          <div className="hidden border-r border-line bg-ink-0 @2xl:block">
            {logbookBatches.map((b) => (
              <div key={b.label} className={cn("border-b border-line px-3 py-2.5", b.selected && "bg-ink-2")}>
                <div className="flex items-center justify-between gap-2">
                  <span className={cn("truncate font-mono text-xs", b.selected ? "text-fg" : "text-fg-1")}>
                    {b.prefix ? <span className="text-fg-3">{b.prefix} </span> : null}
                    {b.label}
                  </span>
                  <span className={cn("shrink-0 font-mono text-xs", b.status.startsWith("4") ? "text-amber" : "text-fg-3")}>{b.status}</span>
                </div>
                <div className="mt-0.5 flex items-center justify-between gap-2 text-xs text-fg-3">
                  <span className={cn("truncate", b.warn && "text-amber")}>{b.warn ? `${b.counts} · duplicates` : b.counts}</span>
                  <span className="shrink-0 tabular-nums">{b.duration}</span>
                </div>
              </div>
            ))}
          </div>

          <div className="min-w-0 p-4 @xl:p-5">
            {/* Baseline-aligned: the mono path, the status and the duration share one baseline. */}
            <div className="flex items-baseline gap-2.5">
              <span className="min-w-0 truncate font-mono text-[0.9375rem] font-medium text-fg">POST /api/v1/deals</span>
              <Badge tone="positive" className="font-mono">
                201
              </Badge>
              <span className="shrink-0 text-[0.8125rem] text-fg-3 tabular-nums">24 ms</span>
              {/* Icon only on narrow frames, so the title row never wraps. */}
              <Button size="sm" icon={RotateCcwIcon} className="ml-auto self-center @max-md:w-7 @max-md:px-0">
                <span className="hidden @md:inline">Replay</span>
              </Button>
            </div>
            <div className="mt-2 flex flex-wrap items-baseline gap-x-4 gap-y-1 text-[0.8125rem] text-fg-3">
              <span className="font-mono text-xs text-fg-2">DealsController::store</span>
              <span>
                User <span className="text-fg-1">Priya Shah</span>
              </span>
              <span>
                Workspace <span className="text-fg-1">acme</span>
              </span>
              <span className="hidden font-mono text-xs @xl:inline">01a17749…</span>
            </div>

            <div className="mt-4 flex gap-4 overflow-hidden border-b border-line text-[0.8125rem] whitespace-nowrap">
              {tabs.map((t) => (
                <span
                  key={t}
                  className={cn(
                    "-mb-px border-b pb-2",
                    t === "Timeline" ? "border-fg font-medium text-fg" : "border-transparent text-fg-3",
                    (t === "Request" || t === "Response") && "hidden @2xl:inline",
                  )}
                >
                  {t}
                </span>
              ))}
            </div>

            <div className="mt-3">
              <Waterfall />
            </div>
          </div>
        </div>
      </AppShell>
    </MockFrame>
  );
}
