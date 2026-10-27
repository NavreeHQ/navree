<script setup lang="ts">
import { Sheet, SheetContent, SheetDescription, SheetTitle } from "@/components/ui/sheet";
import type { User } from "@/lib/api/types";
import { useUiStore } from "@/stores/ui";
import AppSwitcher from "./AppSwitcher.vue";
import SidebarNav from "./SidebarNav.vue";
import UserMenu from "./UserMenu.vue";

/** The sidebar on phones (below `md`): the same switcher, navigation and account menu, off-canvas from the left. */
defineProps<{ user: User }>();
const ui = useUiStore();
const close = () => (ui.mobileNavOpen = false);
</script>

<template>
  <Sheet v-model:open="ui.mobileNavOpen">
    <SheetContent side="left" class="w-(--sidebar-w) max-w-[85vw] gap-0 border-r border-line bg-ink-1 p-0 sm:max-w-(--sidebar-w)">
      <SheetTitle class="sr-only">Navigation</SheetTitle>
      <SheetDescription class="sr-only">Go to a screen of Navree CRM</SheetDescription>
      <div class="flex shrink-0 flex-col p-2 pr-12">
        <AppSwitcher :user="user" mobile @navigate="close" />
      </div>
      <div class="flex min-h-0 flex-1 flex-col overflow-y-auto scroll-thin">
        <SidebarNav :user="user" @navigate="close" />
      </div>
      <div class="flex shrink-0 flex-col p-2">
        <UserMenu :user="user" mobile @navigate="close" />
      </div>
    </SheetContent>
  </Sheet>
</template>
