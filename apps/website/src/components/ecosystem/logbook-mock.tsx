"use client";

import { RotateCcwIcon, SearchIcon } from "lucide-react";

import { cn } from "@/lib/utils";

import { MockFrame } from "./frame";
import { logbookBatches, logbookTimeline, type LogKind } from "./mock-data";
import { usePlayOnView } from "./use-steps";

const TOTAL_MS = 24;

const barTone: Record<LogKind, string> = {
  request: "bg-fg-3/60",
  access: "bg-fg-3/50",
  query: "bg-fg-2/55",
  model: "bg-accent/45",
  event: "bg-accent/60",
  job: "bg-accent/60",
  mail: "bg-accent/60",
  cache: "bg-accent/45",
  log: "bg-fg-3/60",
  response: "bg-accent",
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
            compact ? "grid-cols-[3.5rem_minmax(0,1fr)_32%]" : "grid-cols-[3.75rem_minmax(0,1fr)] @xl:grid-cols-[4rem_minmax(0,1fr)_42%]",
          )}
        >
          <span className="font-mono text-xs text-fg-3">{row.kind}</span>
          <span className="min-w-0 truncate font-mono text-xs leading-5 text-fg-1">{row.text}</span>
          <span className={cn("relative h-2 rounded-full bg-ink-0/50", compact ? "" : "col-start-2 mt-1 @xl:col-start-3 @xl:mt-0")}>
            <span
              className={cn("absolute inset-y-0 origin-left rounded-full transition-transform duration-700 ease-(--ease-out-quint)", barTone[row.kind])}
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
        <div className="mt-2 grid grid-cols-[3.75rem_minmax(0,1fr)] gap-x-3 @xl:grid-cols-[4rem_minmax(0,1fr)_42%]">
          <span className="col-start-2 flex justify-between font-mono text-xs text-fg-3 @xl:col-start-3">
            <span>0</span>
            <span>12</span>
            <span>24 ms</span>
          </span>
        </div>
      )}
    </div>
  );
}

const filters = ["Requests", "Queries", "Jobs", "Mail", "Events", "Cache", "Logs", "Exceptions"];

/** Logbook at /_logbook: the batches on the left, one request's timeline on the right. */
export function LogbookMock({ className }: { className?: string }) {
  return (
    <MockFrame
      url="127.0.0.1:8080/_logbook"
      className={className}
      bodyClassName="@container"
      label="Logbook in development, open on the request POST /api/v1/deals: a list of recent requests and jobs on the left, one marked for duplicate queries; on the right the request's summary and its timeline, with the access check, each query, the deal.created event, the queued job and mail, the cache call, a log line and the 201 response, drawn against its 24 milliseconds."
    >
      <div className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-2.5">
        <div className="flex min-w-0 gap-1 overflow-hidden">
          {filters.map((f, i) => (
            <span
              key={f}
              className={cn(
                "shrink-0 rounded-md px-2 py-0.5 text-xs",
                i === 0 ? "bg-ink-3 text-fg" : "text-fg-3",
                i > 3 && "hidden @2xl:inline",
                i > 5 && "@2xl:hidden @4xl:inline",
              )}
            >
              {f}
            </span>
          ))}
        </div>
        <div className="ml-auto flex h-7 min-w-0 items-center gap-2 rounded-md border border-line bg-ink-0/60 px-2 font-mono text-xs text-fg-2 @xl:w-[17rem]">
          <SearchIcon className="size-3.5 shrink-0 text-fg-3" strokeWidth={1.75} />
          <span className="truncate">user:0192f0c1 route:/api/v1/deals</span>
        </div>
      </div>

      <div className="grid @3xl:grid-cols-[16rem_minmax(0,1fr)]">
        <div className="hidden border-r border-line @3xl:block">
          {logbookBatches.map((b) => (
            <div
              key={b.label}
              className={cn(
                "border-b border-line px-3.5 py-2.5 last:border-b-0",
                b.selected ? "bg-ink-2 shadow-[inset_2px_0_0_var(--color-accent)]" : "",
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className={cn("truncate font-mono text-xs", b.selected ? "text-fg" : "text-fg-1")}>
                  {b.prefix ? <span className="text-fg-3">{b.prefix} </span> : null}
                  {b.label}
                </span>
                <span className={cn("shrink-0 font-mono text-xs", b.status.startsWith("4") ? "text-amber" : "text-fg-3")}>{b.status}</span>
              </div>
              <div className="mt-0.5 flex items-center justify-between gap-2 font-mono text-xs text-fg-3">
                <span className={cn("truncate", b.warn && "text-amber")}>{b.warn ? `${b.counts} · duplicates` : b.counts}</span>
                <span className="shrink-0 tabular-nums">{b.duration}</span>
              </div>
            </div>
          ))}
        </div>

        <div className="min-w-0 p-4 @xl:p-5">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <span className="font-mono text-[0.9375rem] text-fg">POST /api/v1/deals</span>
            <span className="rounded-full border border-accent/30 bg-accent/10 px-2 font-mono text-xs leading-5 text-accent">201</span>
            <span className="font-mono text-xs text-fg-3">24 ms</span>
            <span className="ml-auto inline-flex items-center gap-1.5 rounded-md border border-line-2 px-2 py-1 text-xs text-fg-1">
              <RotateCcwIcon className="size-3" strokeWidth={1.75} />
              Replay
            </span>
          </div>
          <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs text-fg-3">
            <span>DealsController::store</span>
            <span>
              user <span className="text-fg-2">Priya Shah</span>
            </span>
            <span>
              workspace <span className="text-fg-2">acme</span>
            </span>
            <span className="hidden @xl:inline">request 01a17749…</span>
          </div>

          <div className="mt-4 flex gap-4 overflow-hidden border-b border-line text-xs whitespace-nowrap">
            {["Request", "Response", "Queries 6", "Events 2", "Jobs 1", "Mail 1", "Timeline"].map((t) => (
              <span
                key={t}
                className={cn(
                  "-mb-px border-b pb-2",
                  t === "Timeline" ? "border-accent text-fg" : "border-transparent text-fg-3",
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
    </MockFrame>
  );
}
