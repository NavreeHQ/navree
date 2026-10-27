<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { cn } from "@/lib/utils"
import { Input } from '@/components/ui/input'

const props = defineProps<{
  class?: HTMLAttributes["class"]
}>()
</script>

<template>
  <Input
    data-slot="input-group-control"
    :class="cn(
      'h-full flex-1 rounded-none border-0 bg-transparent px-2 shadow-none hover:border-0 focus-visible:shadow-none focus-visible:ring-0',
      props.class,
    )"
  />
</template>
