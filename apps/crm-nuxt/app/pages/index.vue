<script setup lang="ts">
import { ArrowUpRight, CheckCheck } from "@lucide/vue";
import Money from "@/components/crm/Money.vue";
import PageHeader from "@/components/crm/PageHeader.vue";
import Panel from "@/components/crm/Panel.vue";
import StageDot from "@/components/crm/StageDot.vue";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import ActivityTimeline from "@/features/activities/ActivityTimeline.vue";
import DueLabel from "@/features/activities/DueLabel.vue";
import { useActivities } from "@/features/activities/queries";
import TaskCheckbox from "@/features/activities/TaskCheckbox.vue";
import { useCurrentUser } from "@/features/auth/queries";
import { useDashboard } from "@/features/dashboard/queries";
import { DEAL_STAGES, stageLabel } from "@/lib/api/types";
import { formatMoney, formatNumber, formatPercent } from "@/lib/format";
import { isClosed, stageColor } from "@/lib/stages";
import { cn } from "@/lib/utils";

useHead({ title: "Dashboard" });

const longDate = new Intl.DateTimeFormat("en-IN", { weekday: "long", month: "long", day: "numeric" }).format(new Date());
const dashboard = useDashboard();
const me = useCurrentUser();
const userId = computed(() => me.data.value?.id ?? "");

const overdue = useActivities(
  () => ({
    filter: { kind: "task", assignee_id: userId.value, due: "overdue", completed: false },
    per_page: 10,
  }),
  () => Boolean(userId.value),
);
const today = useActivities(
  () => ({
    filter: { kind: "task", assignee_id: userId.value, due: "today", completed: false },
    per_page: 10,
  }),
  () => Boolean(userId.value),
);
const myTasks = computed(() => [...(overdue.data.value?.data ?? []), ...(today.data.value?.data ?? [])]);
const tasksLoading = computed(() => overdue.isPending.value || today.isPending.value);

const pipeline = computed(() => {
  const data = dashboard.data.value;
  if (!data) return [];
  const rows = DEAL_STAGES.map(({ stage, probability }) => {
    const row = data.pipeline.find((p) => p.stage === stage);
    return { stage, probability, count: row?.count ?? 0, amount: row?.amount_cents ?? 0 };
  });
  const max = Math.max(1, ...rows.map((r) => r.amount));
  const openTotal = rows.filter((r) => !isClosed(r.stage)).reduce((s, r) => s + r.amount, 0);
  return rows.map((r) => ({
    ...r,
    width: r.amount === 0 ? 0 : Math.max(1.5, (r.amount / max) * 100),
    share: !isClosed(r.stage) && openTotal > 0 ? (r.amount / openTotal) * 100 : null,
  }));
});

const weightedShare = computed(() => {
  const open = dashboard.data.value?.open_deals;
  return open && open.amount_cents > 0 ? (open.weighted_amount_cents / open.amount_cents) * 100 : null;
});
const taskTotal = computed(() => {
  const t = dashboard.data.value?.tasks;
  return t ? t.overdue + t.due_today + t.upcoming : 0;
});
</script>

