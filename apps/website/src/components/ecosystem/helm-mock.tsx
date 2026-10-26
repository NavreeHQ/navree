"use client";

import {
  BellIcon,
  Building2Icon,
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  CircleCheckIcon,
  CirclePlusIcon,
  ContactIcon,
  DownloadIcon,
  GalleryVerticalEndIcon,
  HandshakeIcon,
  KanbanSquareIcon,
  LayoutDashboardIcon,
  PlusIcon,
  SearchIcon,
  Settings2Icon,
  SettingsIcon,
  TrendingUpIcon,
  UsersIcon,
  XIcon,
} from "lucide-react";

import { cn } from "@/lib/utils";

import { AppShell, Badge, Button, Card, Checkbox, SearchField, Stat, TopBar, type SidebarSpec } from "./app-shell";
import { Sparkline } from "./charts";
import { MockFrame } from "./frame";
import { helmDeals, helmNewDeals, helmStages, type Stage } from "./mock-data";
import { useSteps } from "./use-steps";

export { Checkbox };

/** Steps: idle, three rows ticked one by one, the confirm dialog, done. */
const STEPS = 6;
const picked = helmDeals.flatMap((d, i) => (d.pick ? [i] : []));

export function useHelmSteps() {
  return useSteps(STEPS, { interval: (s) => (s === 0 ? 1000 : s === 4 ? 1500 : 750), hold: 3200, still: 3 });
}

/** A deal's stage: an outline badge; won deals (positive data) in green. */
export function StageBadge({ stage }: { stage: Stage }) {
  if (stage === "Won") {
    return (
      <Badge tone="positive">
        <CheckIcon className="size-3" strokeWidth={2.5} />
        Won
      </Badge>
    );
  }
  return <Badge tone={stage === "Lost" ? "muted" : "outline"}>{stage}</Badge>;
}

const shades: Record<Stage, string> = {
  Lead: "bg-line-2",
  Qualified: "bg-fg-3/45",
  Proposal: "bg-fg-3/75",
  Negotiation: "bg-fg-2",
  Won: "bg-accent",
  Lost: "bg-ink-3",
};

function StageBar() {
  const total = helmStages.reduce((n, s) => n + s.deals, 0);
  return (
    <>
      <div className="mt-3 flex h-2 gap-0.5 overflow-hidden rounded-[2px]">
        {helmStages.map((s) => (
          <span key={s.stage} className={shades[s.stage]} style={{ width: `${(s.deals / total) * 100}%` }} />
        ))}
      </div>
      <div className="mt-2.5 flex flex-wrap gap-x-4 gap-y-1 text-[0.8125rem] text-fg-3">
        {helmStages.slice(2, 5).map((s) => (
          <span key={s.stage} className="flex items-center gap-1.5 whitespace-nowrap">
            <span className={cn("size-2 shrink-0 rounded-[2px]", shades[s.stage])} />
            {s.stage}
            <span className="text-fg-1 tabular-nums">{s.deals}</span>
          </span>
        ))}
      </div>
    </>
  );
}

const sidebar: SidebarSpec = {
  app: { logo: <GalleryVerticalEndIcon strokeWidth={2} />, name: "Acme CRM", context: "Admin" },
  groups: [
    { items: [{ icon: LayoutDashboardIcon, label: "Dashboard" }] },
    {
      label: "Sales",
      items: [
        { icon: HandshakeIcon, label: "Deals", badge: "30", active: true },
        { icon: Building2Icon, label: "Companies", badge: "48" },
        { icon: ContactIcon, label: "Contacts", badge: "212" },
      ],
    },
    { label: "Pages", items: [{ icon: KanbanSquareIcon, label: "Pipeline board" }] },
    {
      label: "Team",
      items: [
        { icon: UsersIcon, label: "Users", badge: "9" },
        { icon: SettingsIcon, label: "Settings", sub: [{ label: "Roles" }, { label: "Audit log" }] },
      ],
    },
  ],
  user: { initials: "MR", name: "Marcus Reyes", email: "marcus@acme.test" },
};

/** Column templates by frame width: what fits, in order of importance. */
const cols =
  "grid-cols-[1rem_minmax(0,1fr)_6.25rem] @xl:grid-cols-[1rem_minmax(0,1fr)_5.5rem_6.75rem] @3xl:grid-cols-[1rem_minmax(0,1fr)_5.5rem_6.75rem_7.5rem_3.75rem] @5xl:grid-cols-[1rem_minmax(0,1.15fr)_5.5rem_6.75rem_minmax(0,1fr)_7.5rem_3.75rem_3rem]";

