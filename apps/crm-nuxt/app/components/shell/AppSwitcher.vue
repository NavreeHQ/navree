<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Check, ChevronsUpDown, Command, Keyboard, ScrollText, Settings, UserCog } from "@lucide/vue";
import NavreeMark from "@/components/brand/NavreeMark.vue";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { User } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

/**
 * The sidebar header (DESIGN.md §9): a 32px square tile with the logo mark,
 * the app's name over its context, and a menu with the workspace's own
 * screens and the keyboard help.
 */
const props = defineProps<{ user: User; collapsed?: boolean; mobile?: boolean }>();
const emit = defineEmits<{ navigate: [] }>();
/**
 * The tooltip's trigger wraps the menu's, and Reka anchors both to the nearest popper (the
 * tooltip's), so the menu is told its anchor explicitly or it never gets positioned.
 */
const trigger = useTemplateRef<HTMLButtonElement>("trigger");
const ui = useUiStore();
const ability = useAbility<AppAbility>();
const isMac = ref(false);
onMounted(() => (isMac.value = /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent)));

function go(href: string) {
  emit("navigate");
  void navigateTo(href);
}
function open(which: "palette" | "shortcuts") {
  emit("navigate");
  if (which === "palette") ui.paletteOpen = true;
  else ui.shortcutsOpen = true;
}
</script>

<template>
  <DropdownMenu>
    <Tooltip :disabled="!collapsed">
      <TooltipTrigger as-child>
        <DropdownMenuTrigger as-child>
          <button
            ref="trigger"
            type="button"
            aria-label="Navree CRM menu"
            :class="
              cn(
                'flex h-12 w-full min-w-0 items-center gap-2 rounded-md p-2 text-left outline-none transition-colors hover:bg-ink-2 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent data-[state=open]:bg-ink-2',
                props.collapsed && 'size-8 justify-center p-0',
              )
            "
          >
            <span class="flex size-8 shrink-0 items-center justify-center rounded-sm border border-line-2 bg-ink-0">
              <NavreeMark class="size-4.5 text-accent" />
            </span>
            <template v-if="!collapsed">
              <span class="flex min-w-0 flex-1 flex-col leading-tight">
                <span class="truncate text-sm font-medium text-fg">Navree CRM</span>
                <span class="truncate text-xs text-fg-3">Sales workspace</span>
              </span>
              <ChevronsUpDown class="size-4 shrink-0 text-fg-3" aria-hidden="true" />
            </template>
          </button>
        </DropdownMenuTrigger>
      </TooltipTrigger>
      <TooltipContent side="right">Navree CRM</TooltipContent>
    </Tooltip>
    <DropdownMenuContent :reference="trigger ?? undefined" :side="mobile ? 'bottom' : 'right'" align="start" :side-offset="4" class="w-64">
      <DropdownMenuLabel class="text-xs font-normal text-fg-3">Workspace</DropdownMenuLabel>
      <DropdownMenuItem class="gap-2 p-2" @select="go('/')">
        <span class="flex size-6 items-center justify-center rounded-sm border border-line-2 bg-ink-0">
          <NavreeMark class="size-3.5 text-accent" />
        </span>
        Navree CRM
        <Check class="ml-auto text-fg-2" aria-label="Current workspace" />
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuItem @select="go('/settings')">
          <Settings />
          Settings
          <DropdownMenuShortcut>G S</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem v-if="ability.can('create', 'User')" @select="go('/settings/users')">
          <UserCog />
          Users and roles
        </DropdownMenuItem>
        <DropdownMenuItem v-if="user.roles.includes('admin')" @select="go('/settings/audit')">
          <ScrollText />
          Audit log
        </DropdownMenuItem>
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuItem @select="open('palette')">
          <Command />
          Command palette
          <DropdownMenuShortcut>{{ isMac ? "⌘ K" : "Ctrl K" }}</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem @select="open('shortcuts')">
          <Keyboard />
          Keyboard shortcuts
          <DropdownMenuShortcut>?</DropdownMenuShortcut>
        </DropdownMenuItem>
      </DropdownMenuGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
