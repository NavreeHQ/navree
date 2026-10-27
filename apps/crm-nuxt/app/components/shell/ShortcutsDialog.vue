<script setup lang="ts">
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { useUiStore } from "@/stores/ui";

const ui = useUiStore();

const GROUPS = [
  {
    title: "Anywhere",
    rows: [
      { keys: ["Ctrl", "K"], label: "Search and commands" },
      { keys: ["/"], label: "Search" },
      { keys: ["?"], label: "This list" },
      { keys: ["Ctrl", "B"], label: "Collapse or expand the sidebar" },
    ],
  },
  {
    title: "Go to",
    rows: [
      { keys: ["G", "H"], label: "Dashboard" },
      { keys: ["G", "D"], label: "Deals" },
      { keys: ["G", "C"], label: "Companies" },
      { keys: ["G", "P"], label: "Contacts" },
      { keys: ["G", "T"], label: "Tasks" },
      { keys: ["G", "S"], label: "Settings" },
    ],
  },
  {
    title: "Create",
    rows: [
      { keys: ["C", "D"], label: "New deal" },
      { keys: ["C", "C"], label: "New company" },
      { keys: ["C", "P"], label: "New contact" },
      { keys: ["C", "T"], label: "New task" },
    ],
  },
  {
    title: "Deal board",
    rows: [
      { keys: ["Space"], label: "Pick up or drop the focused card" },
      { keys: ["←", "→"], label: "Move it to the previous or next stage" },
      { keys: ["↑", "↓"], label: "Move it up or down" },
      { keys: ["Esc"], label: "Put it back" },
    ],
  },
];
</script>

<template>
  <Dialog v-model:open="ui.shortcutsOpen">
    <DialogContent class="sm:max-w-2xl">
      <DialogHeader class="text-left">
        <DialogTitle class="font-sans text-lg font-semibold tracking-[-0.01em]">Keyboard shortcuts</DialogTitle>
        <DialogDescription class="text-sm text-fg-2">Press the keys in order; letters work when you're not typing in a field.</DialogDescription>
      </DialogHeader>
      <div class="grid gap-x-8 gap-y-6 sm:grid-cols-2">
        <section v-for="group in GROUPS" :key="group.title" class="flex flex-col gap-2">
          <h3 class="text-xs font-medium text-fg-3">{{ group.title }}</h3>
          <dl class="flex flex-col">
            <div v-for="row in group.rows" :key="row.label" class="flex items-center justify-between gap-4 border-b border-line py-2 text-sm last:border-0">
              <dt class="text-fg-1">{{ row.label }}</dt>
              <dd class="flex shrink-0 gap-1">
                <kbd v-for="k in row.keys" :key="k" class="kbd">{{ k }}</kbd>
              </dd>
            </div>
          </dl>
        </section>
      </div>
    </DialogContent>
  </Dialog>
</template>
