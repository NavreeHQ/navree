<script setup lang="ts">
import { Calendar, Lock } from "@lucide/vue";
import Money from "@/components/crm/Money.vue";
import UserAvatar from "@/components/crm/UserAvatar.vue";
import type { Deal } from "@/lib/api/types";
import { dueBucket, formatDate } from "@/lib/format";
import { isClosed } from "@/lib/stages";
import { cn } from "@/lib/utils";

/** A deal on the board: title, company, value, close date and owner. */
const props = defineProps<{ deal: Deal; lockReason?: string; lifted?: boolean; flash?: boolean }>();

const closed = computed(() => isClosed(props.deal.stage));
const due = computed(() =>
  !closed.value && props.deal.expected_close_date ? dueBucket(`${props.deal.expected_close_date}T23:59:59`) : null,
);
const dateValue = computed(() => props.deal.closed_at ?? props.deal.expected_close_date);
</script>

<template>
  <div
    :class="
      cn(
        'flex flex-col gap-2.5 rounded-lg border border-line bg-ink-1 p-3 text-left transition-[border-color,box-shadow,background-color] duration-200',
        'group-hover/card:border-line-2 group-hover/card:bg-[#0c110e]',
        lifted && 'border-fg-3/60 bg-ink-2 shadow-float',
        flash && 'border-fg-3 bg-ink-3',
      )
    "
  >
    <div class="flex items-start justify-between gap-2">
      <NuxtLink
        :to="`/deals/${deal.id}`"
        class="line-clamp-2 text-sm leading-snug font-medium text-fg hover:underline hover:decoration-fg-3 hover:underline-offset-2"
        draggable="false"
        @click.stop
      >
        {{ deal.title }}
      </NuxtLink>
      <Lock v-if="lockReason" class="mt-0.5 size-3.5 shrink-0 text-fg-3" role="img" :aria-label="lockReason">
        <title>{{ lockReason }}</title>
      </Lock>
    </div>
    <span class="-mt-1 truncate text-xs text-fg-3">{{ deal.company.name }}</span>
    <span v-if="deal.stage === 'lost' && deal.lost_reason" class="line-clamp-2 rounded-md bg-danger/[0.07] px-2 py-1 text-xs text-fg-2">
      {{ deal.lost_reason }}
    </span>
    <div class="flex items-center justify-between gap-2">
      <Money
        :cents="deal.amount_cents"
        :currency="deal.currency"
        :class="cn('text-[13px] font-semibold', deal.stage === 'won' ? 'text-accent' : 'text-fg')"
      />
      <span class="flex items-center gap-2">
        <span
          v-if="dateValue"
          :class="cn('inline-flex items-center gap-1 text-xs text-fg-3 figures', due === 'overdue' && 'font-medium text-danger')"
          :title="closed ? 'Closed' : due === 'overdue' ? 'Expected close has passed' : 'Expected close'"
        >
          <Calendar class="size-3" aria-hidden="true" />
          {{ formatDate(dateValue) }}
        </span>
        <UserAvatar :name="deal.owner.name" :id="deal.owner.id" size="xs" show-tooltip />
      </span>
    </div>
  </div>
</template>
