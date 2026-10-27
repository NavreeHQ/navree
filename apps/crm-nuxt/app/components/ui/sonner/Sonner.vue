<script lang="ts" setup>
import type { ToasterProps } from "vue-sonner"
import { CircleCheckIcon, InfoIcon, Loader2Icon, OctagonXIcon, TriangleAlertIcon, XIcon } from "@lucide/vue"
import { cn } from "@/lib/utils"
import { Toaster as Sonner } from "vue-sonner"

const props = defineProps<ToasterProps>()
</script>

<template>
  <!-- v-bind first: the merged toast options below must win over the raw prop. -->
  <Sonner
    v-bind="props"
    :class="cn('toaster group', props.class)"
    :style="{
      '--normal-bg': 'var(--color-ink-1)',
      '--normal-text': 'var(--color-fg)',
      '--normal-border': 'var(--color-line-2)',
      '--border-radius': 'var(--radius-lg)',
    }"
    :toast-options="{
      ...props.toastOptions,
      classes: {
        closeButton: '!rounded-sm !border-line-2 !bg-ink-2 !text-fg-2 hover:!text-fg',
        description: '!text-fg-2',
        ...props.toastOptions?.classes,
      },
    }"
  >
    <template #success-icon>
      <CircleCheckIcon class="size-4 text-accent" />
    </template>
    <template #info-icon>
      <InfoIcon class="size-4 text-fg-2" />
    </template>
    <template #warning-icon>
      <TriangleAlertIcon class="size-4 text-amber" />
    </template>
    <template #error-icon>
      <OctagonXIcon class="size-4 text-danger" />
    </template>
    <template #loading-icon>
      <div>
        <Loader2Icon class="size-4 animate-spin" />
      </div>
    </template>
    <template #close-icon>
      <XIcon class="size-3" />
    </template>
  </Sonner>
</template>
