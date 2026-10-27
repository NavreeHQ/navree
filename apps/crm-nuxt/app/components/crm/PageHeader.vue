<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { cn } from "@/lib/utils";

/** A screen's title row (DESIGN.md §9): a 24px semibold H1, a line of context under it, actions on the right. */
const props = defineProps<{
  title: string;
  description?: string;
  class?: HTMLAttributes["class"];
}>();
</script>

<template>
  <header :class="cn('flex flex-col gap-4 pb-5', props.class)">
    <div class="flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
      <div class="flex min-w-0 flex-col gap-1">
        <h1 class="font-sans text-2xl leading-8 font-semibold tracking-[-0.02em] text-fg">
          {{ title }}
        </h1>
        <p v-if="description || $slots.description" class="text-sm text-fg-2">
          <slot name="description">{{ description }}</slot>
        </p>
      </div>
      <div v-if="$slots.actions" class="flex flex-wrap items-center gap-2">
        <slot name="actions" />
      </div>
    </div>
    <slot />
  </header>
</template>
