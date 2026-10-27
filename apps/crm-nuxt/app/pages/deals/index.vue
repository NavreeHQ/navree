<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useEventListener } from "@vueuse/core";
import { Plus } from "@lucide/vue";
import { toast } from "vue-sonner";
import Money from "@/components/crm/Money.vue";
import OwnerFilter from "@/components/crm/OwnerFilter.vue";
import PageHeader from "@/components/crm/PageHeader.vue";
import StageDot from "@/components/crm/StageDot.vue";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useCurrentUser } from "@/features/auth/queries";
import { type Columns, columnsFrom, dropIndex, neighbourStage, placeCard, stageOf } from "@/features/deals/board";
import DealCard from "@/features/deals/DealCard.vue";
import LostReasonDialog from "@/features/deals/LostReasonDialog.vue";
import { lockReason } from "@/features/deals/permissions";
import { useDealBoard, useMoveDeal } from "@/features/deals/queries";
import type { Deal, DealStage } from "@/lib/api/types";
import { DEAL_STAGES, stageLabel } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { formatNumber } from "@/lib/format";
import { isClosed } from "@/lib/stages";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

useHead({ title: "Deals" });

const url = useUrlState();
const me = useCurrentUser();
const ui = useUiStore();
const ability = useAbility<AppAbility>();
const ownerId = computed(() => url.get("owner"));
const board = useDealBoard(() => ({ owner_id: ownerId.value }));
const move = useMoveDeal();

const deals = computed(() => {
  const map = new Map<string, Deal>();
  for (const column of board.data.value?.stages ?? []) for (const d of column.deals) map.set(d.id, d);
  return map;
});

// While a card is dragged (and while the lost-reason dialog is open) the board follows local columns.
const localColumns = ref<Columns | null>(null);
const columns = computed(() => localColumns.value ?? columnsFrom(board.data.value));
const canMove = (deal: Deal) => ability.can("move", deal);

const openTotal = computed(() =>
  board.data.value?.stages
    .filter((s) => !isClosed(s.stage))
    .reduce((acc, s) => ({ count: acc.count + s.count, amount: acc.amount + s.amount_cents }), { count: 0, amount: 0 }),
);

function stageDeals(stage: DealStage): Deal[] {
  return columns.value[stage].map((id) => deals.value.get(id)).filter((d): d is Deal => Boolean(d));
}
function stageAmount(stage: DealStage): number {
  return stageDeals(stage).reduce((sum, d) => sum + d.amount_cents, 0);
}

// ---- Moving a card ---------------------------------------------------------------

type Drag = {
  id: string;
  mode: "pointer" | "keyboard";
  origin: { stage: DealStage; index: number };
  x: number;
  y: number;
  offsetX: number;
  offsetY: number;
  width: number;
};
const drag = ref<Drag | null>(null);
const overStage = ref<DealStage | null>(null);
const pendingLost = ref<{ deal: Deal; stage: DealStage; position: number } | null>(null);
const announcement = ref("");
// Bumped by every drag: a move that settles after the next drag has started must not reset that drag.
let generation = 0;
let suppressClick = false;

const columnEls = new Map<DealStage, HTMLElement>();
function setColumnEl(stage: DealStage, el: unknown) {
  if (el instanceof HTMLElement) columnEls.set(stage, el);
  else columnEls.delete(stage);
}
const scroller = ref<HTMLElement | null>(null);

function commit(deal: Deal, stage: DealStage, position: number, lostReason?: string) {
  const mine = generation;
  move.mutate(
    { id: deal.id, stage, position, ...(lostReason ? { lost_reason: lostReason } : {}) },
    {
      onSuccess: () => {
        pendingLost.value = null;
        if (stage !== deal.stage) toast.success(`Moved “${deal.title}” to ${stageLabel(stage)}`);
      },
      onSettled: () => {
        if (generation === mine) localColumns.value = null;
      },
    },
  );
}

function start(id: string, mode: Drag["mode"], at?: { x: number; y: number; rect: DOMRect }) {
  const current = columnsFrom(board.data.value);
  const stage = stageOf(current, id);
  if (!stage) return;
  generation += 1;
  localColumns.value = current;
  drag.value = {
    id,
    mode,
    origin: { stage, index: current[stage].indexOf(id) },
    x: at?.x ?? 0,
    y: at?.y ?? 0,
    offsetX: at ? at.x - at.rect.left : 0,
    offsetY: at ? at.y - at.rect.top : 0,
    width: at?.rect.width ?? 0,
  };
  overStage.value = stage;
  if (mode === "pointer") document.body.style.userSelect = "none";
}

