<script setup lang="ts">
import { ArrowDown, ArrowUp, ChevronsUpDown } from "@lucide/vue";
import type { HTMLAttributes } from "vue";
import { TableHead } from "@/components/ui/table";
import { cn } from "@/lib/utils";

/** A column header that toggles `field` / `-field` sorting. */
const props = defineProps<{
  label: string;
  field: string;
  sort: string | undefined;
  class?: HTMLAttributes["class"];
}>();
const emit = defineEmits<{ sort: [sort: string] }>();

const active = computed(() => props.sort === props.field || props.sort === `-${props.field}`);
const desc = computed(() => props.sort === `-${props.field}`);
</script>

<template>
  <TableHead :class="props.class" :aria-sort="active ? (desc ? 'descending' : 'ascending') : 'none'">
    <button
      type="button"
      :class="cn('-ml-1.5 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 transition-colors hover:text-fg', active && 'text-fg')"
      @click="emit('sort', active && !desc ? `-${field}` : field)"
    >
      {{ label }}
      <ArrowDown v-if="active && desc" class="size-3.5" aria-hidden="true" />
      <ArrowUp v-else-if="active" class="size-3.5" aria-hidden="true" />
      <ChevronsUpDown v-else class="size-3.5 opacity-50" aria-hidden="true" />
    </button>
  </TableHead>
</template>
