<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Check } from "@lucide/vue";
import { toast } from "vue-sonner";
import type { Deal, DealStage } from "@/lib/api/types";
import { DEAL_STAGES, stageLabel } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { isClosed, stageColor } from "@/lib/stages";
import { cn } from "@/lib/utils";
import LostReasonDialog from "./LostReasonDialog.vue";
import { moveBlockedReason } from "./permissions";
import { useMoveDeal } from "./queries";

/**
 * The deal's path through the pipeline. Stages up to the current one fill with
 * their color; choosing a stage moves the deal (when you may).
 */
const props = defineProps<{ deal: Deal }>();
const ability = useAbility<AppAbility>();
const move = useMoveDeal();
const askLost = ref(false);
const canMove = computed(() => ability.can("move", props.deal));
const currentIndex = computed(() => DEAL_STAGES.findIndex((s) => s.stage === props.deal.stage));
const blocked = computed(() =>
  canMove.value ? null : (moveBlockedReason(ability, props.deal) ?? `Only ${props.deal.owner.name} or a manager can move this deal.`),
);

function moveTo(stage: DealStage, lostReason?: string) {
  move.mutate(
    { id: props.deal.id, stage, ...(lostReason ? { lost_reason: lostReason } : {}) },
    {
      onSuccess: () => {
        askLost.value = false;
        toast.success(`Moved to ${stageLabel(stage)}`);
      },
    },
  );
}

function filled(stage: DealStage, index: number): boolean {
  if (stage === props.deal.stage) return true;
  return props.deal.stage !== "lost" && stage !== "lost" && index < currentIndex.value;
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <ol class="grid grid-cols-3 gap-1.5 sm:flex sm:flex-wrap" aria-label="Deal stage">
      <li
        v-for="(s, i) in DEAL_STAGES"
        :key="s.stage"
        :class="cn('min-w-0 sm:min-w-24 sm:flex-1', s.stage === 'won' && 'sm:ml-3', s.stage === 'lost' && 'sm:flex-none')"
      >
        <button
          type="button"
          :disabled="!canMove || s.stage === deal.stage || move.isPending.value"
          :aria-current="s.stage === deal.stage ? 'step' : undefined"
          :title="canMove && s.stage !== deal.stage ? `Move to ${s.label}` : undefined"
          class="group flex w-full flex-col gap-2 rounded-md px-1.5 pt-1.5 pb-2 text-left transition-colors enabled:hover:bg-ink-2 disabled:cursor-default"
          @click="s.stage === 'lost' ? (askLost = true) : moveTo(s.stage)"
        >
          <span
            aria-hidden="true"
            class="h-1.5 w-full rounded-xs transition-colors"
            :style="{
              backgroundColor: filled(s.stage, i) ? stageColor(s.stage) : 'var(--color-line-2)',
            }"
          />
          <span :class="cn('flex items-center gap-1 text-xs', s.stage === deal.stage ? 'font-semibold text-fg' : 'text-fg-3 group-enabled:group-hover:text-fg-1')">
            <Check v-if="s.stage === deal.stage" class="size-3 text-fg" aria-hidden="true" />
            {{ s.label }}
            <span v-if="!isClosed(s.stage)" class="font-mono opacity-70">{{ s.probability }}%</span>
          </span>
        </button>
      </li>
    </ol>
    <p v-if="blocked" class="px-1.5 text-xs text-fg-3">{{ blocked }}</p>
    <LostReasonDialog
      :open="askLost"
      :deal-title="deal.title"
      :pending="move.isPending.value"
      :error="move.error.value"
      @cancel="
        askLost = false;
        move.reset();
      "
      @confirm="(reason) => moveTo('lost', reason)"
    />
  </div>
</template>
