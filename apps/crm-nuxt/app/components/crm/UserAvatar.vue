<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { initials } from "@/lib/format";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    name: string;
    id?: string;
    size?: "xs" | "sm" | "md" | "lg";
    showTooltip?: boolean;
    class?: HTMLAttributes["class"];
  }>(),
  { size: "sm", showTooltip: false },
);

// Ink tints with a faint green cast, so teammates are told apart without loud colors.
const TINTS = ["#1b2a22", "#232f2a", "#2a3029", "#1d2b29", "#2b2d25", "#26291f"];

function tintIndex(seed: string): number {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  return hash % TINTS.length;
}

const tint = computed(() => TINTS[tintIndex(props.id ?? props.name)]);
const sizeClass = computed(
  () =>
    ({
      xs: "size-5 text-[9px]",
      sm: "size-6 text-[10px]",
      md: "size-8 text-xs",
      lg: "size-10 text-sm",
    })[props.size],
);
</script>

<template>
  <Tooltip v-if="showTooltip">
    <TooltipTrigger as-child>
      <span
        role="img"
        :aria-label="name"
        :class="
          cn(
            'inline-flex shrink-0 items-center justify-center rounded-sm border border-line-2 font-semibold text-fg-1 select-none',
            sizeClass,
            props.class,
          )
        "
        :style="{ backgroundColor: tint }"
        >{{ initials(name) }}</span
      >
    </TooltipTrigger>
    <TooltipContent>{{ name }}</TooltipContent>
  </Tooltip>
  <span
    v-else
    aria-hidden="true"
    :class="
      cn(
        'inline-flex shrink-0 items-center justify-center rounded-sm border border-line-2 font-semibold text-fg-1 select-none',
        sizeClass,
        props.class,
      )
    "
    :style="{ backgroundColor: tint }"
    >{{ initials(name) }}</span
  >
</template>
