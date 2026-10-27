<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";

const ALL = "__all";

/** A select over a fixed list with an "any" choice. */
const props = defineProps<{
  value: string | undefined;
  options: readonly { value: string; label: string }[];
  anyLabel: string;
  /** The accessible name of the select, e.g. "Filter by status". */
  label: string;
  class?: HTMLAttributes["class"];
}>();
const emit = defineEmits<{ change: [value: string | undefined] }>();
</script>

<template>
  <Select
    :model-value="value ?? ALL"
    @update:model-value="(v) => emit('change', v === ALL || typeof v !== 'string' ? undefined : v)"
  >
    <SelectTrigger
      size="sm"
      :aria-label="label"
      :class="cn('h-8 min-w-32 text-[13px]', value && 'border-fg-3/60 bg-ink-2 text-fg', props.class)"
    >
      <SelectValue :placeholder="anyLabel" />
    </SelectTrigger>
    <SelectContent>
      <SelectGroup>
        <SelectItem :value="ALL">{{ anyLabel }}</SelectItem>
        <SelectItem v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</SelectItem>
      </SelectGroup>
    </SelectContent>
  </Select>
</template>
