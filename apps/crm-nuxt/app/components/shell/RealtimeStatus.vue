<script setup lang="ts">
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

/** Whether deal changes arrive live, in the top bar: a status dot, with the word from `sm` up. */
const ui = useUiStore();

const label = computed(
  () =>
    ({
      live: "Live updates on",
      connecting: "Connecting…",
      unavailable: "Live updates unavailable",
      off: "Live updates off",
    })[ui.realtime],
);
const short = computed(() => ({ live: "Live", connecting: "Connecting", unavailable: "Offline", off: "Off" })[ui.realtime]);
const help = computed(
  () =>
    ({
      live: "Deal moves and notifications from your team appear without a reload.",
      connecting: "Opening the real-time connection to the Navree API.",
      unavailable: "The socket couldn't connect. Lists still refresh when you come back to the tab.",
      off: "Real time is turned off for this app.",
    })[ui.realtime],
);
</script>

<template>
  <Tooltip>
    <TooltipTrigger as-child>
      <p
        class="flex h-8 items-center gap-1.5 rounded-md px-2 text-xs text-fg-3 outline-none focus-visible:outline-2 focus-visible:outline-accent"
        tabindex="0"
        :aria-label="label"
        data-testid="realtime-status"
        :data-state="ui.realtime"
      >
        <span
          aria-hidden="true"
          :class="
            cn(
              'size-1.5 shrink-0 rounded-full',
              ui.realtime === 'live' ? 'bg-accent' : ui.realtime === 'connecting' ? 'bg-amber' : 'bg-fg-3/60',
            )
          "
        />
        <span class="hidden sm:inline">{{ short }}</span>
      </p>
    </TooltipTrigger>
    <TooltipContent side="bottom" class="max-w-60">
      <span class="font-medium">{{ label }}.</span> {{ help }}
    </TooltipContent>
  </Tooltip>
</template>
