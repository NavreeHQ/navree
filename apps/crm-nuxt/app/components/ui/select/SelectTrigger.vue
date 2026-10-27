<script setup lang="ts">
import type { SelectTriggerProps } from "reka-ui"
import type { HTMLAttributes } from "vue"
import { ChevronDown } from "@lucide/vue"
import { reactiveOmit } from "@vueuse/core"
import { cn } from "@/lib/utils"
import { SelectIcon, SelectTrigger, useForwardProps } from "reka-ui"

const props = withDefaults(
  defineProps<SelectTriggerProps & { class?: HTMLAttributes["class"], size?: "sm" | "default" }>(),
  { size: "default" },
)

const delegatedProps = reactiveOmit(props, "class", "size")
const forwardedProps = useForwardProps(delegatedProps)
</script>

<template>
  <SelectTrigger
    data-slot="select-trigger"
    :data-size="size"
    v-bind="forwardedProps"
    :class="cn(
      `flex w-fit items-center justify-between gap-2 rounded-md border border-line-2 bg-ink-1 px-3 py-2 text-sm whitespace-nowrap text-fg transition-[border-color,box-shadow] outline-none hover:border-fg-3/50 focus-visible:border-accent focus-visible:shadow-[0_0_0_3px_rgb(61_255_143/0.16)] data-[placeholder]:text-fg-3 aria-invalid:border-danger/70 [&_svg:not([class*='text-'])]:text-fg-3 disabled:cursor-not-allowed disabled:opacity-50 data-[size=default]:h-9 data-[size=sm]:h-8 *:data-[slot=select-value]:line-clamp-1 *:data-[slot=select-value]:flex *:data-[slot=select-value]:items-center *:data-[slot=select-value]:gap-2 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4`,
      props.class,
    )"
  >
    <slot />
    <SelectIcon as-child>
      <ChevronDown class="size-4 opacity-50" />
    </SelectIcon>
  </SelectTrigger>
</template>
