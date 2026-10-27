<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Trash2 } from "@lucide/vue";
import { toast } from "vue-sonner";
import ConfirmDialog from "@/components/crm/ConfirmDialog.vue";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { Activity } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDateTime, formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";
import DueLabel from "./DueLabel.vue";
import { KIND_META, subjectHref } from "./kinds";
import { useDeleteActivity } from "./queries";
import TaskCheckbox from "./TaskCheckbox.vue";

/** A record's history, newest first: notes, calls, meetings and tasks on one rail. */
withDefaults(
  defineProps<{ activities: Activity[] | undefined; loading?: boolean; showSubject?: boolean; emptyText?: string }>(),
  { emptyText: "No activity yet. Log a call or add a note to start the history." },
);
const ability = useAbility<AppAbility>();
const remove = useDeleteActivity();

function deleteActivity(id: string) {
  remove.mutate(id, { onSuccess: () => toast.success("Activity deleted") });
}
</script>

<template>
  <div v-if="loading" class="flex flex-col gap-5" aria-busy="true">
    <div v-for="i in 3" :key="i" class="flex gap-3">
      <Skeleton class="size-7 rounded-md" />
      <div class="flex flex-1 flex-col gap-2 pt-1">
        <Skeleton class="h-3.5 w-1/2" />
        <Skeleton class="h-3 w-1/3" />
      </div>
    </div>
  </div>
  <p v-else-if="!activities || activities.length === 0" class="py-2 text-[13px] text-fg-3">{{ emptyText }}</p>
  <ol v-else class="flex flex-col" data-testid="activity-timeline">
    <li v-for="a in activities" :key="a.id" class="group relative flex gap-3 pb-6 last:pb-0">
      <span aria-hidden="true" class="absolute top-8 bottom-1 left-3.5 w-px bg-line group-last:hidden" />
      <span v-if="a.kind === 'task'" class="flex size-7 shrink-0 items-center justify-center">
        <TaskCheckbox :activity="a" />
      </span>
      <span
        v-else
        aria-hidden="true"
        class="flex size-7 shrink-0 items-center justify-center rounded-md border border-line-2 bg-ink-2 text-fg-2"
      >
        <component :is="KIND_META[a.kind].icon" class="size-3.5" />
      </span>
      <div class="flex min-w-0 flex-1 flex-col gap-1 pt-0.5">
        <div class="flex items-start justify-between gap-2">
          <p :class="cn('text-sm leading-snug font-medium text-fg', a.completed_at && 'text-fg-3 line-through decoration-fg-3/50')">
            {{ a.subject }}
          </p>
          <ConfirmDialog
            v-if="ability.can('delete', a)"
            title="Delete this activity?"
            :description="`“${a.subject}” will be removed from the timeline.`"
            confirm-label="Delete activity"
            :pending="remove.isPending.value"
            @confirm="deleteActivity(a.id)"
          >
            <template #trigger>
              <Button
                variant="ghost"
                size="icon-xs"
                class="-my-1 text-fg-3 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 max-md:opacity-100"
                :aria-label="`Delete ${a.subject}`"
              >
                <Trash2 />
              </Button>
            </template>
          </ConfirmDialog>
        </div>
        <p v-if="a.body" class="text-[13px] leading-relaxed whitespace-pre-line text-fg-2">{{ a.body }}</p>
        <p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-3">
          <span>{{ KIND_META[a.kind].label }} by {{ a.owner.name }}</span>
          <time :datetime="a.created_at" :title="formatDateTime(a.created_at)">{{ formatRelative(a.created_at) }}</time>
          <span v-if="a.kind === 'task' && a.assignee">Assigned to {{ a.assignee.name }}</span>
          <DueLabel :activity="a" />
          <NuxtLink v-if="showSubject" :to="subjectHref(a.subject_type, a.subject_id)" class="link text-fg-2">{{ a.subject_label }}</NuxtLink>
        </p>
      </div>
    </li>
  </ol>
</template>
