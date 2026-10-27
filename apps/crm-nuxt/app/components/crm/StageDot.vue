<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import type { DealStage } from "@/lib/api/types";
import { stageColor } from "@/lib/stages";
import { cn } from "@/lib/utils";

const props = defineProps<{ stage: DealStage; class?: HTMLAttributes["class"] }>();
</script>

<template>
  <span
    aria-hidden="true"
    :class="cn('inline-block size-2 shrink-0 rounded-full', props.class)"
    :style="{ backgroundColor: stageColor(stage) }"
  />
</template>
