<script setup lang="ts">
/** One definition-list row on a record's details panel. An empty value reads "None". */
defineProps<{ label: string; value?: string | null }>();
</script>

<template>
  <div class="grid grid-cols-[7rem_minmax(0,1fr)] items-baseline gap-3 py-2 text-sm">
    <dt class="text-fg-3">{{ label }}</dt>
    <dd class="min-w-0 break-words text-fg-1">
      <slot>
        <template v-if="value">{{ value }}</template>
        <span v-else class="text-fg-3">None</span>
      </slot>
    </dd>
  </div>
</template>
