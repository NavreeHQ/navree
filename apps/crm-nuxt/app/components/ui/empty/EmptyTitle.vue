<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: HTMLAttributes["class"]
}>()
</script>

<template>
  <div
    data-slot="empty-title"
    :class="cn('text-[15px] font-semibold tracking-tight text-fg', props.class)"
  >
    <slot />
  </div>
</template>
