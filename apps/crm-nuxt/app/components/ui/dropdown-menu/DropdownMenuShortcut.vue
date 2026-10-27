<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: HTMLAttributes["class"]
}>()
</script>

<template>
  <span
    data-slot="dropdown-menu-shortcut"
    :class="cn('ml-auto pl-3 font-mono text-xs whitespace-nowrap text-fg-3', props.class)"
  >
    <slot />
  </span>
</template>
