"use client";

import {
  Building2Icon,
  CheckIcon,
  ChevronDownIcon,
  HandshakeIcon,
  KanbanSquareIcon,
  LayoutDashboardIcon,
  SearchIcon,
  UserIcon,
  UsersIcon,
  XIcon,
  type LucideIcon,
} from "lucide-react";

import { cn } from "@/lib/utils";

import { Sparkline } from "./charts";
import { MockFrame } from "./frame";
import { helmDeals, helmNewDeals, helmStages, type Stage } from "./mock-data";
import { useSteps } from "./use-steps";

/** Steps: idle, three rows ticked one by one, the confirm dialog, done. */
const STEPS = 6;
const picked = helmDeals.flatMap((d, i) => (d.pick ? [i] : []));

export function useHelmSteps() {
  return useSteps(STEPS, { interval: (s) => (s === 0 ? 1000 : s === 4 ? 1500 : 750), hold: 3200, still: 3 });
}

export function Checkbox({ checked }: { checked: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex size-4 shrink-0 items-center justify-center rounded-[5px] border transition-colors duration-200",
        checked ? "border-accent bg-accent text-accent-ink" : "border-line-2 bg-ink-0/40",
      )}
    >
      <CheckIcon className={cn("size-3 transition-opacity", checked ? "opacity-100" : "opacity-0")} strokeWidth={3} />
    </span>
  );
}

export function StageBadge({ stage }: { stage: Stage }) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full border px-2 text-xs leading-5 whitespace-nowrap transition-colors duration-300",
        stage === "Won" ? "border-accent/30 bg-accent/10 text-accent" : stage === "Lost" ? "border-line text-fg-3" : "border-line-2 bg-ink-2 text-fg-1",
      )}
    >
      {stage}
    </span>
  );
}

function SideItem({ icon: Icon, label, active }: { icon: LucideIcon; label: string; active?: boolean }) {
  return (
    <div className={cn("flex h-8 items-center gap-2.5 rounded-lg px-2.5 text-[0.8125rem]", active ? "bg-ink-3 text-fg" : "text-fg-2")}>
      <Icon className={cn("size-4", active ? "text-accent" : "text-fg-3")} strokeWidth={1.5} />
      {label}
    </div>
  );
}

