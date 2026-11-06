<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import { formatMoney } from "@/lib/format";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    cents: number;
    currency?: string;
    compact?: boolean;
    showCents?: boolean;
    class?: HTMLAttributes["class"];
  }>(),
  { currency: "INR", compact: false, showCents: false },
);

const text = computed(() =>
  formatMoney(props.cents, props.currency, { compact: props.compact, showCents: props.showCents }),
);
const full = computed(() => (props.compact ? formatMoney(props.cents, props.currency) : undefined));
</script>

<template>
  <span :class="cn('whitespace-nowrap figures', props.class)" :title="full">{{ text }}</span>
</template>
