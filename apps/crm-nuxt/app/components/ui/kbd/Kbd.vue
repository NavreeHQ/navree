<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: HTMLAttributes["class"]
}>()
</script>

<template>
  <kbd
    :class="cn(
      'pointer-events-none inline-flex h-5 w-fit min-w-5 items-center justify-center gap-1 rounded-sm border border-b-2 border-line-2 bg-ink-1 px-1.5 font-sans text-xs font-medium text-fg-2 select-none',
      `[&_svg:not([class*='size-'])]:size-3`,
      
      props.class,
    )"
  >
    <slot />
  </kbd>
</template>
