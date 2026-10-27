"use client";

import { ChartLineIcon, GaugeIcon, LayersIcon, ListChecksIcon, OctagonXIcon, PauseIcon, ServerIcon, SettingsIcon, ShipIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import { AppShell, Badge, Button, Card, Stat, StatusDot, TopBar, type SidebarSpec } from "./app-shell";
import { Bars } from "./charts";
import { MockFrame } from "./frame";
import { fleetFailedJobs, fleetMailPerMinute, fleetSteps } from "./mock-data";
import { useSteps } from "./use-steps";

const SLOTS = 8;

/** A queue's processes as slots: filled in fg while running, outlined while free. */
export function ProcessSlots({ count, capacity = SLOTS, className }: { count: number; capacity?: number; className?: string }) {
  return (
    <div className={cn("grid grid-cols-8 gap-1", className)}>
      {Array.from({ length: SLOTS }, (_, i) => (
        <span
          key={i}
          className={cn(
            "h-3.5 rounded-[2px] border transition-[background-color,border-color] duration-500 ease-(--ease-out-quint)",
            i >= capacity ? "border-transparent" : i < count ? "border-fg-2 bg-fg-2" : "border-line-2 bg-ink-0",
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
        // Baseline-aligned, so the mono queue name and the numbers share a baseline; padded to 40px.
        "grid h-10 items-baseline gap-3 border-b border-line px-3 py-2.5 leading-5 last:border-b-0",
        compact
          ? "grid-cols-[7.5rem_minmax(0,11rem)_minmax(3.25rem,1fr)]"
          : "grid-cols-[7.25rem_minmax(0,1fr)_3.5rem] @lg:grid-cols-[7.5rem_minmax(0,9rem)_4rem_3rem_3.5rem] @lg:justify-between",
      )}
    >
      <span className="truncate font-mono text-[0.8125rem] text-fg">{q.name}</span>
      <ProcessSlots count={q.processes} capacity={q.capacity} className="self-center" />
      <span className="text-right text-sm text-fg-1 tabular-nums">{q.pending}</span>
      {compact ? null : (
        <>
          <span className="hidden text-right text-[0.8125rem] text-fg-2 tabular-nums @lg:block">{q.wait}</span>
          <span className="hidden text-right text-[0.8125rem] text-fg-3 @lg:block">{q.clears}</span>
        </>
      )}
    </div>
  ));
}

function GroupLabel({ children }: { children: ReactNode }) {
  return <div className="flex h-8 items-center border-b border-line bg-ink-1 px-3 font-mono text-xs text-fg-3">{children}</div>;
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
    <div className="overflow-hidden rounded-md border border-line bg-ink-0">
      <GroupLabel>default · auto · 1–8 processes</GroupLabel>
      <WorkloadRows rows={rows} compact />
    </div>
  );
}

const sidebar: SidebarSpec = {
  app: { logo: <ShipIcon strokeWidth={2} />, name: "Fleet", context: "crm-api · 2 supervisors" },
  groups: [
    {
      label: "Queues",
      items: [
        { icon: GaugeIcon, label: "Overview", active: true },
        { icon: LayersIcon, label: "Workload" },
        { icon: ServerIcon, label: "Supervisors", badge: "2" },
      ],
    },
    {
      label: "Jobs",
      items: [
        { icon: ListChecksIcon, label: "Recent" },
        { icon: OctagonXIcon, label: "Failed", badge: "3" },
        { icon: ChartLineIcon, label: "Metrics" },
      ],
    },
    { items: [{ icon: SettingsIcon, label: "Settings" }] },
  ],
  user: { initials: "MR", name: "Marcus Reyes", email: "marcus@acme.test" },
};

/** The Fleet dashboard at /_fleet: figures, the workload table, mail throughput and failed jobs. */
export function FleetMock({ className }: { className?: string }) {
  const { ref, step } = useFleetSteps();
  const state = fleetSteps[step]!;
  const defaultRows: Row[] = (["default", "mail", "notifications"] as const).map((name) => ({ name, ...state.queues[name] }));
  const exportsRow: Row = { name: "exports", processes: 2, capacity: 2, pending: "2", wait: "6 s", clears: "1 min" };

  return (
    <MockFrame
      url="crm.example.com/_fleet"
      className={className}
      bodyClassName="@container"
      label="The Fleet dashboard of the example CRM: two supervisors running queue workers. When a newsletter puts 3,000 mails on the mail queue, Fleet moves processes to it, from one up to six, while the default and notifications queues keep what they need; below, mail jobs per minute spike at 09:00, and a list of failed jobs offers Retry and Forget."
    >
      <div ref={ref} className="h-full">
        <AppShell sidebar={sidebar}>
          <TopBar crumbs={["Fleet", "Overview"]}>
            <span className="hidden items-center gap-2 text-[0.8125rem] text-fg-2 @md:flex">
              <StatusDot />
              Running
            </span>
            <Badge className="font-mono font-normal tabular-nums">{state.time}</Badge>
            <Button icon={PauseIcon} size="sm" className="hidden @xl:inline-flex">
              Pause
            </Button>
          </TopBar>

          <div className="p-4 @xl:p-6">
            <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
              <div className="text-xl font-semibold tracking-[-0.01em] text-fg">Overview</div>
              <div className="hidden truncate text-[0.8125rem] text-fg-3 @md:block">
                2 masters · <span className="font-mono text-xs text-fg-2">worker-1</span>, <span className="font-mono text-xs text-fg-2">worker-2</span>
              </div>
            </div>

            <div className="mt-5 grid grid-cols-2 gap-px overflow-hidden rounded-md border border-line bg-line @2xl:grid-cols-4">
              <div className="bg-ink-1 p-4">
                <Stat label="Jobs / min" value={state.jobsPerMin} note="All queues" />
              </div>
              <div className="bg-ink-1 p-4">
                <Stat label="Jobs past hour" value="28,410" note="2 supervisors" />
              </div>
              <div className="bg-ink-1 p-4">
                <Stat label="Failed, 7 days" value="3" note="Retry or forget" />
              </div>
              <div className="bg-ink-1 p-4">
                <Stat label="Longest wait" value={state.maxWait} note={`Queue ${state.maxWaitQueue}`} />
              </div>
            </div>

            <div className="mt-3 overflow-hidden rounded-md border border-line">
              <div className="grid h-10 grid-cols-[7.25rem_minmax(0,1fr)_3.5rem] items-center gap-3 border-b border-line bg-ink-1 px-3 text-[0.8125rem] font-medium text-fg-2 @lg:grid-cols-[7.5rem_minmax(0,9rem)_4rem_3rem_3.5rem] @lg:justify-between">
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
              <Card className="flex flex-col p-4">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-sm font-medium text-fg">
                    <span className="font-mono text-[0.8125rem]">mail</span> jobs per minute
                  </span>
                  <span className="text-xs text-fg-3">30 min</span>
                </div>
                <Bars data={fleetMailPerMinute} accentFrom={25} accentClassName="fill-fg-2" className="mt-4 h-20 w-full @3xl:h-auto @3xl:flex-1" />
                <div className="mt-1.5 flex justify-between text-xs text-fg-3 tabular-nums">
                  <span>08:35</span>
                  <span className="text-fg-2">09:00 newsletter</span>
                </div>
              </Card>
              <div className="overflow-hidden rounded-md border border-line bg-ink-1">
                <div className="flex h-11 items-center justify-between border-b border-line px-3">
                  <span className="text-sm font-medium text-fg">Failed jobs</span>
                  <Button size="sm">Retry all</Button>
                </div>
                {fleetFailedJobs.map((job) => (
                  <div key={job.type} className="flex items-center gap-3 border-b border-line bg-ink-0 px-3 py-2 last:border-b-0">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <span className="truncate font-mono text-[0.8125rem] text-fg">{job.type}</span>
                        <span className="shrink-0 text-xs text-fg-3 tabular-nums">{job.attempts}</span>
                      </div>
                      <div className="truncate font-mono text-xs leading-5 text-danger/90">{job.error}</div>
                    </div>
                    <span className="hidden gap-1 @sm:flex">
                      <Button size="sm" tone="secondary">
                        Retry
                      </Button>
                      <Button size="sm" tone="ghost">
                        Forget
                      </Button>
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </AppShell>
      </div>
    </MockFrame>
  );
}
