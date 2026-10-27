<script setup lang="ts">
import type { HTMLAttributes } from "vue";
import type { ContactStatus } from "@/lib/api/types";
import { cn } from "@/lib/utils";

const props = defineProps<{ status: ContactStatus; class?: HTMLAttributes["class"] }>();

const STYLE: Record<ContactStatus, { label: string; dot: string }> = {
  lead: { label: "Lead", dot: "bg-fg-3" },
  prospect: { label: "Prospect", dot: "bg-stage-proposal" },
  customer: { label: "Customer", dot: "bg-accent" },
  churned: { label: "Churned", dot: "bg-danger" },
};
</script>

<template>
  <span
    :class="
      cn(
        'inline-flex h-6 items-center gap-1.5 rounded-sm border border-line-2 bg-ink-1 px-1.5 text-xs font-medium whitespace-nowrap text-fg-1',
        props.class,
      )
    "
  >
    <span aria-hidden="true" :class="cn('size-1.5 rounded-full', STYLE[status].dot)" />
    {{ STYLE[status].label }}
  </span>
</template>
