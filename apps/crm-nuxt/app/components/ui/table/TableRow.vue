<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: HTMLAttributes["class"]
}>()
</script>

<template>
  <tr
    data-slot="table-row"
    :class="cn('border-b border-line transition-colors hover:bg-ink-2/60 data-[state=selected]:bg-ink-2', props.class)"
  >
    <slot />
  </tr>
</template>