function cancel() {
  if (!drag.value) return;
  const title = deals.value.get(drag.value.id)?.title;
  const wasKeyboard = drag.value.mode === "keyboard";
  const id = drag.value.id;
  drag.value = null;
  overStage.value = null;
  localColumns.value = null;
  document.body.style.userSelect = "";
  if (wasKeyboard) {
    announcement.value = `Put ${title} back.`;
    void nextTick(() => focusCard(id));
  }
}

function finish() {
  const current = drag.value;
  const cols = localColumns.value;
  drag.value = null;
  overStage.value = null;
  document.body.style.userSelect = "";
  if (!current || !cols) return;
  const deal = deals.value.get(current.id);
  const stage = stageOf(cols, current.id);
  if (!deal || !stage) {
    localColumns.value = null;
    return;
  }
  const position = cols[stage].indexOf(current.id);
  if (current.mode === "keyboard") void nextTick(() => focusCard(current.id));
  if (stage === current.origin.stage && position === current.origin.index) {
    localColumns.value = null;
    return;
  }
  announcement.value = `Dropped ${deal.title} in ${stageLabel(stage)}.`;
  if (stage === "lost" && current.origin.stage !== "lost") {
    pendingLost.value = { deal, stage, position }; // hold the card in Lost while we ask why
    return;
  }
  commit(deal, stage, position);
}

/** Moves the dragged card to where the pointer is: the column under it, between the cards around it. */
function track(x: number, y: number) {
  const current = drag.value;
  const cols = localColumns.value;
  if (!current || !cols) return;
  current.x = x;
  current.y = y;
  let target: DealStage | null = null;
  for (const [stage, el] of columnEls) {
    const rect = el.getBoundingClientRect();
    if (x >= rect.left && x <= rect.right) target = stage;
  }
  overStage.value = target;
  if (!target) return;
  const list = columnEls.get(target)?.querySelector("[data-list]");
  const midpoints = [...(list?.querySelectorAll<HTMLElement>("[data-deal-id]") ?? [])]
    .filter((el) => el.dataset.dealId !== current.id)
    .map((el) => {
      const r = el.getBoundingClientRect();
      return r.top + r.height / 2;
    });
  // `index` counts the other cards only, which is exactly where `placeCard` puts the dragged one.
  const index = dropIndex(midpoints, y);
  autoScroll(x);
  if (stageOf(cols, current.id) === target && cols[target].indexOf(current.id) === index) return;
  localColumns.value = placeCard(cols, current.id, target, index);
}

function autoScroll(x: number) {
  const el = scroller.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const edge = 56;
  if (x < rect.left + edge) el.scrollLeft -= 14;
  else if (x > rect.right - edge) el.scrollLeft += 14;
}

// Pointer: a mouse drag starts after 6 px; on touch, a press-and-hold of 220 ms (so columns still scroll).
let press: { id: string; x: number; y: number; rect: DOMRect; touch: boolean; timer?: ReturnType<typeof setTimeout> } | null = null;

function onPointerDown(event: PointerEvent, deal: Deal) {
  if (event.button !== 0 || !canMove(deal) || drag.value) return;
  const el = event.currentTarget as HTMLElement;
  press = { id: deal.id, x: event.clientX, y: event.clientY, rect: el.getBoundingClientRect(), touch: event.pointerType !== "mouse" };
  if (press.touch) {
    const p = press;
    p.timer = setTimeout(() => {
      if (press === p) start(p.id, "pointer", { x: p.x, y: p.y, rect: p.rect });
    }, 220);
  }
}

useEventListener(window, "pointermove", (event: PointerEvent) => {
  if (drag.value?.mode === "pointer") {
    event.preventDefault();
    track(event.clientX, event.clientY);
    return;
  }
  if (!press) return;
  const distance = Math.hypot(event.clientX - press.x, event.clientY - press.y);
  if (press.touch) {
    if (distance > 8) {
      clearTimeout(press.timer);
      press = null;
    }
  } else if (distance >= 6) {
    const p = press;
    press = null;
    start(p.id, "pointer", { x: p.x, y: p.y, rect: p.rect });
    track(event.clientX, event.clientY);
  }
});
useEventListener(window, "pointerup", () => {
  if (press) {
    clearTimeout(press.timer);
    press = null;
  }
  if (drag.value?.mode === "pointer") {
    suppressClick = true;
    setTimeout(() => (suppressClick = false), 0);
    finish();
  }
});
useEventListener(window, "pointercancel", () => {
  press = null;
  if (drag.value?.mode === "pointer") cancel();
});
// Touch drags must not scroll the page under the finger.
useEventListener(window, "touchmove", (event: TouchEvent) => drag.value?.mode === "pointer" && event.preventDefault(), {
  passive: false,
});
useEventListener(window, "keydown", (event: KeyboardEvent) => {
  if (event.key === "Escape" && drag.value?.mode === "pointer") cancel();
});

