<script setup lang="ts">
/**
 * A record's header (DESIGN.md §9): a status line (stage or status), the name
 * as a 24px semibold H1 and a line of facts, actions on the right. The way
 * back is the breadcrumb in the top bar.
 */
defineProps<{ title: string }>();
</script>

<template>
  <header class="flex flex-col gap-3 pb-5">
    <div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
      <div class="flex min-w-0 flex-col gap-2">
        <slot name="eyebrow" />
        <h1 class="font-sans text-2xl leading-8 font-semibold tracking-[-0.02em] break-words text-fg">
          {{ title }}
        </h1>
        <div v-if="$slots.meta" class="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-[13px] text-fg-2">
          <slot name="meta" />
        </div>
      </div>
      <div v-if="$slots.actions" class="flex flex-wrap items-center gap-2">
        <slot name="actions" />
      </div>
    </div>
  </header>
</template>
