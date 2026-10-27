<script setup lang="ts">
import { Button } from "@/components/ui/button";
import type { AuditEntry } from "@/lib/api/types";

/** "stage: lead → proposal" lines from an audit entry's old and new values. */
const props = defineProps<{ entry: AuditEntry }>();
const expanded = ref(false);

const keys = computed(() =>
  Array.from(new Set([...Object.keys(props.entry.old_values ?? {}), ...Object.keys(props.entry.new_values ?? {})])),
);
const visible = computed(() => (expanded.value ? keys.value : keys.value.slice(0, 2)));

function show(value: unknown): string {
  if (value === null || value === undefined || value === "") return "empty";
  if (Array.isArray(value)) return value.length ? value.join(", ") : "none";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}
const hasOld = (key: string) => Boolean(props.entry.old_values && key in props.entry.old_values);
const hasNew = (key: string) => Boolean(props.entry.new_values && key in props.entry.new_values);
</script>

<template>
  <span v-if="keys.length === 0" class="text-fg-3">No field changes</span>
  <ul v-else class="flex flex-col gap-0.5">
    <li
      v-for="key in visible"
      :key="key"
      class="max-w-md truncate"
      :title="`${key}: ${show(entry.old_values?.[key])} → ${show(entry.new_values?.[key])}`"
    >
      <span class="font-mono text-xs text-fg-3">{{ key }}: </span>
      <template v-if="hasOld(key)">
        <span class="text-fg-3 line-through decoration-fg-3/50">{{ show(entry.old_values?.[key]) }}</span>
        <span aria-label="changed to"> → </span>
      </template>
      <span class="text-fg-1">{{ hasNew(key) ? show(entry.new_values?.[key]) : "removed" }}</span>
    </li>
    <li v-if="keys.length > 2">
      <Button variant="link" size="xs" class="text-xs text-fg-2 hover:text-fg" @click="expanded = !expanded">{{ expanded ? "Show less" : `${keys.length - 2} more` }}</Button>
    </li>
  </ul>
</template>
