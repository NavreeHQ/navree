<script setup lang="ts">
import type { User } from "@/lib/api/types";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";
import AppSwitcher from "./AppSwitcher.vue";
import SidebarNav from "./SidebarNav.vue";
import UserMenu from "./UserMenu.vue";

/**
 * The desktop sidebar (from `md`), the shadcn `sidebar-07` shell (DESIGN.md §9):
 * full height and sticky on ink-1 with a 1px line on the right; the app
 * switcher on top, the navigation, the account menu at the foot. 16rem wide,
 * or a 3rem icon rail when collapsed (Ctrl/Cmd B or the trigger in the top
 * bar; the choice is kept per device).
 */
defineProps<{ user: User }>();
const ui = useUiStore();
</script>

<template>
  <!-- The column carries the surface and the border, so they run the page's full height; the panel inside sticks. -->
  <div
    data-testid="sidebar"
    :data-state="ui.sidebarCollapsed ? 'collapsed' : 'expanded'"
    :class="
      cn(
        'hidden shrink-0 border-r border-line bg-ink-1 transition-[width] duration-200 ease-(--ease-out-quint) md:block',
        ui.sidebarCollapsed ? 'w-(--sidebar-w-icon)' : 'w-(--sidebar-w)',
      )
    "
  >
    <aside class="sticky top-0 flex h-dvh flex-col" aria-label="Sidebar">
      <div :class="cn('flex shrink-0 flex-col p-2', ui.sidebarCollapsed && 'items-center')">
        <AppSwitcher :user="user" :collapsed="ui.sidebarCollapsed" />
      </div>
      <div class="flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto scroll-thin">
        <SidebarNav :user="user" :collapsed="ui.sidebarCollapsed" />
      </div>
      <div :class="cn('flex shrink-0 flex-col p-2', ui.sidebarCollapsed && 'items-center')">
        <UserMenu :user="user" :collapsed="ui.sidebarCollapsed" />
      </div>
    </aside>
  </div>
</template>
