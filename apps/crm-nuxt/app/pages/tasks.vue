<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { ListChecks, Plus } from "@lucide/vue";
import PageHeader from "@/components/crm/PageHeader.vue";
import PaginationBar from "@/components/crm/PaginationBar.vue";
import PastEndNotice from "@/components/crm/PastEndNotice.vue";
import UserAvatar from "@/components/crm/UserAvatar.vue";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import DueLabel from "@/features/activities/DueLabel.vue";
import { subjectHref } from "@/features/activities/kinds";
import { type ActivityListParams, useActivities } from "@/features/activities/queries";
import TaskCheckbox from "@/features/activities/TaskCheckbox.vue";
import { useCurrentUser } from "@/features/auth/queries";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDateTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

useHead({ title: "Tasks" });

const TABS = [
  { value: "overdue", label: "Overdue", empty: "Nothing overdue. Nice work." },
  { value: "today", label: "Today", empty: "Nothing due today. Add a task or check what's coming up." },
  { value: "upcoming", label: "Upcoming", empty: "Nothing scheduled after today." },
  { value: "completed", label: "Completed", empty: "Completed tasks will be listed here." },
] as const;
type Tab = (typeof TABS)[number]["value"];

const url = useUrlState();
const me = useCurrentUser();
const ui = useUiStore();
const ability = useAbility<AppAbility>();

const tab = computed<Tab>(() => {
  const raw = url.get("tab");
  return TABS.some((t) => t.value === raw) ? (raw as Tab) : "today";
});
const everyone = computed(() => url.get("scope") === "everyone");
const assignee = computed(() => (everyone.value ? undefined : me.data.value?.id));
const current = computed(() => TABS.find((t) => t.value === tab.value)!);

function paramsFor(t: Tab): ActivityListParams {
  return t === "completed"
    ? { filter: { kind: "task", assignee_id: assignee.value, completed: true } }
    : { filter: { kind: "task", assignee_id: assignee.value, due: t, completed: false } };
}

const list = useActivities(() => ({ ...paramsFor(tab.value), page: url.page.value, per_page: 25 }));
// One-row queries per tab: the totals for the tab badges.
const counts = Object.fromEntries(TABS.map((t) => [t.value, useActivities(() => ({ ...paramsFor(t.value), per_page: 1 }))])) as Record<
  Tab,
  ReturnType<typeof useActivities>
>;
</script>

<template>
  <div class="flex flex-col">
    <PageHeader title="Tasks" description="Follow-ups across your deals, companies and contacts.">
      <template #actions>
        <Button v-if="ability.can('create', 'Activity')" @click="ui.create('task')"><Plus /> New task</Button>
      </template>
    </PageHeader>

    <Tabs :model-value="tab" class="gap-4" @update:model-value="(v) => url.set({ tab: String(v) })">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="-mx-4 overflow-x-auto px-4 scroll-thin sm:mx-0 sm:px-0">
          <TabsList>
            <TabsTrigger v-for="t in TABS" :key="t.value" :value="t.value" class="gap-1.5">
              {{ t.label }}
              <span
                v-if="counts[t.value].data.value"
                :class="
                  cn(
                    'rounded-xs px-1 font-mono text-xs leading-5 figures',
                    t.value === 'overdue' && counts[t.value].data.value!.meta.total > 0 ? 'bg-danger/12 text-danger' : 'text-fg-3',
                  )
                "
                >{{ counts[t.value].data.value!.meta.total }}</span
              >
            </TabsTrigger>
          </TabsList>
        </div>
        <ToggleGroup
          type="single"
          size="sm"
          :model-value="everyone ? 'everyone' : 'mine'"
          aria-label="Whose tasks"
          @update:model-value="(v) => v && url.set({ scope: v === 'everyone' ? 'everyone' : undefined })"
        >
          <ToggleGroupItem value="mine">Assigned to me</ToggleGroupItem>
          <ToggleGroupItem value="everyone">Everyone</ToggleGroupItem>
        </ToggleGroup>
      </div>

      <TabsContent :value="tab">
        <section :aria-label="`${current.label} tasks`" class="surface overflow-hidden">
          <div v-if="list.isPending.value" class="flex flex-col gap-3 p-4">
            <Skeleton v-for="i in 5" :key="i" class="h-11 w-full" />
          </div>
          <ul v-else-if="list.data.value?.data.length" class="divide-y divide-line">
            <li v-for="task in list.data.value.data" :key="task.id" class="flex items-start gap-3 px-4 py-3 transition-colors hover:bg-ink-2/40" data-testid="task-row">
              <span class="pt-0.5"><TaskCheckbox :activity="task" /></span>
              <div class="flex min-w-0 flex-1 flex-col gap-1">
                <span :class="cn('text-sm leading-snug font-medium text-fg', task.completed_at && 'text-fg-3 line-through decoration-fg-3/50')">{{ task.subject }}</span>
                <span v-if="task.body" class="line-clamp-1 text-[13px] text-fg-2">{{ task.body }}</span>
                <span class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-3">
                  <span v-if="task.completed_at" class="figures">Completed {{ formatDateTime(task.completed_at) }}</span>
                  <DueLabel v-else :activity="task" />
                  <NuxtLink :to="subjectHref(task.subject_type, task.subject_id)" class="link text-fg-2">{{ task.subject_label }}</NuxtLink>
                </span>
              </div>
              <span v-if="everyone && task.assignee" class="hidden items-center gap-2 text-xs text-fg-2 sm:flex">
                <UserAvatar :name="task.assignee.name" :id="task.assignee.id" size="xs" />
                {{ task.assignee.name }}
              </span>
            </li>
          </ul>
          <PastEndNotice
            v-else-if="list.data.value && list.data.value.meta.total > 0"
            noun="tasks"
            :total="list.data.value.meta.total"
            @first="url.set({ page: undefined })"
          />
          <Empty v-else class="py-16">
            <EmptyHeader>
              <EmptyMedia variant="icon"><ListChecks /></EmptyMedia>
              <EmptyTitle>No {{ current.label.toLowerCase() }} tasks</EmptyTitle>
              <EmptyDescription>{{ current.empty }}</EmptyDescription>
            </EmptyHeader>
          </Empty>
          <PaginationBar :meta="list.data.value?.meta" noun="tasks" @page="(page) => url.set({ page })" />
        </section>
      </TabsContent>
    </Tabs>
  </div>
</template>
