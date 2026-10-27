<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Building2, Columns3, ListChecks, Plus, User } from "@lucide/vue";
import type { ComponentPublicInstance } from "vue";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { AppAbility } from "@/lib/casl/ability";
import { useUiStore } from "@/stores/ui";

/** "Create" from anywhere: the same dialogs the palette and the `c` shortcuts open. */
/**
 * The tooltip's trigger wraps the menu's, and Reka anchors both to the nearest popper (the
 * tooltip's), so the menu is told its anchor explicitly or it never gets positioned.
 */
const trigger = useTemplateRef<ComponentPublicInstance>("trigger");
const triggerEl = computed(() => (trigger.value?.$el as HTMLElement | undefined) ?? undefined);
const ui = useUiStore();
const ability = useAbility<AppAbility>();
const items = computed(() =>
  [
    { kind: "deal" as const, label: "Deal", icon: Columns3, keys: "C D", can: ability.can("create", "Deal") },
    { kind: "company" as const, label: "Company", icon: Building2, keys: "C C", can: ability.can("create", "Company") },
    { kind: "contact" as const, label: "Contact", icon: User, keys: "C P", can: ability.can("create", "Contact") },
    { kind: "task" as const, label: "Task", icon: ListChecks, keys: "C T", can: ability.can("create", "Activity") },
  ].filter((i) => i.can),
);
</script>

<template>
  <DropdownMenu v-if="items.length">
    <Tooltip>
      <TooltipTrigger as-child>
        <DropdownMenuTrigger as-child>
          <Button ref="trigger" variant="ghost" size="icon-sm" aria-label="Create">
            <Plus />
          </Button>
        </DropdownMenuTrigger>
      </TooltipTrigger>
      <TooltipContent>Create</TooltipContent>
    </Tooltip>
    <DropdownMenuContent :reference="triggerEl" align="end" class="w-52">
      <DropdownMenuLabel class="text-xs font-normal text-fg-3">Create</DropdownMenuLabel>
      <DropdownMenuItem v-for="item in items" :key="item.kind" @select="ui.create(item.kind)">
        <component :is="item.icon" />
        {{ item.label }}
        <DropdownMenuShortcut>{{ item.keys }}</DropdownMenuShortcut>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
