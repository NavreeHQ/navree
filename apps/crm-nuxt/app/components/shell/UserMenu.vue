<script setup lang="ts">
import { ChevronsUpDown, Keyboard, LogOut, MonitorSmartphone, Settings } from "@lucide/vue";
import UserAvatar from "@/components/crm/UserAvatar.vue";
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
import { useSignOut } from "@/features/auth/queries";
import type { User } from "@/lib/api/types";
import { roleLabel } from "@/lib/roles";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

/**
 * The sidebar footer (DESIGN.md §9): a 32px square avatar, name and email on
 * two lines, and the account menu: settings, shortcuts and signing out (here,
 * or on every device).
 */
const props = defineProps<{ user: User; collapsed?: boolean; mobile?: boolean }>();
const emit = defineEmits<{ navigate: [] }>();

/**
 * The tooltip's trigger wraps the menu's, and Reka anchors both to the nearest popper (the
 * tooltip's), so the menu is told its anchor explicitly or it never gets positioned.
 */
const trigger = useTemplateRef<HTMLButtonElement>("trigger");
const ui = useUiStore();
const signOutOf = useSignOut();
const signingOut = ref(false);

function goToSettings() {
  emit("navigate");
  void navigateTo("/settings");
}
function showShortcuts() {
  emit("navigate");
  ui.shortcutsOpen = true;
}

async function signOut(all = false) {
  signingOut.value = true;
  try {
    await signOutOf(all ? { all: true } : {});
  } catch {
    // The API could not be reached: this browser is signed out anyway.
  } finally {
    signingOut.value = false;
  }
  void navigateTo("/login", { replace: true });
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
            :aria-label="`Account menu for ${user.name}`"
            :class="
              cn(
                'flex h-12 w-full min-w-0 items-center gap-2 rounded-md p-2 text-left outline-none transition-colors hover:bg-ink-2 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent data-[state=open]:bg-ink-2',
                props.collapsed && 'size-8 justify-center p-0',
              )
            "
          >
            <UserAvatar :name="user.name" :id="user.id" size="md" />
            <template v-if="!collapsed">
              <span class="flex min-w-0 flex-1 flex-col leading-tight">
                <span class="truncate text-sm font-medium text-fg">{{ user.name }}</span>
                <span class="truncate text-xs text-fg-3">{{ user.email }}</span>
              </span>
              <ChevronsUpDown class="size-4 shrink-0 text-fg-3" aria-hidden="true" />
            </template>
          </button>
        </DropdownMenuTrigger>
      </TooltipTrigger>
      <TooltipContent side="right">{{ user.name }}</TooltipContent>
    </Tooltip>
    <DropdownMenuContent :reference="trigger ?? undefined" :side="mobile ? 'top' : 'right'" align="end" :side-offset="4" class="w-60">
      <DropdownMenuLabel class="flex items-center gap-2 px-2 py-1.5 font-normal">
        <UserAvatar :name="user.name" :id="user.id" size="md" />
        <span class="flex min-w-0 flex-1 flex-col leading-tight">
          <span class="truncate text-sm font-medium text-fg">{{ user.name }}</span>
          <span class="truncate text-xs text-fg-3">{{ user.email }}</span>
        </span>
      </DropdownMenuLabel>
      <p class="px-2 pb-1.5 text-xs text-fg-3">Role: <span class="text-fg-2">{{ roleLabel(user.roles) }}</span></p>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuItem @select="goToSettings">
          <Settings />
          Settings
          <DropdownMenuShortcut>G S</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem @select="showShortcuts">
          <Keyboard />
          Keyboard shortcuts
          <DropdownMenuShortcut>?</DropdownMenuShortcut>
        </DropdownMenuItem>
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuItem :disabled="signingOut" @select="signOut(false)">
          <LogOut />
          Sign out
        </DropdownMenuItem>
        <DropdownMenuItem :disabled="signingOut" @select="signOut(true)">
          <MonitorSmartphone />
          Sign out on every device
        </DropdownMenuItem>
      </DropdownMenuGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