function FacetButton({ label, value, className }: { label: string; value?: string; className?: string }) {
  return (
    <span className={cn("inline-flex h-8 shrink-0 items-center gap-1.5 rounded-sm border border-dashed border-line-2 px-2.5 text-[0.8125rem] text-fg-1", className)}>
      <CirclePlusIcon className="size-4 text-fg-3" strokeWidth={1.75} />
      {label}
      {value ? (
        <>
          <span className="mx-0.5 h-4 w-px bg-line-2" />
          <span className="rounded-sm bg-ink-3 px-1.5 text-xs leading-5 text-fg-1">{value}</span>
        </>
      ) : null}
    </span>
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
  const dialog = step === 4;

  const overlay = (
    <>
      {/* The confirmation, over the whole app. */}
      <div
        className={cn(
          "absolute inset-0 z-10 flex items-center justify-center bg-ink-0/60 p-4 transition-opacity duration-300",
          dialog ? "opacity-100" : "pointer-events-none opacity-0",
        )}
      >
        <div
          className={cn(
            "w-full max-w-[25rem] rounded-md border border-line-2 bg-ink-1 p-5 shadow-float transition-transform duration-300 ease-(--ease-out-quint)",
            dialog ? "scale-100" : "scale-[0.97]",
          )}
        >
          <div className="text-base font-semibold text-fg">Mark 3 deals as won?</div>
          <div className="mt-1.5 text-sm leading-relaxed text-fg-2">Their stage changes to Won and their probability to 100%. Owners are notified.</div>
          <div className="mt-5 flex justify-end gap-2">
            <Button>Cancel</Button>
            <Button tone="primary">Mark as won</Button>
          </div>
        </div>
      </div>

      <div
        className={cn(
          "absolute right-4 bottom-4 z-10 flex items-center gap-2.5 rounded-md border border-line-2 bg-ink-2 py-2.5 pr-4 pl-3 text-sm text-fg shadow-float transition-[opacity,translate] duration-300 ease-(--ease-out-quint) @xl:right-6 @xl:bottom-6",
          done ? "translate-y-0 opacity-100" : "translate-y-1 opacity-0",
        )}
      >
        <CircleCheckIcon className="size-4 text-accent" strokeWidth={2} />3 deals marked as won
      </div>
    </>
  );

  return (
    <MockFrame
      url="crm.example.com/admin/resources/deals"
      className={className}
      bodyClassName="@container"
      label="Helm, the admin panel of the example CRM, on the Deals resource: a sidebar with the CRM's resources, metric cards for the open pipeline, new deals and deals by stage; search and filters for stage and owner; a table of deals with amount, stage, company, owner and expected close date. Three deals are ticked and the bulk action Mark as won runs on them after a confirmation."
    >
      <div ref={ref} className="h-full">
        <AppShell sidebar={sidebar} overlay={overlay}>
          <TopBar crumbs={["Sales", "Deals"]}>
            <SearchField className="hidden w-64 @3xl:flex" placeholder="Search Acme CRM…" />
            <Button tone="ghost" icon={BellIcon} />
          </TopBar>

          <div className="p-4 @xl:p-6">
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0">
                <div className="text-xl font-semibold tracking-[-0.01em] text-fg">Deals</div>
                <div className="mt-1 hidden truncate text-[0.8125rem] text-fg-3 @lg:block">Every deal in the workspace, by expected close date.</div>
              </div>
              <div className="flex gap-2">
                <Button icon={DownloadIcon} className="hidden @2xl:inline-flex">
                  Export
                </Button>
                <Button tone="primary" icon={PlusIcon}>
                  <span className="hidden @md:inline">Create deal</span>
                  <span className="@md:hidden">New</span>
                </Button>
              </div>
            </div>

            <div className="mt-5 grid gap-3 @2xl:grid-cols-3">
              <Card className="p-4">
                <Stat
                  label="Open pipeline"
                  value="$1,284,500"
                  aside={
                    <Badge tone="positive">
                      <TrendingUpIcon className="size-3" strokeWidth={2} />
                      12.4%
                    </Badge>
                  }
                  note="On the previous 30 days"
                />
              </Card>
              <Card className="hidden p-4 @2xl:block">
                <Stat label="New deals" value="164" aside={<span className="font-mono text-xs text-fg-3">30 days</span>} />
                <Sparkline data={helmNewDeals} area className="mt-1.5 h-6 w-full text-fg-3" />
              </Card>
              <Card className="hidden p-4 @2xl:block">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[0.8125rem] text-fg-2">Deals by stage</span>
                  <span className="font-mono text-xs text-fg-3">30 open</span>
                </div>
                <StageBar />
              </Card>
            </div>

            {/* Filters on the left; the bulk action takes the right once rows are ticked. Same height either way. */}
            <div className="mt-5 flex h-8 items-center gap-2">
              <span className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-sm border border-line-2 bg-ink-1 px-2.5 text-sm text-fg-3 @xl:max-w-[15rem]">
                <SearchIcon className="size-4 shrink-0" strokeWidth={1.75} />
                <span className="truncate">Filter deals…</span>
              </span>
              <FacetButton label="Stage" value="Open" className="hidden @xl:inline-flex" />
              <FacetButton label="Owner" className="hidden @3xl:inline-flex" />
              <span className="hidden h-8 items-center gap-1.5 px-2 text-[0.8125rem] text-fg-2 @5xl:inline-flex">
                Reset
                <XIcon className="size-3.5" strokeWidth={1.75} />
              </span>
              <div className="ml-auto flex shrink-0 items-center gap-2">
                {count > 0 ? (
                  <>
                    <span className="hidden text-[0.8125rem] text-fg-2 tabular-nums @lg:inline">{count} selected</span>
                    <span className="inline-flex h-8 items-center gap-2 rounded-sm border border-line-2 bg-ink-1 px-2.5 text-[0.8125rem] text-fg-1">
                      Mark as won
                      <ChevronDownIcon className="size-4 text-fg-3" strokeWidth={1.75} />
                    </span>
                    <Button tone={step >= 4 ? "primary" : "secondary"}>Run</Button>
                  </>
                ) : (
                  <Button icon={Settings2Icon} className="hidden @lg:inline-flex">
                    View
                  </Button>
                )}
              </div>
            </div>

            <div className="mt-3 overflow-hidden rounded-md border border-line">
              <div className={cn("grid h-10 items-center gap-3 border-b border-line bg-ink-1 px-3 text-[0.8125rem] font-medium text-fg-2", cols)}>
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
                      "grid min-h-10 items-center gap-3 border-b border-line px-3 py-2 text-sm transition-colors duration-200 last:border-b-0 @5xl:py-0",
                      cols,
                      isTicked && "bg-ink-2",
                    )}
                  >
                    <Checkbox checked={isTicked} />
                    <div className="min-w-0">
                      <div className="truncate text-fg">{deal.title}</div>
                      <div className="truncate text-[0.8125rem] text-fg-3 @5xl:hidden">{deal.company}</div>
                    </div>
                    <span className={cn("hidden text-right tabular-nums @xl:block", stage === "Won" ? "text-accent" : "text-fg-1")}>
                      {deal.amount}
                    </span>
                    <span>
                      <StageBadge stage={stage} />
                    </span>
                    <span className="hidden truncate text-fg-2 @5xl:block">{deal.company}</span>
                    <span className="hidden truncate text-fg-2 @3xl:block">{deal.owner}</span>
                    <span className="hidden text-[0.8125rem] text-fg-2 tabular-nums @3xl:block">{deal.close}</span>
                    <span className="hidden text-right text-[0.8125rem] text-fg-2 tabular-nums @5xl:block">{done && deal.pick ? "100%" : deal.probability}</span>
                  </div>
                );
              })}
            </div>

            <div className="mt-3 flex items-center justify-between gap-3 text-[0.8125rem] text-fg-3">
              <span className="tabular-nums">
                {count} of 6 row(s) selected<span className="hidden @lg:inline"> · 30 deals</span>
              </span>
              <span className="flex items-center gap-2">
                <span className="hidden @xl:inline">Page 1 of 5</span>
                <Button size="sm" icon={ChevronLeftIcon} className="text-fg-3" />
                <Button size="sm" icon={ChevronRightIcon} />
              </span>
            </div>
          </div>
        </AppShell>
      </div>
    </MockFrame>
  );
}
