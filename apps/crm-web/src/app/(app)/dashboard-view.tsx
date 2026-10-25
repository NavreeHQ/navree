"use client";

import { ArrowUpRightIcon } from "lucide-react";
import Link from "next/link";
import { ActivityTimeline, DueLabel, TaskCheckbox } from "@/features/activities/components";
import { useActivities } from "@/features/activities/queries";
import { useCurrentUser } from "@/features/auth/queries";
import { useDashboard } from "@/features/dashboard/queries";
import { Money } from "@/components/money";
import { PageHeader, Panel } from "@/components/page-header";
import { StageDot } from "@/components/stage";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Dashboard } from "@/lib/api/types";
import { DEAL_STAGES, stageLabel } from "@/lib/api/types";
import { formatMoney, formatNumber, formatPercent } from "@/lib/format";
import { cn } from "@/lib/utils";

const longDate = new Intl.DateTimeFormat("en-US", { weekday: "long", month: "long", day: "numeric" });

function Figure({
  label,
  children,
  sub,
  href,
}: {
  label: string;
  children: React.ReactNode;
  sub?: React.ReactNode;
  href?: string;
}) {
  const body = (
    <>
      <span className="text-[13px] text-muted-foreground">{label}</span>
      <span className="text-[28px] leading-none font-semibold tracking-[-0.01em] font-condensed">{children}</span>
      {sub ? <span className="text-xs text-muted-foreground">{sub}</span> : null}
    </>
  );
  const className = "flex min-w-0 flex-col gap-2 px-5 py-4";
  return href ? (
    <Link href={href} className={cn(className, "transition-colors hover:bg-muted/40 focus-visible:bg-muted/40")}>
      {body}
    </Link>
  ) : (
    <div className={className}>{body}</div>
  );
}

function KeyFigures({ data }: { data: Dashboard }) {
  const { open_deals: open, won_this_month: won, tasks } = data;
  return (
    <section
      aria-label="Key figures"
      className="grid grid-cols-1 divide-y rounded-lg border bg-card sm:grid-cols-2 sm:divide-y-0 lg:grid-cols-4 lg:divide-x [&>*:nth-child(2)]:sm:border-l [&>*:nth-child(n+3)]:sm:border-t lg:[&>*:nth-child(n+3)]:border-t-0"
    >
      <Figure label="Open pipeline" sub={`${formatNumber(open.count)} open deals`} href="/deals">
        <Money cents={open.amount_cents} compact />
      </Figure>
      <Figure
        label="Weighted forecast"
        sub={
          open.amount_cents > 0
            ? `${formatPercent((open.weighted_amount_cents / open.amount_cents) * 100)} of pipeline, by stage probability`
            : "By stage probability"
        }
      >
        <Money cents={open.weighted_amount_cents} compact />
      </Figure>
      <Figure label="Won this month" sub={`${formatNumber(won.count)} ${won.count === 1 ? "deal" : "deals"} closed`}>
        <Money cents={won.amount_cents} compact />
      </Figure>
      <Figure
        label="My open tasks"
        href="/tasks"
        sub={
          <span className="flex flex-wrap gap-x-2.5 whitespace-nowrap">
            <span className={cn(tasks.overdue > 0 && "font-medium text-destructive")}>{tasks.overdue} overdue</span>
            <span>{tasks.due_today} today</span>
            <span>{tasks.upcoming} later</span>
          </span>
        }
      >
        <span className="figures">{formatNumber(tasks.overdue + tasks.due_today + tasks.upcoming)}</span>
      </Figure>
    </section>
  );
}

/**
 * Pipeline by stage: one horizontal bar per stage on a shared amount axis,
 * coloured with the stage depth ramp, labelled directly (stage, count, amount)
 * so colour is never the only cue. Hover a row for its share.
 */