<template>
  <div class="flex flex-col">
    <PageHeader title="Dashboard" :description="longDate" />

    <div v-if="dashboard.isPending.value" class="flex flex-col gap-6" aria-busy="true" aria-label="Loading the dashboard">
      <Skeleton class="h-32 w-full rounded-lg" />
      <div class="grid gap-6 xl:grid-cols-5">
        <Skeleton class="h-80 rounded-lg xl:col-span-3" />
        <Skeleton class="h-80 rounded-lg xl:col-span-2" />
      </div>
    </div>
    <Panel v-else-if="dashboard.isError.value">
      <p class="text-sm text-fg-2">The dashboard couldn't be loaded. Refresh to try again.</p>
    </Panel>
    <div v-else-if="dashboard.data.value" class="flex flex-col gap-6">
      <!-- Key figures: one surface, hairline dividers, the won figure in the accent. -->
      <section
        aria-label="Key figures"
        class="surface grid grid-cols-1 divide-y divide-line overflow-hidden sm:grid-cols-2 sm:divide-y-0 xl:grid-cols-4 [&>*:nth-child(2)]:sm:border-l [&>*:nth-child(n+3)]:sm:border-t xl:[&>*:nth-child(n+2)]:border-l xl:[&>*:nth-child(n+3)]:border-t-0"
      >
        <NuxtLink to="/deals" class="group flex min-w-0 flex-col gap-3 px-5 py-4 transition-colors hover:bg-ink-2/60">
          <span class="flex items-center justify-between text-[13px] text-fg-2">
            Open pipeline
            <ArrowUpRight class="size-4 text-fg-3 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" aria-hidden="true" />
          </span>
          <Money :cents="dashboard.data.value.open_deals.amount_cents" compact class="font-display text-[1.75rem] leading-none font-semibold tracking-[-0.03em] text-fg" />
          <span class="text-xs text-fg-3">{{ formatNumber(dashboard.data.value.open_deals.count) }} open deals</span>
        </NuxtLink>
        <div class="flex min-w-0 flex-col gap-3 px-5 py-4">
          <span class="text-[13px] text-fg-2">Weighted forecast</span>
          <Money :cents="dashboard.data.value.open_deals.weighted_amount_cents" compact class="font-display text-[1.75rem] leading-none font-semibold tracking-[-0.03em] text-fg" />
          <span class="text-xs text-fg-3">{{ weightedShare !== null ? `${formatPercent(weightedShare)} of pipeline, by stage probability` : "By stage probability" }}</span>
        </div>
        <div class="flex min-w-0 flex-col gap-3 px-5 py-4">
          <span class="flex items-center gap-2 text-[13px] text-fg-2">
            <span aria-hidden="true" class="size-1.5 rounded-full bg-accent" />
            Won this month
          </span>
          <Money :cents="dashboard.data.value.won_this_month.amount_cents" compact class="font-display text-[1.75rem] leading-none font-semibold tracking-[-0.03em] text-accent" />
          <span class="text-xs text-fg-3">
            {{ formatNumber(dashboard.data.value.won_this_month.count) }} {{ dashboard.data.value.won_this_month.count === 1 ? "deal" : "deals" }} closed
          </span>
        </div>
        <NuxtLink to="/tasks" class="group flex min-w-0 flex-col gap-3 px-5 py-4 transition-colors hover:bg-ink-2/60">
          <span class="flex items-center justify-between text-[13px] text-fg-2">
            My open tasks
            <ArrowUpRight class="size-4 text-fg-3 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" aria-hidden="true" />
          </span>
          <span class="font-display text-[1.75rem] leading-none font-semibold tracking-[-0.03em] text-fg figures">{{ formatNumber(taskTotal) }}</span>
          <span class="flex flex-wrap gap-x-3 text-xs whitespace-nowrap text-fg-3">
            <span :class="cn(dashboard.data.value.tasks.overdue > 0 && 'font-medium text-danger')">{{ dashboard.data.value.tasks.overdue }} overdue</span>
            <span>{{ dashboard.data.value.tasks.due_today }} today</span>
            <span>{{ dashboard.data.value.tasks.upcoming }} later</span>
          </span>
        </NuxtLink>
      </section>

      <div class="grid gap-6 xl:grid-cols-5">
        <Panel title="Pipeline by stage" class="xl:col-span-3">
          <template #actions>
            <Button as-child variant="ghost" size="xs">
              <NuxtLink to="/deals">Open board <ArrowUpRight /></NuxtLink>
            </Button>
          </template>
          <div role="table" aria-label="Pipeline by stage" class="grid grid-cols-[minmax(7rem,auto)_1fr_auto] items-center gap-x-4 text-sm">
            <div role="row" class="contents text-xs text-fg-3">
              <span role="columnheader" class="pb-2 pl-1">Stage</span>
              <span role="columnheader" class="pb-2">Amount</span>
              <span role="columnheader" class="pr-1 pb-2 text-right">Deals</span>
            </div>
            <Tooltip v-for="(r, i) in pipeline" :key="r.stage">
              <TooltipTrigger as-child>
                <NuxtLink
                  to="/deals"
                  role="row"
                  :class="cn('group col-span-3 grid grid-cols-subgrid items-center rounded-md py-2 transition-colors hover:bg-ink-2/70 focus-visible:bg-ink-2/70', i === 4 && 'mt-2 border-t border-line pt-3')"
                >
                  <span role="rowheader" class="flex items-center gap-2 pl-1">
                    <StageDot :stage="r.stage" />
                    <span class="text-fg-1">{{ stageLabel(r.stage) }}</span>
                    <span v-if="!isClosed(r.stage)" class="font-mono text-xs text-fg-3">{{ r.probability }}%</span>
                  </span>
                  <span role="cell" class="flex min-w-0 items-center gap-3">
                    <span class="relative h-2 flex-1 overflow-hidden rounded-xs bg-ink-2">
                      <span
                        class="absolute inset-y-0 left-0 rounded-xs transition-[width] duration-500 ease-(--ease-out-quint)"
                        :style="{
                          width: `${r.width}%`,
                          backgroundColor: stageColor(r.stage),
                        }"
                      />
                    </span>
                    <Money :cents="r.amount" compact :class="cn('w-16 text-right text-[13px] font-medium', r.stage === 'won' ? 'text-accent' : 'text-fg')" />
                  </span>
                  <span role="cell" class="pr-1 text-right text-fg-3 figures">{{ r.count }}</span>
                </NuxtLink>
              </TooltipTrigger>
              <TooltipContent side="top" align="start" class="flex flex-col gap-0.5">
                <span class="font-medium">{{ stageLabel(r.stage) }}</span>
                <span class="figures">{{ formatMoney(r.amount, "INR") }} across {{ r.count }} {{ r.count === 1 ? "deal" : "deals" }}</span>
                <span v-if="r.share !== null" class="figures">{{ formatPercent(r.share) }} of open pipeline</span>
              </TooltipContent>
            </Tooltip>
          </div>
          <p class="mt-4 text-xs text-fg-3">Won and lost include every closed deal, not just this month.</p>
        </Panel>

        <Panel title="My tasks" class="xl:col-span-2">
          <template #actions>
            <Button as-child variant="ghost" size="xs">
              <NuxtLink to="/tasks">All tasks <ArrowUpRight /></NuxtLink>
            </Button>
          </template>
          <div v-if="tasksLoading" class="flex flex-col gap-3">
            <Skeleton v-for="i in 4" :key="i" class="h-10 w-full" />
          </div>
          <div v-else-if="myTasks.length === 0" class="flex flex-col items-start gap-3 py-2">
            <span class="flex size-9 items-center justify-center rounded-md border border-line-2 bg-ink-2 text-fg-2">
              <CheckCheck class="size-4" aria-hidden="true" />
            </span>
            <div class="flex flex-col gap-1">
              <p class="text-sm font-medium text-fg">You're clear for today.</p>
              <p class="text-sm text-pretty text-fg-3">Nothing is overdue or due today. Upcoming tasks are on the Tasks page.</p>
            </div>
          </div>
          <ul v-else class="-my-1 flex flex-col divide-y divide-line">
            <li v-for="t in myTasks" :key="t.id" class="flex items-start gap-3 py-2.5">
              <span class="pt-0.5"><TaskCheckbox :activity="t" /></span>
              <div class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm leading-snug font-medium text-fg">{{ t.subject }}</span>
                <span class="flex flex-wrap gap-x-3 text-xs text-fg-3">
                  <DueLabel :activity="t" />
                  <span class="truncate">{{ t.subject_label }}</span>
                </span>
              </div>
            </li>
          </ul>
        </Panel>
      </div>

      <Panel title="Recent activity">
        <ActivityTimeline :activities="dashboard.data.value.recent_activities" show-subject empty-text="No activity has been logged yet." />
      </Panel>
    </div>
  </div>
</template>