function StageBar() {
  const total = helmStages.reduce((n, s) => n + s.deals, 0);
  const shades: Record<Stage, string> = {
    Lead: "bg-line-2",
    Qualified: "bg-fg-3/45",
    Proposal: "bg-fg-3/70",
    Negotiation: "bg-fg-2/85",
    Won: "bg-accent",
    Lost: "bg-ink-3",
  };
  return (
    <>
      <div className="mt-2.5 flex h-2 gap-0.5 overflow-hidden rounded-full">
        {helmStages.map((s) => (
          <span key={s.stage} className={shades[s.stage]} style={{ width: `${(s.deals / total) * 100}%` }} />
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-2.5 gap-y-0.5 text-xs text-fg-3">
        {helmStages.slice(2, 5).map((s) => (
          <span key={s.stage} className="inline-flex items-center gap-1">
            <span className={cn("size-1.5 rounded-full", shades[s.stage])} />
            {s.stage} {s.deals}
          </span>
        ))}
      </div>
    </>
  );
}

/** Helm at /admin: the Deals resource with metrics, filters, a table and a bulk action. */
export function HelmMock({ className }: { className?: string }) {
  const { ref, step } = useHelmSteps();
  const ticked = (i: number) => {
    const order = picked.indexOf(i);
    return order >= 0 && step >= order + 1 && step <= 4;
  };
  const done = step >= 5;
  const count = step >= 1 && step <= 4 ? Math.min(step, picked.length) : 0;

  return (
    <MockFrame
      url="crm.example.com/admin/resources/deals"
      className={className}
      bodyClassName="@container"
      label="Helm, the admin panel of the example CRM, on the Deals resource: metric cards for the open pipeline, new deals and deals by stage; search and filters for stage and owner; a table of deals with amount, stage, company, owner and expected close date. Three deals are ticked and the bulk action Mark as won runs on them after a confirmation."
    >
      <div ref={ref} className="grid min-h-full @4xl:grid-cols-[12rem_minmax(0,1fr)]">
        <aside className="hidden flex-col gap-0.5 border-r border-line bg-ink-0/40 p-3 @4xl:flex">
          <div className="mb-3 flex items-center gap-2 px-2.5 pt-1">
            <span className="flex size-5 items-center justify-center rounded-md bg-accent font-display text-xs font-bold text-accent-ink">A</span>
            <span className="text-[0.8125rem] font-semibold text-fg">Acme CRM</span>
          </div>
          <SideItem icon={LayoutDashboardIcon} label="Dashboard" />
          <div className="mt-3 mb-1 px-2.5 text-xs text-fg-3">Sales</div>
          <SideItem icon={HandshakeIcon} label="Deals" active />
          <SideItem icon={Building2Icon} label="Companies" />
          <SideItem icon={UsersIcon} label="Contacts" />
          <div className="mt-3 mb-1 px-2.5 text-xs text-fg-3">Pages</div>
          <SideItem icon={KanbanSquareIcon} label="Pipeline board" />
          <div className="mt-3 mb-1 px-2.5 text-xs text-fg-3">Team</div>
          <SideItem icon={UserIcon} label="Users" />
          <div className="mt-auto border-t border-line px-2.5 pt-3 text-xs leading-5">
            <div className="text-fg-1">Marcus Reyes</div>
            <div className="text-fg-3">manager</div>
          </div>
        </aside>

        <div className="relative min-w-0 p-4 @xl:p-5">
          <div className="text-xs text-fg-3">Sales / Deals</div>
          <div className="mt-1 flex items-center justify-between gap-3">
            <span className="text-lg font-semibold tracking-[-0.01em] text-fg">Deals</span>
            <span className="rounded-lg bg-accent px-3 py-1.5 text-xs font-semibold text-accent-ink">Create deal</span>
          </div>

          <div className="mt-4 grid gap-3 @2xl:grid-cols-3">
            <div className="rounded-xl border border-line bg-ink-1 p-3.5">
              <div className="text-xs text-fg-3">Open pipeline</div>
              <div className="mt-1 font-display text-[1.375rem] leading-tight font-semibold tracking-[-0.02em] text-fg tabular-nums">$1,284,500</div>
              <div className="mt-0.5 text-xs text-fg-3">
                <span className="text-accent">▲ 12.4%</span> on the previous 30 days
              </div>
            </div>
            <div className="hidden rounded-xl border border-line bg-ink-1 p-3.5 @md:block">
              <div className="flex items-baseline justify-between">
                <span className="text-xs text-fg-3">New deals</span>
                <span className="font-mono text-xs text-fg-3">30 days</span>
              </div>
              <div className="mt-1 font-display text-[1.375rem] leading-tight font-semibold tracking-[-0.02em] text-fg tabular-nums">164</div>
              <Sparkline data={helmNewDeals} area className="mt-1 h-6 w-full text-accent/70" />
            </div>
            <div className="hidden rounded-xl border border-line bg-ink-1 p-3.5 @2xl:block">
              <div className="text-xs text-fg-3">Deals by stage</div>
              <StageBar />
            </div>
          </div>

          <div className="mt-4 flex flex-wrap items-center gap-2">
            <div className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg border border-line-2 bg-ink-0/50 px-2.5 text-xs text-fg-3 @lg:max-w-[15rem]">
              <SearchIcon className="size-3.5 shrink-0" strokeWidth={1.75} />
              <span className="truncate">Search deals</span>
            </div>
            <span className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-line bg-ink-2 px-2.5 text-xs text-fg-1">
              Open deals only
              <XIcon className="size-3 text-fg-3" strokeWidth={1.75} />
            </span>
            <span className="hidden h-8 items-center gap-1.5 rounded-lg border border-line bg-ink-2 px-2.5 text-xs text-fg-1 @xl:inline-flex">
              Close: next 90 days
              <XIcon className="size-3 text-fg-3" strokeWidth={1.75} />
            </span>
          </div>

          {/* The bulk action bar keeps its height whether rows are ticked or not. */}
          <div className="mt-3 flex h-9 items-center gap-2 rounded-lg border border-line bg-ink-1 px-2.5 text-xs">
            {count > 0 ? (
              <>
                <span className="font-medium text-fg tabular-nums">{count} selected</span>
                <span className="ml-auto inline-flex h-6 items-center gap-1.5 rounded-md border border-line-2 bg-ink-2 px-2 text-fg-1">
                  Mark as won
                  <ChevronDownIcon className="size-3 text-fg-3" strokeWidth={1.75} />
                </span>
                <span className={cn("inline-flex h-6 items-center rounded-md px-2.5 font-semibold transition-colors", step >= 4 ? "bg-accent text-accent-ink" : "bg-ink-3 text-fg")}>Run</span>
              </>
            ) : (
              <span className="text-fg-3">6 of 10 deals · sorted by expected close</span>
            )}
          </div>

          <div className="mt-2 overflow-hidden rounded-xl border border-line bg-ink-1">
            <div className="grid grid-cols-[1rem_minmax(0,1fr)_6.25rem] items-center gap-3 border-b border-line px-3 py-2 text-xs text-fg-3 @xl:grid-cols-[1rem_minmax(0,1fr)_5rem_6.25rem] @3xl:grid-cols-[1rem_minmax(0,1fr)_5rem_6.25rem_6.5rem_3.5rem] @5xl:grid-cols-[1rem_minmax(0,1fr)_5rem_6.25rem_9.5rem_6.5rem_3.5rem_2.75rem]">
              <Checkbox checked={false} />
              <span>Title</span>
              <span className="hidden text-right @xl:block">Amount</span>
              <span>Stage</span>
              <span className="hidden @5xl:block">Company</span>
              <span className="hidden @3xl:block">Owner</span>
              <span className="hidden @3xl:block">Close</span>
              <span className="hidden text-right @5xl:block">Prob.</span>
            </div>
            {helmDeals.map((deal, i) => {
              const isTicked = ticked(i);
              const stage: Stage = done && deal.pick ? "Won" : deal.stage;
              return (
                <div
                  key={deal.title}
                  className={cn(
                    "grid grid-cols-[1rem_minmax(0,1fr)_6.25rem] items-center gap-3 border-b border-line px-3 py-2.5 transition-colors duration-200 last:border-b-0 @xl:grid-cols-[1rem_minmax(0,1fr)_5rem_6.25rem] @3xl:grid-cols-[1rem_minmax(0,1fr)_5rem_6.25rem_6.5rem_3.5rem] @5xl:grid-cols-[1rem_minmax(0,1fr)_5rem_6.25rem_9.5rem_6.5rem_3.5rem_2.75rem]",
                    isTicked && "bg-accent/[0.05]",
                  )}
                >
                  <Checkbox checked={isTicked} />
                  <div className="min-w-0">
                    <div className="truncate text-[0.8125rem] text-fg">{deal.title}</div>
                    <div className="truncate text-xs text-fg-3 @5xl:hidden">{deal.company}</div>
                  </div>
                  <span className="hidden text-right font-mono text-[0.8125rem] text-fg-1 tabular-nums @xl:block">{deal.amount}</span>
                  <span>
                    <StageBadge stage={stage} />
                  </span>
                  <span className="hidden truncate text-[0.8125rem] text-fg-2 @5xl:block">{deal.company}</span>
                  <span className="hidden truncate text-[0.8125rem] text-fg-2 @3xl:block">{deal.owner}</span>
                  <span className="hidden font-mono text-xs text-fg-3 @3xl:block">{deal.close}</span>
                  <span className="hidden text-right font-mono text-xs text-fg-2 tabular-nums @5xl:block">{done && deal.pick ? "100%" : deal.probability}</span>
                </div>
              );
            })}
          </div>

          {/* The confirmation, over the panel. */}
          <div
            className={cn(
              "absolute inset-0 flex items-center justify-center bg-ink-0/55 p-4 transition-opacity duration-300",
              step === 4 ? "opacity-100" : "pointer-events-none opacity-0",
            )}
          >
            <div
              className={cn(
                "w-full max-w-[19rem] rounded-xl border border-line-2 bg-ink-2 p-4 shadow-float transition-transform duration-300 ease-(--ease-out-quint)",
                step === 4 ? "scale-100" : "scale-[0.97]",
              )}
            >
              <div className="text-[0.875rem] font-semibold text-fg">Mark as won</div>
              <div className="mt-1 text-[0.8125rem] text-fg-2">Mark the selected deals as won?</div>
              <div className="mt-4 flex justify-end gap-2 text-xs">
                <span className="rounded-md px-2.5 py-1.5 text-fg-2">Cancel</span>
                <span className="rounded-md bg-accent px-2.5 py-1.5 font-semibold text-accent-ink">Mark as won</span>
              </div>
            </div>
          </div>

          <div
            className={cn(
              "absolute right-4 bottom-4 flex items-center gap-2 rounded-lg border border-line-2 bg-ink-3 px-3 py-2 text-xs text-fg shadow-float transition-[opacity,translate] duration-300 ease-(--ease-out-quint) @xl:right-5 @xl:bottom-5",
              done ? "translate-y-0 opacity-100" : "translate-y-1 opacity-0",
            )}
          >
            <CheckIcon className="size-3.5 text-accent" strokeWidth={2.25} />3 deals marked as won.
          </div>
        </div>
      </div>
    </MockFrame>
  );
}