function PipelineChart({ data }: { data: Dashboard }) {
  const rows = DEAL_STAGES.map(({ stage, probability }) => {
    const row = data.pipeline.find((p) => p.stage === stage);
    return { stage, probability, count: row?.count ?? 0, amount: row?.amount_cents ?? 0 };
  });
  const max = Math.max(1, ...rows.map((r) => r.amount));
  const openTotal = rows.filter((r) => r.stage !== "won" && r.stage !== "lost").reduce((s, r) => s + r.amount, 0);

  return (
    <div className="flex flex-col">
      <div
        role="table"
        aria-label="Pipeline by stage"
        className="grid grid-cols-[minmax(6.5rem,auto)_1fr_auto] items-center gap-x-4 text-sm"
      >
        <div role="row" className="contents text-xs text-muted-foreground">
          <span role="columnheader" className="pb-2">
            Stage
          </span>
          <span role="columnheader" className="pb-2">
            Amount
          </span>
          <span role="columnheader" className="pb-2 text-right">
            Deals
          </span>
        </div>
        {rows.map((r, i) => {
          const closed = r.stage === "won" || r.stage === "lost";
          const share = !closed && openTotal > 0 ? (r.amount / openTotal) * 100 : null;
          return (
            <Tooltip key={r.stage}>
              <TooltipTrigger asChild>
                <Link
                  href="/deals"
                  role="row"
                  className={cn(
                    "group col-span-3 grid grid-cols-subgrid items-center rounded-md py-2 outline-none hover:bg-muted/50 focus-visible:bg-muted/50",
                    i === 4 && "mt-2 border-t pt-3",
                  )}
                >
                  <span role="rowheader" className="flex items-center gap-2 pl-1">
                    <StageDot stage={r.stage} />
                    <span>{stageLabel(r.stage)}</span>
                    {!closed ? <span className="text-xs text-muted-foreground figures">{r.probability}%</span> : null}
                  </span>
                  <span role="cell" className="flex min-w-0 items-center gap-3">
                    <span className="relative h-3 flex-1">
                      <span
                        className="absolute inset-y-0 left-0 rounded-r-[4px]"
                        style={{
                          width: `${r.amount === 0 ? 0 : Math.max(1.5, (r.amount / max) * 100)}%`,
                          backgroundColor: `var(--stage-${r.stage})`,
                        }}
                      />
                    </span>
                    <Money cents={r.amount} compact className="w-16 text-right text-[13px] font-medium" />
                  </span>
                  <span role="cell" className="pr-1 text-right text-muted-foreground figures">
                    {r.count}
                  </span>
                </Link>
              </TooltipTrigger>
              <TooltipContent side="top" align="start" className="flex flex-col gap-0.5">
                <span className="font-medium">{stageLabel(r.stage)}</span>
                <span className="figures">
                  {formatMoney(r.amount, "USD")} across {r.count} {r.count === 1 ? "deal" : "deals"}
                </span>
                {share !== null ? <span className="figures">{formatPercent(share)} of open pipeline</span> : null}
              </TooltipContent>
            </Tooltip>
          );
        })}
      </div>
      <p className="mt-4 text-xs text-muted-foreground">Won and lost include every closed deal, not just this month.</p>
    </div>
  );
}

function MyTasks({ userId }: { userId: string }) {
  const overdue = useActivities({ kind: "task", assignee_id: userId, due: "overdue", completed: false, per_page: 10 });
  const today = useActivities({ kind: "task", assignee_id: userId, due: "today", completed: false, per_page: 10 });
  const tasks = [...(overdue.data?.data ?? []), ...(today.data?.data ?? [])];

  if (overdue.isPending || today.isPending) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 4 }, (_, i) => (
          <Skeleton key={i} className="h-9 w-full" />
        ))}
      </div>
    );
  }
  if (tasks.length === 0) {
    return <p className="text-sm text-muted-foreground">Nothing due today. Upcoming tasks are on the Tasks page.</p>;
  }
  return (
    <ul className="-my-1 flex flex-col divide-y">
      {tasks.map((t) => (
        <li key={t.id} className="flex items-start gap-3 py-2.5">
          <span className="pt-0.5">
            <TaskCheckbox activity={t} />
          </span>
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="text-sm leading-snug font-medium">{t.subject}</span>
            <span className="flex flex-wrap gap-x-3 text-xs text-muted-foreground">
              <DueLabel activity={t} />
              <span className="truncate">{t.subject_label}</span>
            </span>
          </div>
        </li>
      ))}
    </ul>
  );
}

function DashboardSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy="true">
      <Skeleton className="h-28 w-full" />
      <div className="grid gap-6 lg:grid-cols-5">
        <Skeleton className="h-80 lg:col-span-3" />
        <Skeleton className="h-80 lg:col-span-2" />
      </div>
    </div>
  );
}

export function DashboardView() {
  const dashboard = useDashboard();
  const me = useCurrentUser();

  return (
    <div className="flex flex-col">
      <PageHeader title="Dashboard" description={longDate.format(new Date())} />
      {dashboard.isPending ? (
        <DashboardSkeleton />
      ) : dashboard.isError ? (
        <Panel>
          <p className="text-sm text-muted-foreground">The dashboard couldn&apos;t be loaded. Refresh to try again.</p>
        </Panel>
      ) : (
        <div className="flex flex-col gap-6">
          <KeyFigures data={dashboard.data} />
          <div className="grid gap-6 lg:grid-cols-5">
            <Panel
              title="Pipeline by stage"
              className="lg:col-span-3"
              actions={
                <Button asChild variant="ghost" size="xs">
                  <Link href="/deals">
                    Open board
                    <ArrowUpRightIcon data-icon="inline-end" />
                  </Link>
                </Button>
              }
            >
              <PipelineChart data={dashboard.data} />
            </Panel>
            <Panel
              title="My tasks"
              className="lg:col-span-2"
              actions={
                <Button asChild variant="ghost" size="xs">
                  <Link href="/tasks">
                    All tasks
                    <ArrowUpRightIcon data-icon="inline-end" />
                  </Link>
                </Button>
              }
            >
              {me.data ? <MyTasks userId={me.data.id} /> : null}
            </Panel>
          </div>
          <Panel title="Recent activity">
            <ActivityTimeline
              activities={dashboard.data.recent_activities}
              showSubject
              emptyText="No activity has been logged yet."
            />
          </Panel>
        </div>
      )}
    </div>
  );
}
