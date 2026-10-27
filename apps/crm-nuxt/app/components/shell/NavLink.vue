<script setup lang="ts">
import type { Component } from "vue";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/**
 * A sidebar item (DESIGN.md §9): 32px tall, a 16px icon and 14px text in fg-2.
 * Hover is ink-2, the current page ink-3 with medium text: no bar, no green,
 * no glow. Collapsed to the icon rail, the label moves into a tooltip. A
 * sub-item sits under its parent on a 1px guide line and has no icon.
 */
const props = defineProps<{
  href: string;
  label: string;
  icon?: Component;
  active: boolean;
  collapsed?: boolean;
  count?: number;
  /** What the count means, read after the label (e.g. "overdue"). */
  countLabel?: string;
  sub?: boolean;
}>();
const emit = defineEmits<{ navigate: [] }>();
</script>

<template>
  <Tooltip :disabled="!collapsed">
    <TooltipTrigger as-child>
      <NuxtLink
        :to="href"
        :aria-current="active ? 'page' : undefined"
        :aria-label="collapsed ? (count ? `${label}, ${count} ${countLabel ?? ''}`.trim() : label) : undefined"
        :class="
          cn(
            'group/nav relative flex h-8 min-w-0 items-center gap-2 rounded-md px-2 text-sm text-fg-2 outline-none transition-colors duration-150 hover:bg-ink-2 hover:text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent',
            active && 'bg-ink-3 font-medium text-fg hover:bg-ink-3',
            collapsed && 'size-8 justify-center px-0',
            sub && 'h-7',
          )
        "
        @click="emit('navigate')"
      >
        <component
          :is="icon"
          v-if="icon"
          :stroke-width="1.75"
          :class="cn('size-4 shrink-0', active ? 'text-fg' : 'text-fg-2 group-hover/nav:text-fg')"
          aria-hidden="true"
        />
        <span v-if="!collapsed" class="truncate">{{ label }}</span>
        <span v-if="!collapsed && count" class="ml-auto pl-2 font-mono text-xs text-fg-3 figures"
          ><span class="sr-only">, </span>{{ count }}<span v-if="countLabel" class="sr-only"> {{ countLabel }}</span></span
        >
        <span
          v-if="collapsed && count"
          aria-hidden="true"
          class="absolute top-1 right-1 size-1.5 rounded-full bg-fg-2"
        />
      </NuxtLink>
    </TooltipTrigger>
    <TooltipContent side="right">{{ label }}<template v-if="count"> ({{ count }} {{ countLabel }})</template></TooltipContent>
  </Tooltip>
</template>