function openDeal(deal: Deal) {
  if (suppressClick || drag.value) return;
  void navigateTo(`/deals/${deal.id}`);
}

function focusCard(id: string) {
  document.querySelector<HTMLElement>(`[data-testid="deal-card"][data-deal-id="${id}"]`)?.focus();
}

/** Keyboard: Space picks up and drops, arrows move between and within stages, Escape puts it back. */
function onCardKeydown(event: KeyboardEvent, deal: Deal) {
  const current = drag.value;
  if (!current) {
    if (event.key === "Enter") {
      event.preventDefault();
      openDeal(deal);
    } else if (event.key === " " && canMove(deal)) {
      event.preventDefault();
      start(deal.id, "keyboard");
      announcement.value = `Picked up ${deal.title}. Use the arrow keys to move it, Space to drop it, Escape to cancel.`;
    }
    return;
  }
  if (current.mode !== "keyboard" || current.id !== deal.id) return;
  const cols = localColumns.value;
  if (!cols) return;
  const stage = stageOf(cols, deal.id)!;
  const index = cols[stage].indexOf(deal.id);
  let next: Columns | null = null;
  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    const to = neighbourStage(stage, event.key === "ArrowLeft" ? -1 : 1);
    if (to !== stage) next = placeCard(cols, deal.id, to, Math.min(index, cols[to].length));
  } else if (event.key === "ArrowUp" && index > 0) {
    next = placeCard(cols, deal.id, stage, index - 1);
  } else if (event.key === "ArrowDown" && index < cols[stage].length - 1) {
    next = placeCard(cols, deal.id, stage, index + 1);
  } else if (event.key === " " || event.key === "Enter") {
    event.preventDefault();
    finish();
    return;
  } else if (event.key === "Escape") {
    event.preventDefault();
    cancel();
    return;
  } else {
    return;
  }
  event.preventDefault();
  if (!next) return;
  localColumns.value = next;
  const newStage = stageOf(next, deal.id)!;
  overStage.value = newStage;
  announcement.value = `${stageLabel(newStage)}, position ${next[newStage].indexOf(deal.id) + 1} of ${next[newStage].length}.`;
  void nextTick(() => {
    focusCard(deal.id);
    document.querySelector(`[data-deal-id="${deal.id}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
}

onBeforeUnmount(() => (document.body.style.userSelect = ""));

const dragged = computed(() => (drag.value?.mode === "pointer" ? deals.value.get(drag.value.id) : undefined));
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <PageHeader title="Deals">
      <template #description>
        <template v-if="openTotal">
          {{ formatNumber(openTotal.count) }} open {{ openTotal.count === 1 ? "deal" : "deals" }} worth
          <Money :cents="openTotal.amount" class="font-medium text-fg" />. Drag a card to change its stage.
        </template>
        <template v-else>Your pipeline by stage</template>
      </template>
      <template #actions>
        <OwnerFilter :value="ownerId" :current-user-id="me.data.value?.id" @change="(owner) => url.set({ owner })" />
        <Button v-if="ability.can('create', 'Deal')" @click="ui.create('deal')">
          <Plus />
          New deal
        </Button>
      </template>
    </PageHeader>

    <div v-if="board.isPending.value" class="flex gap-3 overflow-hidden" aria-busy="true" aria-label="Loading the board">
      <div v-for="s in DEAL_STAGES" :key="s.stage" class="flex w-[17.5rem] shrink-0 flex-col gap-2 rounded-lg border border-line bg-ink-1/40 p-2.5">
        <Skeleton class="h-10 rounded-md" />
        <Skeleton class="h-24 rounded-lg" />
        <Skeleton class="h-24 rounded-lg" />
        <Skeleton class="h-24 rounded-lg opacity-60" />
      </div>
    </div>

    <div
      v-else
      ref="scroller"
      role="region"
      aria-label="Deal board"
      aria-describedby="board-help"
      class="-mx-4 flex h-[calc(100dvh-12rem)] min-h-[26rem] scroll-px-4 snap-x snap-mandatory gap-3 overflow-x-auto px-4 pb-3 scroll-thin sm:-mx-6 sm:scroll-px-6 sm:px-6 lg:snap-none"
    >
      <p id="board-help" class="sr-only">
        To move a deal with the keyboard, focus its card and press Space to pick it up. Use the arrow keys to move it between stages, then press Space to drop it, or Escape to cancel.
      </p>
      <section
        v-for="stage in DEAL_STAGES"
        :key="stage.stage"
        :ref="(el) => setColumnEl(stage.stage, el)"
        :data-stage="stage.stage"
        :aria-labelledby="`stage-${stage.stage}`"
        :class="
          cn(
            'relative flex max-h-full w-[17.5rem] shrink-0 snap-start flex-col rounded-lg border border-line bg-ink-1/40 transition-colors duration-150',
            drag && overStage === stage.stage && 'border-line-2 bg-ink-1',
            stage.stage === 'won' && 'lg:ml-3',
          )
        "
      >
        <header class="flex flex-col gap-1 px-3.5 pt-3 pb-2.5">
          <div class="flex items-center justify-between gap-2">
            <h2 :id="`stage-${stage.stage}`" class="flex items-center gap-2 text-sm font-semibold text-fg">
              <StageDot :stage="stage.stage" />
              {{ stage.label }}
            </h2>
            <span v-if="!isClosed(stage.stage)" class="font-mono text-xs text-fg-3">{{ stage.probability }}%</span>
          </div>
          <div class="flex items-baseline justify-between gap-2 text-xs text-fg-3">
            <span data-testid="column-count" class="figures">
              {{ formatNumber(columns[stage.stage].length) }} {{ columns[stage.stage].length === 1 ? "deal" : "deals" }}
            </span>
            <Money :cents="stageAmount(stage.stage)" compact class="font-medium text-fg-1" />
          </div>
        </header>
        <ol data-list class="flex min-h-24 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-2 scroll-thin">
          <li
            v-for="deal in stageDeals(stage.stage)"
            :key="deal.id"
            data-testid="deal-card"
            :data-deal-id="deal.id"
            :data-stage="deal.stage"
            tabindex="0"
            :aria-roledescription="canMove(deal) ? 'draggable deal' : 'deal'"
            :aria-label="`${deal.title}, ${deal.company.name}, ${stageLabel(deal.stage)}`"
            :aria-pressed="drag?.mode === 'keyboard' && drag.id === deal.id ? true : undefined"
            :class="
              cn(
                'group/card rounded-lg outline-none select-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-ink-0',
                canMove(deal) ? 'cursor-grab active:cursor-grabbing' : 'cursor-pointer',
                drag?.mode === 'pointer' && drag.id === deal.id && 'opacity-40 [&>div]:border-dashed [&>div]:border-line-2',
              )
            "
            @pointerdown="onPointerDown($event, deal)"
            @click="openDeal(deal)"
            @keydown="onCardKeydown($event, deal)"
            @dragstart.prevent
          >
            <DealCard
              :deal="deal"
              :lock-reason="canMove(deal) ? undefined : lockReason(ability, deal)"
              :lifted="drag?.mode === 'keyboard' && drag.id === deal.id"
              :flash="ui.flashing.has(deal.id)"
            />
          </li>
          <li
            v-if="columns[stage.stage].length === 0"
            class="flex flex-1 items-center justify-center rounded-md border border-dashed border-line-2 px-3 py-8 text-center text-xs text-fg-3"
          >
            {{ stage.stage === "lost" ? "Drop a deal here to mark it lost" : "No deals in this stage" }}
          </li>
        </ol>
      </section>
    </div>

    <Teleport to="body">
      <div
        v-if="dragged && drag"
        data-testid="deal-drag-overlay"
        class="pointer-events-none fixed top-0 left-0 z-50 cursor-grabbing"
        :style="{
          width: `${drag.width}px`,
          transform: `translate3d(${drag.x - drag.offsetX}px, ${drag.y - drag.offsetY}px, 0) rotate(1.5deg)`,
        }"
      >
        <DealCard :deal="dragged" lifted />
      </div>
    </Teleport>
    <p class="sr-only" aria-live="assertive">{{ announcement }}</p>

    <LostReasonDialog
      :open="pendingLost !== null"
      :deal-title="pendingLost?.deal.title ?? ''"
      :pending="move.isPending.value"
      :error="move.error.value"
      @cancel="
        pendingLost = null;
        localColumns = null;
        move.reset();
      "
      @confirm="(reason) => pendingLost && commit(pendingLost.deal, pendingLost.stage, pendingLost.position, reason)"
    />
  </div>
</template>
