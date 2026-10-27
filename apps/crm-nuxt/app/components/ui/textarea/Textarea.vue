<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { useVModel } from "@vueuse/core"
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: HTMLAttributes["class"]
  defaultValue?: string | number
  modelValue?: string | number
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
  <textarea
    v-model="modelValue"
    data-slot="textarea"
    :class="cn('flex field-sizing-content min-h-16 w-full rounded-md border border-line-2 bg-ink-1 px-3 py-2 text-sm text-fg transition-[border-color,box-shadow] outline-none placeholder:text-fg-3 hover:border-fg-3/50 focus-visible:border-accent focus-visible:shadow-[0_0_0_3px_rgb(61_255_143/0.16)] aria-invalid:border-danger/70 disabled:cursor-not-allowed disabled:opacity-50', props.class)"
  />
</template>
