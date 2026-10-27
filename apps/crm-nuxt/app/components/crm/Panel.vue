<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { cn } from "@/lib/utils";

/** A titled surface inside a screen: dashboard panels, detail sidebars, timelines. */
const props = defineProps<{
  title?: string;
  class?: HTMLAttributes["class"];
  bodyClass?: HTMLAttributes["class"];
}>();
</script>

<template>
  <section :class="cn('surface min-w-0 text-fg', props.class)">
    <div
      v-if="title || $slots.title || $slots.actions"
      class="flex min-h-12 items-center justify-between gap-3 border-b border-line px-4 py-2"
    >
      <h2 class="text-sm font-semibold text-fg">
        <slot name="title">{{ title }}</slot>
      </h2>
      <div v-if="$slots.actions" class="flex items-center gap-1.5">
        <slot name="actions" />
      </div>
    </div>
    <div :class="cn('p-4', props.bodyClass)">
      <slot />
    </div>
  </section>
</template>
