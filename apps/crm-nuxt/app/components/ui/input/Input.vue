<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { useVModel } from "@vueuse/core"
import { cn } from "@/lib/utils"

const props = defineProps<{
  defaultValue?: string | number
  modelValue?: string | number
  class?: HTMLAttributes["class"]
}>()

const emits = defineEmits<{
  (e: "update:modelValue", payload: string | number): void
}>()

const modelValue = useVModel(props, "modelValue", emits, {
  passive: true,
  defaultValue: props.defaultValue,
})
</script>

<template>
  <input
    v-model="modelValue"
    data-slot="input"
    :class="cn(
      'h-9 w-full min-w-0 rounded-md border border-line-2 bg-ink-1 px-3 py-1 text-sm text-fg transition-[border-color,box-shadow] outline-none placeholder:text-fg-3 hover:border-fg-3/50 file:text-fg file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50',
      'focus-visible:border-accent focus-visible:outline-none focus-visible:shadow-[0_0_0_3px_rgb(61_255_143/0.16)]',
      'aria-invalid:border-danger/70',
      props.class,
    )"
  >
</template>
