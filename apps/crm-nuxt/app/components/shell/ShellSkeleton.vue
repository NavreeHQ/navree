<script setup lang="ts">
import NavreeMark from "@/components/brand/NavreeMark.vue";
import { Skeleton } from "@/components/ui/skeleton";
import { useUiStore } from "@/stores/ui";

/** The shell while the session loads: the sidebar and top bar in place, so nothing jumps when they arrive. */
const ui = useUiStore();
</script>

<template>
  <div class="flex min-h-dvh" aria-busy="true" aria-label="Loading Navree CRM">
    <div
      :class="[
        'hidden shrink-0 flex-col gap-2 border-r border-line bg-ink-1 p-2 md:flex',
        ui.sidebarCollapsed ? 'w-(--sidebar-w-icon) items-center' : 'w-(--sidebar-w)',
      ]"
    >
      <div class="flex h-12 items-center gap-2 p-2" :class="ui.sidebarCollapsed && 'h-8 p-0'">
        <span class="flex size-8 shrink-0 items-center justify-center rounded-sm border border-line-2 bg-ink-0">
          <NavreeMark class="size-4.5 text-accent" />
        </span>
        <Skeleton v-if="!ui.sidebarCollapsed" class="h-3.5 w-28" />
      </div>
      <div v-if="!ui.sidebarCollapsed" class="mt-2 flex flex-col gap-3 px-2">
        <Skeleton v-for="i in 5" :key="i" class="h-4" :class="i % 2 ? 'w-3/4' : 'w-2/3'" />
      </div>
    </div>
    <div class="flex min-w-0 flex-1 flex-col">
      <div class="h-(--header-h) border-b border-line" />
      <div class="flex flex-col gap-4 px-4 pt-5 sm:px-6 sm:pt-6">
        <Skeleton class="h-7 w-48" />
        <Skeleton class="h-4 w-72 max-w-full" />
        <Skeleton class="mt-2 h-28 w-full rounded-lg" />
        <Skeleton class="h-72 w-full rounded-lg" />
      </div>
    </div>
  </div>
</template>
