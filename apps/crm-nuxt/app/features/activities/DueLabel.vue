<script setup lang="ts">
import type { Activity } from "@/lib/api/types";
import { dueBucket, formatDateTime } from "@/lib/format";
import { cn } from "@/lib/utils";

/** A task's due date with overdue and today emphasis; meetings and calls show their scheduled time. */
const props = defineProps<{ activity: Activity }>();
const bucket = computed(() =>
  props.activity.kind === "task" && !props.activity.completed_at ? dueBucket(props.activity.due_at) : null,
);
</script>

<template>
  <template v-if="activity.due_at">
    <span v-if="activity.kind !== 'task'" class="text-xs text-fg-3 figures">Scheduled for {{ formatDateTime(activity.due_at) }}</span>
    <span
      v-else
      :class="
        cn(
          'text-xs figures',
          bucket === 'overdue' ? 'font-medium text-danger' : bucket === 'today' ? 'font-medium text-amber' : 'text-fg-3',
        )
      "
    >
      {{ bucket === "overdue" ? "Overdue, " : "Due " }}{{ formatDateTime(activity.due_at) }}
    </span>
  </template>
</template>
