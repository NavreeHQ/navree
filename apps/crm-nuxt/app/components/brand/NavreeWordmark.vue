<script setup lang="ts">
import NavreeMark from "./NavreeMark.vue";

/** The mark, the wordmark and the product label, as in the sidebar and on the login page. */
withDefaults(defineProps<{ compact?: boolean }>(), { compact: false });
</script>

<template>
  <span class="inline-flex items-center gap-2">
    <NavreeMark class="size-6 shrink-0 text-accent" />
    <template v-if="!compact">
      <span class="font-display text-[1.1875rem] font-semibold tracking-[-0.03em] text-fg [font-variation-settings:'wdth'_112]"
        >navree</span
      >
      <span class="rounded-sm border border-line-2 px-1.5 py-px font-mono text-xs leading-4 text-fg-2">CRM</span>
    </template>
  </span>
</template>
