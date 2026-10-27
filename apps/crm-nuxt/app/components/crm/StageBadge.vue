<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import type { DealStage } from "@/lib/api/types";
import { stageLabel } from "@/lib/api/types";
import { cn } from "@/lib/utils";
import StageDot from "./StageDot.vue";

const props = defineProps<{ stage: DealStage; class?: HTMLAttributes["class"] }>();
</script>

<template>
  <span
    :class="
      cn(
        'inline-flex h-6 items-center gap-1.5 rounded-sm border border-line-2 bg-ink-2 px-1.5 text-xs font-medium whitespace-nowrap text-fg-1',
        stage === 'won' && 'border-accent/30 bg-accent/10 text-accent',
        stage === 'lost' && 'border-danger/30 bg-danger/10 text-danger',
        props.class,
      )
    "
  >
    <StageDot :stage="stage" />
    {{ stageLabel(stage) }}
  </span>
</template>
