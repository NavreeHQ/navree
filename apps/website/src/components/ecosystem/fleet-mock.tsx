"use client";

import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import { Bars } from "./charts";
import { Figure, LiveLight, MockFrame } from "./frame";
import { fleetFailedJobs, fleetMailPerMinute, fleetSteps } from "./mock-data";
import { useSteps } from "./use-steps";

const SLOTS = 8;

/** A queue's processes as lit slots: green while running, outlined while free. */
export function ProcessSlots({ count, capacity = SLOTS, className }: { count: number; capacity?: number; className?: string }) {
  return (
    <div className={cn("grid grid-cols-8 gap-1", className)}>
      {Array.from({ length: SLOTS }, (_, i) => (
        <span
          key={i}
          className={cn(
            "h-3.5 rounded-[3px] border transition-[background-color,border-color,box-shadow] duration-500 ease-(--ease-out-quint)",
            i >= capacity
              ? "border-transparent"
              : i < count
                ? "border-accent/60 bg-accent/85 shadow-[0_0_10px_rgb(61_255_143/0.35)]"
                : "border-line-2 bg-ink-0/40",
          )}
        />
      ))}
    </div>
  );
}

type Row = { name: string; processes: number; capacity?: number; pending: string; wait: string; clears: string };

function WorkloadRows({ rows, compact }: { rows: Row[]; compact?: boolean }) {
  return rows.map((q) => (
    <div
      key={q.name}
      className={cn(
        "grid items-center gap-3 border-b border-line px-3.5 py-2.5 last:border-b-0",
        compact
          ? "grid-cols-[7rem_minmax(0,11rem)_minmax(3.25rem,1fr)]"
          : "grid-cols-[7rem_minmax(0,1fr)_3.5rem] @lg:grid-cols-[7.5rem_minmax(0,9rem)_4rem_3rem_3.5rem] @lg:justify-between",
      )}
    >
      <span className="truncate font-mono text-[0.8125rem] text-fg-1">{q.name}</span>
      <ProcessSlots count={q.processes} capacity={q.capacity} />
      <span className="text-right font-mono text-[0.8125rem] text-fg-1 tabular-nums">{q.pending}</span>
      {compact ? null : (
        <>
          <span className="hidden text-right font-mono text-xs text-fg-2 tabular-nums @lg:block">{q.wait}</span>
          <span className="hidden text-right font-mono text-xs text-fg-3 @lg:block">{q.clears}</span>
        </>
      )}
    </div>
  ));
}

function GroupLabel({ children }: { children: ReactNode }) {
  return <div className="border-b border-line bg-ink-0/40 px-3.5 py-1.5 font-mono text-xs text-fg-3">{children}</div>;
}

/** The balancing example, step by step: shared by the full mock and the homepage tile. */
export function useFleetSteps() {
  return useSteps(fleetSteps.length, {
    interval: (s) => (s === 5 ? 2400 : 1500),
    hold: 2800,
    still: 5,
  });
}

/** Just the workload table of the `default` supervisor, for small tiles. */
export function FleetWorkloadCompact({ step }: { step: number }) {
  const state = fleetSteps[step]!;
  const rows: Row[] = (["default", "mail", "notifications"] as const).map((name) => ({ name, ...state.queues[name] }));
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-ink-1">
      <GroupLabel>default · auto · 1–8 processes</GroupLabel>
      <WorkloadRows rows={rows} compact />
    </div>
  );
}

