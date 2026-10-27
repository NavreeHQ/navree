<script setup lang="ts">
import { useMediaQuery } from "@vueuse/core";
import { ChevronLeft, ChevronRight, PanelLeft, Search } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { crumbsFor } from "@/lib/navigation";
import { useUiStore } from "@/stores/ui";
import NotificationsBell from "./NotificationsBell.vue";
import QuickCreate from "./QuickCreate.vue";
import RealtimeStatus from "./RealtimeStatus.vue";

/**
 * The top bar (DESIGN.md §9): 48px, the sidebar trigger, a separator and the
 * breadcrumb; search (Ctrl/Cmd K), live status, create and notifications on
 * the right. On phones the trigger opens the navigation sheet and the
 * breadcrumb keeps only the current screen.
 */
const ui = useUiStore();
const route = useRoute();
const desktop = useMediaQuery("(min-width: 768px)");
const isMac = ref(false);
onMounted(() => (isMac.value = /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent)));

const crumbs = computed(() => crumbsFor(route.path, ui.record?.path === route.path ? ui.record.title : null));
/** On phones only the current crumb shows; its parent becomes a back arrow. */
const parent = computed(() => (crumbs.value.length > 1 ? crumbs.value[crumbs.value.length - 2] : undefined));

function toggleSidebar() {
  if (desktop.value) ui.sidebarCollapsed = !ui.sidebarCollapsed;
  else ui.mobileNavOpen = true;
}
const triggerLabel = computed(() =>
  desktop.value ? (ui.sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar") : "Open navigation",
);
</script>

<template>
  <header
    class="sticky top-0 z-30 flex h-(--header-h) shrink-0 items-center gap-2 border-b border-line bg-ink-0/85 px-3 backdrop-blur-md sm:px-4"
  >
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon-sm"
          class="-ml-1 size-7 text-fg-2"
          :aria-label="triggerLabel"
          :aria-expanded="desktop ? !ui.sidebarCollapsed : ui.mobileNavOpen"
          aria-keyshortcuts="Control+B Meta+B"
          @click="toggleSidebar"
        >
          <PanelLeft />
        </Button>
      </TooltipTrigger>
      <TooltipContent side="bottom">
        {{ triggerLabel }} <span class="ml-1 text-fg-3">{{ isMac ? "⌘ B" : "Ctrl B" }}</span>
      </TooltipContent>
    </Tooltip>
    <Separator orientation="vertical" class="mr-1 h-4! data-[orientation=vertical]:h-4" />

    <nav aria-label="Breadcrumb" class="min-w-0 flex-1">
      <ol class="flex min-w-0 items-center gap-1.5 text-sm">
        <li v-if="parent?.href" class="-ml-1 shrink-0 md:hidden">
          <NuxtLink
            :to="parent.href"
            :aria-label="`Back to ${parent.label}`"
            class="flex size-7 items-center justify-center rounded-md text-fg-2 transition-colors hover:bg-ink-2 hover:text-fg"
          >
            <ChevronLeft class="size-4" aria-hidden="true" />
          </NuxtLink>
        </li>
        <template v-for="(crumb, i) in crumbs" :key="`${i}-${crumb.label}`">
          <li :class="i < crumbs.length - 1 ? 'hidden shrink-0 md:block' : 'min-w-0'">
            <NuxtLink
              v-if="crumb.href && i < crumbs.length - 1"
              :to="crumb.href"
              class="rounded-sm text-fg-3 transition-colors hover:text-fg"
              >{{ crumb.label }}</NuxtLink
            >
            <span v-else aria-current="page" class="block truncate font-medium text-fg">{{ crumb.label }}</span>
          </li>
          <li v-if="i < crumbs.length - 1" role="presentation" aria-hidden="true" class="hidden text-fg-3 md:block">
            <ChevronRight class="size-3.5" />
          </li>
        </template>
      </ol>
    </nav>

    <div class="flex shrink-0 items-center gap-1">
      <button
        type="button"
        class="hidden h-8 w-56 items-center gap-2 rounded-md border border-line bg-ink-1 px-2.5 text-[13px] text-fg-3 transition-colors hover:border-line-2 hover:text-fg-2 lg:flex"
        aria-label="Search companies, contacts and deals"
        aria-keyshortcuts="Control+K Meta+K"
        @click="ui.paletteOpen = true"
      >
        <Search class="size-4 shrink-0" aria-hidden="true" />
        <span class="flex-1 truncate text-left">Search…</span>
        <span class="flex items-center gap-0.5" aria-hidden="true">
          <kbd class="kbd">{{ isMac ? "⌘" : "Ctrl" }}</kbd><kbd class="kbd">K</kbd>
        </span>
      </button>
      <Button
        variant="ghost"
        size="icon-sm"
        class="lg:hidden"
        aria-label="Search companies, contacts and deals"
        aria-keyshortcuts="Control+K Meta+K"
        @click="ui.paletteOpen = true"
      >
        <Search />
      </Button>
      <RealtimeStatus />
      <QuickCreate />
      <NotificationsBell />
    </div>
  </header>
</template>