/** The Fleet dashboard at /_fleet: figures, the workload table, mail throughput and failed jobs. */
export function FleetMock({ className }: { className?: string }) {
  const { ref, step } = useFleetSteps();
  const state = fleetSteps[step]!;
  const defaultRows: Row[] = (["default", "mail", "notifications"] as const).map((name) => ({ name, ...state.queues[name] }));
  const exportsRow: Row = { name: "exports", processes: 2, capacity: 2, pending: "2", wait: "6 s", clears: "1 min" };

  return (
    <MockFrame
      url="crm.example.com/_fleet"
      status={<LiveLight label="Running" />}
      className={className}
      bodyClassName="@container"
      label="The Fleet dashboard of the example CRM: two supervisors running queue workers. When a newsletter puts 3,000 mails on the mail queue, Fleet moves processes to it, from one up to six, while the default and notifications queues keep what they need; below, mail jobs per minute spike at 09:00, and a list of failed jobs offers Retry and Forget."
    >
      <div ref={ref} className="p-4 @xl:p-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2.5">
            <span className="text-[0.9375rem] font-semibold text-fg">Overview</span>
            <span className="hidden truncate font-mono text-xs text-fg-3 @md:inline">2 masters · worker-1, worker-2</span>
          </div>
          <span className="rounded-md border border-line px-1.5 font-mono text-xs leading-5 text-fg-2 tabular-nums">{state.time}</span>
        </div>

        <div className="mt-4 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line @2xl:grid-cols-4">
          <div className="bg-ink-1 p-3">
            <Figure label="Jobs / min" value={state.jobsPerMin} sub="all queues" />
          </div>
          <div className="bg-ink-1 p-3">
            <Figure label="Jobs past hour" value="28,410" sub="2 supervisors" />
          </div>
          <div className="bg-ink-1 p-3">
            <Figure label="Failed, 7 days" value="3" sub="retry or forget" />
          </div>
          <div className="bg-ink-1 p-3">
            <Figure label="Longest wait" value={state.maxWait} sub={`queue ${state.maxWaitQueue}`} />
          </div>
        </div>

        <div className="mt-3 overflow-hidden rounded-xl border border-line bg-ink-1">
          <div className="grid grid-cols-[7rem_minmax(0,1fr)_3.5rem] gap-3 border-b border-line px-3.5 py-2 text-xs text-fg-3 @lg:grid-cols-[7.5rem_minmax(0,9rem)_4rem_3rem_3.5rem] @lg:justify-between">
            <span>Queue</span>
            <span>Processes</span>
            <span className="text-right">Pending</span>
            <span className="hidden text-right @lg:block">Wait</span>
            <span className="hidden text-right @lg:block">Clears in</span>
          </div>
          <GroupLabel>default · auto · 1–8 processes</GroupLabel>
          <WorkloadRows rows={defaultRows} />
          <div className="border-t border-line">
            <GroupLabel>exports · off · 2 processes</GroupLabel>
            <WorkloadRows rows={[exportsRow]} />
          </div>
        </div>

        <div className="mt-3 grid gap-3 @3xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
          <div className="flex flex-col rounded-xl border border-line bg-ink-1 p-3.5">
            <div className="flex items-center justify-between text-xs">
              <span className="text-fg-2">mail · jobs per minute</span>
              <span className="font-mono text-fg-3">30 min</span>
            </div>
            <Bars data={fleetMailPerMinute} accentFrom={25} className="mt-3 h-20 w-full flex-1" />
            <div className="mt-1.5 flex justify-between font-mono text-xs text-fg-3">
              <span>08:35</span>
              <span className="text-fg-2">09:00 newsletter</span>
            </div>
          </div>
          <div className="overflow-hidden rounded-xl border border-line bg-ink-1">
            <div className="flex items-center justify-between border-b border-line px-3.5 py-2 text-xs">
              <span className="text-fg-2">Failed jobs</span>
              <span className="rounded-md border border-line-2 px-2 py-0.5 text-fg-1">Retry all</span>
            </div>
            {fleetFailedJobs.map((job) => (
              <div key={job.type} className="flex items-center gap-3 border-b border-line px-3.5 py-2 last:border-b-0">
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    <span className="truncate font-mono text-[0.8125rem] text-fg-1">{job.type}</span>
                    <span className="shrink-0 font-mono text-xs text-fg-3">{job.attempts}</span>
                  </div>
                  <div className="truncate font-mono text-xs leading-5 text-danger/90">{job.error}</div>
                </div>
                <span className="hidden shrink-0 rounded-md bg-ink-3 px-2 py-0.5 text-xs text-fg @sm:inline">Retry</span>
                <span className="hidden shrink-0 px-1 text-xs text-fg-3 @sm:inline">Forget</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </MockFrame>
  );
}
